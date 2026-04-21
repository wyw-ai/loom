use std::sync::Arc;

use anyhow::Result;
use proto::methods::*;
use serde_json::json;

use crate::client::Client;

pub async fn run(
    client: Arc<Client>,
    actor_id: String,
    thread_id: String,
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
    let payload = json!({
        "event": {
            "type": "content.add",
            "actorId": actor_id,
            "scope": { "kind": "thread", "id": thread_id },
            "payload": { "contentType": "text/markdown", "text": text },
            "relations": relations,
        }
    });
    let res: EventAppendResult = client.call(method::EVENT_APPEND, payload).await?;
    println!("event {}", res.event.id);
    Ok(())
}
