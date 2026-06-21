//! Signal handling — process-wide shutdown hooks + child-process signalling.
//!
//! Skeleton (P0-PAL-1). Real implementation lands in P0-PAL-5.
//!
//! Public API will be:
//!
//! ```ignore
//! pub fn install_shutdown<F>(on_shutdown: F) -> std::io::Result<ShutdownReceiver>
//! where F: FnOnce() + Send + 'static;
//!
//! pub struct ShutdownReceiver { /* tokio::sync::watch::Receiver<bool> */ }
//! impl ShutdownReceiver {
//!     pub async fn recv(&mut self);
//!     pub fn is_triggered(&self) -> bool;
//! }
//! ```
//!
//! Internal split: `windows.rs` (ctrlc), `unix.rs` (tokio::signal + signal-hook).
