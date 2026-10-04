#!/usr/bin/env bash
# Activation parity harness (issue #4): neat_ai_predict against
# @stsoftware/neat-ai's `creature.activate` — the call GRQ's daily scoring and
# its TypeScript historical inference make — on a GRQ-format archive of
# deterministic pseudo-random rows.
#
# On demand only (it needs Deno and fetches neat-ai from jsr.io); never part of
# ./quality.sh or PR CI. A missing prerequisite fails the run: a skipped parity
# gate that reports success would be worse than none.
#
# Usage: parity/run.sh [creature.json] [rows] [tolerance]
#   creature   default: ../GRQ-cluster/network.json beside this checkout
#   rows       default: 2000
#   tolerance  max |diff| allowed per output, default 1e-5. Native libm and
#              JavaScript Math (in WASM) disagree in the last bits of tanh/exp,
#              so 0 is not achievable; 1.9e-6 was measured on the GRQ cluster
#              creature (see src/engine.rs).
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
creature="${1:-${repo_root}/../GRQ-cluster/network.json}"
rows="${2:-2000}"
tolerance="${3:-0.00001}"

if ! command -v deno >/dev/null 2>&1; then
  echo "❌ deno is required — https://docs.deno.com/runtime/getting_started/installation/" >&2
  exit 1
fi
if [[ ! -f "${creature}" ]]; then
  echo "❌ creature not found: ${creature} (pass one as the first argument)" >&2
  exit 1
fi

echo "==> Checking the harness"
(cd "${repo_root}/parity" && deno fmt --check && deno lint && deno check parity.ts)

echo "==> Building neat_ai_predict (release)"
cargo build --release --locked --manifest-path "${repo_root}/Cargo.toml"

echo "==> Comparing ${rows} rows of $(basename "${creature}") against @stsoftware/neat-ai"
work="$(mktemp -d)"
trap 'rm -rf "${work}"' EXIT
(
  cd "${repo_root}/parity"
  deno run --no-prompt --allow-read --allow-write --allow-run --allow-env --allow-net=jsr.io \
    parity.ts --creature "${creature}" --binary "${repo_root}/target/release/neat_ai_predict" \
    --rows "${rows}" --tolerance "${tolerance}" --work "${work}"
)
echo "✅ parity within ${tolerance}"
