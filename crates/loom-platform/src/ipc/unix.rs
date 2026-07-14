//! Unix backend for `loom_platform::ipc`.
//!
//! Provides:
//!
//! - [`default_socket_path`] — picks `$XDG_RUNTIME_DIR/<stem>.sock` when
//!   set, falling back to `std::env::temp_dir()/<stem>-<uid>.sock`. The
//!   UID suffix on the fallback prevents two users on the same host
//!   from colliding on `/tmp`.
//! - [`tighten_permissions`] — `chmod 0o600` on the socket file via
//!   `libc::chmod`, applied immediately after bind so the resulting
//!   mode is independent of the caller's umask.
//! - [`cleanup_stale`] — best-effort `unlink` of a leftover socket
//!   file. We first probe the socket and refuse to unlink it when a live
//!   listener accepts connections, then check `S_IFSOCK` before removing a
//!   stale path so we never delete a regular file by accident.
//!
//! The cross-platform listener type (Unix Domain Socket vs Named Pipe
//! abstraction) is provided by the `interprocess` crate; this module
//! only handles the filesystem-side concerns that `interprocess` does
//! not.

use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

/// Resolve the default socket path for the given stem.
pub(super) fn default_socket_path(stem: &str) -> io::Result<PathBuf> {
    if stem.is_empty() || stem.contains('/') || stem.contains('\0') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "socket stem must be non-empty and contain no '/' or NUL",
        ));
    }

    // Prefer the user's runtime dir when the desktop environment has
    // set one — that directory is tmpfs-backed, cleaned at logout, and
    // already scoped to the current user.
    if let Some(rt) = std::env::var_os("XDG_RUNTIME_DIR") {
        let rt = PathBuf::from(rt);
        if !rt.as_os_str().is_empty() {
            return Ok(rt.join(format!("{stem}.sock")));
        }
    }

    // Fallback: temp dir, with the UID baked in so two users do not
    // collide. `libc::getuid` is the canonical real-uid lookup.
    // SAFETY: scalar-returning syscall wrapper, no Rust invariants.
    let uid = unsafe { libc::getuid() };
    Ok(std::env::temp_dir().join(format!("{stem}-{uid}.sock")))
}

/// Restrict the socket file to mode `0o600` (owner read/write only).
pub(super) fn tighten_permissions(path: &Path) -> io::Result<()> {
    let c_path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL byte"))?;
    // SAFETY: `libc::chmod` is a scalar/CStr-arg syscall wrapper; we
    // own the CString for the duration of the call.
    let rc = unsafe { libc::chmod(c_path.as_ptr(), 0o600) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Best-effort removal of a stale socket file. Returns `Ok(())` if the path
/// did not exist, or if it existed without a live listener and was
/// successfully removed. A live listener is reported as `AddrInUse`.
///
/// We refuse to unlink anything that is not an `S_IFSOCK` to avoid
/// trampling a regular file that happens to share the same name.
pub(super) fn cleanup_stale(path: &Path) -> io::Result<()> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };

    use std::os::unix::fs::FileTypeExt;
    if !meta.file_type().is_socket() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "refusing to remove {} — not a socket (file type {:?})",
                path.display(),
                meta.file_type()
            ),
        ));
    }

    // A socket pathname may outlive its listener after a crash, but it may
    // also belong to a healthy process. Unlinking the latter allows a second
    // server to bind the same pathname while the first keeps running on the
    // now-unlinked inode. Probe before cleanup and conservatively preserve the
    // path on every error except the two states that identify a stale socket.
    match std::os::unix::net::UnixStream::connect(path) {
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                format!("socket {} has a live listener", path.display()),
            ));
        }
        Err(err)
            if matches!(
                err.kind(),
                io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
            ) => {}
        Err(err) => {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                format!(
                    "refusing to remove socket {} after connection probe failed: {err}",
                    path.display()
                ),
            ));
        }
    }

    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        // Race: another process unlinked the file between our stat
        // and our unlink. Treat as success — the cleanup goal is met.
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}
