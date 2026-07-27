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
    /// Do not register this daemon machine as a server-visible service actor.
    #[arg(long = "no-machine-actor")]
    no_machine_actor: bool,
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
    loom_platform::console::init();
    agent_runtime::tracing_setup::init_file_tracing("daemon", "info");

    // Install a panic hook that logs panics to the tracing system before the
    // process exits. This ensures we have a record of what went wrong even if
    // the panic happens outside the main loop's catch_unwind guards.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Log the panic through tracing so it appears in the file log.
        let location = info
            .location()
            .map(|loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()))
            .unwrap_or_else(|| "unknown location".to_string());
        let payload = if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else if let Some(s) = info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else {
            "unknown panic payload".to_string()
        };
        tracing::error!(
            location = %location,
            payload = %payload,
            "loom-daemon PANIC"
        );
        // Also call the default hook (prints to stderr).
        default_hook(info);
    }));

    let args = Args::parse();
    if let Err(e) = loom_cli::cmd::daemon::run(
        args.machine_id,
        args.machine_name,
        args.data_root,
        args.allow_actors,
        args.list_providers,
        args.services,
        args.allow_services,
        args.no_services,
        args.no_machine_actor,
        args.socket,
        args.no_ipc,
        args.server,
    )
    .await
    {
        // Log startup/run-time failures before exiting.
        // The daemon typically has no visible terminal (CREATE_NO_WINDOW on
        // Windows), so tracing is the only record.
        tracing::error!(error = %e, "loom-daemon run failed, exiting");
        // Ensure logs are flushed.
        std::thread::sleep(std::time::Duration::from_millis(500));
        std::process::exit(1);
    }
    Ok(())
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
            "--no-machine-actor",
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
        assert!(args.no_machine_actor);
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
