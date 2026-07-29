# Contributing to Loom

Thanks for your interest in contributing. This document covers the development
setup and the workflow we expect for pull requests.

## Development setup

- **Rust**: a stable toolchain, as pinned in `rust-toolchain.toml`.
- **Node.js 22+ and pnpm**: only needed for the desktop GUI (`apps/gui-web`).
- **make**: used for the common entry points below. On Windows you can run the
  underlying `cargo` / `pnpm` commands directly; see
  [docs/windows-build-guide.md](docs/windows-build-guide.md).

```bash
make build                       # build server, CLI, daemon
pnpm --dir apps/gui-web install  # GUI dependencies (first time only)
make gui-dev                     # run the desktop app in dev mode
```

## Before you open a PR

Run the same checks CI runs:

```bash
make fmt    # rustfmt
make lint   # clippy (workspace lints are deny-level)
make test   # cargo test across default workspace members
```

The full local matrix (fmt + clippy + tests + frontend lint/build) is:

```bash
scripts/ci/run-matrix.sh        # macOS / Linux
scripts/ci/run-matrix.ps1       # Windows (PowerShell)
```

You can install the repository pre-push hook to run that matrix automatically
before every push:

```bash
git config core.hooksPath .githooks
```

## Pull requests

- Keep PRs focused: one change per PR, with a clear description of what and why.
- Target day-to-day feature and fix pull requests at `preview`.
- Pull requests into `main` must come from a same-repository release branch
  named `preview-X.Y.Z` or from a branch under `hotfix/`.
- Link the related issue when there is one.
- Follow the commit style used in the project history (conventional prefixes
  such as `feat:`, `fix:`, `docs:`, `refactor:`).
- Update documentation when you change user-visible behavior, configuration,
  or the JSON-RPC protocol.
- CI must be green: Rust fmt/clippy/tests on Linux, macOS, and Windows, plus
  the frontend lint/build.

## Reporting bugs

Open an issue using the bug report template:
<https://github.com/wyw-ai/loom/issues/new/choose>

Include reproduction steps, your environment, and relevant logs. Please redact
private information from logs before posting.

## Security issues

Do **not** report vulnerabilities in public issues. See
[SECURITY.md](SECURITY.md) for the private reporting channel.

## License

By contributing, you agree that your contributions are licensed under the
[Apache License 2.0](LICENSE).
