#[cfg(debug_assertions)]
mod imp {
    use std::net::{SocketAddr, TcpStream};
    use std::path::PathBuf;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    const DEV_ADDR: ([u8; 4], u16) = ([127, 0, 0, 1], 5173);
    const ENV_NO_DEV_SERVER: &str = "LOOM_GUI_NO_DEV_SERVER";
    const LEGACY_ENV_NO_DEV_SERVER: &str = "JOI_GUI_NO_DEV_SERVER";

    pub struct DevFrontend {
        child: Option<Child>,
    }

    impl DevFrontend {
        pub fn start_if_needed() -> Self {
            if env_flag(ENV_NO_DEV_SERVER)
                || env_flag(LEGACY_ENV_NO_DEV_SERVER)
                || dev_server_reachable()
            {
                return Self { child: None };
            }

            let frontend_dir = frontend_dir();
            // FIXME(windows-compat-iter): GUI surface deferred per PRD §2.3 /
            // PM-Arbitration-002 — dev-mode `pnpm dev` keeps stdout/stderr
            // inherited; deferring the migration to `loom_platform::process`
            // until P1-Cmd-Sweep verifies that the newtype's default flags
            // don't disturb the inherited handles. P0-D's
            // `clippy::disallowed_methods` is also deferred; bare comment
            // suffices.
            let mut command = Command::new("pnpm");
            command
                .arg("--dir")
                .arg(&frontend_dir)
                .arg("dev")
                .stdin(Stdio::null())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());

            let mut child = match command.spawn() {
                Ok(child) => child,
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        dir = %frontend_dir.display(),
                        "failed to start GUI frontend dev server"
                    );
                    return Self { child: None };
                }
            };

            if wait_for_dev_server(&mut child, Duration::from_secs(60)) {
                tracing::info!("GUI frontend dev server started");
            } else {
                tracing::warn!("GUI frontend dev server did not become reachable before timeout");
            }

            Self { child: Some(child) }
        }
    }

    impl Drop for DevFrontend {
        fn drop(&mut self) {
            if let Some(child) = self.child.as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    fn frontend_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("apps")
            .join("gui-web")
    }

    fn env_flag(name: &str) -> bool {
        std::env::var(name)
            .ok()
            .map(|value| {
                let value = value.trim().to_ascii_lowercase();
                matches!(value.as_str(), "1" | "true" | "yes" | "on")
            })
            .unwrap_or(false)
    }

    fn dev_server_reachable() -> bool {
        let addr = SocketAddr::from(DEV_ADDR);
        TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok()
    }

    fn wait_for_dev_server(child: &mut Child, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if dev_server_reachable() {
                return true;
            }
            if matches!(child.try_wait(), Ok(Some(_))) {
                return false;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

#[cfg(not(debug_assertions))]
mod imp {
    pub struct DevFrontend;

    impl DevFrontend {
        pub fn start_if_needed() -> Self {
            Self
        }
    }
}

pub use imp::DevFrontend;
