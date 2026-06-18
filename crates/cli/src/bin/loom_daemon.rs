use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "loom-daemon", about = "Loom machine-scoped agent host")]
struct Args {
    /// Override the configured server URL (defaults to ws://127.0.0.1:7878/rpc).
    #[arg(long, env = "LOOM_SERVER")]
    server: Option<String>,
    /// Machine id from the desktop machine config. Defaults to the active
    /// workspace's first machine.
    #[arg(long = "machine-id", env = "LOOM_MACHINE_ID")]
    machine_id: Option<String>,
    /// Human-readable daemon host name stored in daemon.toml.
    #[arg(long = "machine-name", env = "LOOM_MACHINE_NAME")]
    machine_name: Option<String>,
    /// Override the daemon data root. Defaults to the machine data root.
    #[arg(long = "data-root", env = "LOOM_AGENT_DATA_ROOT")]
    data_root: Option<PathBuf>,
    /// Comma-separated actor ids to load. Empty/omitted = load every
    /// agent configured on the machine.
    #[arg(long = "allow-actors", value_delimiter = ',')]
    allow_actors: Vec<String>,
    /// Print auto-detected local providers and exit.
    #[arg(long = "list-providers")]
    list_providers: bool,
    /// Override the directory of ServiceSpec JSON files loaded by the
    /// daemon. Defaults to `~/.config/loom/services/` or
    /// `$LOOM_SERVICE_SPECS`.
    #[arg(long = "services")]
    services: Option<PathBuf>,
    /// Comma-separated service ids to load through the daemon. Empty or
    /// omitted means load every ServiceSpec under --services.
    #[arg(long = "allow-services", value_delimiter = ',')]
    allow_services: Vec<String>,
    /// Do not start the service host from this daemon.
    #[arg(long = "no-services")]
    no_services: bool,
    /// Unix socket used by local `loom` CLI clients to reach this daemon.
    #[arg(long = "socket", env = "LOOM_DAEMON_SOCKET")]
    socket: Option<PathBuf>,
    /// Do not expose the local daemon IPC socket. Useful for tests where
    /// agents connect to the server directly.
    #[arg(long = "no-ipc")]
    no_ipc: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    windows_console::init();
    init_tracing();
    let args = Args::parse();
    loom_cli::cmd::daemon::run(
        args.machine_id,
        args.machine_name,
        args.data_root,
        args.allow_actors,
        args.list_providers,
        args.services,
        args.allow_services,
        args.no_services,
        args.socket,
        args.no_ipc,
        args.server,
    )
    .await
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    use tracing_subscriber::prelude::*;
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));

    // File log directory.  On Windows: %LOCALAPPDATA%\loom\logs\daemon.
    // On Unix: ~/.local/share/loom/logs/daemon.
    #[cfg(target_os = "windows")]
    let log_dir = {
        let local = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
        std::path::PathBuf::from(local).join("loom").join("logs").join("daemon")
    };
    #[cfg(not(target_os = "windows"))]
    let log_dir = {
        let home = dirs::data_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
        home.join("loom").join("logs").join("daemon")
    };
    let _ = std::fs::create_dir_all(&log_dir);
    let file_appender = tracing_appender::rolling::daily(&log_dir, "loom-daemon.log");

    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr);
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(file_appender)
        .with_ansi(false);

    let _ = tracing_subscriber::registry()
        .with(env_filter)
        .with(stderr_layer)
        .with(file_layer)
        .try_init();
    tracing::info!(path = %log_dir.display(), "loom-daemon logging to file");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_accepts_runtime_host_options() {
        let args = Args::try_parse_from([
            "loom-daemon",
            "--server",
            "ws://127.0.0.1:9/rpc",
            "--machine-id",
            "machine_a",
            "--data-root",
            "/tmp/loom-daemon-test",
            "--machine-name",
            "Test Mac",
            "--allow-actors",
            "actor_a,actor_b",
            "--services",
            "/tmp/loom-services",
            "--allow-services",
            "svc_a,svc_b",
            "--no-services",
            "--socket",
            "/tmp/loom.sock",
            "--no-ipc",
        ])
        .expect("parse loom-daemon options");

        assert_eq!(args.server.as_deref(), Some("ws://127.0.0.1:9/rpc"));
        assert_eq!(args.machine_id.as_deref(), Some("machine_a"));
        assert_eq!(args.machine_name.as_deref(), Some("Test Mac"));
        assert_eq!(
            args.data_root.as_deref(),
            Some(std::path::Path::new("/tmp/loom-daemon-test"))
        );
        assert_eq!(args.allow_actors, ["actor_a", "actor_b"]);
        assert_eq!(
            args.services.as_deref(),
            Some(std::path::Path::new("/tmp/loom-services"))
        );
        assert_eq!(args.allow_services, ["svc_a", "svc_b"]);
        assert!(args.no_services);
        assert_eq!(
            args.socket.as_deref(),
            Some(std::path::Path::new("/tmp/loom.sock"))
        );
        assert!(args.no_ipc);
    }

    #[test]
    fn daemon_exposes_provider_inventory_probe() {
        let args = Args::try_parse_from(["loom-daemon", "--list-providers"])
            .expect("parse list-providers");

        assert!(args.list_providers);
    }
}
