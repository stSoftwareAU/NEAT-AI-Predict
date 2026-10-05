# PR summary — Issue #29: Bash: missing sourced/called script `scripts/=`

## Change

- `scripts/test-script-refs.sh` (new) scans every tracked `*.sh` for
  `source`/`.` and `./scripts/…`/`scripts/…`/`bash scripts/…` references and
  fails on any path that does not exist (`MISSING`) or still holds an
  unexpanded variable (`UNRESOLVED`). Self-tests prove the scanner first.
- `scripts/test-cargo-update-quarantined.sh` keeps its Cargo.lock fixture
  `source = "git+…"` line in a variable, so no line starting with `source`
  reads like a source command — the false `scripts/=` reference in #29.
- `scripts/test-script-refs.sh` writes its own fixture `source …` and
  `./scripts/…` lines through variables for the same reason.
- `quality.sh` runs the new test; `README.md` lists it under the gate.

**Docs sweep** — grep: `test-script-refs`, `test-cargo-update-quarantined`, "Script tests", `quality.sh`, "sourced", "called script", `source =` (in `README.md`, `CONTRIBUTING.md`, `AGENTS.md`, `SECURITY.md`; no `docs/` manual and no `*/README.md` exist); section: `README.md#build-and-quality-gate`; updated: `README.md` — step 2 of the gate list names `scripts/test-script-refs.sh`, and the "CI runs the same checks" sentence now says no workflow runs `scripts/test-cargo-update-quarantined.sh` or `scripts/test-script-refs.sh` (only `./quality.sh` does), which this change had made more false

**Branch outcomes:**

Line numbers are for `scripts/test-script-refs.sh` as it stands in the
working tree (the uncommitted change the worker commits after this
summary). Every flip was made in a
throwaway copy of the repository, running `./scripts/test-script-refs.sh`.

- `scripts/test-script-refs.sh:38` — `source`/`.` form matched — `scripts/test-script-refs.sh` self-tests (b) "sourcing a missing sibling", (c) "Cargo.lock heredoc fixture line (issue #29 shape)", (d) "an unresolved variable path" — flipped the regex so it never matches, all three went red
- `scripts/test-script-refs.sh:47` — repo-root call form, resolved against ROOT — `scripts/test-script-refs.sh` self-test (f) "a repo-root call-form reference that is missing" — flipped the pattern so it never matches, test went red
- `scripts/test-script-refs.sh:53` — absent (no reference on the line) → skipped — `scripts/test-script-refs.sh` self-test (a) "sourcing an existing sibling" — flipped to "check anyway", test went red (shebang line reported MISSING)
- `scripts/test-script-refs.sh:68` — absolute path kept as is, relative path prefixed with its base — `scripts/test-script-refs.sh` self-test (a) "sourcing an existing sibling" — flipped so absolute paths get prefixed too, test went red
- `scripts/test-script-refs.sh:89` — error (unexpanded `$variable` left) → UNRESOLVED — `scripts/test-script-refs.sh` self-test (d) "an unresolved variable path" — flipped the `$` check, test went red (got MISSING instead)
- `scripts/test-script-refs.sh:94` — error (resolved path absent) → MISSING; success (path exists) → no output — self-tests (a), (b), (c) — inverted the `-f` test, all three went red
- `scripts/test-script-refs.sh:110` — `assert_no_problems` failure arm — reached by self-test (a) when line 53 is flipped (prints `FAIL: sourcing an existing sibling`) — went red as above
- `scripts/test-script-refs.sh:122` — `assert_problem` "expected a problem, got none" arm — reached by self-tests (b)–(d) when line 38 is flipped — with its failure counter neutralised as well, the script went green, so this arm is what turns it red
- `scripts/test-script-refs.sh:127` — `assert_problem` wrong kind or token arm — reached by self-test (d) when line 89 is flipped — with its failure counter neutralised as well, the script went green, so this arm is what turns it red
- `scripts/test-script-refs.sh:218` — error (any self-test failed) → exit 1 before the repo scan — with line 38 flipped and this check neutralised, the script went green, so this exit is what turns it red
- `scripts/test-script-refs.sh:231` — error (`git ls-files '*.sh'` empty) → exit 1 — no automated test; checked by hand by running the script in an empty git repository (printed "found no tracked shell scripts", exit 1)
- `scripts/test-script-refs.sh:238`/`:243` — error (a real tracked script has a MISSING/UNRESOLVED reference) → exit 1; success → "all sourced and called script paths exist" — the repo-wide scan in `scripts/test-script-refs.sh`, run by `quality.sh` — restoring the literal `source = "git+…"` line in `scripts/test-cargo-update-quarantined.sh` went red (`MISSING: scripts/test-cargo-update-quarantined.sh:66 -> =`, the #29 bug); restoring the literal fixture lines in `scripts/test-script-refs.sh` (as committed at `c9ec493`) went red; with line 243 neutralised as well, the script went green
- `scripts/test-script-refs.sh:33` — comment line → skipped — self-test (e) "a commented-out source line" — **flipped, stayed green**: the line 38 regex and the line 47 first-word check already ignore a line that starts with `#`, so this guard is defence in depth that no test reaches on its own
- `scripts/test-script-refs.sh:44` — `bash scripts/<name>` call form → the second word is the command — **no test reaches it**: no self-test uses the `bash` prefix and no tracked script calls a script that way; flipped, stayed green
- `scripts/test-script-refs.sh:88` — empty record → skipped — **unreachable**: `script_refs` never prints an empty line; flipped, stayed green
- `quality.sh:48` — `./scripts/test-script-refs.sh` exits non-zero → `set -euo pipefail` stops the gate — reached by every red case above; not flipped on its own (it adds no condition, only a step)
- `scripts/test-cargo-update-quarantined.sh:31` — no new branch (a fixture value moves into a variable)

## Notes for the reviewer

- The commit at the branch head (`c9ec493`) still holds literal fixture
  `source …`/`./scripts/…` lines in `scripts/test-script-refs.sh`, which its
  own repo-wide scan reports as MISSING/UNRESOLVED. The uncommitted
  working-tree change, which the worker commits, routes them through variables, and the script
  then passes (`test-script-refs: all sourced and called script paths exist`).
- Gaps in the list above: line 44 (`bash` prefix) has no test, and line 33
  (comment guard) is not reached by any test on its own.
