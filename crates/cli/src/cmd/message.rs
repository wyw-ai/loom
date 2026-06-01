use std::collections::BTreeSet;
use std::io::Read;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use proto::methods::*;
use proto::types::{AudienceKind, AudienceRef, DeliveryPolicy, DeliveryState, MessageIntent};
use serde_json::{json, Value};

use crate::client::Client;
use crate::render;

pub async fn send(
    client: Arc<Client>,
    _actor_id: String,
    target: Option<String>,
    to: Option<String>,
    private_to: Vec<String>,
    text: Option<String>,
    intent: Option<String>,
    delivery_policy: Option<String>,
    if_latest: Option<String>,
    attachment_ids: Vec<String>,
) -> Result<()> {
    let private_to = normalize_actor_ids(private_to)?;
    if to.is_some() && !private_to.is_empty() {
        bail!("use either --to for global DM or --private-to for same-scope private delivery, not both");
    }
    let target = resolve_send_target(client.as_ref(), target, to, !private_to.is_empty()).await?;
    let body = read_message_body(text)?;
    if body.trim().is_empty() && attachment_ids.is_empty() {
        bail!("message body is empty");
    }
    let mut intent = parse_message_intent(intent)?;
    let mut delivery_policy = parse_delivery_policy(delivery_policy)?;
    if !private_to.is_empty() {
        intent.get_or_insert(MessageIntent::RequestAction);
        delivery_policy.get_or_insert(DeliveryPolicy::WakeAgent);
    }
    let mut params = json!({
        "target": target,
        "body": body,
        "attachments": attachment_ids,
    });
    if !private_to.is_empty() {
        let audience: Vec<AudienceRef> = private_to
            .iter()
            .map(|actor_id| AudienceRef {
                kind: AudienceKind::Actor,
                id: actor_id.clone(),
                display: None,
            })
            .collect();
        params["audience"] = serde_json::to_value(audience)?;
        params["metadata"] = json!({
            "private": true,
            "privateTo": private_to,
        });
    }
    if let Some(intent) = intent {
        params["intent"] = serde_json::to_value(intent)?;
    }
    if let Some(delivery_policy) = delivery_policy {
        params["deliveryPolicy"] = serde_json::to_value(delivery_policy)?;
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

pub async fn ask(
    client: Arc<Client>,
    _actor_id: String,
    target: Option<String>,
    recipients: Vec<String>,
    text: Option<String>,
    if_latest: Option<String>,
    attachment_ids: Vec<String>,
) -> Result<()> {
    let target = resolve_send_target(client.as_ref(), target, None, false).await?;
    let body = read_message_body(text)?;
    if body.trim().is_empty() && attachment_ids.is_empty() {
        bail!("message body is empty");
    }
    let params = build_ask_params(target, recipients, body, if_latest, attachment_ids)?;
    let res: MessageSendResult = client.call(method::MESSAGE_SEND, params).await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!("message {}", res.message.id);
    }
    Ok(())
}

fn read_message_body(text: Option<String>) -> Result<String> {
    match text {
        Some(t) => Ok(t),
        None => {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .context("read stdin")?;
            Ok(buf)
        }
    }
}

fn build_ask_params(
    target: String,
    recipients: Vec<String>,
    body: String,
    if_latest: Option<String>,
    attachment_ids: Vec<String>,
) -> Result<Value> {
    let audience = normalize_audience_refs(recipients)?;
    let mut params = json!({
        "target": target,
        "body": body,
        "attachments": attachment_ids,
        "audience": audience,
        "intent": MessageIntent::Ask,
        "deliveryPolicy": DeliveryPolicy::WakeAgent,
    });
    if let Some(if_latest) = if_latest.filter(|value| !value.trim().is_empty()) {
        params["ifLatestMessageId"] = json!(if_latest);
    }
    Ok(params)
}

async fn resolve_send_target(
    client: &Client,
    target: Option<String>,
    to: Option<String>,
    scope_private: bool,
) -> Result<String> {
    match (target, to, scope_private) {
        (Some(target), None, _) if !target.trim().is_empty() => Ok(target),
        (None, Some(to), false) if !to.trim().is_empty() => {
            let to = to.trim();
            if to.starts_with("dm:") {
                Ok(to.to_string())
            } else if to.starts_with('@') {
                Ok(format!("dm:{to}"))
            } else {
                Ok(format!("dm:@{to}"))
            }
        }
        (None, None, true) => infer_current_scope_target(client).await,
        (Some(_), Some(_), _) => bail!("use either --target or --to, not both"),
        (None, Some(_), true) => unreachable!("--to/--private-to conflict checked earlier"),
        _ => bail!("missing destination: pass --target or --to"),
    }
}

async fn infer_current_scope_target(client: &Client) -> Result<String> {
    if let Some(target) = std::env::var("LOOM_REPLY_TARGET")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return Ok(target);
    }
    let scope_id = std::env::var("LOOM_SCOPE_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .context("missing destination: pass --target or run inside a Loom agent turn")?;
    let scope_kind = std::env::var("LOOM_SCOPE_KIND")
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .context("missing destination: pass --target or run inside a Loom agent turn")?;
    match scope_kind.as_str() {
        "channel" => Ok(format!("#{scope_id}")),
        "thread" => {
            let mut params = json!({});
            if let Some(channel_id) = std::env::var("LOOM_CHANNEL_ID")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
            {
                params["channelId"] = json!(channel_id);
            }
            let res: ThreadListResult = client.call(method::THREAD_LIST, params).await?;
            let thread = res
                .threads
                .into_iter()
                .find(|thread| thread.id == scope_id)
                .context("missing destination: current thread scope was not found")?;
            Ok(format!("#{}:{}", thread.channel_id, thread.root_message_id))
        }
        _ => bail!("missing destination: unsupported LOOM_SCOPE_KIND `{scope_kind}`"),
    }
}

fn normalize_actor_ids(raw_values: Vec<String>) -> Result<Vec<String>> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for raw in raw_values {
        for part in raw.split(',') {
            let mut value = part.trim();
            if let Some(rest) = value.strip_prefix("dm:") {
                value = rest.trim();
            }
            if let Some(rest) = value.strip_prefix('@') {
                value = rest.trim();
            }
            if value.is_empty() {
                continue;
            }
            if seen.insert(value.to_string()) {
                out.push(value.to_string());
            }
        }
    }
    Ok(out)
}

fn normalize_audience_refs(raw_values: Vec<String>) -> Result<Vec<AudienceRef>> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for raw in raw_values {
        for part in raw.split(',') {
            let mut value = part.trim();
            if let Some(rest) = value.strip_prefix('@') {
                value = rest.trim();
            }
            if value.is_empty() {
                continue;
            }
            let lower = value.to_ascii_lowercase();
            let (kind, id) = match lower.as_str() {
                "all" => (AudienceKind::All, "all".to_string()),
                "agents" => (AudienceKind::Agents, "agents".to_string()),
                "humans" => (AudienceKind::Humans, "humans".to_string()),
                _ => {
                    if let Some(group_id) = value.strip_prefix("group:") {
                        let group_id = group_id.trim();
                        if group_id.is_empty() {
                            bail!("empty group recipient in message ask");
                        }
                        (AudienceKind::Group, group_id.to_string())
                    } else if value.starts_with("actor_") {
                        (AudienceKind::Actor, value.to_string())
                    } else {
                        bail!(
                            "invalid message ask recipient `{value}`; use an actor id like @actor_..., @all, @agents, @humans, or group:<id>"
                        );
                    }
                }
            };
            let seen_key = format!("{kind:?}:{id}");
            if seen.insert(seen_key) {
                out.push(AudienceRef {
                    kind,
                    id,
                    display: None,
                });
            }
        }
    }
    if out.is_empty() {
        bail!("message ask requires at least one recipient");
    }
    Ok(out)
}

fn parse_message_intent(raw: Option<String>) -> Result<Option<MessageIntent>> {
    raw.map(|value| {
        serde_json::from_value::<MessageIntent>(json!(value.trim())).with_context(|| {
            "invalid --intent; expected chat, ask, request_action, assign_task, status_update, review, or notify"
        })
    })
    .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_ask_params_wake_single_actor() {
        let params = build_ask_params(
            "#chan_1:msg_root".into(),
            vec!["@actor_agent_qzz_729cf432".into()],
            "Q仔，请开始白天发言。".into(),
            Some("msg_latest".into()),
            Vec::new(),
        )
        .expect("build ask params");

        assert_eq!(params["target"], "#chan_1:msg_root");
        assert_eq!(params["intent"], "ask");
        assert_eq!(params["deliveryPolicy"], "wake_agent");
        assert_eq!(params["ifLatestMessageId"], "msg_latest");
        assert_eq!(params["audience"][0]["kind"], "actor");
        assert_eq!(params["audience"][0]["id"], "actor_agent_qzz_729cf432");
    }

    #[test]
    fn message_ask_params_support_multiple_and_all_recipients() {
        let params = build_ask_params(
            "#chan_1".into(),
            vec![
                "@actor_agent_a".into(),
                "actor_agent_b,@all,@agents,@humans,group:reviewers".into(),
            ],
            "please respond".into(),
            None,
            Vec::new(),
        )
        .expect("build ask params");
        let audience = params["audience"].as_array().expect("audience array");

        assert_eq!(audience.len(), 6);
        assert_eq!(audience[0]["kind"], "actor");
        assert_eq!(audience[0]["id"], "actor_agent_a");
        assert_eq!(audience[1]["kind"], "actor");
        assert_eq!(audience[1]["id"], "actor_agent_b");
        assert_eq!(audience[2]["kind"], "all");
        assert_eq!(audience[2]["id"], "all");
        assert_eq!(audience[3]["kind"], "agents");
        assert_eq!(audience[3]["id"], "agents");
        assert_eq!(audience[4]["kind"], "humans");
        assert_eq!(audience[4]["id"], "humans");
        assert_eq!(audience[5]["kind"], "group");
        assert_eq!(audience[5]["id"], "reviewers");
    }

    #[test]
    fn message_ask_rejects_display_name_recipient() {
        let err = build_ask_params(
            "#chan_1".into(),
            vec!["@Q仔".into()],
            "please respond".into(),
            None,
            Vec::new(),
        )
        .expect_err("display name should not be accepted");

        assert!(err.to_string().contains("use an actor id"));
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

pub async fn reaction_toggle(
    client: Arc<Client>,
    _actor_id: String,
    message_id: String,
    emoji: String,
) -> Result<()> {
    let res: MessageReactionToggleResult = client
        .call(
            method::MESSAGE_REACTION_TOGGLE,
            json!({
                "messageId": message_id,
                "emoji": emoji,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!("message {}", res.message.id);
    }
    Ok(())
}

pub async fn inbox_list(
    client: Arc<Client>,
    actor_id: String,
    limit: u32,
    ack: bool,
    state: String,
) -> Result<()> {
    let state_filter = parse_delivery_state_filter(&state)?;
    let mut params = json!({
        "actorId": actor_id,
        "limit": limit,
    });
    if let Some(state_filter) = state_filter {
        params["state"] = serde_json::to_value(state_filter)?;
    }
    let res: InboxListResult = client.call(method::INBOX_LIST, params).await?;
    if render::is_json() {
        render::print_json(&res);
    } else if res.deliveries.is_empty() {
        println!("(no inbox items)");
    } else {
        for entry in &res.deliveries {
            if let Some(message) = entry.message.as_ref() {
                render::render_message(message);
            } else if let Some(event) = entry.event.as_ref() {
                println!(
                    "{:?} event {}\t{}\t{}",
                    entry.delivery.state, event.id, event.kind, event.occurred_at
                );
            } else {
                println!(
                    "{:?} delivery {}",
                    entry.delivery.state, entry.delivery.source_id
                );
            }
        }
    }
    if ack {
        for entry in res.deliveries {
            if entry.delivery.state != DeliveryState::Pending {
                continue;
            }
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

fn parse_delivery_state_filter(raw: &str) -> Result<Option<DeliveryState>> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "" | "pending" => Ok(Some(DeliveryState::Pending)),
        "delivered" => Ok(Some(DeliveryState::Delivered)),
        "failed" => Ok(Some(DeliveryState::Failed)),
        "all" => Ok(None),
        other => bail!("invalid --state `{other}`; expected pending, delivered, failed, or all"),
    }
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
