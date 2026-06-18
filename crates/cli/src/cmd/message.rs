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
    actor_id: String,
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
    let trigger_actor = std::env::var("LOOM_TRIGGER_ACTOR").ok();
    let inferred_reply = inferred_reply_audience(
        &target,
        &actor_id,
        &body,
        delivery_policy,
        trigger_actor.as_deref(),
    );
    if let Some(reply_actor_id) = inferred_reply.as_ref() {
        params["audience"] = json!([{ "kind": "actor", "id": reply_actor_id }]);
    }
    if let Some(if_latest) = if_latest.filter(|value| !value.trim().is_empty()) {
        params["ifLatestMessageId"] = json!(if_latest);
    }
    let res: MessageSendResult = client.call(method::MESSAGE_SEND, params).await?;
    if let Some(warning) = channel_fragmentation_warning(&target, !private_to.is_empty()) {
        eprintln!("{warning}");
    }
    let will_wake = !private_to.is_empty() || delivery_policy == Some(DeliveryPolicy::WakeAgent);
    let has_targeted_audience = !private_to.is_empty() || inferred_reply.is_some();
    if !will_wake && !has_targeted_audience && looks_like_call_for_action(&body) {
        eprintln!(
            "loom: warning: this message is notify_only and will wake nobody, but its \
             text looks like a call for others to act (discuss/vote/answer/your turn). \
             If you expect a response, send it with `loom message ask @actor_id ...` \
             (or `--private-to @actor_id` for a hidden prompt). A notify_only \
             call-for-action wakes no one and is the #1 cause of stalled multi-actor flows."
        );
    }
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
    let params = build_ask_params(target.clone(), recipients, body, if_latest, attachment_ids)?;
    let res: MessageSendResult = client.call(method::MESSAGE_SEND, params).await?;
    if let Some(warning) = channel_fragmentation_warning(&target, false) {
        eprintln!("{warning}");
    }
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!("message {}", res.message.id);
    }
    Ok(())
}

/// Best-effort, non-blocking heuristic: does this body read like a request for
/// other actors to act (discuss, vote, answer, take a turn)? Used only to print
/// a stderr nudge when such a message is sent notify_only (wakes nobody).
fn looks_like_call_for_action(body: &str) -> bool {
    let lower = body.to_lowercase();
    const CUES: &[&str] = &[
        "please discuss",
        "please vote",
        "please respond",
        "please answer",
        "please reply",
        "please choose",
        "please decide",
        "please share",
        "your turn",
        "take a turn",
        "cast your vote",
        "start the discussion",
        "open the floor",
        "请发言",
        "开始发言",
        "请讨论",
        "请投票",
        "请回复",
        "请回答",
        "请选择",
        "请决定",
        "轮到",
        "到你了",
        "大家发言",
        "各位发言",
        "投票开始",
        "开始投票",
    ];
    CUES.iter().any(|cue| lower.contains(cue))
}

/// Best-effort, non-blocking nudge: warn when a non-private message is being
/// posted to the bare channel root (`#<channel_id>`) even though this turn has a
/// thread reply target (`#<channel_id>:<root>`). Posting to the bare channel both
/// surfaces on the channel ("public board") and spawns a fresh thread rooted at
/// that message, fragmenting an activity that otherwise lives in one shared
/// thread. Returns the warning text (for testing) when the situation applies.
fn channel_fragmentation_warning(target: &str, scope_private: bool) -> Option<String> {
    let reply_target = std::env::var("LOOM_REPLY_TARGET").ok();
    channel_fragmentation_warning_inner(target, scope_private, reply_target.as_deref())
}

fn channel_fragmentation_warning_inner(
    target: &str,
    scope_private: bool,
    reply_target: Option<&str>,
) -> Option<String> {
    if scope_private {
        return None;
    }
    let target = target.trim();
    // Only same-scope channel targets matter; ignore dm:/global targets.
    if !target.starts_with('#') {
        return None;
    }
    // A thread target carries a `:`; a bare channel target does not.
    if target.contains(':') {
        return None;
    }
    let reply_target = reply_target?.trim();
    // Only warn when the established reply target IS a thread we are bypassing.
    if !reply_target.contains(':') || reply_target == target {
        return None;
    }
    Some(format!(
        "loom: warning: you sent this to the bare channel `{target}`, but this turn's \
         shared thread is `{reply_target}`. A bare-channel message shows on the channel \
         surface and starts a NEW thread, fragmenting the conversation. Send activity \
         messages to \"$LOOM_REPLY_TARGET\" so everyone stays in one thread."
    ))
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
        (None, None, true) => infer_current_scope_target(client, true).await,
        (Some(_), Some(_), _) => bail!("use either --target or --to, not both"),
        (None, Some(_), true) => unreachable!("--to/--private-to conflict checked earlier"),
        _ => bail!("missing destination: pass --target or --to"),
    }
}

async fn infer_current_scope_target(client: &Client, scope_private: bool) -> Result<String> {
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
        "channel" => current_channel_scope_target(&scope_id, scope_private),
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

fn current_channel_scope_target(scope_id: &str, scope_private: bool) -> Result<String> {
    if scope_private {
        bail!(
            "missing destination: same-scope private delivery from a channel turn would target the bare channel and create a new thread; pass --target \"$LOOM_REPLY_TARGET\" or an explicit #channel:root target, or pass --target #channel if a new root is intentional"
        );
    }
    Ok(format!("#{scope_id}"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_for_action_heuristic_flags_group_prompts_not_plain_info() {
        assert!(looks_like_call_for_action("🔔 各位玩家，请开始发言讨论"));
        assert!(looks_like_call_for_action("Okay everyone, please vote now"));
        assert!(looks_like_call_for_action("轮到你了，发表你的看法"));
        assert!(!looks_like_call_for_action(
            "天亮了，昨晚是平安夜，无人死亡。"
        ));
        assert!(!looks_like_call_for_action("Game over. Villagers win."));
    }

    #[test]
    fn channel_fragmentation_warns_only_when_bypassing_an_active_thread() {
        let thread = Some("#chan_x:msg_root");
        // Bare channel target while a thread reply target exists -> warn.
        assert!(channel_fragmentation_warning_inner("#chan_x", false, thread).is_some());
        // Already targeting the thread -> no warning.
        assert!(channel_fragmentation_warning_inner("#chan_x:msg_root", false, thread).is_none());
        // Private message (role card / hidden prompt) -> never warn.
        assert!(channel_fragmentation_warning_inner("#chan_x", true, thread).is_none());
        // Global DM target -> not our concern.
        assert!(channel_fragmentation_warning_inner("dm:@actor_x", false, thread).is_none());
        // No active thread (e.g. the channel itself is the reply target) -> no warning.
        assert!(channel_fragmentation_warning_inner("#chan_x", false, Some("#chan_x")).is_none());
        assert!(channel_fragmentation_warning_inner("#chan_x", false, None).is_none());
    }

    #[test]
    fn channel_scope_private_delivery_requires_explicit_target() {
        let error = current_channel_scope_target("chan_demo", true).unwrap_err();
        assert!(error.to_string().contains("explicit #channel:root"));
        assert_eq!(
            current_channel_scope_target("chan_demo", false).unwrap(),
            "#chan_demo"
        );
    }

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
