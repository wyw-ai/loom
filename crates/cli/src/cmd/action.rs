use std::sync::Arc;

use anyhow::{anyhow, Result};
use proto::methods::*;
use proto::types::{Event, ScopeKind};
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
    let request_event = fetch_event(&client, &event_id).await?;
    if request_event.kind != "action.request" {
        return Err(anyhow!("event {} is not an action.request", event_id));
    }
    let kind = if accepted { "accepted" } else { "declined" };
    let mut response_payload = json!({ "optionId": option_id.clone(), "kind": kind });
    if let Some(request_id) = request_event
        .payload
        .get("requestId")
        .and_then(|value| value.as_str())
    {
        response_payload["requestId"] = json!(request_id);
    }
    let resp_payload = json!({
        "event": {
            "type": "action.response",
            "actorId": actor_id.clone(),
            "scope": request_event.scope,
            "payload": response_payload,
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

async fn fetch_event(client: &Client, event_id: &str) -> Result<Event> {
    // We don't have a direct event/get RPC. Infer scope by walking every thread
    // the server knows about and matching event ids — fine for v0 demo scale.
    let lst: ThreadListResult = client.call(method::THREAD_LIST, json!({})).await?;
    for t in lst.threads {
        let res: ScopeReadResult = client
            .call(
                method::SCOPE_READ,
                json!({ "scope": { "kind": "thread", "id": t.id }, "limit": 200 }),
            )
            .await?;
        for ev in res.events {
            if ev.id == event_id {
                return Ok(ev);
            }
        }
    }
    let channels: ChannelListResult = client.call(method::CHANNEL_LIST, json!({})).await?;
    for c in channels.channels {
        let res: ScopeReadResult = client
            .call(
                method::SCOPE_READ,
                json!({ "scope": { "kind": ScopeKind::Channel, "id": c.id }, "limit": 200 }),
            )
            .await?;
        for ev in res.events {
            if ev.id == event_id {
                return Ok(ev);
            }
        }
    }
    Err(anyhow!("could not find event {}", event_id))
}
