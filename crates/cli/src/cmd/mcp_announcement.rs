//! `joi mcp announcement` — stdio MCP server exposing pinned-announcement
//! tools to the running agent.
//!
//! Unlike `mcp_memory` (which reads/writes a local JSONL store), this MCP
//! is a thin proxy to the running joi-server: each tool call translates to
//! one `event/append` over the same WebSocket the chat client uses. The
//! resulting `announcement.set` / `announcement.clear` event lands in the
//! journal, gets broadcast to every connected client subscribed to the
//! scope, and the chat TUI's right-side panel reduces it into the new
//! pinned message.
//!
//! Spawned by `agent_runtime::build_mcp_servers` when an agent's spec opts
//! into `announcement.mcp = true`. The runtime passes `--actor-id` and
//! `--server` so we don't have to scrape env at every layer.
//!
//! Protocol: minimal MCP 2024-11-05 (matches `mcp_memory.rs`).

use std::sync::Arc;

use anyhow::Result;
use proto::methods::{method, EventAppendResult};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::client::Client;

const PROTOCOL_VERSION: &str = "2024-11-05";
const SERVER_NAME: &str = "joi-announcement";

pub async fn run(actor_id: String, server_url: String) -> Result<()> {
    // Bind the actor up-front so every tool call can reuse the same
    // websocket; the server's ACL gate runs against the connection's bound
    // actor, not per-request, so we *must* call `connection/open` here for
    // the agent to be able to publish into private channels it belongs to.
    let client = Client::connect(&server_url).await?;
    client.initialize().await?;
    let _ = client
        .open_connection_as(&actor_id, "agent", Some(&actor_id))
        .await?;

    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin).lines();
    let mut stdout = tokio::io::stdout();

    while let Some(line) = reader.next_line().await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(err) => {
                let resp = error_response(Value::Null, -32700, &format!("parse: {err}"));
                write_response(&mut stdout, &resp).await?;
                continue;
            }
        };
        // Notifications carry no `id` and expect no response.
        if request.get("id").is_none() {
            continue;
        }
        let response = handle_request(&actor_id, client.clone(), &request).await;
        write_response(&mut stdout, &response).await?;
    }
    Ok(())
}

async fn write_response(w: &mut tokio::io::Stdout, response: &Value) -> std::io::Result<()> {
    let s = serde_json::to_string(response).unwrap_or_else(|_| "{}".into());
    w.write_all(s.as_bytes()).await?;
    w.write_all(b"\n").await?;
    w.flush().await?;
    Ok(())
}

async fn handle_request(actor_id: &str, client: Arc<Client>, request: &Value) -> Value {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));

    match method {
        "initialize" => ok_response(
            id,
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": { "tools": {} },
                "serverInfo": {
                    "name": SERVER_NAME,
                    "version": env!("CARGO_PKG_VERSION"),
                },
            }),
        ),
        "tools/list" => ok_response(id, json!({ "tools": tool_definitions() })),
        "tools/call" => match handle_tool_call(actor_id, client, &params).await {
            Ok(content) => ok_response(id, json!({ "content": content })),
            Err(err) => error_response(id, -32000, &err),
        },
        _ => error_response(id, -32601, &format!("method not found: {method}")),
    }
}

fn tool_definitions() -> Value {
    json!([
        {
            "name": "announcement.set",
            "description": "Pin (or replace) the announcement shown in the right-side panel of the given scope. Use this for recap summaries: read the recent conversation, summarize the state, then call this tool. The previous announcement is replaced wholesale.",
            "inputSchema": {
                "type": "object",
                "required": ["scopeId", "scopeKind", "text"],
                "properties": {
                    "scopeId":   { "type": "string", "description": "Channel id (chan_…) or thread id (thread_…) to pin into." },
                    "scopeKind": { "type": "string", "enum": ["channel", "thread"], "description": "Which kind of scope `scopeId` refers to." },
                    "text":      { "type": "string", "description": "Announcement body. Markdown is rendered. Empty/whitespace clears." }
                }
            }
        },
        {
            "name": "announcement.clear",
            "description": "Drop the pinned announcement for the given scope. The right-side panel disappears for every connected client.",
            "inputSchema": {
                "type": "object",
                "required": ["scopeId", "scopeKind"],
                "properties": {
                    "scopeId":   { "type": "string" },
                    "scopeKind": { "type": "string", "enum": ["channel", "thread"] }
                }
            }
        }
    ])
}

async fn handle_tool_call(
    actor_id: &str,
    client: Arc<Client>,
    params: &Value,
) -> Result<Vec<Value>, String> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing tool name".to_string())?;
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    match name {
        "announcement.set" => announcement_set(actor_id, client, &args).await,
        "announcement.clear" => announcement_clear(actor_id, client, &args).await,
        other => Err(format!("unknown tool: {other}")),
    }
}

async fn announcement_set(
    actor_id: &str,
    client: Arc<Client>,
    args: &Value,
) -> Result<Vec<Value>, String> {
    let scope_id = require_str(args, "scopeId")?;
    let scope_kind = require_scope_kind(args)?;
    let text = args
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    // Normalize: an empty body is semantically equivalent to a clear, so let
    // callers pick whichever spelling reads better.
    let kind = if text.trim().is_empty() {
        "announcement.clear"
    } else {
        "announcement.set"
    };
    publish(client, actor_id, &scope_id, &scope_kind, kind, &text)
        .await
        .map(|id| {
            vec![json!({
                "type": "text",
                "text": format!("{kind} ok (event {id}, scope {scope_kind}/{scope_id})"),
            })]
        })
}

async fn announcement_clear(
    actor_id: &str,
    client: Arc<Client>,
    args: &Value,
) -> Result<Vec<Value>, String> {
    let scope_id = require_str(args, "scopeId")?;
    let scope_kind = require_scope_kind(args)?;
    publish(
        client,
        actor_id,
        &scope_id,
        &scope_kind,
        "announcement.clear",
        "",
    )
    .await
    .map(|id| {
        vec![json!({
            "type": "text",
            "text": format!("announcement.clear ok (event {id}, scope {scope_kind}/{scope_id})"),
        })]
    })
}

async fn publish(
    client: Arc<Client>,
    actor_id: &str,
    scope_id: &str,
    scope_kind: &str,
    kind: &str,
    text: &str,
) -> Result<String, String> {
    let payload = json!({
        "event": {
            "type": kind,
            "actorId": actor_id,
            "scope": { "kind": scope_kind, "id": scope_id },
            "payload": { "text": text },
        }
    });
    let res: EventAppendResult = client
        .call(method::EVENT_APPEND, payload)
        .await
        .map_err(|e| e.to_string())?;
    Ok(res.event.id)
}

fn require_str(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("`{key}` is required"))
}

fn require_scope_kind(args: &Value) -> Result<String, String> {
    let raw = require_str(args, "scopeKind")?;
    match raw.as_str() {
        "channel" | "thread" => Ok(raw),
        other => Err(format!(
            "`scopeKind` must be channel or thread, got `{other}`"
        )),
    }
}

fn ok_response(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_list_exposes_set_and_clear() {
        let tools = tool_definitions();
        let names: Vec<&str> = tools
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["announcement.set", "announcement.clear"]);
    }

    #[test]
    fn require_scope_kind_rejects_garbage() {
        let v = json!({ "scopeKind": "thread" });
        assert_eq!(require_scope_kind(&v).unwrap(), "thread");
        let v = json!({ "scopeKind": "channel" });
        assert_eq!(require_scope_kind(&v).unwrap(), "channel");
        let v = json!({ "scopeKind": "blob" });
        assert!(require_scope_kind(&v).is_err());
        let v = json!({});
        assert!(require_scope_kind(&v).is_err());
    }

    #[test]
    fn require_str_rejects_empty() {
        assert_eq!(require_str(&json!({ "x": "ok" }), "x").unwrap(), "ok");
        assert!(require_str(&json!({ "x": "" }), "x").is_err());
        assert!(require_str(&json!({ "x": "   " }), "x").is_err());
        assert!(require_str(&json!({}), "x").is_err());
    }
}
