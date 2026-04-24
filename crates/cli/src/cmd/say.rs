use std::sync::Arc;

use anyhow::Result;
use proto::methods::*;
use proto::types::{ScopeKind, ScopeRef};
use serde_json::json;

use crate::client::Client;
use crate::render;

pub async fn run(
    client: Arc<Client>,
    actor_id: String,
    scope_id: String,
    is_channel: bool,
    text: String,
    reply: Option<String>,
) -> Result<()> {
    let mut relations = Vec::new();
    if let Some(reply_to) = reply {
        relations.push(json!({
            "kind": "replies_to",
            "target": { "kind": "event", "id": reply_to }
        }));
    }
    let scope = ScopeRef {
        kind: if is_channel {
            ScopeKind::Channel
        } else {
            ScopeKind::Thread
        },
        id: scope_id,
    };
    let payload = json!({
        "event": {
            "type": "content.add",
            "actorId": actor_id,
            "scope": scope,
            "payload": { "contentType": "text/markdown", "text": text },
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
