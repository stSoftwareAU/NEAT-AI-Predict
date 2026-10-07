#!/usr/bin/env bash
# test-cargo-update-quarantined.sh — hermetic unit tests for the helper
# functions in scripts/cargo-update-quarantined.sh.
#
# No network and no cargo: the crates.io API is replaced by `file://`
# fixtures in a private temporary directory, so the tests are parallel-safe
# and finish in well under a second.
#
# Usage: scripts/test-cargo-update-quarantined.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/cargo-update-quarantined.sh
source "${SCRIPT_DIR}/cargo-update-quarantined.sh"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

failures=0
assert_eq() {
  local description="$1" expected="$2" actual="$3"
  if [[ "$expected" != "$actual" ]]; then
    printf 'FAIL: %s\n  expected: %q\n  actual:   %q\n' "$description" "$expected" "$actual" >&2
    failures=$((failures + 1))
  fi
}

registry='source = "registry+https://github.com/rust-lang/crates.io-index"'
# Fixture `source =` lines go through variables so no line in this script
# starts with `source` unless it is a real source command (Issue #29).
git_source='source = "git+https://example.invalid/gamma?rev=abc#abc"'
cat >"${WORK}/old.lock" <<EOF
version = 4

[[package]]
name = "alpha"
version = "1.0.0"
${registry}
checksum = "00"

[[package]]
name = "beta"
version = "2.0.0"
${registry}
checksum = "00"

[[package]]
name = "my-crate"
version = "0.1.0"
EOF
cat >"${WORK}/new.lock" <<EOF
version = 4

[[package]]
name = "alpha"
version = "1.1.0"
${registry}
checksum = "00"

[[package]]
name = "beta"
version = "2.0.0"
${registry}
checksum = "00"

[[package]]
name = "gamma"
version = "0.3.0"
${git_source}

[[package]]
name = "delta"
version = "0.1.0"
${registry}
checksum = "00"

[[package]]
name = "my-crate"
version = "0.1.0"
EOF

assert_eq "registry_packages lists only crates.io packages, sorted" \
  "$(printf 'alpha 1.0.0\nbeta 2.0.0')" \
  "$(registry_packages "${WORK}/old.lock")"

assert_eq "added_packages reports bumped and newly added registry crates" \
  "$(printf 'alpha 1.1.0\ndelta 0.1.0')" \
  "$(added_packages "${WORK}/old.lock" "${WORK}/new.lock")"

assert_eq "previous_version finds the version a bump replaced" \
  "1.0.0" \
  "$(previous_version "${WORK}/old.lock" "${WORK}/new.lock" alpha)"

assert_eq "previous_version is empty for a newly added crate" \
  "" \
  "$(previous_version "${WORK}/old.lock" "${WORK}/new.lock" delta)"

if is_quarantined 1000 $((1000 + 23 * 3600)) 24; then held=yes; else held=no; fi
assert_eq "is_quarantined holds a version published 23h ago" "yes" "$held"

if is_quarantined 1000 $((1000 + 24 * 3600)) 24; then held=yes; else held=no; fi
assert_eq "is_quarantined releases a version exactly 24h old" "no" "$held"

mkdir -p "${WORK}/api/crates/alpha" "${WORK}/api/crates/beta"
printf '{"version":{"created_at":"2026-06-05T12:33:04.440539Z"}}' >"${WORK}/api/crates/alpha/1.1.0"
printf '{"version":{"created_at":"2024-01-02T03:04:05+00:00"}}' >"${WORK}/api/crates/beta/2.0.0"
CRATES_IO_API="file://${WORK}/api"

assert_eq "published_epoch parses a fractional-second UTC timestamp" \
  "1780662784" \
  "$(published_epoch alpha 1.1.0)"

assert_eq "published_epoch parses an offset timestamp" \
  "1704164645" \
  "$(published_epoch beta 2.0.0)"

if published_epoch missing 9.9.9 >/dev/null 2>&1; then lookup=succeeded; else lookup=failed; fi
assert_eq "published_epoch fails loudly when the crate is not found" "failed" "$lookup"

if [[ "$failures" -ne 0 ]]; then
  echo "test-cargo-update-quarantined: ${failures} assertion(s) failed" >&2
  exit 1
fi
echo "test-cargo-update-quarantined: all assertions passed"
