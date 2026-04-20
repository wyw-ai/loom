use std::sync::Arc;

use anyhow::Result;
use proto::methods::*;
use proto::types::ScopeKind;
use serde_json::json;

use crate::client::Client;
use crate::render;

pub async fn list(
    client: Arc<Client>,
    scope_id: String,
    is_space: bool,
    limit: u32,
    before: Option<String>,
) -> Result<()> {
    let kind = if is_space {
        ScopeKind::Space
    } else {
        ScopeKind::Conversation
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
            println!("(more events before {} — use --before {})", first.id, first.id);
        }
    }
    Ok(())
}
