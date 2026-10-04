# Security Policy

This document describes how to report a security vulnerability in this
repository and what to expect once you do. It follows
[GitHub's guidance on adding a security policy](https://docs.github.com/en/code-security/getting-started/adding-a-security-policy-to-your-repository),
so GitHub surfaces it in the repository's **Security** tab.

## Reporting a vulnerability

**Please do not open a public issue for security vulnerabilities.** A public
issue discloses the problem before a fix exists and puts every consumer of the
code at risk.

Use one of the private channels below instead:

1. **GitHub private vulnerability reporting (preferred).** Open the
   repository's **Security** tab and choose **Report a vulnerability** to start
   a private advisory visible only to you and the maintainers. See
   [Privately reporting a security vulnerability](https://docs.github.com/en/code-security/security-advisories/guidance-on-reporting-and-writing-information-about-vulnerabilities/privately-reporting-a-security-vulnerability).
2. **Email.** If you cannot use GitHub private reporting, email
   <security@stsoftware.com.au> with the details, naming this repository in
   the subject line.

Whichever channel you choose, please include as much of the following as you
can so we can reproduce and triage quickly:

- a description of the vulnerability and its impact;
- the affected component, file, or command;
- step-by-step reproduction instructions;
- any proof-of-concept input, logs, or stack traces;
- the commit SHA or branch you tested against.

## Response targets

We aim to honour the following timeline. These are targets, not contractual
guarantees, and they are measured in business days.

| Stage                       | Target                                |
| --------------------------- | ------------------------------------- |
| Acknowledge your report     | Within 3 business days                |
| Initial assessment / triage | Within 10 business days               |
| Fix or mitigation plan      | Communicated after triage             |
| Public disclosure           | Coordinated with you once a fix lands |

We will keep you informed of progress and coordinate the timing of any public
disclosure with you. Please give us a reasonable opportunity to remediate
before disclosing publicly.

## Supported versions

Security fixes are applied to the active development branch only.

| Version            | Supported          |
| ------------------ | ------------------ |
| `Develop` (latest) | :white_check_mark: |
| Older commits      | :x:                |

Always update to the latest commit on `Develop` to receive security fixes.

## Automated safeguards

Every pull request runs `cargo audit` ([RustSec](https://rustsec.org/)
advisories), `cargo deny` (licences, sources and advisories), dependency
review, CodeQL, Semgrep and Gitleaks secret scanning; `cargo audit` also runs
weekly so an advisory published after the last merge still surfaces.
Dependabot registers the Cargo and GitHub Actions ecosystems so a newly
published advisory raises a security update pull request without waiting for
the next change.

## Dependency quarantine and emergency bumps

Newly published external releases are quarantined for at least 24 hours
before a routine update proposes them: `renovate.json` sets
`minimumReleaseAge` to `24 hours` and `.github/dependabot.yml` sets a 7-day
`cooldown`. Internal `stSoftwareAU/*` packages are exempt and update
immediately. Security updates are not delayed by either setting.

When a vulnerability is under active exploitation and the fix is younger than
the quarantine window, a maintainer may bypass the quarantine for that one
package — by merging the security update pull request directly, or by adding a
temporary `packageRules` entry with `"minimumReleaseAge": false` for that
package. Record the bypass and its reason in the pull request, and remove the
temporary rule once the window has passed.

## Escalation

Advisories reach the maintainers (`@stSoftwareAU/developers`, per
`.github/CODEOWNERS`) through Dependabot security alerts and pull requests,
failing required CI checks on every pull request, and the private reporting
channels above.
