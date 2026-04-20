use std::sync::Arc;

use anyhow::{anyhow, Result};
use proto::methods::*;
use serde_json::json;

use crate::client::Client;

pub async fn respond(
    client: Arc<Client>,
    actor_id: String,
    event_id: String,
    option_id: String,
    accepted: bool,
) -> Result<()> {
    // Look up the action.request event to get its scope.
    let scope_payload = fetch_event_scope(&client, &event_id).await?;
    let kind = if accepted { "accepted" } else { "declined" };
    let resp_payload = json!({
        "event": {
            "type": "action.response",
            "actorId": actor_id.clone(),
            "scope": scope_payload,
            "payload": { "optionId": option_id.clone(), "kind": kind },
            "relations": [
                { "kind": "responds_to", "target": { "kind": "event", "id": event_id.clone() } }
            ],
        }
    });
    let _r: EventAppendResult = client.call(method::EVENT_APPEND, resp_payload).await?;
    let receipt_kind = if accepted { "accepted" } else { "declined" };
    let _: serde_json::Value = client
        .call_raw(
            method::RECEIPT_RECORD,
            Some(json!({
                "eventId": event_id,
                "actorId": actor_id,
                "kind": receipt_kind,
            })),
        )
        .await?;
    println!("ok");
    Ok(())
}

async fn fetch_event_scope(client: &Client, event_id: &str) -> Result<serde_json::Value> {
    // We don't have a direct event/get RPC; use scope/read with no scope info isn't possible.
    // For v0, infer scope by asking the user to also pass it; simplify by reading recent
    // history on a hypothesis isn't practical. Punt: require the action.request to live in
    // the only conversation the actor knows. Use conversation/list as a fallback.
    let lst: ConversationListResult = client.call(method::CONVERSATION_LIST, json!({})).await?;
    for c in lst.conversations {
        let res: ScopeReadResult = client
            .call(
                method::SCOPE_READ,
                json!({ "scope": { "kind": "conversation", "id": c.id }, "limit": 200 }),
            )
            .await?;
        for ev in res.events {
            if ev.id == event_id {
                return Ok(json!({ "kind": "conversation", "id": c.id }));
            }
        }
    }
    Err(anyhow!(
        "could not find event {} in any conversation",
        event_id
    ))
}
