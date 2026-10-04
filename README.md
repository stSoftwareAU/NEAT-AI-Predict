# template-rust

Template repository for stSoftwareAU Rust projects. A repository created from
it starts with the files, lint settings, CI workflows and security tooling
that the [VibeCoder](https://github.com/stSoftwareAU/VibeCoder) fleet scans
expect, so adopting it into VibeCoder does not open a backlog of
best-practice issues.

It ships a small working crate — a library (`src/lib.rs`) with a typed
error, and a command-line binary (`src/main.rs`) that calls it — so every
gate has real code to check from the first commit. Replace it with your own.

## Using this template

1. Create the repository from this template on GitHub (**Use this
   template**), with `Develop` as the default branch.
2. Rename the crate. In `Cargo.toml`, set `name`, `description` and
   `repository`. Then replace `template_rust` in `src/main.rs` and
   `tests/public_api.rs` with the new library name (hyphens become
   underscores), and `template-rust` in the binary's usage message.
3. Replace the example `greet` API and its tests with your own code.
4. Rewrite this README for the new project. Keep the
   [Build and quality gate](#build-and-quality-gate) section.
5. If the binary will be copied to another machine, published as a crate or
   built for `wasm32`, delete `.cargo/config.toml` (see
   [Build profiles](#build-profiles)).
6. Copy the template's GitHub settings onto the new repository:
   `scripts/apply-repo-settings.sh stSoftwareAU/<name>` (see
   [Repository settings](#repository-settings)).
7. Add the repository to VibeCoder (an `add-repo: stSoftwareAU/<name>`
   issue).

## Usage

```bash
cargo run -- Ada          # prints "Hello, Ada!"
cargo run -- ""           # prints an error on stderr and exits 1
```

The binary takes exactly one argument. A wrong argument count exits with
code 64, the usage-error code from
[`sysexits.h`](https://man.freebsd.org/cgi/man.cgi?query=sysexits).

The library's public API is `template_rust::greet`, `GreetError` and
`MAX_NAME_CHARS`. Run `cargo doc --open` for its documentation.

## Build and quality gate

Prerequisites:

- Rust via [rustup](https://rustup.rs). `rust-toolchain.toml` pins the
  compiler, so rustup installs the right version on first use.
- `shellcheck`, `codespell`, `markdownlint-cli2`, `actionlint`, `jq`,
  `cargo-deny` and `cargo-audit` for the full gate. `./quality.sh` names the
  install command for any that are missing.

```bash
cargo build                # debug build
cargo build --release      # optimised build
cargo test                 # unit, public-API and doc tests
cargo clippy --all-targets -- -D warnings   # lint
cargo fmt --all            # format
./quality.sh               # the full local gate; run it before every push
```

`./quality.sh` runs these checks, and every one must pass:

1. `bash -n` and ShellCheck on every shell script.
2. The script tests.
3. codespell.
4. markdownlint.
5. actionlint.
6. `cargo fmt --check`, Clippy and `cargo check`.
7. The tests and the documentation build.
8. `cargo deny` and `cargo audit`.

CI runs the same checks on every pull request into `Develop`, `main` or
`milestone/*`.

### Code style

- **Formatting.** `rustfmt` with default settings.
- **Lints.** The `[workspace.lints]` tables in `Cargo.toml` deny:
  - every warning;
  - `unsafe_code` and missing documentation;
  - `unwrap()` and `expect()` outside tests;
  - `dbg!` and `todo!`.
- **Tests.** `clippy.toml` relaxes the `unwrap()`, `expect()` and print
  lints inside tests only.
- **Coding and test standards.** See
  [CONTRIBUTING.md](./CONTRIBUTING.md), including **Unit, Integration and
  Benchmark Tests**.

### Build profiles

- **`[profile.dev]`** uses `debug = "line-tables-only"`, which keeps rebuilds
  fast.
- **`[profile.release]`** uses `opt-level = 3`, `lto = "fat"` and
  `codegen-units = 1`, which gives the fastest artefact.
- **`.cargo/config.toml`** adds `-C target-cpu=native` for builds on the
  machine that runs the binary. `./quality.sh` and CI set `RUSTFLAGS`, which
  replaces it, so their builds stay portable.

## Branches and pull requests

- **Branches.** Branch from `Develop`, as
  `<type>/<issue-number>-<short-slug>`.
- **Pull requests.** Open the pull request back into `Develop`, or into a
  `milestone/<slug>` branch for staged work.
- **Commits.** Reference the issue, e.g. `Fix: reject empty names
  (Issue #42)`.

[CONTRIBUTING.md](./CONTRIBUTING.md) has the full conventions.

## CI workflows

| Workflow | What it checks |
| --- | --- |
| `cargo-quality.yml` | Formatting, Clippy, `cargo check`, tests, docs and `cargo deny`. A `quality` job aggregates the results. |
| `cargo-audit.yml` | [RustSec](https://rustsec.org/) advisories, on every PR and weekly. |
| `cargo-upgrade.yml` | Weekly `cargo update` pull request. Holds back versions under 24 hours old. |
| `shellcheck.yml` | `bash -n` and ShellCheck. |
| `markdown-lint.yml` | markdownlint and codespell. |
| `actionlint.yml` | Workflow YAML lint. |
| `gitleaks.yml` | Secret scanning of the PR's commits. |
| `semgrep.yml` | [Static application security testing (SAST)](https://en.wikipedia.org/wiki/Static_application_security_testing). |
| `codeql.yml` | GitHub CodeQL scanning. Public repositories only. |
| `dependency-review.yml` | New-dependency vulnerability and licence review. Public repositories only. |
| `sbom.yml` | [Software Bill of Materials (SBOM)](https://en.wikipedia.org/wiki/Software_bill_of_materials) in CycloneDX format, uploaded as an artefact. |

Every third-party action is pinned to a full commit SHA, with its version in
a trailing comment. Downloaded binaries are pinned by version and SHA-256.
Dependabot (`.github/dependabot.yml`) and Renovate (`renovate.json`) keep
the pins and crates current. Both quarantine new external releases; see
[SECURITY.md](./SECURITY.md).

## Repository settings

GitHub's **Use this template** copies files only, so a new repository starts
without this template's settings. `scripts/apply-repo-settings.sh` reads
them from this repository and applies them to the new one:

- **Merge options:** squash and merge commits, no rebase merging,
  auto-merge, update-branch, and delete-branch-on-merge.
- **Rulesets:** these mirror the NEAT-AI repositories.
  - **`Develop`** requires a pull request with one approval, squash merging,
    and up-to-date status checks.
  - **`milestone/**`** requires the same status checks, without the
    up-to-date rule.
  - **Both** block deletion and force-pushes; repository admins may bypass.
  - **Required checks:** `quality`, `audit`, `shellcheck`, `markdownlint`,
    `spelling`, `actionlint`, `gitleaks`, `semgrep` and `sbom`.
- **Labels:** the fleet's workflow labels, such as `work-on`, `planning`,
  `needs-human` and `severity:*`.
- **Actions:**
  - only GitHub-owned actions plus an allow-list of the third-party actions
    the workflows use;
  - full-length commit SHA pins required;
  - read-only default workflow token, which cannot approve pull requests.
- **Security:**
  - Dependabot alerts and security updates;
  - secret scanning with push protection;
  - private vulnerability reporting, on public repositories.

Secrets cannot be copied, because their values are not readable. Add
`GITLEAKS_LICENSE`, `SEMGREP_APP_TOKEN` and `ACTIONS_PUSH` to the new
repository where the organisation does not already provide them. Gitleaks
falls back to the open-source CLI when it has no licence.

When you add a workflow job that should gate merges, add its name to both
rulesets here first, then re-run the script on each derived repository.

## Layout

| Path | Purpose |
| --- | --- |
| `src/lib.rs` | Library: the public API and its unit tests. |
| `src/main.rs` | Binary: parses arguments, calls the library, maps errors to an exit code. |
| `tests/` | Public-API tests, run in process. |
| `scripts/` | Shell helpers (quarantined `cargo update`, spelling, repository settings) and their tests. |
| `.github/` | Workflows, the shared `setup-rust` action, Dependabot and CODEOWNERS. |

## Licence

[Apache-2.0](./LICENSE).
