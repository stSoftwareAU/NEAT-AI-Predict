#!/usr/bin/env bash
# apply-repo-settings.sh — copy this template's GitHub repository settings
# onto a repository created from it.
#
# GitHub's "Use this template" copies files only. Rulesets, labels, merge
# options, Actions permissions and security features stay behind, so a new
# repository would otherwise start with none of the governance the VibeCoder
# scans expect. This script reads them from the source repository (the
# template, by default) and applies them to the target.
#
# Usage: scripts/apply-repo-settings.sh <owner/target-repo> [owner/source-repo]
#
# Requires `gh` (authenticated as a repository admin) and `jq`. Every API
# failure stops the script: a half-applied configuration is reported, never
# passed off as complete.
set -euo pipefail

usage() {
  echo "usage: $0 <owner/target-repo> [owner/source-repo]" >&2
  exit 64
}

[[ $# -ge 1 && $# -le 2 ]] || usage
TARGET="$1"
SOURCE="${2:-stSoftwareAU/template-rust}"
[[ "$TARGET" == */* && "$SOURCE" == */* ]] || usage
if [[ "$TARGET" == "$SOURCE" ]]; then
  echo "target and source are the same repository: $TARGET" >&2
  exit 64
fi

for tool in gh jq; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "apply-repo-settings: '$tool' is required" >&2
    exit 1
  fi
done

step() {
  echo "==> $1"
}

step "Merge options and features"
gh api "repos/${SOURCE}" --jq '{
    has_issues, has_projects, has_wiki, has_discussions,
    allow_squash_merge, allow_merge_commit, allow_rebase_merge,
    allow_auto_merge, delete_branch_on_merge, allow_update_branch,
    squash_merge_commit_title, squash_merge_commit_message,
    merge_commit_title, merge_commit_message, web_commit_signoff_required
  }' </dev/null |
  gh api -X PATCH "repos/${TARGET}" --input - --silent

step "Security features"
gh api -X PUT "repos/${TARGET}/vulnerability-alerts" --silent </dev/null
gh api -X PUT "repos/${TARGET}/automated-security-fixes" --silent </dev/null
gh api -X PATCH "repos/${TARGET}" --silent --input - <<'EOF'
{"security_and_analysis": {
  "secret_scanning": {"status": "enabled"},
  "secret_scanning_push_protection": {"status": "enabled"}}}
EOF
if [[ "$(gh api "repos/${TARGET}" --jq .visibility </dev/null)" == "public" ]]; then
  gh api -X PUT "repos/${TARGET}/private-vulnerability-reporting" --silent </dev/null
else
  echo "    private repository: private vulnerability reporting needs a public repository; enable it after publishing"
fi

step "Actions permissions"
gh api "repos/${SOURCE}/actions/permissions" \
  --jq '{enabled, allowed_actions, sha_pinning_required}' </dev/null |
  gh api -X PUT "repos/${TARGET}/actions/permissions" --input - --silent
gh api "repos/${SOURCE}/actions/permissions/selected-actions" </dev/null |
  gh api -X PUT "repos/${TARGET}/actions/permissions/selected-actions" --input - --silent
gh api "repos/${SOURCE}/actions/permissions/workflow" </dev/null |
  gh api -X PUT "repos/${TARGET}/actions/permissions/workflow" --input - --silent

step "Labels"
gh api --paginate "repos/${SOURCE}/labels" --jq '.[] | [.name, .color, .description] | @tsv' </dev/null |
  while IFS=$'\t' read -r name color description; do
    gh label create "$name" --repo "$TARGET" --color "$color" \
      --description "$description" --force </dev/null >/dev/null
  done

step "Rulesets"
existing="$(gh api "repos/${TARGET}/rulesets" --jq '[.[].name]' </dev/null)"
for id in $(gh api "repos/${SOURCE}/rulesets" --jq '.[].id' </dev/null); do
  ruleset="$(gh api "repos/${SOURCE}/rulesets/${id}" \
    --jq '{name, target, enforcement, bypass_actors, conditions, rules}' </dev/null)"
  name="$(jq -r .name <<<"$ruleset")"
  if jq -e --arg name "$name" 'index($name)' <<<"$existing" >/dev/null; then
    target_id="$(gh api "repos/${TARGET}/rulesets" \
      --jq ".[] | select(.name == \"${name}\") | .id" </dev/null)"
    gh api -X PUT "repos/${TARGET}/rulesets/${target_id}" --input - --silent <<<"$ruleset"
    echo "    updated ruleset: ${name}"
  else
    gh api -X POST "repos/${TARGET}/rulesets" --input - --silent <<<"$ruleset"
    echo "    created ruleset: ${name}"
  fi
done

echo "Settings from ${SOURCE} applied to ${TARGET}."
echo "Not copied (values are not readable): secrets GITLEAKS_LICENSE, SEMGREP_APP_TOKEN, ACTIONS_PUSH — set them on ${TARGET} where the organisation does not already provide them."
