use std::collections::BTreeSet;
use std::io::Read;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use proto::methods::*;
use proto::types::{
    AudienceKind, AudienceRef, DeliveryPolicy, DeliveryState, Message, MessageIntent,
};
use serde_json::{json, Value};

use crate::client::Client;
use crate::render;

pub async fn send(
    client: Arc<Client>,
    actor_id: String,
    target: Option<String>,
    thread: Option<String>,
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
    let target =
        resolve_send_target(client.as_ref(), target, thread, to, !private_to.is_empty()).await?;
    let body = read_message_body(text)?;
    if body.trim().is_empty() && attachment_ids.is_empty() {
        bail!("message body is empty");
    }
    let is_private = !private_to.is_empty();
    let mut intent = parse_message_intent(intent)?;
    let mut delivery_policy = parse_delivery_policy(delivery_policy)?;
    if is_private {
        intent.get_or_insert(MessageIntent::RequestAction);
        delivery_policy.get_or_insert(DeliveryPolicy::WakeAgent);
    }
    let mut params = json!({
        "target": target,
        "body": body,
        "attachments": attachment_ids,
    });
    if is_private {
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
    apply_inferred_reply_audience(
        &mut params,
        is_private,
        &target,
        &actor_id,
        &body,
        delivery_policy,
        std::env::var("LOOM_TRIGGER_ACTOR").ok().as_deref(),
    );
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
    thread: Option<String>,
    recipients: Vec<String>,
    text: Option<String>,
    if_latest: Option<String>,
    attachment_ids: Vec<String>,
) -> Result<()> {
    let target = resolve_send_target(client.as_ref(), target, thread, None, false).await?;
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
    thread: Option<String>,
    to: Option<String>,
    scope_private: bool,
) -> Result<String> {
    let target = target
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let thread = thread
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let to = to
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    match (target, thread, to, scope_private) {
        (Some(target), None, None, _) => Ok(target),
        (None, Some(thread), None, _) => resolve_thread_target(client, &thread).await,
        (None, None, Some(to), false) => {
            let to = to.trim();
            if to.starts_with("dm:") {
                Ok(to.to_string())
            } else if to.starts_with('@') {
                Ok(format!("dm:{to}"))
            } else {
                Ok(format!("dm:@{to}"))
            }
        }
        (None, None, None, true) => infer_current_scope_target(client).await,
        (Some(_), Some(_), _, _) => bail!("use either --target or --thread, not both"),
        (Some(_), None, Some(_), _) | (None, Some(_), Some(_), _) => {
            bail!("use either --target/--thread or --to, not both")
        }
        (None, None, Some(_), true) => unreachable!("--to/--private-to conflict checked earlier"),
        _ => bail!("missing destination: pass --target or --to"),
    }
}

async fn resolve_thread_target(client: &Client, thread_id: &str) -> Result<String> {
    let res: ThreadListResult = client.call(method::THREAD_LIST, json!({})).await?;
    thread_target_from_list(thread_id, res.threads)
}

fn thread_target_from_list(thread_id: &str, threads: Vec<proto::types::Thread>) -> Result<String> {
    let thread_id = thread_id.trim();
    if thread_id.is_empty() {
        bail!("empty --thread value");
    }
    let thread = threads
        .into_iter()
        .find(|thread| thread.id == thread_id)
        .with_context(|| format!("missing destination: thread {thread_id} was not found"))?;
    Ok(format!("#{}:{}", thread.channel_id, thread.root_message_id))
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

fn apply_inferred_reply_audience(
    params: &mut Value,
    is_private: bool,
    target: &str,
    actor_id: &str,
    body: &str,
    delivery_policy: Option<DeliveryPolicy>,
    trigger_actor: Option<&str>,
) {
    if is_private {
        return;
    }
    if let Some(reply_actor_id) =
        inferred_reply_audience(target, actor_id, body, delivery_policy, trigger_actor)
    {
        params["audience"] = json!([{ "kind": "actor", "id": reply_actor_id }]);
    }
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

    #[test]
    fn private_send_keeps_private_audience_when_reply_inference_matches() {
        let mut params = json!({
            "target": "#chan_1:msg_root",
            "body": "【私信·身份】你的身份是狼人。",
            "audience": [{ "kind": "actor", "id": "actor_agent_player" }],
            "metadata": {
                "private": true,
                "privateTo": ["actor_agent_player"],
            },
            "intent": MessageIntent::RequestAction,
            "deliveryPolicy": DeliveryPolicy::WakeAgent,
        });

        apply_inferred_reply_audience(
            &mut params,
            true,
            "#chan_1:msg_root",
            "actor_agent_dm",
            "【私信·身份】你的身份是狼人。",
            Some(DeliveryPolicy::WakeAgent),
            Some("actor_human_local"),
        );

        assert_eq!(params["audience"][0]["id"], "actor_agent_player");
        assert_eq!(params["metadata"]["privateTo"][0], "actor_agent_player");
    }

    #[test]
    fn public_thread_wake_reply_infers_trigger_actor_audience() {
        let mut params = json!({
            "target": "#chan_1:msg_root",
            "body": "继续。",
            "intent": MessageIntent::RequestAction,
            "deliveryPolicy": DeliveryPolicy::WakeAgent,
        });

        apply_inferred_reply_audience(
            &mut params,
            false,
            "#chan_1:msg_root",
            "actor_agent_dm",
            "继续。",
            Some(DeliveryPolicy::WakeAgent),
            Some("actor_agent_player"),
        );

        assert_eq!(params["audience"][0]["id"], "actor_agent_player");
    }

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

    #[test]
    fn thread_target_uses_thread_root_message() {
        let target = thread_target_from_list(
            "thread_1",
            vec![proto::types::Thread {
                id: "thread_1".into(),
                channel_id: "chan_1".into(),
                title: "topic".into(),
                root_message_id: "msg_root".into(),
                archived_at: None,
                _meta: None,
            }],
        )
        .expect("thread target");

        assert_eq!(target, "#chan_1:msg_root");
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
    target: Option<String>,
    thread: Option<String>,
    limit: u32,
    before: Option<String>,
    include_private: bool,
) -> Result<()> {
    let target = resolve_read_target(client.as_ref(), target, thread).await?;
    let mut params = json!({
        "target": target,
        "limit": limit,
    });
    if let Some(before) = before {
        params["beforeMessageId"] = json!(before);
    }
    let mut res: MessageListResult = client.call(method::MESSAGE_LIST, params).await?;
    if !include_private {
        res.messages
            .retain(|message| !is_same_scope_private(message));
    }
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

async fn resolve_read_target(
    client: &Client,
    target: Option<String>,
    thread: Option<String>,
) -> Result<String> {
    match (
        target
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        thread
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
    ) {
        (Some(target), None) => Ok(target),
        (None, Some(thread)) => resolve_thread_target(client, &thread).await,
        (Some(_), Some(_)) => bail!("use either --target or --thread, not both"),
        (None, None) => bail!("missing destination: pass --target or --thread"),
    }
}

fn is_same_scope_private(message: &Message) -> bool {
    message
        .metadata
        .get("private")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || message
            .metadata
            .get("visibility")
            .and_then(Value::as_str)
            .is_some_and(|value| value.eq_ignore_ascii_case("private"))
        || message.metadata.contains_key("privateTo")
        || message.metadata.contains_key("privateActorIds")
}

#[cfg(test)]
mod read_tests {
    use super::*;
    use proto::types::{MessageKind, ScopeKind, ScopeRef};

    fn sample_read_message() -> Message {
        Message {
            id: "msg_1".into(),
            scope: ScopeRef {
                kind: ScopeKind::Thread,
                id: "thread_1".into(),
            },
            target: "#chan_1:msg_root".into(),
            author_actor_id: "actor_agent_a".into(),
            created_at: chrono::Utc::now(),
            kind: MessageKind::Agent,
            body: "hello".into(),
            mentions: Vec::new(),
            audience: Vec::new(),
            intent: MessageIntent::Chat,
            delivery_policy: DeliveryPolicy::NotifyOnly,
            parent_message_id: None,
            thread_root_message_id: Some("msg_root".into()),
            task_id: None,
            attachments: Vec::new(),
            reactions: Vec::new(),
            metadata: Default::default(),
        }
    }

    #[test]
    fn same_scope_private_detection_catches_private_to_metadata() {
        let mut message = sample_read_message();
        assert!(!is_same_scope_private(&message));

        message
            .metadata
            .insert("privateTo".into(), json!(["actor_agent_b"]));

        assert!(is_same_scope_private(&message));
    }
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
