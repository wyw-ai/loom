# Local CI Matrix — Windows / macOS / Linux

> Status: **active** (iteration: Windows compatibility integration, P0-CI work unit)
> Owner: OPS
> Companion docs: ARCH design `art_33746e5783f9`, PM PRD `art_8dc5a0e6eaef`, OPS audit `msg_a0f348c04bce`

## TL;DR

Run the full local quality gate before any push or merge.

```bash
# macOS / Linux
bash scripts/ci/run-matrix.sh

# Windows (PowerShell)
.\scripts\ci\run-matrix.ps1
```

Enable the pre-push hook once per clone:

```bash
git config core.hooksPath .githooks
```

That's it. Read on for the why.

---

## Why an offline (three-platform) matrix?

1. **CI quota is exhausted.** GitHub Actions runner minutes for this org are
   currently capped; remote `cargo test` matrix is not available for the
   foreseeable iteration window.
2. **Remote `ci.yml` is Ubuntu-only.** Even when CI is healthy, the main
   gate at `.github/workflows/ci.yml` runs `cargo test` only on
   `ubuntu-latest`. Windows / macOS runners exist in `release.yml` and
   `gui-build.yml`, but only for build/bundle stages — they do **not**
   gate functional regressions.
3. **Founder hard constraint**: "all iterations local commits only, never
   push to remote." We cannot rely on a remote CI signal during this
   iteration, so the local matrix is the authoritative gate.
4. **PAL refactor needs regression protection.** Before BE collapses
   `cfg(windows)` scatter into `loom-platform`, every commit on at least
   one platform must pass `fmt + clippy + test` locally. The matrix script
   is that floor.

See the OPS audit report `msg_a0f348c04bce` (Windows audit baseline) for
the structural facts that motivated this work unit.

---

## What the matrix runs

Each script runs three stages in order. The first failing stage exits
non-zero and stops the script.

| stage      | default command                                                        | failure exit code |
|------------|------------------------------------------------------------------------|-------------------|
| fmt        | `cargo fmt --all -- --check`                                           | 2 |
| clippy-pal | `cargo clippy --workspace --all-targets -- -D clippy::disallowed_methods` (PAL guardrail; **always strict**, includes `loom-gui`) | 3 |
| clippy     | `cargo clippy --workspace --exclude loom-gui --all-targets` (informational by default; add `-D warnings` via `--strict-clippy` / `-StrictClippy`) | 3 |
| test       | `cargo test --workspace --exclude loom-gui --no-fail-fast`             | 4 |

Notes:

- The **`clippy-pal` PAL guardrail** runs across the full workspace
  (including `loom-gui`) with only `clippy::disallowed_methods` denied,
  so any new `std::process::Command::new` / `tokio::process::Command::new`
  regression in `gui/` is caught even though the broader baseline rewrite
  for `loom-gui` is still deferred (Iter#3 §8.9 Known Limitation). The two
  documented G1 OS-shell exemptions in `crates/gui` (`account.rs::open_browser`
  and `ipc.rs::open_path_with_system`) carry function-level
  `#[allow(clippy::disallowed_methods)]` per PM-Arbitration-003.
- `loom-gui` is excluded **from the broader `clippy` and `test` stages** because Tauri build is heavy and depends on
  native toolchains not available in every developer setup. GUI builds
  remain gated by `.github/workflows/gui-build.yml` on a separate cadence.
- `--all-targets` covers unit tests, integration tests, examples, and
  benches — examples and benches are not exercised by `cargo test` alone.
- **Clippy is informational by default.** The current baseline has 20
  pre-existing style lints (`manual_flatten`, `needless_borrow`,
  `useless_format`, etc., none correctness bugs) which the loom-platform
  iteration will fix. Use `--strict-clippy` / `-StrictClippy` to flip
  clippy to `-D warnings` (the target end-state); once BE/QA confirms the
  baseline is clean, this flag will become the default.

### Test target isolation

The scripts redirect `CARGO_TARGET_DIR` so the matrix build does not
contaminate the host project cache:

| platform | default `CARGO_TARGET_DIR`                          |
|----------|------------------------------------------------------|
| Windows  | `%LOCALAPPDATA%\loom-test\target`                   |
| macOS    | `~/Library/Caches/loom-test/target`                  |
| Linux    | `${XDG_CACHE_HOME:-$HOME/.cache}/loom-test/target`   |
| other    | `~/.loom-test/target`                                |

Override:

```bash
# unix
LOOM_MATRIX_TARGET_DIR=/path/to/dir bash scripts/ci/run-matrix.sh

# windows
$env:LOOM_MATRIX_TARGET_DIR='F:\custom\path'; .\scripts\ci\run-matrix.ps1
```

---

## Pre-push hook

Activate once per clone:

```bash
git config core.hooksPath .githooks
```

On `git push`, the hook detects the platform and runs the matching
matrix script. A failed stage blocks the push.

| env var                | effect                                                  |
|------------------------|---------------------------------------------------------|
| `LOOM_SKIP_PREPUSH=1`  | bypass the hook entirely (emergency escape hatch)       |
| `LOOM_PREPUSH_QUICK=1` | run matrix with `--quick` / `-Quick` (skip clippy)      |

> Reminder: the **primary** push discipline for this iteration is the
> organizational policy ("local commits only"). The hook is a tripwire,
> not the policy. Once we leave this iteration, the hook stays in place
> as ordinary belt-and-braces protection.

### Common bypass mistakes

- `git push --no-verify` bypasses the hook silently. Don't.
- Hooks are not active until you run `git config core.hooksPath .githooks`
  in the cloned repo. Each fresh clone needs the one-time setup.
- Symlinked hook directories on Windows can require admin / dev-mode; the
  in-tree `.githooks/` approach via `core.hooksPath` sidesteps that.

---

## Troubleshooting

### `cargo: command not found`

Install rustup → stable toolchain. On Windows, `cargo.exe` lives in
`%USERPROFILE%\.cargo\bin\`. Ensure that path is on `PATH` for the shell
the hook runs under (Git Bash inherits Windows `PATH`).

### `git: command not found` inside PowerShell

Default Git-for-Windows installation places `git.exe` at
`C:\Program Files\Git\bin\git.exe` but does not always add it to `PATH`.
Either re-run the installer with "Use Git from the command line", or add
`C:\Program Files\Git\cmd` to `$env:Path`.

### Tests are slow on first run

The matrix uses an isolated `CARGO_TARGET_DIR`, so the first run
re-compiles the workspace from cold. Subsequent runs are incremental.
To share the host project cache instead (faster but less isolated):

```powershell
$env:LOOM_MATRIX_TARGET_DIR = "$env:LOCALAPPDATA\joi-apps-temp\target"
.\scripts\ci\run-matrix.ps1
```

### Clippy is the slowest stage

Use `--quick` / `-Quick` during iteration. The pre-push hook still runs
the full matrix unless you also set `LOOM_PREPUSH_QUICK=1`.

### "Permission denied" on hook (Linux / macOS)

```bash
chmod +x .githooks/pre-push
```

Git on Windows does not require the executable bit.

### A platform-specific test fails locally but ARCH/BE says it passes elsewhere

That **is the whole point** of this matrix — historically Windows / macOS
regressions slipped through Ubuntu-only CI. File a thread report with
the failing crate, target triple, and the failing test's name. Do not
disable the test.

---

## Relationship to the PAL refactor (`loom-platform` crate)

This matrix is **prerequisite** to the BE PAL refactor (per STRAT P0
sequencing). The order is:

1. **OPS (this work unit)**: matrix + hook + docs land first. Every
   subsequent commit on any developer machine can be validated against
   the same three-stage gate.
2. **BE P0-PAL-1**: introduces `crates/loom-platform` with the agreed
   `path` / `process` / `signal` / `fs` / `ipc` / `console` / `env` /
   `time` submodules (ARCH D2).
3. **BE P0-PAL-N**: migrates `cfg(windows)` callers into the new crate
   one subsystem at a time. Each migration commit must keep the matrix
   green on at least one platform; cross-platform parity is verified by
   running the matrix on every supported OS before declaring a subsystem
   "migrated".
4. **`crates/windows-console` is folded into `loom_platform::console`**
   (ARCH D2 workspace -1). After that fold, the matrix's `--exclude
   loom-gui` remains the only carve-out.

Until then, the matrix preserves the **685-tests-green baseline** that
OPS established in the audit report (`msg_a0f348c04bce`, §4).

---

## Known limitations

1. **Single-platform per run.** Each invocation runs the matrix for the
   host platform only — Windows developers cannot validate macOS / Linux
   parity from one machine. That is the irreducible cost of running
   offline; remote GHA matrix would be required for true triple-coverage
   in one shot, and remote CI is currently out-of-scope per PRD §2.
2. **`loom-gui` is excluded from the broader matrix.** Tauri's GUI bundle
   is left to `.github/workflows/gui-build.yml` for build/test, and the
   broad `clippy --exclude loom-gui` stage skips it. The new `clippy-pal`
   guardrail stage (added in Iteration #4) DOES include `loom-gui` for
   `clippy::disallowed_methods`, so PAL regressions in the GUI are caught
   locally even though style-warning rewrites remain deferred.
3. **`--all-targets` is on but `cargo bench` is not.** Benches compile
   under clippy/test but are not executed (Criterion benches would
   blow up matrix runtime). Add explicit bench runs to this doc when
   needed.
4. **Hook is opt-in.** A fresh clone has no protection until
   `git config core.hooksPath .githooks` is run. The repo-level setting
   `init.templateDir` could enforce this org-wide but is out of scope
   for OPS this iteration (would need ARCH/Founder sign-off on changing
   per-developer git config).
5. **The Windows matrix relies on Git for Windows' bash for the hook.**
   Developers on `cmd.exe` without Git Bash will get no pre-push gate;
   they should run `scripts\ci\run-matrix.ps1` manually.
