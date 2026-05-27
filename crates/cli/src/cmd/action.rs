use std::sync::Arc;

use anyhow::{anyhow, Result};
use proto::methods::*;
use proto::types::{AudienceKind, DeliveryPolicy, Message, MessageIntent};
use serde_json::json;

use crate::client::Client;

pub async fn respond(
    client: Arc<Client>,
    actor_id: String,
    message_id: String,
    option_id: String,
    accepted: bool,
) -> Result<()> {
    let request_message = fetch_message(&client, &message_id).await?;
    if request_message
        .metadata
        .get("kind")
        .and_then(|value| value.as_str())
        != Some("action.request")
    {
        return Err(anyhow!("message {} is not an action.request", message_id));
    }
    let kind = if accepted { "accepted" } else { "declined" };
    let mut metadata = json!({
        "kind": "action.response",
        "optionId": option_id.clone(),
        "responseKind": kind,
        "requestMessageId": message_id.clone(),
    });
    if let Some(request_id) = request_message
        .metadata
        .get("requestId")
        .and_then(|value| value.as_str())
    {
        metadata["requestId"] = json!(request_id);
    }
    let _r: MessageSendResult = client
        .call(
            method::MESSAGE_SEND,
            json!({
                "target": request_message.target,
                "body": format!("{kind}: {option_id}"),
                "audience": [{ "kind": AudienceKind::Actor, "id": request_message.author_actor_id }],
                "intent": MessageIntent::Notify,
                "deliveryPolicy": DeliveryPolicy::WakeAgent,
                "parentMessageId": message_id.clone(),
                "metadata": metadata,
            }),
        )
        .await?;
    let _: DeliveryAckResult = client
        .call(
            method::DELIVERY_ACK,
            json!({
                "actorId": actor_id,
                "sourceId": message_id,
            }),
        )
        .await?;
    println!("ok");
    Ok(())
}

async fn fetch_message(client: &Client, message_id: &str) -> Result<Message> {
    let res: MessageReadResult = client
        .call(method::MESSAGE_READ, json!({ "messageId": message_id }))
        .await?;
    Ok(res.message)
}
