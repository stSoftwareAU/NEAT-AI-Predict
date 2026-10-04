#!/usr/bin/env bash
# spell-check.sh — codespell over the repository, shared by CI
# (`.github/workflows/markdown-lint.yml`) and `./quality.sh`.
#
# The options live here rather than in a hidden `.codespellrc`, because the
# fleet's `.gitignore` ignores dotfiles by default. Add genuine domain terms to
# `scripts/codespell-ignore.txt`, one per line, rather than skipping files.
#
# Usage: scripts/spell-check.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if ! command -v codespell >/dev/null 2>&1; then
  echo "spell-check: codespell is required — install with 'pipx install codespell' or 'brew install codespell'" >&2
  exit 1
fi

cd "$ROOT"
codespell \
  --skip "./target,./.git,./Cargo.lock" \
  --ignore-words "scripts/codespell-ignore.txt" \
  </dev/null
echo "spell-check: no typos found"
