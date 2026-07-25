# Security Policy

## Supported versions

Loom is pre-1.0. Only the **latest tagged release** receives security fixes;
older tags and intermediate commits are not patched.

| Version        | Supported |
| -------------- | --------- |
| latest tag     | yes       |
| older tags     | no        |

## Reporting a vulnerability

Please report vulnerabilities privately through GitHub:

<https://github.com/wyw-ai/loom/security/advisories/new>

(Security → Advisories → "Report a vulnerability" on the repository page.)

**Do not open a public issue for a vulnerability.**

A good report includes:

- a description of the issue and its impact,
- steps to reproduce or a proof of concept,
- the version or commit you tested against,
- any mitigation you are aware of.

We aim to acknowledge reports within a few days and to coordinate the fix and
disclosure timeline with you. We will credit reporters in the release notes
unless you prefer to remain anonymous.

## Scope notes

- `loom-server` and `loom-daemon` are designed to run on localhost / trusted
  networks. Issues that require the operator to expose these services to an
  untrusted network against the documented defaults may be treated as lower
  priority, but we still want to hear about them.
- Please do not test against installations you do not own, and do not access
  or modify other users' data while researching.
