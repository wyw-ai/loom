//! Filesystem helpers — advisory `FileLock`, atomic write, future symlink stub.
//!
//! Skeleton (P0-PAL-1). Real implementation lands in P1-FS-Lock (fs4).
//!
//! Public API will be:
//!
//! ```ignore
//! pub struct FileLock { /* fs4-backed handle */ }
//! impl FileLock {
//!     pub fn try_acquire(path: &Path) -> std::io::Result<Self>;
//!     pub fn acquire_blocking(path: &Path) -> std::io::Result<Self>;
//!     pub fn release(self) -> std::io::Result<()>;
//! }
//! ```
//!
//! Internal split: `windows.rs` (`FILE_SHARE` flags + fs4),
//! `unix.rs` (`flock(2)` advisory lock via fs4).
