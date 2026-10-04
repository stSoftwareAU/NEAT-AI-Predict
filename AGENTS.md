# Agent instructions

This file is a thin pointer for coding agents. Do not add standalone standards
here — put them in the documents below so people and agents read one copy.

- [README.md](./README.md) — what the project is, how to build, run and test
  it, and the single local gate (`./quality.sh`).
- [CONTRIBUTING.md](./CONTRIBUTING.md) — branch and PR conventions, coding
  standards, and the **Unit, Integration and Benchmark Tests** rules.
- [SECURITY.md](./SECURITY.md) — vulnerability reporting and the dependency
  quarantine policy.

Before declaring a change done, run `./quality.sh` and make it pass. Never
weaken a gate (lint level, test, workflow check) to make a change pass; fix the
cause or raise it with a maintainer.
