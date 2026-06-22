//! Cross-platform local IPC primitives.
//!
//! `loom-platform::ipc` papers over the Unix Domain Socket / Windows
//! Named Pipe split so that upper layers (daemon, agent-runtime in
//! P0-PAL-8) operate against one uniform async API. The implementation
//! is backed by the [`interprocess`] crate's `local_socket::tokio`
//! sub-module (~v2.4 series), with per-platform helpers living in
//! `unix.rs` and `windows.rs` to enforce filesystem permissions /
//! Windows DACLs respectively.
//!
//! # Naming convention
//!
//! Names are constructed via [`local_socket_name`] from a short
//! human-readable *stem* (for example `"loom-daemon"`):
//!
//! - **Unix**: a filesystem-path name rooted at the user's runtime
//!   directory (`$XDG_RUNTIME_DIR/<stem>.sock`, falling back to
//!   `std::env::temp_dir()/<stem>-<uid>.sock`). The socket file is
//!   created with mode `0o600` by [`LocalListener::bind`] and unlinked
//!   on drop / stale-socket cleanup.
//! - **Windows**: a namespaced name `\\.\pipe\<stem>-<sid>` resolved via
//!   `interprocess`'s `GenericNamespaced` name type. The Named Pipe is
//!   created with a per-user DACL so other principals cannot connect
//!   (see [`crate::ipc::windows`] for the security descriptor).
//!
//! # Invariants
//!
//! - The default feature `ipc-interprocess` enables this implementation
//!   path; the alternative `ipc-native` feature is reserved for a
//!   future hand-rolled rollback path (`tokio::net::windows::named_pipe`
//!   + `tokio::net::UnixListener`).
//! - The newtype wrappers ([`LocalListener`], [`LocalStream`]) do **not**
//!   expose the underlying `interprocess` types: that lets PAL-8 swap
//!   the backend without touching upper-layer call sites.
//! - All async I/O is Tokio-runtime bound (same constraint as
//!   `interprocess::local_socket::tokio`). Callers must be running
//!   under a Tokio reactor.
//!
//! # Errors
//!
//! All fallible operations return [`std::io::Error`]. Common cases:
//!
//! - `AddrInUse` — another listener is already bound to the same name
//!   (Unix: stale socket file may need [`cleanup_stale`]).
//! - `NotFound` — connect to a name with no listener.
//! - `PermissionDenied` — Windows DACL rejected the open (running as a
//!   different user / SID).
//!
//! # D4 — empty `LOOM_DAEMON_SOCKET` ⇒ WebSocket fallback
//!
//! The daemon's transport selection lives one layer up
//! (`loom-cli::daemon_ipc::env_socket_path` filters out empty strings;
//! `connect_client` then falls back to `Client::connect_ws`). This
//! module deliberately stays out of that decision: callers pass a
//! resolved [`LocalSocketName`] only when they have decided to use
//! local IPC. So on Windows, an unset or empty `LOOM_DAEMON_SOCKET`
//! never reaches the named-pipe path at all — the CLI quietly opens
//! a WebSocket to the server URL instead. This is the intended D4
//! contract; do not move the empty-string check into this crate.

#![allow(clippy::needless_return)]

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use interprocess::local_socket::tokio::{
    Listener as IpcListener, Stream as IpcStream,
};
use interprocess::local_socket::traits::tokio::{
    Listener as ListenerTrait, Stream as StreamTrait,
};
use interprocess::local_socket::ListenerOptions;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// Opaque, owned identifier for a local socket.
///
/// Use [`local_socket_name`] to construct one. Behind the scenes this
/// stores enough state to recreate a borrowed
/// [`interprocess::local_socket::Name`] for each `bind` / `connect` call,
/// so a single `LocalSocketName` may be reused (or cloned) across
/// reconnect attempts.
#[derive(Debug, Clone)]
pub struct LocalSocketName {
    inner: NameRepr,
}

#[derive(Debug, Clone)]
enum NameRepr {
    /// Filesystem-path local socket (Unix).
    #[cfg(unix)]
    Fs(std::path::PathBuf),
    /// Namespaced local socket (Windows named pipe).
    #[cfg(windows)]
    Ns(String),
}

impl LocalSocketName {
    /// Human-readable form for log / error messages. The string
    /// returned here is **not** a stable wire identifier — callers
    /// must round-trip via [`local_socket_name`] when reconstructing
    /// from configuration.
    pub fn display(&self) -> String {
        match &self.inner {
            #[cfg(unix)]
            NameRepr::Fs(p) => p.display().to_string(),
            #[cfg(windows)]
            NameRepr::Ns(s) => format!(r"\\.\pipe\{s}"),
        }
    }

    /// Returns the underlying filesystem path (Unix only).
    #[cfg(unix)]
    pub fn as_fs_path(&self) -> &std::path::Path {
        match &self.inner {
            NameRepr::Fs(p) => p.as_path(),
        }
    }

    /// Construct a [`LocalSocketName`] from an explicit filesystem path.
    ///
    /// This is the bridge for legacy call sites — the daemon proxy,
    /// the server's `--unix-socket` CLI flag, the `unix://` URL scheme
    /// — that pick a path independently of [`local_socket_name`] (for
    /// example via `config::config_dir()/daemon/daemon.sock`).
    ///
    /// - **Unix**: the path is used verbatim as the socket file
    ///   location. An empty path is rejected.
    /// - **Windows**: a Named Pipe has no filesystem backing, so the
    ///   path's file *stem* (e.g. `daemon` from `…/daemon.sock`) is
    ///   taken as the pipe stem and run through the same per-user
    ///   SID-suffixing as [`local_socket_name`]. Two paths that differ
    ///   only in directory therefore alias to the same pipe — fine in
    ///   practice because the daemon and its clients always derive the
    ///   path from one shared per-user config function.
    pub fn from_path(
        path: impl Into<std::path::PathBuf>,
    ) -> io::Result<Self> {
        let path = path.into();
        #[cfg(unix)]
        {
            if path.as_os_str().is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "socket path must not be empty",
                ));
            }
            Ok(Self {
                inner: NameRepr::Fs(path),
            })
        }
        #[cfg(windows)]
        {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!(
                            "cannot derive pipe stem from path {}",
                            path.display()
                        ),
                    )
                })?;
            let name = windows::default_pipe_name(stem)?;
            Ok(Self {
                inner: NameRepr::Ns(name),
            })
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = path;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "loom-platform::ipc is not supported on this target",
            ))
        }
    }
}

/// Construct a platform-appropriate [`LocalSocketName`] from a short
/// stem (for example `"loom-daemon"`).
///
/// The stem is mixed with per-user state (UID on Unix, SID on Windows)
/// so that two different desktop users on the same host do not
/// accidentally collide on the same name.
pub fn local_socket_name(stem: &str) -> io::Result<LocalSocketName> {
    #[cfg(unix)]
    {
        let path = unix::default_socket_path(stem)?;
        Ok(LocalSocketName {
            inner: NameRepr::Fs(path),
        })
    }
    #[cfg(windows)]
    {
        let name = windows::default_pipe_name(stem)?;
        Ok(LocalSocketName {
            inner: NameRepr::Ns(name),
        })
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = stem;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "loom-platform::ipc is not supported on this target",
        ))
    }
}

/// Best-effort removal of a stale socket file (Unix) — no-op on
/// Windows where named pipes are kernel objects with no leftover state.
pub fn cleanup_stale(name: &LocalSocketName) -> io::Result<()> {
    match &name.inner {
        #[cfg(unix)]
        NameRepr::Fs(p) => unix::cleanup_stale(p),
        #[cfg(windows)]
        NameRepr::Ns(_) => Ok(()),
    }
}

fn build_borrowed_name(
    name: &LocalSocketName,
) -> io::Result<interprocess::local_socket::Name<'_>> {
    match &name.inner {
        #[cfg(unix)]
        NameRepr::Fs(p) => {
            use interprocess::local_socket::{GenericFilePath, ToFsName};
            p.as_path().to_fs_name::<GenericFilePath>()
        }
        #[cfg(windows)]
        NameRepr::Ns(s) => {
            use interprocess::local_socket::{GenericNamespaced, ToNsName};
            s.as_str().to_ns_name::<GenericNamespaced>()
        }
    }
}

/// Tokio-based local socket listener.
///
/// Built by [`LocalListener::bind`]. Dropping the listener releases the
/// underlying OS resource; on Unix this additionally unlinks the
/// socket file (best-effort).
pub struct LocalListener {
    inner: IpcListener,
    #[cfg(unix)]
    socket_path: std::path::PathBuf,
}

impl std::fmt::Debug for LocalListener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalListener").finish_non_exhaustive()
    }
}

impl LocalListener {
    /// Bind a listener to `name`.
    ///
    /// On Unix this creates a fresh socket file at the resolved path,
    /// applies mode `0o600`, and registers it for cleanup on drop.
    /// If a stale socket file already exists (no live owner), it is
    /// removed first. Concurrent bind attempts to a *live* name will
    /// fail with `AddrInUse`.
    ///
    /// On Windows the named pipe is created with an explicit DACL
    /// permitting only the current user's SID to read or write.
    pub async fn bind(name: &LocalSocketName) -> io::Result<Self> {
        // Unix only: pre-cleanup so a crashed previous server does not
        // wedge us. On Windows the kernel discards named pipes when
        // their last handle closes, so no equivalent step exists.
        #[cfg(unix)]
        {
            if let NameRepr::Fs(p) = &name.inner {
                unix::cleanup_stale(p)?;
            }
        }

        let borrowed = build_borrowed_name(name)?;
        let opts = ListenerOptions::new().name(borrowed);

        #[cfg(windows)]
        let opts = windows::apply_security_options(opts)?;

        let listener = opts.create_tokio()?;

        // Unix: tighten the socket file permissions to 0o600 after bind.
        // We do this *after* `create_tokio` (which performs the bind)
        // rather than relying on an `umask` so the resulting mode does
        // not depend on the caller's process-wide umask.
        #[cfg(unix)]
        let socket_path = match &name.inner {
            NameRepr::Fs(p) => {
                unix::tighten_permissions(p)?;
                p.clone()
            }
        };

        Ok(LocalListener {
            inner: listener,
            #[cfg(unix)]
            socket_path,
        })
    }

    /// Accept a single inbound connection.
    pub async fn accept(&self) -> io::Result<LocalStream> {
        let stream = self.inner.accept().await?;
        Ok(LocalStream { inner: stream })
    }
}

#[cfg(unix)]
impl Drop for LocalListener {
    fn drop(&mut self) {
        // Best-effort: a failure here is not actionable (the listener
        // is being torn down anyway).
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

/// Tokio-based local socket byte stream.
///
/// Implements [`AsyncRead`] and [`AsyncWrite`]. Use [`LocalStream::connect`]
/// from the client side, or obtain one via [`LocalListener::accept`] on
/// the server side.
pub struct LocalStream {
    inner: IpcStream,
}

impl std::fmt::Debug for LocalStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalStream").finish_non_exhaustive()
    }
}

impl LocalStream {
    /// Connect to a listener at `name`.
    ///
    /// Returns `NotFound` if no listener is bound. On Windows, returns
    /// `PermissionDenied` if the listener's DACL rejected the open.
    pub async fn connect(name: &LocalSocketName) -> io::Result<Self> {
        let borrowed = build_borrowed_name(name)?;
        let stream = IpcStream::connect(borrowed).await?;
        Ok(LocalStream { inner: stream })
    }
}

impl AsyncRead for LocalStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for LocalStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_is_clone_and_debug() {
        let name = local_socket_name("loom-platform-ipc-self-test").expect("build name");
        let _cloned = name.clone();
        let dbg = format!("{name:?}");
        assert!(dbg.contains("LocalSocketName"));
        assert!(!name.display().is_empty());
    }

    #[test]
    fn from_path_accepts_explicit_path() {
        let path = std::env::temp_dir().join("loom-platform-ipc-from-path.sock");
        let name = LocalSocketName::from_path(&path).expect("build name from path");
        // Display must round-trip something non-empty regardless of
        // platform (Unix: path string; Windows: pipe stem).
        assert!(!name.display().is_empty());
    }

    #[test]
    fn from_path_rejects_unstemmable_input() {
        // On Unix this is an "empty path"; on Windows it has no file
        // stem. Either way `from_path` should refuse.
        let result = LocalSocketName::from_path(std::path::PathBuf::new());
        assert!(result.is_err(), "from_path('') should fail, got {result:?}");
    }
}
