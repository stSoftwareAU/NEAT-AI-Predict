#!/usr/bin/env bash
# cargo-update-quarantined.sh — `cargo update` that honours the fleet's
# external-dependency quarantine.
#
# Runs `cargo update`, then looks up the crates.io publish time of every
# registry crate version the update introduced. A version published less than
# VIBE_BUMP_QUARANTINE_HOURS (default 24) ago is held back with
# `cargo update --precise <previous version>`. When a young version has no
# single previous version to return to (a newly added transitive crate), the
# whole update is abandoned and Cargo.lock is restored, so nothing younger
# than the quarantine ever reaches a pull request.
#
# Usage: scripts/cargo-update-quarantined.sh
#
# Environment:
#   VIBE_BUMP_QUARANTINE_HOURS  quarantine window in hours (default 24)
#   CRATES_IO_API               API base URL (default https://crates.io/api/v1)
#
# Exit codes:
#   0 — Cargo.lock updated (or deliberately left unchanged, as reported)
#   1 — a lookup or cargo command failed, or the hold-back did not converge
set -euo pipefail

CRATES_IO_API="${CRATES_IO_API:-https://crates.io/api/v1}"
REGISTRY_SOURCE='registry+https://github.com/rust-lang/crates.io-index'
MAX_ROUNDS=5

# Prints "name version" for every crates.io package in a Cargo.lock, sorted.
registry_packages() {
  awk -v source="\"${REGISTRY_SOURCE}\"" '
    /^\[\[package\]\]/ { name = ""; version = ""; next }
    /^name = /         { name = $3; gsub(/"/, "", name); next }
    /^version = /      { version = $3; gsub(/"/, "", version); next }
    /^source = /       { if ($3 == source) print name " " version }
  ' "$1" | LC_ALL=C sort -u
}

# Prints "name version" pairs present in the NEW lockfile but not the OLD one.
added_packages() {
  LC_ALL=C comm -13 <(registry_packages "$1") <(registry_packages "$2")
}

# Prints the publish time of crate $1 version $2 as seconds since the epoch.
published_epoch() {
  curl -sSf --max-time 30 --retry 3 \
    -A "cargo-update-quarantined (https://github.com/stSoftwareAU)" \
    "${CRATES_IO_API}/crates/$1/$2" |
    jq -er '.version.created_at
      | sub("\\.[0-9]+"; "")
      | sub("\\+00:00$"; "Z")
      | fromdateiso8601'
}

# Succeeds when a version published at epoch $1 is still inside the
# quarantine of $3 hours at time $2.
is_quarantined() {
  (($2 - $1 < $3 * 3600))
}

# Prints the single version of crate $3 in OLD lockfile $1 that is absent
# from NEW lockfile $2; prints nothing when there is none or more than one.
previous_version() {
  local candidates
  candidates="$(LC_ALL=C comm -23 <(registry_packages "$1") <(registry_packages "$2") |
    awk -v name="$3" '$1 == name { print $2 }')"
  if [[ -n "$candidates" && "$(printf '%s\n' "$candidates" | wc -l)" -eq 1 ]]; then
    printf '%s\n' "$candidates"
  fi
}

main() {
  local hours="${VIBE_BUMP_QUARANTINE_HOURS:-24}"
  if ! [[ "$hours" =~ ^[0-9]+$ ]]; then
    echo "VIBE_BUMP_QUARANTINE_HOURS must be a whole number of hours, got '$hours'" >&2
    exit 1
  fi

  # Global, not local: the EXIT trap runs after main has returned.
  ORIGINAL_LOCK="$(mktemp)"
  trap 'rm -f "$ORIGINAL_LOCK"' EXIT
  cp Cargo.lock "$ORIGINAL_LOCK"
  local original="$ORIGINAL_LOCK"

  cargo update --quiet

  local round now name version published previous held held_any=0
  for ((round = 1; round <= MAX_ROUNDS; round++)); do
    now="$(date +%s)"
    held=0
    while read -r name version; do
      [[ -z "$name" ]] && continue
      published="$(published_epoch "$name" "$version")"
      if ! is_quarantined "$published" "$now" "$hours"; then
        continue
      fi
      held=1
      held_any=1
      previous="$(previous_version "$original" Cargo.lock "$name")"
      if [[ -z "$previous" ]]; then
        echo "${name} ${version} is inside the ${hours}h quarantine and has no single previous version to hold back to."
        echo "Restoring Cargo.lock: no update is proposed this run."
        cp "$original" Cargo.lock
        return 0
      fi
      echo "Holding back ${name} ${version} (inside the ${hours}h quarantine) at ${previous}."
      cargo update --quiet --package "${name}@${version}" --precise "$previous"
    done < <(added_packages "$original" Cargo.lock)

    if [[ "$held" -eq 0 ]]; then
      if [[ "$held_any" -eq 1 ]]; then
        echo "Cargo.lock updated; versions inside the ${hours}h quarantine were held back."
      else
        echo "Cargo.lock updated; every new version is older than ${hours}h."
      fi
      return 0
    fi
  done

  echo "Holding back quarantined versions did not converge after ${MAX_ROUNDS} rounds." >&2
  cp "$original" Cargo.lock
  exit 1
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  main "$@"
fi
