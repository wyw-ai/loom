# Testing Standards

> **Authority**: OPS (@actor_agent_ops_cf4161ee) — Operations & Knowledge Infrastructure
> **Status**: Active
> **Last Updated**: 2026-06-19

## Overview

This document defines testing standards for the Loom project. All changes must pass the applicable test gates before merge.

---

## 1. Test Isolation — Production vs Test Environment

### Critical Rule

**Never run tests against the production Loom environment.** Production environment corruption can prevent actors from functioning, causing catastrophic workflow failure.

### Test Environment Setup

| Component | Production | Test |
|-----------|-----------|------|
| Data directory | `~/.loom/` | `~/.loom-test/` |
| Server port | 7878 | 7879 |
| Database | `~/.loom/data/` | `~/.loom-test/data/` |
| Config | `~/.loom-apps/desktop.toml` | `~/.loom-test-apps/desktop.toml` |
| Workspace base | `~/.loom-apps/tmp/` | `~/.loom-test-apps/tmp/` |

### Setup Commands (Windows)

```powershell
# Create isolated test directory
mkdir F:\pj\loom-test

# Clone repo for testing
git clone <repo-url> F:\pj\loom-test\joi-apps-temp
cd F:\pj\loom-test\joi-apps-temp
git checkout <test-branch>

# Set test environment variables
$env:LOOM_HOME = "F:\pj\loom-test\.loom"
$env:LOOM_SERVER = "ws://127.0.0.1:7879/rpc"
```

### Setup Commands (Unix)

```bash
# Create isolated test directory
mkdir -p ~/loom-test

# Clone repo for testing
git clone <repo-url> ~/loom-test/joi-apps-temp
cd ~/loom-test/joi-apps-temp
git checkout <test-branch>

# Set test environment variables
export LOOM_HOME=~/loom-test/.loom
export LOOM_SERVER=ws://127.0.0.1:7879/rpc
```

---

## 2. Test Gates

### Pre-Commit (Developer)

```bash
cargo check --workspace
cargo fmt --check
cargo clippy --workspace -- -D warnings
```

### Unit Tests

```bash
cargo test -p loom-server
cargo test -p loom-cli
cargo test -p agent-runtime
cargo test -p proto
```

### Integration Tests

```bash
cargo test -p loom-server --test '*'
cargo test -p loom-cli --test '*'
cargo test -p agent-runtime --test '*'
```

### Full Test Suite

```bash
cargo test --workspace
cargo build --workspace
```

### Test-Specific Module Tests

```bash
# Thread routing tests
cargo test -p loom-cli agent_serve

# UNC path tests (Windows)
cargo test -p agent-runtime unc_path

# Daemon IPC tests
cargo test -p loom-cli daemon_ipc

# Windows spawn tests
cargo test -p agent-runtime windows_spawn
```

---

## 3. Test Categories

### Unit Tests

- Scope: single function or module.
- Location: `#[cfg(test)] mod tests` within source files.
- Mock: external dependencies (file system, network) must be mocked.

### Integration Tests

- Scope: cross-crate or end-to-end behavior.
- Location: `tests/` directory within each crate.
- Environment: may use real file system; must use temporary directories (`tempfile`).

### Loom-Specific Tests

- **Loom server tests**: require a running loom-server instance on test port. Must use isolated data directory.
- **Loom daemon tests**: require a running loom-server. Must use test-only machine config.
- **Loom end-to-end tests**: full agent lifecycle test. This is the riskiest category — must use fully isolated test environment.

---

## 4. Test Environment Management

### Before Testing

1. Verify production environment is NOT affected:
   ```bash
   # Check production server is not on test port
   curl http://127.0.0.1:7879/health || echo "Port 7879 is free (good)"
   ```
2. Set up test environment with isolated directories and ports.
3. Verify no production data directories are referenced.

### During Testing

1. Monitor test output for any references to production paths.
2. Stop immediately if a test touches production data.

### After Testing

1. Stop all test server/daemon processes.
2. Optionally clean up test data directory.
3. Verify production environment is intact:
   ```bash
   loom --server ws://127.0.0.1:7878/rpc actor list
   ```

---

## 5. CI/CD Testing (GitHub Actions)

> **Note**: GitHub Actions quota is currently exhausted. All CI tests must be run locally until quota is restored.

### What CI Normally Runs

| Workflow | Trigger | Tests |
|----------|---------|-------|
| `release.yml` | Push to main, tag | `cargo test --workspace`, `cargo build --release` |
| `gui-build.yml` | Push, PR | GUI frontend build, Tauri bundle |

### Local CI Simulation

```bash
# Simulate CI release check
cargo check --workspace
cargo test --workspace
cargo build --workspace
cargo fmt --check
git diff --check
```

---

## 6. Test Writing Guidelines

### For New Features

- Unit tests for all new functions with >1 code path.
- Integration tests for cross-crate behavior.
- Platform-specific tests gated with `#[cfg(windows)]` / `#[cfg(unix)]`.

### For Bug Fixes

- Regression test that reproduces the bug.
- Verify the test fails before the fix and passes after.

### For Refactors

- Existing tests must continue to pass.
- No change in test count (unless intentionally adding/removing).

---

## 7. Known Test Quirks

### Windows-Specific

- `make_executable` creates `.cmd` files on Windows (not executable bits).
- File permissions tests behave differently (Windows has no Unix exec bit).
- UNC paths (`\\?\`) required for paths exceeding 260 characters.
- `tempfile` crate must be used as dev-dependency for temporary directory tests.

### Cross-Platform

- Symlink tests are Unix-only.
- Daemon IPC socket tests are Unix-only (IPC is no-op on Windows).
- Process spawn tests need platform-specific flags (`CREATE_NO_WINDOW` on Windows).

---

## Known Limitations

1. **No automated CI testing**: GitHub Actions quota exhaustion means all tests are manual. Assessment: acceptable for small team; CI re-enablement is P1 follow-up.
2. **No performance/benchmark tests**: No criterion.rs benchmarks or load tests exist. Assessment: not needed at current scale; performance testing is future work for production deployment.
3. **No test coverage tracking**: No `cargo-tarpaulin` or codecov integration. Assessment: manual review of test coverage per PR is sufficient at current scale.
4. **Loom daemon integration tests are fragile**: Tests that require a running loom-server on a specific port can conflict with other tests or running instances. Assessment: port isolation (7879 for test) mitigates but does not eliminate risk.
5. **No GUI test automation**: No WebDriver or Tauri-specific GUI tests exist. Assessment: GUI complexity is low; manual verification suffices for now.
