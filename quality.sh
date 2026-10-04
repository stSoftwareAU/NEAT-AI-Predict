#!/usr/bin/env bash
# quality.sh — the local gate. Runs the same checks as the pull-request CI
# workflows, so a green run here predicts a green PR. Every tool is required:
# a missing tool fails the gate rather than skipping its check.
#
# Usage: ./quality.sh
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"

# Replace `.cargo/config.toml` rustflags (host-tuned `target-cpu=native`) so
# local results match CI, and fail on every warning.
export RUSTFLAGS="-D warnings"
export RUSTDOCFLAGS="-D warnings"

require() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "quality.sh: '$1' is required — $2" >&2
    exit 1
  fi
}

require cargo "install Rust with rustup: https://rustup.rs"
require shellcheck "brew install shellcheck (or see https://github.com/koalaman/shellcheck#installing)"
require codespell "pipx install codespell (or brew install codespell)"
require markdownlint-cli2 "npm install -g --ignore-scripts markdownlint-cli2"
require actionlint "brew install actionlint (or see https://github.com/rhysd/actionlint)"
require cargo-deny "cargo install --locked cargo-deny"
require cargo-audit "cargo install --locked cargo-audit"
require jq "brew install jq (or apt-get install jq)"

step() {
  echo "==> $1"
}

step "Shell syntax (bash -n)"
while IFS= read -r script; do
  bash -n "$script"
done < <(git ls-files '*.sh')

step "ShellCheck"
git ls-files -z '*.sh' | xargs -0 shellcheck -x --severity=style

step "Script tests"
./scripts/test-cargo-update-quarantined.sh

step "runlib.sh already-installed contract (issue #2)"
./scripts/test-runlib.sh

step "Spelling"
./scripts/spell-check.sh

step "Markdown lint"
markdownlint-cli2 </dev/null

step "Workflow lint"
actionlint

step "Formatting"
cargo fmt --all -- --check

step "Clippy"
cargo clippy --quiet --locked --workspace --all-targets --all-features -- -D warnings

step "Compile check"
cargo check --quiet --locked --workspace --all-targets --all-features

step "Tests"
cargo test -q --locked --workspace --all-features

step "Documentation"
cargo doc --quiet --locked --workspace --no-deps

step "Licences, bans, advisories and sources (cargo deny)"
cargo deny --log-level error --locked check

step "RustSec advisories (cargo audit)"
cargo audit --quiet

echo "All quality checks passed."
