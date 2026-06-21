//! Local IPC — `LocalListener` / `LocalStream` newtypes hiding the
//! Win Named Pipe / Unix Domain Socket split.
//!
//! Skeleton (P0-PAL-1). Real implementation lands in P0-PAL-7
//! (interprocess crate backend, default feature `ipc-interprocess`) with the
//! hand-rolled `ipc-native` rollback path tracked as a feature flag (see
//! ARCH §4.5).
//!
//! Public API will be:
//!
//! ```ignore
//! pub struct LocalSocketName(/* opaque */);
//! impl LocalSocketName {
//!     pub fn for_daemon(user_scope: &str) -> Self;
//!     pub fn to_discovery_string(&self) -> String;
//! }
//!
//! pub struct LocalListener { /* tokio-friendly */ }
//! impl LocalListener {
//!     pub async fn bind_for_current_user(name: &LocalSocketName) -> std::io::Result<Self>;
//!     pub async fn accept(&mut self) -> std::io::Result<LocalStream>;
//! }
//!
//! pub struct LocalStream { /* tokio AsyncRead+AsyncWrite */ }
//! impl LocalStream {
//!     pub async fn connect(name: &LocalSocketName) -> std::io::Result<Self>;
//! }
//!
//! pub fn cleanup_stale(name: &LocalSocketName) -> std::io::Result<()>;
//! ```
//!
//! Internal split: `windows.rs` (named-pipe name `\\.\pipe\loom-daemon-<sid>`,
//! DACL → current user), `unix.rs` (socket file at `<config>/daemon/daemon.sock`,
//! permissions 0600, stale-socket cleanup).
