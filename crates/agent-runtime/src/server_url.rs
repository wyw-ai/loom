use std::net::TcpStream;
use std::time::Duration;

pub fn agent_child_server_url(server_url: &str) -> String {
    child_server_url(server_url, false)
}

pub fn local_agent_child_server_url(server_url: &str) -> String {
    child_server_url(server_url, true)
}

fn child_server_url(server_url: &str, assume_local: bool) -> String {
    if let Ok(explicit) = std::env::var("LOOM_AGENT_SERVER") {
        if !explicit.trim().is_empty() {
            return explicit;
        }
    }

    let Ok(mut parsed) = url::Url::parse(server_url) else {
        return server_url.to_string();
    };
    if parsed.scheme() != "ws" {
        return server_url.to_string();
    }
    let Some(host) = parsed.host_str().map(|s| s.to_string()) else {
        return server_url.to_string();
    };
    if matches!(host.as_str(), "127.0.0.1" | "localhost" | "::1") {
        return server_url.to_string();
    }
    let Some(port) = parsed.port_or_known_default() else {
        return server_url.to_string();
    };

    if (assume_local || matches!(host.as_str(), "0.0.0.0" | "::") || loopback_accepts(port))
        && parsed.set_host(Some("127.0.0.1")).is_ok()
    {
        return parsed.to_string();
    }
    server_url.to_string()
}

fn loopback_accepts(port: u16) -> bool {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(80)).is_ok()
}
