//! Synthesize the `session/new.mcpServers` array for an ACP session.
//!
//! Right now the only server we auto-inject is the `joi-memory` stdio
//! bridge, opted in via `memory.delivery.mcp = true`. The list is just a
//! `Vec<serde_json::Value>` on purpose — ACP accepts an untyped array of
//! `McpServer` objects, and future spec additions (user-declared MCP
//! servers) should append to this list without touching the ACP adapter.

use std::path::Path;

use proto::methods::{AnnouncementSpec, MemorySpec};
use serde_json::{json, Value};

/// Build the `mcpServers` array for a given actor. Returns an empty vec
/// when no MCP is requested (which matches pre-envelope behavior — the
/// adapter always sent `mcpServers: []`).
///
/// `server_url` is required only by MCPs that proxy to joi-server (e.g.
/// the announcement bridge). Pass `None` if no such MCP is being injected
/// or the caller can't surface a URL — those entries are silently skipped.
pub fn build_mcp_servers(
    joi_binary: Option<&Path>,
    actor_id: &str,
    profile_dir: &Path,
    memory: Option<&MemorySpec>,
    announcement: Option<&AnnouncementSpec>,
    server_url: Option<&str>,
) -> Vec<Value> {
    let mut servers: Vec<Value> = Vec::new();
    let Some(bin) = joi_binary else {
        return servers;
    };
    if let Some(mem) = memory {
        if mem.delivery.mcp {
            servers.push(joi_memory_entry(bin, actor_id, profile_dir, mem));
        }
    }
    if let (Some(ann), Some(url)) = (announcement, server_url) {
        if ann.mcp {
            servers.push(joi_announcement_entry(bin, actor_id, url));
        }
    }
    servers
}

fn joi_announcement_entry(joi_binary: &Path, actor_id: &str, server_url: &str) -> Value {
    json!({
        "name": "joi-announcement",
        "command": joi_binary.display().to_string(),
        "args": [
            "mcp",
            "announcement",
            "--actor-id", actor_id,
            "--server", server_url,
        ],
        "env": [],
    })
}

fn joi_memory_entry(
    joi_binary: &Path,
    actor_id: &str,
    profile_dir: &Path,
    mem: &MemorySpec,
) -> Value {
    // ACP's `McpServerStdio` shape: `{ name, command, args, env }`.
    // Agents translate this into a spawned child on their side and route
    // tool calls over JSON-RPC/stdio.
    let mut args: Vec<String> = vec![
        "mcp".into(),
        "memory".into(),
        "--actor-id".into(),
        actor_id.to_string(),
        "--profile-dir".into(),
        profile_dir.display().to_string(),
    ];
    if !mem.store.shard_by.is_empty() && mem.store.shard_by != "month" {
        args.push("--shard-by".into());
        args.push(mem.store.shard_by.clone());
    }
    json!({
        "name": "joi-memory",
        "command": joi_binary.display().to_string(),
        "args": args,
        "env": [],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proto::methods::{AnnouncementSpec, MemoryDeliverySpec, MemorySpec};
    use std::path::PathBuf;

    fn mem(mcp: bool) -> MemorySpec {
        MemorySpec {
            delivery: MemoryDeliverySpec { mcp, prompt: true },
            ..Default::default()
        }
    }

    fn ann(mcp: bool) -> AnnouncementSpec {
        AnnouncementSpec { mcp }
    }

    #[test]
    fn no_specs_yields_empty() {
        let got = build_mcp_servers(
            Some(&PathBuf::from("/x/joi")),
            "a",
            &PathBuf::from("/p"),
            None,
            None,
            Some("ws://localhost"),
        );
        assert!(got.is_empty());
    }

    #[test]
    fn memory_without_mcp_yields_empty() {
        let got = build_mcp_servers(
            Some(&PathBuf::from("/x/joi")),
            "a",
            &PathBuf::from("/p"),
            Some(&mem(false)),
            None,
            None,
        );
        assert!(got.is_empty());
    }

    #[test]
    fn no_binary_means_no_server() {
        let got = build_mcp_servers(
            None,
            "a",
            &PathBuf::from("/p"),
            Some(&mem(true)),
            Some(&ann(true)),
            Some("ws://localhost"),
        );
        assert!(got.is_empty());
    }

    #[test]
    fn mcp_enabled_injects_joi_memory() {
        let got = build_mcp_servers(
            Some(&PathBuf::from("/opt/bin/joi")),
            "actor_x",
            &PathBuf::from("/data/profile"),
            Some(&mem(true)),
            None,
            None,
        );
        assert_eq!(got.len(), 1);
        assert_eq!(got[0]["name"], "joi-memory");
        assert_eq!(got[0]["command"], "/opt/bin/joi");
        let args: Vec<&str> = got[0]["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(
            args,
            vec![
                "mcp",
                "memory",
                "--actor-id",
                "actor_x",
                "--profile-dir",
                "/data/profile"
            ]
        );
    }

    #[test]
    fn shard_by_day_is_passed_through() {
        let mut m = mem(true);
        m.store.shard_by = "day".into();
        let got = build_mcp_servers(
            Some(&PathBuf::from("/joi")),
            "a",
            &PathBuf::from("/p"),
            Some(&m),
            None,
            None,
        );
        let args: Vec<&str> = got[0]["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(args.contains(&"--shard-by"));
        assert!(args.contains(&"day"));
    }

    #[test]
    fn announcement_mcp_enabled_injects_joi_announcement() {
        let got = build_mcp_servers(
            Some(&PathBuf::from("/opt/bin/joi")),
            "actor_x",
            &PathBuf::from("/data/profile"),
            None,
            Some(&ann(true)),
            Some("ws://127.0.0.1:7878/rpc"),
        );
        assert_eq!(got.len(), 1);
        assert_eq!(got[0]["name"], "joi-announcement");
        let args: Vec<&str> = got[0]["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(
            args,
            vec![
                "mcp",
                "announcement",
                "--actor-id",
                "actor_x",
                "--server",
                "ws://127.0.0.1:7878/rpc",
            ]
        );
    }

    #[test]
    fn announcement_without_server_url_is_skipped() {
        // The agent_serve / runtime callers should always plumb the URL,
        // but if someone forgets we'd rather omit the entry than spawn an
        // MCP that immediately fails to connect.
        let got = build_mcp_servers(
            Some(&PathBuf::from("/opt/bin/joi")),
            "a",
            &PathBuf::from("/p"),
            None,
            Some(&ann(true)),
            None,
        );
        assert!(got.is_empty());
    }

    #[test]
    fn memory_and_announcement_both_inject_when_enabled() {
        let got = build_mcp_servers(
            Some(&PathBuf::from("/joi")),
            "actor_x",
            &PathBuf::from("/p"),
            Some(&mem(true)),
            Some(&ann(true)),
            Some("ws://localhost/rpc"),
        );
        assert_eq!(got.len(), 2);
        let names: Vec<&str> = got.iter().map(|s| s["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"joi-memory"));
        assert!(names.contains(&"joi-announcement"));
    }
}
