//! Shared tracing initialization.
//!
//! Provides a single `init_file_tracing()` function used by `loom-server` and
//! `loom-daemon` to eliminate the duplicate `init_tracing()` implementations
//! previously living in each binary.

/// Initialize tracing with stderr + file logging.
///
/// The log file is placed under `<data_dir>/loom/logs/<service>/loom-<service>.log`
/// (on Windows: `%LOCALAPPDATA%\loom\logs\<service>`).
///
/// # Parameters
/// * `service` — short service name used for the log directory and filename
///   (e.g. `"server"`, `"daemon"`).
/// * `default_level` — fallback `tracing` level when the `RUST_LOG` env var is
///   not set (e.g. `"info"`, `"warn"`).
pub fn init_file_tracing(service: &str, default_level: &str) {
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::EnvFilter;

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));

    // Log directory: Windows uses %LOCALAPPDATA%, Unix uses the XDG data dir.
    #[cfg(target_os = "windows")]
    let log_dir = {
        let local = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
        std::path::PathBuf::from(local)
            .join("loom")
            .join("logs")
            .join(service)
    };
    #[cfg(not(target_os = "windows"))]
    let log_dir = {
        let home = dirs::data_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
        home.join("loom").join("logs").join(service)
    };
    let _ = std::fs::create_dir_all(&log_dir);

    // Fixed-name log file so loom-shell can always find it.
    let log_path = log_dir.join(format!("loom-{service}.log"));

    // Stderr layer for console/daemon-manager output.
    let stderr_layer = tracing_subscriber::fmt::layer().with_writer(std::io::stderr);

    // Only add the file layer if the log file can be opened.  A missing log
    // file must never prevent the daemon or server from starting (disk full,
    // permissions, etc.).
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        Ok(file) => {
            let (non_blocking, guard) = tracing_appender::non_blocking(file);
            let file_layer = tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false);
            let _ = tracing_subscriber::registry()
                .with(env_filter)
                .with(stderr_layer)
                .with(file_layer)
                .try_init();
            // Keep guard alive so the non_blocking worker stays active.
            std::mem::forget(guard);
            tracing::info!(path = %log_path.display(), "loom-{service} logging to file");
        }
        Err(e) => {
            eprintln!(
                "WARNING: failed to open {}: {e} — logging to stderr only",
                log_path.display()
            );
            let _ = tracing_subscriber::registry()
                .with(env_filter)
                .with(stderr_layer)
                .try_init();
            tracing::info!("loom-{service} logging to stderr only");
        }
    }
}
