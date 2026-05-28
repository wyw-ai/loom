use std::io::Read;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use proto::methods::*;
use proto::types::{DeliveryPolicy, MessageIntent};
use serde_json::json;

use crate::client::Client;
use crate::render;

pub async fn send(
    client: Arc<Client>,
    actor_id: String,
    target: Option<String>,
    to: Option<String>,
    text: Option<String>,
    intent: Option<String>,
    delivery_policy: Option<String>,
    if_latest: Option<String>,
    attachment_ids: Vec<String>,
) -> Result<()> {
    let target = resolve_send_target(target, to)?;
    let body = match text {
        Some(t) => t,
        None => {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .context("read stdin")?;
            buf
        }
    };
    if body.trim().is_empty() && attachment_ids.is_empty() {
        bail!("message body is empty");
    }
    let intent = parse_message_intent(intent)?;
    let delivery_policy = parse_delivery_policy(delivery_policy)?;
    let mut params = json!({
        "target": target,
        "body": body,
        "attachments": attachment_ids,
    });
    if let Some(intent) = intent {
        params["intent"] = serde_json::to_value(intent)?;
    }
    if let Some(delivery_policy) = delivery_policy {
        params["deliveryPolicy"] = serde_json::to_value(delivery_policy)?;
    }
    if let Some(reply_actor_id) = inferred_reply_audience(
        &target,
        &actor_id,
        &body,
        delivery_policy,
        std::env::var("LOOM_TRIGGER_ACTOR").ok().as_deref(),
    ) {
        params["audience"] = json!([{ "kind": "actor", "id": reply_actor_id }]);
    }
    if let Some(if_latest) = if_latest.filter(|value| !value.trim().is_empty()) {
        params["ifLatestMessageId"] = json!(if_latest);
    }
    let res: MessageSendResult = client.call(method::MESSAGE_SEND, params).await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!("message {}", res.message.id);
    }
    Ok(())
}

fn resolve_send_target(target: Option<String>, to: Option<String>) -> Result<String> {
    match (target, to) {
        (Some(target), None) if !target.trim().is_empty() => Ok(target),
        (None, Some(to)) if !to.trim().is_empty() => {
            let to = to.trim();
            if to.starts_with("dm:") {
                Ok(to.to_string())
            } else if to.starts_with('@') {
                Ok(format!("dm:{to}"))
            } else {
                Ok(format!("dm:@{to}"))
            }
        }
        (Some(_), Some(_)) => bail!("use either --target or --to, not both"),
        _ => bail!("missing destination: pass --target or --to"),
    }
}

fn parse_message_intent(raw: Option<String>) -> Result<Option<MessageIntent>> {
    raw.map(|value| {
        serde_json::from_value::<MessageIntent>(json!(value.trim())).with_context(|| {
            "invalid --intent; expected chat, ask, request_action, assign_task, status_update, review, or notify"
        })
    })
        .transpose()
}

fn inferred_reply_audience<'a>(
    target: &str,
    actor_id: &str,
    body: &str,
    delivery_policy: Option<DeliveryPolicy>,
    trigger_actor: Option<&'a str>,
) -> Option<&'a str> {
    if delivery_policy != Some(DeliveryPolicy::WakeAgent) {
        return None;
    }
    if !target.trim().starts_with('#') || !target.contains(':') {
        return None;
    }
    if body.contains('@') {
        return None;
    }
    let trigger_actor = trigger_actor
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter(|value| *value != actor_id)
        .filter(|value| value.starts_with("actor_"))?;
    Some(trigger_actor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infers_thread_wake_reply_audience_from_trigger_actor() {
        assert_eq!(
            inferred_reply_audience(
                "#chan:msg_root",
                "actor_agent_maker",
                "偏小，继续猜。",
                Some(DeliveryPolicy::WakeAgent),
                Some("actor_agent_guesser"),
            ),
            Some("actor_agent_guesser")
        );
    }

    #[test]
    fn explicit_mentions_disable_reply_audience_inference() {
        assert_eq!(
            inferred_reply_audience(
                "#chan:msg_root",
                "actor_agent_maker",
                "@actor_agent_guesser 偏小，继续猜。",
                Some(DeliveryPolicy::WakeAgent),
                Some("actor_agent_guesser"),
            ),
            None
        );
    }
}

fn parse_delivery_policy(raw: Option<String>) -> Result<Option<DeliveryPolicy>> {
    raw.map(|value| {
        serde_json::from_value::<DeliveryPolicy>(json!(value.trim())).with_context(|| {
            "invalid --delivery-policy; expected notify_only, wake_agent, route_by_intent, or silent"
        })
    })
    .transpose()
}

pub async fn read(
    client: Arc<Client>,
    _actor_id: String,
    target: String,
    limit: u32,
    before: Option<String>,
) -> Result<()> {
    let mut params = json!({
        "target": target,
        "limit": limit,
    });
    if let Some(before) = before {
        params["beforeMessageId"] = json!(before);
    }
    let res: MessageListResult = client.call(method::MESSAGE_LIST, params).await?;
    if render::is_json() {
        render::print_json(&res);
        return Ok(());
    }
    for message in &res.messages {
        render::render_message(message);
    }
    if res.messages.is_empty() {
        println!("(no messages)");
    }
    Ok(())
}

pub async fn inbox_list(
    client: Arc<Client>,
    actor_id: String,
    limit: u32,
    ack: bool,
) -> Result<()> {
    let res: InboxListResult = client
        .call(
            method::INBOX_LIST,
            json!({
                "actorId": actor_id,
                "state": "pending",
                "limit": limit,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else if res.deliveries.is_empty() {
        println!("(no pending inbox items)");
    } else {
        for entry in &res.deliveries {
            if let Some(message) = entry.message.as_ref() {
                render::render_message(message);
            } else {
                println!("pending delivery {}", entry.delivery.source_id);
            }
        }
    }
    if ack {
        for entry in res.deliveries {
            let _: DeliveryAckResult = client
                .call(
                    method::DELIVERY_ACK,
                    json!({
                        "actorId": entry.delivery.actor_id,
                        "sourceId": entry.delivery.source_id,
                    }),
                )
                .await?;
        }
    }
    Ok(())
}

pub async fn search(
    client: Arc<Client>,
    _actor_id: String,
    query: String,
    target: Option<String>,
    limit: u32,
) -> Result<()> {
    let res: MessageSearchResult = client
        .call(
            method::MESSAGE_SEARCH,
            json!({
                "query": query,
                "target": target,
                "limit": limit,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else if res.messages.is_empty() {
        println!("(no matches)");
    } else {
        for message in &res.messages {
            render::render_message(message);
        }
    }
    Ok(())
}
