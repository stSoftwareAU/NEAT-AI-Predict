# PR summary — issue #59: compaction audit

## Summary

Audited prediction, inference and creature loading for whole-creature
compaction and for duplicated structural rewrites. There are none:
`neat_ai_predict` loads a creature, compiles it unchanged through `neat-core`
and activates it. Nothing here needs to move to the shared Rust compaction API,
and under the parent's rule that "repositories that do not compact must not
acquire new runtime compaction", this PR adds no code. This file is the audit
record. Closes #59.

## Spec

### Intent and Rationale

- Stage 4K of stSoftwareAU/NEAT-AI-core#775 asks each consumer to replace its
  own compaction with the shared core API. When a repository has no compaction,
  the issue says to record the evidence and add no code. That is the case here.

### Essential Design Decisions

- No compaction call is added. Prediction is a read-only path: the creature's
  identity is the SHA-256 of its file bytes (`src/engine.rs` `CreatureFile`),
  and every prediction is pinned to it. Compacting before activation would
  change what is activated without changing that identity.
- The `neat-core` pin stays at `v0.22.16`. This repository calls none of the
  compaction surface, so the unified API (stSoftwareAU/NEAT-AI-core#779) is
  not needed to finish this audit. Moving to a later release (`v0.22.17`, or
  the `0.23` line) is left to the normal dependency sync: Renovate
  (`renovate.json`) and the weekly `cargo update` PR (#51).

### Undiscoverable Facts

- stSoftwareAU/NEAT-AI-core#779 was still open when this audit ran
  (2026-11-26). The audit's finding does not depend on it, because nothing here
  calls compaction, before or after that release.
- The pinned `neat-core` `v0.22.16` does ship pruning modules
  (`neat-core/src/prune_*.rs`). Neither entry point this repository calls
  reaches them: `parse_creature_json` deserialises and checks the width, and
  `compile_creature` checks the width and duplicate synapses, then builds the
  network as given (`neat-core/src/creature.rs` at `v0.22.16`, lines 832 and
  886).

## Evidence

The change is documentation only (backend/CLI repository, no UI files).

Audit of every creature-handling path at the head:

| Path | What it does to the creature | Compaction or structural rewrite? |
| --- | --- | --- |
| `src/engine.rs` `load_creature` | Reads the bytes, hashes them, calls `neat_core::creature::parse_creature_json` | None |
| `src/engine.rs` `Predictor::new` | Refuses recurrent or wider-than-archive creatures, calls `neat_core::creature::compile_creature` | None. These are refusals, not rewrites |
| `src/engine.rs` `Predictor::predict` / `activate_one` | `CompiledNetwork::activate_into` / `activate` per row | None |
| Narrower creature than the archive | Feeds the full-width row; only the first `input` values reach input neurons | None. The creature is not extended or edited |
| `src/archive.rs`, `src/output.rs`, `src/run.rs` | Read observations, write predictions and the manifest | Do not touch the creature's structure |
| `parity/parity.ts` (TypeScript, on demand) | `Creature.fromJSON` then `creature.activate` from `@stsoftware/neat-ai` 7.0.48 | None. No compaction call, so no TS adapter (stSoftwareAU/NEAT-AI#4196) is needed |
| `benches/predict.rs`, `tests/` | Build fixture creatures and run them | None |

How it was checked:

- `grep -rniE 'compact|prune|simplif|tidy|rewrite'` over `src/`, `tests/`,
  `parity/`, `benches/`, `build.rs`, `Cargo.toml` and `README.md` returns no
  hits.
- Read `src/engine.rs` in full; read `parse_creature_json` and
  `compile_creature` in `neat-core` `v0.22.16`.

Serialised-creature compatibility and score semantics are unchanged, because
no source, manifest or lockfile changes. The existing parse and refusal tests
(`src/engine.rs` `widthless_creatures_are_refused_on_parse`,
`a_recurrent_creature_is_refused`, `a_wider_creature_is_refused`) and the
bit-identity test (`bulk_is_bit_identical_to_single_record_activation`) still
pass.

## Test Plan

No tests added or changed; no code changed.

- `cargo fmt --all -- --check`: passed.
- `cargo clippy --quiet --locked --workspace --all-targets --all-features -- -D warnings`:
  passed.
- `cargo test -q --locked --workspace --all-features`: passed, 43 tests, 0
  failed.

<!-- vibe-quality-gate-skipped reason="./quality.sh refused at its tool check: 'cargo-audit' is not installed in this worker container. Its fmt, clippy and test steps were run individually and passed; the remaining steps (cargo-audit, cargo deny, doc, script and lint steps) were not run here. CI runs the full set." -->

What was not checked: the full `./quality.sh` (cargo-audit is missing in this
container); `parity/run.sh` (on demand, needs a creature file and network
access, and no activation code changed); a pin bump to a release that contains
stSoftwareAU/NEAT-AI-core#779, which is still open.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
