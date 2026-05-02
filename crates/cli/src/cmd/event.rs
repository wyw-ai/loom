use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use proto::methods::*;
use proto::types::{ScopeKind, ScopeRef};
use serde_json::json;

use crate::client::Client;
use crate::render;

pub async fn list(
    client: Arc<Client>,
    scope_id: String,
    is_channel: bool,
    limit: u32,
    before: Option<String>,
) -> Result<()> {
    let kind = if is_channel {
        ScopeKind::Channel
    } else {
        ScopeKind::Thread
    };
    let mut params = json!({
        "scope": { "kind": kind, "id": scope_id },
        "limit": limit,
    });
    if let Some(b) = before {
        params["beforeEventId"] = json!(b);
    }
    let res: ScopeReadResult = client.call(method::SCOPE_READ, params).await?;
    if render::is_json() {
        render::print_json(&res);
        return Ok(());
    }
    if res.events.is_empty() {
        println!("(no events)");
    }
    for ev in &res.events {
        render::render_event(ev);
    }
    if res.page_info.has_more {
        if let Some(first) = res.events.first() {
            println!(
                "(more events before {} — use --before {})",
                first.id, first.id
            );
        }
    }
    Ok(())
}

/// Inputs to `joi event append` — gathered by main.rs and handed off
/// to a single emitter so all relation kinds (reply, handoff, artifact
/// link) flow through the same code path.
pub struct AppendArgs {
    pub actor_id: String,
    pub scope_id: String,
    pub is_channel: bool,
    pub event_type: String,
    pub content_type: String,
    pub text: Option<String>,
    pub file: Option<PathBuf>,
    pub stdin: bool,
    pub reply_to: Option<String>,
    pub handoff_to: Option<String>,
    pub artifact_links: Vec<String>,
}

pub async fn append(client: Arc<Client>, args: AppendArgs) -> Result<()> {
    let mut relations: Vec<serde_json::Value> = Vec::new();
    if let Some(reply) = &args.reply_to {
        relations.push(json!({
            "kind": "replies_to",
            "target": { "kind": "event", "id": reply }
        }));
    }
    if let Some(target) = &args.handoff_to {
        relations.push(json!({
            "kind": "hands_off_to",
            "target": { "kind": "actor", "id": target }
        }));
    }
    for uri in &args.artifact_links {
        relations.push(json!({
            "kind": "links",
            "target": { "kind": "artifact", "id": uri }
        }));
    }

    let body = read_event_body(&args)?;

    let scope = ScopeRef {
        kind: if args.is_channel {
            ScopeKind::Channel
        } else {
            ScopeKind::Thread
        },
        id: args.scope_id,
    };
    let payload = json!({
        "event": {
            "type": args.event_type,
            "actorId": args.actor_id,
            "scope": scope,
            "payload": { "contentType": args.content_type, "text": body },
            "relations": relations,
        }
    });
    let res: EventAppendResult = client.call(method::EVENT_APPEND, payload).await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!("event {}", res.event.id);
    }
    Ok(())
}

fn read_event_body(args: &AppendArgs) -> Result<String> {
    let provided = [args.text.is_some(), args.file.is_some(), args.stdin]
        .iter()
        .filter(|b| **b)
        .count();
    if provided > 1 {
        return Err(anyhow!("--text / --file / --stdin are mutually exclusive"));
    }
    if let Some(t) = &args.text {
        return Ok(t.clone());
    }
    if let Some(p) = &args.file {
        return std::fs::read_to_string(p)
            .with_context(|| format!("read {}", p.display()));
    }
    if args.stdin {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
        return Ok(buf);
    }
    // No body: tolerated for relation-only events (e.g. pure handoff /
    // artifact link). Server will store an empty payload.text.
    Ok(String::new())
}
