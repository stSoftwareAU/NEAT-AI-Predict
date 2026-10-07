# PR summary — Issue #29: Bash: missing sourced/called script `scripts/=`

## Summary

Closes #29

- `scripts/test-cargo-update-quarantined.sh` keeps its Cargo.lock fixture
  `source = "git+…"` line in a variable (`git_source`). No line in the script
  starts with `source` unless it is a real source command, so the false
  `scripts/=` reference is gone.
- `scripts/test-script-refs.sh` (new) is the static guard the issue asks for.
  It scans every tracked `*.sh` for `source`/`.` and
  `./scripts/…`/`scripts/…`/`bash scripts/…` references. It fails on a path
  that does not exist (`MISSING`) or that still holds an unexpanded variable
  (`UNRESOLVED`). Self-tests (a)–(g) check the scanner before it scans the
  repository.
- `quality.sh` runs the new test, and `README.md` lists it in the gate.

## Spec

### Intent and Rationale

The missing-script audit reads any line that starts with `source` as a
source command. A heredoc line in the Cargo.lock fixture,
`source = "git+…"`, looked like `source =`, so the audit reported a missing
script called `scripts/=`. Moving that line into a variable removes the false
match. The new guard fails the local gate if a real sourced or called path
ever goes missing, as in the FLEET Discovery outage where a deleted helper
was still being `source`d.

### Essential Design Decisions

- The guard is a repo-local Bash script run by `quality.sh`, not a shared
  action, so the repository owns its own gate.
- `$SCRIPT_DIR`/`${SCRIPT_DIR}` are expanded to the script's own directory.
  Any other variable left in a path is reported as `UNRESOLVED` rather than
  skipped, because a quiet skip would hide a broken reference.
- Fixture lines inside `scripts/test-script-refs.sh` are also written through
  variables. The repo-wide scan then also covers the scanner's own file.

### Undiscoverable Facts

- The fleet audit that filed #29 matches any line that starts with `source`,
  heredoc bodies included. That is why a data line inside a heredoc was
  reported.

## Evidence

This is a script-only change; there is no visual surface, so there is no
screenshot.

- `./scripts/test-script-refs.sh` → `test-script-refs: all sourced and called script paths exist` (exit 0).
- Red without the fix: restoring the literal `source = "git+…"` heredoc line
  in `scripts/test-cargo-update-quarantined.sh` gives
  `MISSING: scripts/test-cargo-update-quarantined.sh:66 -> =` and exit 1.
  That is the #29 reference, at its line number on base.
- The full `./quality.sh` gate passed: `All quality checks passed.`, exit 0.

**Docs sweep** — grep: `git grep -n -e 'test-script-refs' -e 'test-cargo-update-quarantined' -e 'Script tests'` over `*.md`, `*.sh` and `*.rs` (the PR summaries are excluded); section: `README.md#build-and-quality-gate`; updated: `README.md` (gate item 2 names `scripts/test-script-refs.sh`; the CI sentence says no workflow runs `scripts/test-cargo-update-quarantined.sh` or `scripts/test-script-refs.sh`, so only `./quality.sh` does). Hits outside the diff:

- `quality.sh:44` — still true because the `Script tests` step still runs the script tests, and the new step follows it.
- `quality.sh:45` — still true because `./scripts/test-cargo-update-quarantined.sh` still exists and still runs as part of that step.
- `scripts/test-cargo-update-quarantined.sh:2` — still true because the file is still hermetic unit tests for the helper; only a fixture line moved into a variable.
- `scripts/test-cargo-update-quarantined.sh:9` — still true because the usage line is unchanged.
- `scripts/test-cargo-update-quarantined.sh:121` — still true because the failure message is unchanged and still printed when an assertion fails.
- `scripts/test-cargo-update-quarantined.sh:124` — still true because the success message is unchanged.

Every hit in `README.md` (`:219`, `:229`, `:230`) and in
`scripts/test-script-refs.sh` (new file) is inside this diff.

## Test Plan

- [x] `shellcheck -x --severity=style` and `bash -n` are clean on every
      changed script.
- [x] `./scripts/test-script-refs.sh` passes on the head, including
      self-tests (a)–(g).
- [x] `./scripts/test-cargo-update-quarantined.sh` passes.
- [x] `./quality.sh` passes.

**Branch outcomes:**

Line numbers are for `scripts/test-script-refs.sh` at the head. Every flip
was made in a throwaway copy of the repository, with
`./scripts/test-script-refs.sh` run after each one.

- `scripts/test-script-refs.sh:38` — `source`/`.` form matched — self-tests (b) "sourcing a missing sibling", (c) "Cargo.lock heredoc fixture line (issue #29 shape)", (d) "an unresolved variable path" in `scripts/test-script-refs.sh` — I flipped the regex so it never matches, and all three went red.
- `scripts/test-script-refs.sh:44` — `bash scripts/<name>` call form, where the second word is the command — self-test (g) "a bash-prefixed call-form reference that is missing" in `scripts/test-script-refs.sh` — flipped, went red (`FAIL: a bash-prefixed call-form reference that is missing: expected a problem, got none`, exit 1).
- `scripts/test-script-refs.sh:47` — repo-root call form, resolved against ROOT — self-test (f) "a repo-root call-form reference that is missing" and self-test (g) — flipped the pattern so it never matches; both went red.
- `scripts/test-script-refs.sh:53` — absent (no reference on the line) → skipped — self-test (a) "sourcing an existing sibling" — flipped to "check anyway"; it went red because the shebang line was reported MISSING.
- `scripts/test-script-refs.sh:68` — an absolute path is kept as it is, and a relative path gets its base prefixed — self-test (a) — flipped so absolute paths are prefixed too; it went red.
- `scripts/test-script-refs.sh:89` — error (an unexpanded `$variable` is left) → UNRESOLVED — self-test (d) — flipped the `$` check; it went red with MISSING instead of UNRESOLVED.
- `scripts/test-script-refs.sh:94` — error (resolved path absent) → MISSING; success (path exists) → no output — self-tests (a), (b), (c) — inverted the `-f` test, all three went red.
- `scripts/test-script-refs.sh:110` — `assert_no_problems` failure arm — reached by self-test (a) when line 53 is flipped (prints `FAIL: sourcing an existing sibling`) — went red as above.
- `scripts/test-script-refs.sh:122` — `assert_problem` arm for "expected a problem, got none" — reached by self-tests (b)–(d) when line 38 is flipped. When its failure counter was also neutralised, the script went green, so this arm is what turns the run red.
- `scripts/test-script-refs.sh:127` — `assert_problem` arm for the wrong kind or token — reached by self-test (d) when line 89 is flipped. When its failure counter was also neutralised, the script went green, so this arm is what turns the run red.
- `scripts/test-script-refs.sh:235` — error (any self-test failed) → exit 1 before the repo scan — I flipped line 38 and also neutralised this check, and the script went green, so this exit is what turns the run red.
- `scripts/test-script-refs.sh:248` — error (`git ls-files '*.sh'` is empty) → exit 1 — no automated test. Checked by hand in an empty git repository: it printed "found no tracked shell scripts" and exited 1.
- `scripts/test-script-refs.sh:255`/`:260` — error (a real tracked script has a MISSING or UNRESOLVED reference) → exit 1; success → "all sourced and called script paths exist" — the repo-wide scan in `scripts/test-script-refs.sh`, run by `quality.sh` — restoring the literal `source = "git+…"` line went red (`MISSING: scripts/test-cargo-update-quarantined.sh:66 -> =`). When line 260 was also neutralised, the script went green.
- `scripts/test-script-refs.sh:33` — comment line → skipped — self-test (e) "a commented-out source line" — **flipped, stayed green**. The line 38 regex and the line 47 first-word check already ignore a line that starts with `#`, so this guard is defence in depth that no test reaches on its own.
- `scripts/test-script-refs.sh:88` — empty record → skipped — **unreachable**, because `script_refs` never prints an empty line. Flipped, stayed green.
- `quality.sh:48` — when `./scripts/test-script-refs.sh` exits non-zero, `set -euo pipefail` stops the gate — reached by every red case above. Not flipped on its own, because it adds a step, not a condition.
- `scripts/test-cargo-update-quarantined.sh:31` — no new branch; a fixture value moved into a variable.

## Security self-check

No new input surface, secrets, network calls or dependencies. The scanner
only reads tracked `*.sh` files listed by `git ls-files` and checks paths
with `-f`.
