#!/usr/bin/env bash
# test-script-refs.sh — asserts that every sourced or called script path in
# the repository's tracked shell scripts exists.
#
# A naive text scanner cannot tell real shell code from heredoc fixture data,
# so a line such as `source = "git+https://example.invalid/..."` inside a
# Cargo.lock fixture reads exactly like a `source` command at the start of a
# line (Issue #29). This script does not try to be clever about that — it
# flags such lines as missing, the same as a genuinely absent helper script,
# and callers avoid the false positive by keeping fixture `source =` lines in
# a shell variable instead of writing them literally (see
# scripts/test-cargo-update-quarantined.sh). That keeps the scanner itself
# small, simple and loud about anything it cannot resolve.
#
# Usage: scripts/test-script-refs.sh
set -euo pipefail

# Prints one "LINE<TAB>TOKEN<TAB>RESOLVED" line per sourced or called script
# reference found in FILE. ROOT resolves the repository-root call form
# (`./scripts/<name>` or `scripts/<name>`); the `source`/`.` form resolves
# relative to FILE's own directory, with `${SCRIPT_DIR}`/`$SCRIPT_DIR`
# substituted for that directory.
script_refs() {
  local file="$1" root="$2"
  local dir
  dir="$(cd "$(dirname "$file")" && pwd)"

  local line_no=0 line
  while IFS= read -r line || [[ -n "$line" ]]; do
    line_no=$((line_no + 1))

    # Ignore comment lines.
    if [[ "$line" =~ ^[[:space:]]*# ]]; then
      continue
    fi

    local token="" base="$dir"
    if [[ "$line" =~ ^[[:space:]]*(source|\.)[[:space:]]+([^[:space:]]+) ]]; then
      token="${BASH_REMATCH[2]}"
    else
      local first second cmd
      read -r first second _ <<<"$line"
      cmd="$first"
      if [[ "$first" == "bash" ]]; then
        cmd="$second"
      fi
      if [[ "$cmd" == ./scripts/* || "$cmd" == scripts/* ]]; then
        token="$cmd"
        base="$root"
      fi
    fi

    if [[ -z "$token" ]]; then
      continue
    fi

    local stripped="$token"
    stripped="${stripped#\"}"
    stripped="${stripped%\"}"
    stripped="${stripped#\'}"
    stripped="${stripped%\'}"

    local resolved="$stripped"
    resolved="${resolved//\$\{SCRIPT_DIR\}/$dir}"
    resolved="${resolved//\$SCRIPT_DIR/$dir}"

    case "$resolved" in
      /*) : ;;
      *) resolved="${base}/${resolved}" ;;
    esac

    printf '%s\t%s\t%s\n' "$line_no" "$token" "$resolved"
  done <"$file"
}

# Prints "MISSING: FILE:LINE -> TOKEN" for each reference in FILE whose
# resolved path does not exist, and "UNRESOLVED: FILE:LINE -> TOKEN" for each
# reference whose path still contains an unexpanded `$variable` after
# substitution (fail loud, never silently skip). Returns non-zero when any
# problem is found. ROOT is the repository root, used to resolve
# `./scripts/<name>` and `scripts/<name>` call-form references.
check_file() {
  local file="$1" root="$2"
  local problems=0
  local line_no token resolved

  while IFS=$'\t' read -r line_no token resolved; do
    [[ -z "$line_no" ]] && continue
    if [[ "$resolved" == *'$'* ]]; then
      echo "UNRESOLVED: ${file}:${line_no} -> ${token}" >&2
      problems=1
      continue
    fi
    if [[ ! -f "$resolved" ]]; then
      echo "MISSING: ${file}:${line_no} -> ${token}" >&2
      problems=1
    fi
  done < <(script_refs "$file" "$root")

  return "$problems"
}

SELF_TEST_WORK="$(mktemp -d)"
trap 'rm -rf "$SELF_TEST_WORK"' EXIT

self_test_failures=0
assert_no_problems() {
  local description="$1" file="$2" root="$3"
  local output
  if output="$(check_file "$file" "$root" 2>&1)"; then
    :
  else
    printf 'FAIL: %s: expected no problems, got:\n%s\n' "$description" "$output" >&2
    self_test_failures=$((self_test_failures + 1))
  fi
}

assert_problem() {
  local description="$1" file="$2" root="$3" expected_kind="$4" expected_token="$5"
  local output rc=0
  output="$(check_file "$file" "$root" 2>&1)" || rc=$?
  if [[ "$rc" -eq 0 ]]; then
    printf 'FAIL: %s: expected a problem, got none\n' "$description" >&2
    self_test_failures=$((self_test_failures + 1))
    return
  fi
  if [[ "$output" != *"${expected_kind}: "*" -> ${expected_token}"* ]]; then
    printf 'FAIL: %s: expected %s for %q, got:\n%s\n' \
      "$description" "$expected_kind" "$expected_token" "$output" >&2
    self_test_failures=$((self_test_failures + 1))
  fi
}

# (a) sourcing an existing sibling via ${SCRIPT_DIR} is not a problem.
touch "${SELF_TEST_WORK}/helper.sh"
cat >"${SELF_TEST_WORK}/sources-sibling.sh" <<'EOF'
#!/usr/bin/env bash
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/helper.sh"
EOF
assert_no_problems "sourcing an existing sibling" \
  "${SELF_TEST_WORK}/sources-sibling.sh" "$SELF_TEST_WORK"

# (b) sourcing a missing sibling is reported MISSING.
cat >"${SELF_TEST_WORK}/sources-missing.sh" <<'EOF'
#!/usr/bin/env bash
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/gone.sh"
EOF
# shellcheck disable=SC2016 # literal token to match, not an expansion.
assert_problem "sourcing a missing sibling" \
  "${SELF_TEST_WORK}/sources-missing.sh" "$SELF_TEST_WORK" MISSING '"${SCRIPT_DIR}/gone.sh"'

# (c) a Cargo.lock-style heredoc `source =` line reads like a source command
# at the start of a line (Issue #29) and is reported MISSING for the token
# `=`, the same naive-scanner shape as the original false positive.
cat >"${SELF_TEST_WORK}/heredoc-fixture.sh" <<'EOF'
#!/usr/bin/env bash
cat <<LOCK
source = "git+https://example.invalid/gamma?rev=abc#abc"
LOCK
EOF
assert_problem "Cargo.lock heredoc fixture line (issue #29 shape)" \
  "${SELF_TEST_WORK}/heredoc-fixture.sh" "$SELF_TEST_WORK" MISSING "="

# (d) an unexpanded, non-SCRIPT_DIR variable in the path is reported
# UNRESOLVED rather than silently skipped.
cat >"${SELF_TEST_WORK}/sources-unresolved.sh" <<'EOF'
#!/usr/bin/env bash
source "$OTHER/x.sh"
EOF
# shellcheck disable=SC2016 # literal, unexpanded token: the point is it
# stays as "$OTHER/x.sh" so the resolved path still contains "$".
assert_problem "an unresolved variable path" \
  "${SELF_TEST_WORK}/sources-unresolved.sh" "$SELF_TEST_WORK" UNRESOLVED '"$OTHER/x.sh"'

# (e) a commented-out source line is not a problem.
cat >"${SELF_TEST_WORK}/commented-source.sh" <<'EOF'
#!/usr/bin/env bash
# source nothing.sh
EOF
assert_no_problems "a commented-out source line" \
  "${SELF_TEST_WORK}/commented-source.sh" "$SELF_TEST_WORK"

# (f) a repo-root call form (`./scripts/<name>`) resolves against ROOT.
mkdir -p "${SELF_TEST_WORK}/root/scripts"
touch "${SELF_TEST_WORK}/root/scripts/called.sh"
cat >"${SELF_TEST_WORK}/root/scripts/caller.sh" <<'EOF'
#!/usr/bin/env bash
./scripts/called.sh
EOF
assert_no_problems "a repo-root call-form reference that exists" \
  "${SELF_TEST_WORK}/root/scripts/caller.sh" "${SELF_TEST_WORK}/root"

cat >"${SELF_TEST_WORK}/root/scripts/caller-missing.sh" <<'EOF'
#!/usr/bin/env bash
./scripts/does-not-exist.sh
EOF
assert_problem "a repo-root call-form reference that is missing" \
  "${SELF_TEST_WORK}/root/scripts/caller-missing.sh" "${SELF_TEST_WORK}/root" MISSING "./scripts/does-not-exist.sh"

if [[ "$self_test_failures" -ne 0 ]]; then
  echo "test-script-refs: ${self_test_failures} self-test assertion(s) failed" >&2
  exit 1
fi

# With the functions proven correct, check every tracked shell script in the
# real repository. Capture the file list with command substitution, not
# process substitution, so `set -e` catches a `git ls-files` failure rather
# than the loop silently checking zero files and reporting a false green.
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

files="$(git ls-files '*.sh')"
if [[ -z "$files" ]]; then
  echo "test-script-refs: git ls-files '*.sh' found no tracked shell scripts" >&2
  exit 1
fi

repo_problems=0
while IFS= read -r file; do
  if ! check_file "$file" "$REPO_ROOT"; then
    repo_problems=1
  fi
done <<<"$files"

if [[ "$repo_problems" -ne 0 ]]; then
  echo "test-script-refs: one or more sourced or called script paths do not exist" >&2
  exit 1
fi
echo "test-script-refs: all sourced and called script paths exist"
