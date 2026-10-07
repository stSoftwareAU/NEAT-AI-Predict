# Contributing

Thanks for improving this project. The build, run and gate commands are
documented once, in [README › Build and quality gate](./README.md#build-and-quality-gate);
this guide covers the habits for raising a change.

## Branches and pull requests

- `Develop` is the default branch. Branch from it and open the pull request
  back into it.
- Name branches `<type>/<issue-number>-<short-slug>`, for example
  `fix/42-empty-name-panic` or `feat/57-json-output`. Larger pieces of work
  may collect sub-issue PRs on a `milestone/<slug>` branch first; every CI gate
  runs on PRs into `milestone/**` too.
- Reference the issue in the commit message and the PR title, e.g.
  `Fix: reject empty names (Issue #42)`, and put `Closes #42` in the PR body.
- Run `./quality.sh` before you push. CI runs the same checks in the
  pull-request workflows — each script test `quality.sh` runs has a workflow
  step, enforced by `scripts/test-ci-script-tests.sh` — and a red CI run
  blocks the merge.
- There is no changelog file: the git history and the pull requests record
  what changed.
- Docs-only and CI-config-only changes do not need a version bump; anything
  that changes the shipped binary or library bumps the patch version in
  `Cargo.toml` (and `Cargo.lock`).

## Coding standards

- **Fail loud.** Return a typed error (`Result<_, E>` with an `E` that
  implements `std::error::Error`) or exit non-zero; never return a plausible
  success value when the work did not happen. `unwrap()` / `expect()` are
  denied outside tests by the Clippy configuration in `Cargo.toml`.
- **No `unsafe`** without a `// SAFETY:` comment naming the invariants the
  caller must uphold. The crate denies `unsafe_code` by default; lifting that
  is a reviewed decision.
- **Document the contract, not the name.** Every public item carries a `///`
  comment stating what the signature cannot: units, limits, errors, side
  effects. Fallible functions have an `# Errors` section; non-trivial ones an
  `# Examples` block that runs as a doctest.
- **Dependencies.** Add a crate only when a source file `use`s it, with a
  compatible-range version (`"1"`, never `"*"`), from crates.io. `cargo deny`
  refuses unknown registries and git sources.
- **Spelling.** Australian English in prose, comments and messages
  (behaviour, colour, organisation, licence as the noun).
- **Shell scripts** start with `#!/usr/bin/env bash` and `set -euo pipefail`,
  stay compatible with macOS bash 3.2, and pass `bash -n` and `shellcheck`.

## Unit, Integration and Benchmark Tests

Tests fall into exactly three categories. Choose the category by what the test
touches, not by where it is convenient to put it.

| Category | Where it lives | Rules |
| --- | --- | --- |
| **Unit** | `#[cfg(test)] mod tests` beside the code, or `tests/*.rs` for public-API tests | Behavioural (asserts what the code does, never how fast); in-process; parallel-safe; finishes in well under a second. |
| **Integration** | `tests/integration/` behind a Cargo feature or `#[ignore]`, listed in this section | May spawn processes, use the network or a real filesystem; prerequisites missing must **fail**, never skip silently. |
| **Benchmark** | `benches/` | Run on demand only (`cargo bench`), on an otherwise idle machine; never part of `./quality.sh` or PR CI. |

A unit test must not mutate process-wide state: no `std::env::set_var`, no
`std::env::set_current_dir`, no global singletons, no fixed ports and no fixed
shared temporary paths. Take the value as a parameter or through an injected
seam (an environment reader, a clock, a directory handed in by the caller)
instead. A test that cannot meet these rules is an integration test or a
benchmark.

Current integration harnesses and benchmarks:

| Kind | Command | What it proves |
| --- | --- | --- |
| Integration | `./parity/run.sh [creature.json] [rows] [tolerance]` | Predictions agree with `@stsoftware/neat-ai`'s `creature.activate` (needs Deno and jsr.io; defaults to `../GRQ-cluster/network.json`). Fails when a prerequisite is missing. |
| Benchmark | `cargo bench --bench predict [-- --creature <json>] [-- --rows N]` | Activation rows/s on one thread and on the whole pool. |

The tests under `tests/` build GRQ-format archives in private temporary
directories and run in process, so they are unit-category tests of the public
API.
