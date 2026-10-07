#!/usr/bin/env bash
# test-ci-script-tests.sh — asserts that every `./scripts/test-*.sh` run by
# quality.sh is also run by at least one pull-request-triggered workflow
# under .github/workflows.
#
# quality.sh is the local gate and is meant to predict a green PR, but a
# script test added to quality.sh without a matching CI step gives a false
# sense of security: it is never actually checked on a pull request (Issue
# #32). This script closes that gap by comparing the two lists directly.
#
# Usage: scripts/test-ci-script-tests.sh
set -euo pipefail

# Prints the path of each `./scripts/test-*.sh` listed in ROOT/quality.sh
# that is not run by any pull_request-triggered workflow under
# ROOT/.github/workflows. Fails loud (returns 2, message on stderr) if
# ROOT/quality.sh is missing, if the workflows directory is missing, or if
# quality.sh lists zero script tests — none of those are a silent pass.
uncovered_script_tests() {
  local root="$1"
  local quality="${root}/quality.sh"
  local workflows_dir="${root}/.github/workflows"

  if [[ ! -f "$quality" ]]; then
    echo "uncovered_script_tests: missing ${quality}" >&2
    return 2
  fi
  if [[ ! -d "$workflows_dir" ]]; then
    echo "uncovered_script_tests: missing ${workflows_dir}" >&2
    return 2
  fi

  local script_tests=()
  local line trimmed
  while IFS= read -r line || [[ -n "$line" ]]; do
    trimmed="${line#"${line%%[![:space:]]*}"}"
    trimmed="${trimmed%"${trimmed##*[![:space:]]}"}"
    if [[ "$trimmed" =~ ^\./scripts/test-[A-Za-z0-9_-]+\.sh$ ]]; then
      script_tests+=("$trimmed")
    fi
  done <"$quality"

  if [[ "${#script_tests[@]}" -eq 0 ]]; then
    echo "uncovered_script_tests: ${quality} lists zero script tests" >&2
    return 2
  fi

  # Collect every workflow file that is triggered on `pull_request` (but not
  # merely `pull_request_target`) once, up front.
  local pr_workflows=()
  local workflow_file wline
  while IFS= read -r workflow_file; do
    [[ -z "$workflow_file" ]] && continue
    while IFS= read -r wline; do
      if [[ "$wline" =~ ^[[:space:]]*pull_request: ]]; then
        pr_workflows+=("$workflow_file")
        break
      fi
    done <"$workflow_file"
  done < <(find "$workflows_dir" -maxdepth 1 -type f \( -name '*.yml' -o -name '*.yaml' \) | sort)

  local script_test
  for script_test in "${script_tests[@]}"; do
    local covered=0
    local wf
    for wf in "${pr_workflows[@]:-}"; do
      [[ -z "$wf" ]] && continue
      local cline
      while IFS= read -r cline; do
        local cline_trimmed="${cline#"${cline%%[![:space:]]*}"}"
        if [[ "$cline_trimmed" =~ ^# ]]; then
          continue
        fi
        if grep -qF "$script_test" <<<"$cline"; then
          covered=1
          break
        fi
      done <"$wf"
      [[ "$covered" -eq 1 ]] && break
    done
    if [[ "$covered" -eq 0 ]]; then
      echo "$script_test"
    fi
  done
}

SELF_TEST_WORK="$(mktemp -d)"
trap 'rm -rf "$SELF_TEST_WORK"' EXIT

self_test_failures=0
assert_eq() {
  local description="$1" expected="$2" actual="$3"
  if [[ "$expected" != "$actual" ]]; then
    printf 'FAIL: %s\n  expected: %q\n  actual:   %q\n' "$description" "$expected" "$actual" >&2
    self_test_failures=$((self_test_failures + 1))
  fi
}

assert_fails() {
  local description="$1" root="$2" expected="$3"
  local rc=0 err
  err="$(uncovered_script_tests "$root" 2>&1 >/dev/null)" || rc=$?
  if [[ "$rc" -ne 2 ]]; then
    printf 'FAIL: %s: expected status 2, got %s\n' "$description" "$rc" >&2
    self_test_failures=$((self_test_failures + 1))
  fi
  if [[ "$err" != *"$expected"* ]]; then
    printf 'FAIL: %s: expected stderr to contain %q, got:\n%s\n' "$description" "$expected" "$err" >&2
    self_test_failures=$((self_test_failures + 1))
  fi
}

# Fixture quality.sh and workflow lines are built from a variable, not
# written literally in a heredoc, so this file's own source has no line
# starting with a literal `./scripts/` or `scripts/` token for a
# nonexistent script (Issue #29) — scripts/test-script-refs.sh would
# otherwise flag it.
prefix='./scripts'

mkdir -p "${SELF_TEST_WORK}/a/.github/workflows"
cat >"${SELF_TEST_WORK}/a/quality.sh" <<EOF
#!/usr/bin/env bash
step "Script tests"
${prefix}/test-alpha.sh
${prefix}/test-beta.sh
EOF
cat >"${SELF_TEST_WORK}/a/.github/workflows/ci.yml" <<EOF
name: CI
on:
  pull_request:
    branches: [Develop]
jobs:
  test:
    steps:
      - run: ${prefix}/test-alpha.sh
      - run: ${prefix}/test-beta.sh
EOF

# (a) positive: both script tests covered by a pull_request workflow.
assert_eq "both covered prints nothing" \
  "" \
  "$(uncovered_script_tests "${SELF_TEST_WORK}/a")"

# (b) negative: one covered, one not.
mkdir -p "${SELF_TEST_WORK}/b/.github/workflows"
cat >"${SELF_TEST_WORK}/b/quality.sh" <<EOF
#!/usr/bin/env bash
step "Script tests"
${prefix}/test-alpha.sh
${prefix}/test-beta.sh
EOF
cat >"${SELF_TEST_WORK}/b/.github/workflows/ci.yml" <<EOF
name: CI
on:
  pull_request:
    branches: [Develop]
jobs:
  test:
    steps:
      - run: ${prefix}/test-alpha.sh
EOF
assert_eq "one covered, one not prints the uncovered one" \
  "${prefix}/test-beta.sh" \
  "$(uncovered_script_tests "${SELF_TEST_WORK}/b")"

# (c) negative: covered only by a workflow triggered on push/schedule, not
# pull_request.
mkdir -p "${SELF_TEST_WORK}/c/.github/workflows"
cat >"${SELF_TEST_WORK}/c/quality.sh" <<EOF
#!/usr/bin/env bash
step "Script tests"
${prefix}/test-alpha.sh
EOF
cat >"${SELF_TEST_WORK}/c/.github/workflows/ci.yml" <<EOF
name: CI
on:
  push:
    branches: [Develop]
  schedule:
    - cron: "0 0 * * *"
jobs:
  test:
    steps:
      - run: ${prefix}/test-alpha.sh
EOF
assert_eq "push/schedule-only workflow does not count as coverage" \
  "${prefix}/test-alpha.sh" \
  "$(uncovered_script_tests "${SELF_TEST_WORK}/c")"

# (d) negative: covered only by a pull_request_target workflow.
mkdir -p "${SELF_TEST_WORK}/d/.github/workflows"
cat >"${SELF_TEST_WORK}/d/quality.sh" <<EOF
#!/usr/bin/env bash
step "Script tests"
${prefix}/test-alpha.sh
EOF
cat >"${SELF_TEST_WORK}/d/.github/workflows/ci.yml" <<EOF
name: CI
on:
  pull_request_target:
    branches: [Develop]
jobs:
  test:
    steps:
      - run: ${prefix}/test-alpha.sh
EOF
assert_eq "pull_request_target-only workflow does not count as coverage" \
  "${prefix}/test-alpha.sh" \
  "$(uncovered_script_tests "${SELF_TEST_WORK}/d")"

# (e) negative: covered only in a comment line of a pull_request workflow.
mkdir -p "${SELF_TEST_WORK}/e/.github/workflows"
cat >"${SELF_TEST_WORK}/e/quality.sh" <<EOF
#!/usr/bin/env bash
step "Script tests"
${prefix}/test-alpha.sh
EOF
cat >"${SELF_TEST_WORK}/e/.github/workflows/ci.yml" <<EOF
name: CI
on:
  pull_request:
    branches: [Develop]
jobs:
  test:
    steps:
      # ${prefix}/test-alpha.sh
      - run: echo hi
EOF
assert_eq "a comment-only mention does not count as coverage" \
  "${prefix}/test-alpha.sh" \
  "$(uncovered_script_tests "${SELF_TEST_WORK}/e")"

# (f) quality.sh with no script tests must fail loud, not pass silently.
mkdir -p "${SELF_TEST_WORK}/f/.github/workflows"
cat >"${SELF_TEST_WORK}/f/quality.sh" <<EOF
#!/usr/bin/env bash
step "Nothing to see here"
EOF
cat >"${SELF_TEST_WORK}/f/.github/workflows/ci.yml" <<EOF
name: CI
on:
  pull_request:
    branches: [Develop]
jobs:
  test:
    steps:
      - run: echo hi
EOF
assert_fails "quality.sh with zero script tests fails loud" "${SELF_TEST_WORK}/f" \
  "lists zero script tests"

# (g) missing quality.sh must fail loud.
mkdir -p "${SELF_TEST_WORK}/g/.github/workflows"
assert_fails "missing quality.sh fails loud" "${SELF_TEST_WORK}/g" \
  "missing ${SELF_TEST_WORK}/g/quality.sh"

# (h) missing .github/workflows directory must fail loud.
mkdir -p "${SELF_TEST_WORK}/h"
cat >"${SELF_TEST_WORK}/h/quality.sh" <<EOF
#!/usr/bin/env bash
step "Script tests"
${prefix}/test-alpha.sh
EOF
assert_fails "missing .github/workflows fails loud" "${SELF_TEST_WORK}/h" \
  "missing ${SELF_TEST_WORK}/h/.github/workflows"

if [[ "$self_test_failures" -ne 0 ]]; then
  echo "test-ci-script-tests: ${self_test_failures} self-test assertion(s) failed" >&2
  exit 1
fi

# With the function proven correct, check the real repository.
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

uncovered="$(uncovered_script_tests "$REPO_ROOT")"
if [[ -n "$uncovered" ]]; then
  while IFS= read -r script; do
    [[ -z "$script" ]] && continue
    echo "FAIL: ${script} is run by quality.sh but by no pull_request workflow — add a step to a pull_request workflow" >&2
  done <<<"$uncovered"
  exit 1
fi

echo "test-ci-script-tests: every quality.sh script test runs in a PR workflow"
