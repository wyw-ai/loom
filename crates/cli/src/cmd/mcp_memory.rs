//! `joi mcp memory` — stdio MCP server exposing per-actor memory as tools.
//!
//! The same `joi` binary runs as an MCP server when spawned with this
//! subcommand. The ACP runtime injects it into `session/new.mcpServers`
//! whenever the spec has `memory.delivery.mcp = true`, giving the agent
//! first-class tools:
//!
//! * `memory.query`  — search (text / tags / types / channel scope)
//! * `memory.append` — record a new memory
//! * `memory.get`    — fetch by id
//!
//! Protocol: minimal MCP 2024-11-05. Handshake via `initialize`, then
//! `tools/list` and `tools/call`. Anything we don't recognize gets a
//! `-32601 method not found` reply; the agent just skips unknown features.

use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;

use anyhow::Result;
use chrono::Utc;
use serde_json::{json, Value};
use uuid::Uuid;

use agent_runtime::memory::{
    JsonlMemoryStore, MemoryQuery, MemoryRecord, MemorySource, MemoryStore,
};

const PROTOCOL_VERSION: &str = "2024-11-05";
const SERVER_NAME: &str = "joi-memory";

pub fn run(actor_id: String, profile_dir: PathBuf, shard_by: Option<String>) -> Result<()> {
    let root_path = resolve_memory_root(&profile_dir);
    let store = match shard_by.as_deref() {
        Some(s) => JsonlMemoryStore::with_shard_by(root_path, s),
        None => JsonlMemoryStore::new(root_path),
    };
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut writer = BufWriter::new(stdout.lock());
    let reader = BufReader::new(stdin.lock());
    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(err) => {
                let resp = error_response(Value::Null, -32700, &format!("parse: {err}"));
                write_response(&mut writer, &resp)?;
                continue;
            }
        };
        // Notifications have no `id` and expect no response. Still skip any
        // work we don't handle.
        if request.get("id").is_none() {
            continue;
        }
        let response = handle_request(&actor_id, &store, &request);
        write_response(&mut writer, &response)?;
    }
    Ok(())
}

fn write_response(w: &mut impl Write, response: &Value) -> io::Result<()> {
    let s = serde_json::to_string(response).unwrap_or_else(|_| "{}".into());
    w.write_all(s.as_bytes())?;
    w.write_all(b"\n")?;
    w.flush()?;
    Ok(())
}

fn resolve_memory_root(profile_dir: &std::path::Path) -> PathBuf {
    // Assume default spec layout (`./memory/records`). If the spec overrode
    // the root, the runtime will have passed a fully-resolved profile_dir
    // anyway — we just join the conventional subpath.
    profile_dir.join("memory").join("records")
}

fn handle_request(actor_id: &str, store: &JsonlMemoryStore, request: &Value) -> Value {
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
        "tools/call" => match handle_tool_call(actor_id, store, &params) {
            Ok(content) => ok_response(id, json!({ "content": content })),
            Err(err) => error_response(id, -32000, &err),
        },
        _ => error_response(id, -32601, &format!("method not found: {method}")),
    }
}

fn tool_definitions() -> Value {
    json!([
        {
            "name": "memory.query",
            "description": "Search this actor's long-term memory. Returns accepted records matching the query, newest first.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text":      { "type": "string", "description": "Free-text search over summary / detail / tags." },
                    "tags":      { "type": "array", "items": { "type": "string" }, "description": "Match any of these tags (case-insensitive)." },
                    "types":     { "type": "array", "items": { "type": "string" }, "description": "Filter by record type (e.g. fact / decision / task)." },
                    "channelId": { "type": "string", "description": "Scope to records whose source.channelId equals this. Omit to include all visible channels." },
                    "limit":     { "type": "integer", "default": 10, "minimum": 1, "maximum": 200 },
                    "includeNonAccepted": { "type": "boolean", "default": false }
                }
            }
        },
        {
            "name": "memory.append",
            "description": "Record a new memory entry for this actor. Status defaults to 'accepted'. The runtime auto-fills id / ts / actorId.",
            "inputSchema": {
                "type": "object",
                "required": ["summary"],
                "properties": {
                    "summary":    { "type": "string", "description": "One-line fact / decision / task — this is what shows up in prompt injection." },
                    "detail":     { "type": "string" },
                    "type":       { "type": "string", "default": "note", "description": "fact / decision / task / note / preference — not enforced." },
                    "confidence": { "type": "string", "enum": ["high", "medium", "low"], "default": "medium" },
                    "status":     { "type": "string", "default": "accepted", "description": "accepted / pending / rejected / archived." },
                    "tags":       { "type": "array", "items": { "type": "string" } },
                    "channelId":  { "type": "string", "description": "Channel this memory was learned in. Drives channel-scoped retrieval." },
                    "threadId":   { "type": "string" },
                    "messageIds": { "type": "array", "items": { "type": "string" } }
                }
            }
        },
        {
            "name": "memory.get",
            "description": "Fetch a memory record by id.",
            "inputSchema": {
                "type": "object",
                "required": ["id"],
                "properties": { "id": { "type": "string" } }
            }
        }
    ])
}

fn handle_tool_call(
    actor_id: &str,
    store: &JsonlMemoryStore,
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
        "memory.query" => memory_query(store, &args),
        "memory.append" => memory_append(actor_id, store, &args),
        "memory.get" => memory_get(store, &args),
        other => Err(format!("unknown tool: {other}")),
    }
}

fn memory_query(store: &JsonlMemoryStore, args: &Value) -> Result<Vec<Value>, String> {
    let query = MemoryQuery {
        text: args.get("text").and_then(Value::as_str).map(str::to_string),
        tags: args
            .get("tags")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        types: args
            .get("types")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        channel_scope: args
            .get("channelId")
            .and_then(Value::as_str)
            .map(str::to_string),
        limit: args
            .get("limit")
            .and_then(Value::as_u64)
            .map(|n| n as usize)
            .unwrap_or(10),
        include_non_accepted: args
            .get("includeNonAccepted")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    };
    let records = store.query(&query).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(&records).unwrap_or_else(|_| "[]".into());
    Ok(vec![json!({ "type": "text", "text": text })])
}

fn memory_append(
    actor_id: &str,
    store: &JsonlMemoryStore,
    args: &Value,
) -> Result<Vec<Value>, String> {
    let summary = args
        .get("summary")
        .and_then(Value::as_str)
        .ok_or_else(|| "`summary` is required".to_string())?
        .to_string();
    if summary.trim().is_empty() {
        return Err("`summary` must be non-empty".into());
    }
    let record = MemoryRecord {
        schema_version: 1,
        id: format!("mem_{}", Uuid::new_v4().simple()),
        actor_id: actor_id.to_string(),
        ts: Utc::now().to_rfc3339(),
        record_type: args
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("note")
            .to_string(),
        status: args
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("accepted")
            .to_string(),
        summary,
        detail: args
            .get("detail")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        confidence: args
            .get("confidence")
            .and_then(Value::as_str)
            .unwrap_or("medium")
            .to_string(),
        source: MemorySource {
            channel_id: args
                .get("channelId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            thread_id: args
                .get("threadId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            message_ids: args
                .get("messageIds")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
        },
        tags: args
            .get("tags")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
    };
    store.append(&record).map_err(|e| e.to_string())?;
    let text = format!(
        "recorded {} (type={}, confidence={}, channel={})",
        record.id, record.record_type, record.confidence, record.source.channel_id
    );
    Ok(vec![json!({ "type": "text", "text": text })])
}

fn memory_get(store: &JsonlMemoryStore, args: &Value) -> Result<Vec<Value>, String> {
    let id = args
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "`id` is required".to_string())?;
    match store.get(id).map_err(|e| e.to_string())? {
        Some(record) => {
            let text = serde_json::to_string_pretty(&record).unwrap_or_else(|_| "{}".into());
            Ok(vec![json!({ "type": "text", "text": text })])
        }
        None => Err(format!("no record with id {id}")),
    }
}

fn ok_response(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
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

    fn tmpdir() -> PathBuf {
        let p = std::env::temp_dir().join(format!("joi-mcp-memory-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn store_at(dir: &std::path::Path) -> JsonlMemoryStore {
        JsonlMemoryStore::new(dir.to_path_buf())
    }

    #[test]
    fn initialize_returns_protocol_version() {
        let dir = tmpdir();
        let store = store_at(&dir);
        let resp = handle_request(
            "actor_x",
            &store,
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize" }),
        );
        let proto = resp["result"]["protocolVersion"].as_str().unwrap();
        assert_eq!(proto, PROTOCOL_VERSION);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn tools_list_has_three_tools() {
        let dir = tmpdir();
        let store = store_at(&dir);
        let resp = handle_request(
            "actor_x",
            &store,
            &json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
        );
        let tools = resp["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(names, vec!["memory.query", "memory.append", "memory.get"]);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn append_then_query_round_trips() {
        let dir = tmpdir();
        let store = store_at(&dir);
        let _append = handle_request(
            "actor_x",
            &store,
            &json!({
                "jsonrpc": "2.0", "id": 3, "method": "tools/call",
                "params": {
                    "name": "memory.append",
                    "arguments": {
                        "summary": "user prefers tabs",
                        "channelId": "ch1",
                        "confidence": "high",
                        "type": "preference"
                    }
                }
            }),
        );
        let query = handle_request(
            "actor_x",
            &store,
            &json!({
                "jsonrpc": "2.0", "id": 4, "method": "tools/call",
                "params": {
                    "name": "memory.query",
                    "arguments": { "text": "tabs", "channelId": "ch1" }
                }
            }),
        );
        let body = query["result"]["content"][0]["text"].as_str().unwrap();
        assert!(body.contains("user prefers tabs"));
        assert!(body.contains("actor_x"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn unknown_method_errors() {
        let dir = tmpdir();
        let store = store_at(&dir);
        let resp = handle_request(
            "actor_x",
            &store,
            &json!({ "jsonrpc": "2.0", "id": 5, "method": "nonsense/foo" }),
        );
        assert_eq!(resp["error"]["code"].as_i64().unwrap(), -32601);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn append_requires_summary() {
        let dir = tmpdir();
        let store = store_at(&dir);
        let resp = handle_request(
            "actor_x",
            &store,
            &json!({
                "jsonrpc": "2.0", "id": 6, "method": "tools/call",
                "params": {
                    "name": "memory.append",
                    "arguments": { "type": "note" }
                }
            }),
        );
        assert_eq!(resp["error"]["code"].as_i64().unwrap(), -32000);
        std::fs::remove_dir_all(dir).ok();
    }
}
