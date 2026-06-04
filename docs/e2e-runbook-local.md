# Local E2E Runbook

This document describes public local verification entry points for the Loom
workspace. Product-specific smoke tests and private deployment cutover notes
belong in the owning integration repository.

## Prerequisites

- Rust toolchain installed.
- Project dependencies installed as described by the root README or build
  scripts.
- Optional provider CLIs installed if running real-agent scenarios.
- No existing process is using the ports selected by the local e2e harness.

## Build

```bash
cargo build --workspace
```

For a release-style smoke:

```bash
cargo build --release --workspace
```

## Core Checks

Use the narrowest check that covers the change:

```bash
cargo fmt --check
cargo check --workspace
cargo test --workspace
```

If a change only touches one crate, prefer the relevant package-specific check
first, then run broader checks before release.

## E2E Harness

If the repository contains `scripts/e2e/run-all.sh`, run:

```bash
scripts/e2e/run-all.sh
```

If provider-backed flows are optional in your environment, use the harness
flag documented by the script itself to skip real model calls.

## Troubleshooting

- If the server fails to start, check whether the selected port is already in
  use and whether the data directory is writable.
- If an agent appears online but produces no events, inspect the agent host log
  and confirm the provider CLI is available on `PATH`.
- If artifacts are missing, verify the server data directory and the artifact
  publish/read RPC path.
- If a service emits duplicate events, inspect its cursor and dedupe state.

## Release Gate

Before publishing a public release, run:

- `cargo fmt --check`
- `cargo test --workspace`
- any documented frontend checks for `apps/gui-web`
- the public e2e harness if present and supported in the current environment
