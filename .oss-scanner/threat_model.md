# Threat model

The reporting route is in [SECURITY.md](../SECURITY.md); the archive format
and exit codes are in the [README](../README.md#usage). This file is the short
brief for the scanner.

## What this project does and where untrusted input enters

`neat_ai_predict` is a CLI that runs one NEAT-AI creature over a GRQ
identified observation archive and writes per-row predictions plus a
provenance `manifest.json`. Its archive reader is written on the premise that
[nothing in the archive is trusted](../README.md#input-the-grq-identified-observation-archive).
Treat as untrusted:

- the creature JSON named by `--creature` (widths, `forwardOnly`, neurons,
  synapses, squashes, weights), parsed and compiled by the
  [`neat-core`](https://github.com/stSoftwareAU/NEAT-AI-core) dependency;
- everything under `--archive`: `latest.json`, the chained
  `datasets/<id>.json` manifests, each `*.index.json` (shard paths, row
  ordinals, symbols, dates, sizes, SHA-256 digests) and the `*.bin` payloads;
- command-line arguments: `--dataset`, `--from`, `--to` and the three paths;
- whatever already exists at `--output` and in its parent directory.

## Components that matter most / least

Most important:

- `src/archive.rs`: dataset-id and shard-path validation before any join onto
  the root, the manifest chain walk (loops, depth), index/manifest
  consistency, size and SHA-256 checks before a payload is used, and row
  framing;
- `src/output.rs`: the staging directory beside `<output>`, the atomic rename,
  the refusal to overwrite a different run, and every path it derives from
  archive data (year, month, symbol prefix);
- `src/engine.rs`: the input-width contract and the call into `neat-core`;
  report a `neat-core` root cause against that repository as well;
- `src/run.rs`: the rayon fan-out and the non-finite-output accounting.

The crate denies `unsafe_code`. Lower priority: `benches/`, the Deno parity
harness under `parity/`, and `scripts/`, which are maintainer tooling.

## How to exercise it

From `/src`: `cargo test --workspace --all-features` runs the suite in
seconds; `tests/common/` builds GRQ-format fixture archives in process. The
binary is `target/debug/neat_ai_predict predict --creature FILE --archive DIR
--output DIR`.

## How you rate severity

- High: any read or write outside the `--archive` root or outside `<output>`
  and its staging sibling (path traversal, symlinks, crafted dataset ids or
  shard paths), or deleting or overwriting an existing different run.
- Medium: a panic, unbounded allocation or hang on a malformed archive or
  creature; a payload used before its size and digest are verified; a
  prediction written for the wrong row, symbol or date; a run that looks
  finished but is not.
- Low: wrong exit codes or messages, and faults that need the operator to
  name paths they already control.

## Anything to leave alone

- Small float differences from GRQ's TypeScript scoring (native `libm`
  against JavaScript `Math`) are documented in the README, not a bug.
- Refusing a wider or recurrent creature is the contract, not a defect.
