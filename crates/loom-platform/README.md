# loom-platform

Workspace-level Platform Abstraction Layer (PAL) for the Loom multi-actor
runtime. Every cross-platform difference — path normalisation, process
spawning, signal handling, file locking, local IPC, console init, environment
helpers, monotonic time — is funneled through this crate so business crates
stay free of `#[cfg(windows)]` scatter.

## Modules

| Module    | Responsibility                                         | Status (P0-PAL-1) |
|-----------|--------------------------------------------------------|-------------------|
| `path`    | UNC prefix, normalisation, `create_dir_all`            | skeleton          |
| `process` | `Command` / `TokioCommand` newtypes with platform flags| skeleton          |
| `signal`  | `install_shutdown`, child signal delivery              | skeleton          |
| `fs`      | `FileLock` (fs4), atomic write helpers                 | skeleton          |
| `ipc`     | `LocalListener` / `LocalStream` (interprocess)         | skeleton          |
| `console` | Windows console init + double-click help dialog        | **populated**     |
| `env`     | Case-insensitive env helpers, temp dir wrappers        | skeleton          |
| `time`    | Monotonic time helpers                                 | skeleton          |

## Design rules

1. **Abstraction shape**: free functions + thin newtype wrappers. No
   `trait Platform` / `Box<dyn ...>`. See ARCH §2.3.
2. **Files**: when a function's cfg branch is ≤ 10 lines, keep both arms in
   the same `mod.rs`. Above that, split into `windows.rs` / `unix.rs` with
   identical `pub` signatures; `mod.rs` does `#[cfg(...)] pub use ...`.
3. **No runtime overhead**: all platform dispatch happens at compile time.
4. **The PAL is the only place `std::process::Command::new`,
   `tokio::process::Command::new`, raw `kill(2)`, raw `flock(2)`, etc. may
   appear**. The workspace clippy config enforces this in business crates;
   this crate's local `clippy.toml` lifts that restriction.

## Migration map

See `docs/` (when P1-KB-Archive lands) for the full module-by-module
migration table tracked against PRD §3.1.
