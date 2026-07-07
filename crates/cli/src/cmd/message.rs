use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use proto::methods::*;
use proto::types::{
    AudienceKind, AudienceRef, DeliveryPolicy, DeliveryState, Message, MessageIntent,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::client::Client;
use crate::config;
use crate::render;

const LONG_MESSAGE_BODY_CHAR_LIMIT: usize = 6_000;
const ENV_LONG_MESSAGE_DIR: &str = "LOOM_LONG_MESSAGE_DIR";

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
    let explicit_notify_intent = matches!(intent, Some(MessageIntent::Notify));
    let explicit_silent_policy = matches!(delivery_policy, Some(DeliveryPolicy::Silent));
    let infer_default_agent_reply = agent_turn_is_active() && !explicit_notify_intent;
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
    let trigger_actor = std::env::var("LOOM_TRIGGER_ACTOR").ok();
    let inferred_reply = apply_inferred_reply_audience(
        &mut params,
        is_private,
        &target,
        &actor_id,
        &body,
        delivery_policy,
        trigger_actor.as_deref(),
        infer_default_agent_reply,
    );
    if inferred_reply.is_some() && delivery_policy.is_none() {
        delivery_policy = Some(DeliveryPolicy::WakeAgent);
        params["deliveryPolicy"] = serde_json::to_value(DeliveryPolicy::WakeAgent)?;
    }
    if let Some(if_latest) = if_latest.filter(|value| !value.trim().is_empty()) {
        params["ifLatestMessageId"] = json!(if_latest);
    }
    let will_wake = !private_to.is_empty() || delivery_policy == Some(DeliveryPolicy::WakeAgent);
    let has_targeted_audience = !private_to.is_empty() || inferred_reply.is_some();
    if !will_wake && !has_targeted_audience && looks_like_call_for_action(&body) {
        let warning = notify_only_call_for_action_warning();
        if should_reject_notify_only_call_for_action(
            agent_turn_is_active(),
            explicit_notify_intent,
            explicit_silent_policy,
        ) {
            bail!(
                "{warning} This is blocked inside an agent run. Use `loom message ask @actor_id ...` \
                 or `loom message send --private-to @actor_id --target \"$LOOM_REPLY_TARGET\" ...` \
                 for hidden same-scope work. If this is intentionally a no-action notification, \
                 rerun with `--intent notify`."
            );
        }
        eprintln!("{warning}");
    }
    if let Some(warning) = channel_fragmentation_warning(&target, !private_to.is_empty()) {
        eprintln!("{warning}");
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
    let params = build_ask_params(target.clone(), recipients, body, if_latest, attachment_ids)?;
    if let Some(warning) = channel_fragmentation_warning(&target, false) {
        eprintln!("{warning}");
    }
    let res: MessageSendResult = client.call(method::MESSAGE_SEND, params).await?;
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

fn notify_only_call_for_action_warning() -> &'static str {
    "loom: warning: this message is notify_only and will wake nobody, but its text looks \
     like a call for others to act (discuss/vote/answer/your turn). If you expect a \
     response, send it with `loom message ask @actor_id ...` (or `--private-to @actor_id` \
     for a hidden prompt). A notify_only call-for-action wakes no one and is the #1 cause \
     of stalled multi-actor flows."
}

fn agent_turn_is_active() -> bool {
    std::env::var("LOOM_RUN_ID")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

fn should_reject_notify_only_call_for_action(
    agent_turn_active: bool,
    explicit_notify_intent: bool,
    explicit_silent_policy: bool,
) -> bool {
    agent_turn_active && !explicit_notify_intent && !explicit_silent_policy
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
        (None, None, None, true) => infer_current_scope_target(client, true).await,
        (Some(_), Some(_), _, _) => bail!("use either --target or --thread, not both"),
        (Some(_), None, Some(_), _) | (None, Some(_), Some(_), _) => {
            bail!("use either --target/--thread or --to, not both")
        }
        (None, None, Some(_), true) => unreachable!("--to/--private-to conflict checked earlier"),
        _ => bail!("missing destination: pass --target, --thread, or --to"),
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
    infer_default_agent_reply: bool,
) -> Option<&'a str> {
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
    let should_wake = delivery_policy == Some(DeliveryPolicy::WakeAgent)
        || (delivery_policy.is_none()
            && infer_default_agent_reply
            && trigger_actor.starts_with("actor_agent_"));
    if !should_wake {
        return None;
    }
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
    infer_default_agent_reply: bool,
) -> Option<String> {
    if is_private {
        return None;
    }
    let reply_actor_id = inferred_reply_audience(
        target,
        actor_id,
        body,
        delivery_policy,
        trigger_actor,
        infer_default_agent_reply,
    )?;
    params["audience"] = json!([{ "kind": "actor", "id": reply_actor_id }]);
    Some(reply_actor_id.to_string())
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
    fn agent_turn_rejects_notify_only_call_for_action_unless_explicitly_notify() {
        assert!(should_reject_notify_only_call_for_action(
            true, false, false
        ));
        assert!(!should_reject_notify_only_call_for_action(
            false, false, false
        ));
        assert!(!should_reject_notify_only_call_for_action(
            true, true, false
        ));
        assert!(!should_reject_notify_only_call_for_action(
            true, false, true
        ));
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
                false,
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
                false,
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
            false,
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
            false,
        );

        assert_eq!(params["audience"][0]["id"], "actor_agent_player");
    }

    #[test]
    fn agent_thread_reply_without_explicit_policy_infers_agent_requester() {
        let mut params = json!({
            "target": "#chan_1:msg_root",
            "body": "到",
        });

        let inferred = apply_inferred_reply_audience(
            &mut params,
            false,
            "#chan_1:msg_root",
            "actor_agent_player",
            "到",
            None,
            Some("actor_agent_dm"),
            true,
        );

        if inferred.is_some() {
            params["deliveryPolicy"] = serde_json::to_value(DeliveryPolicy::WakeAgent).unwrap();
        }

        assert_eq!(inferred.as_deref(), Some("actor_agent_dm"));
        assert_eq!(params["audience"][0]["id"], "actor_agent_dm");
        assert_eq!(params["deliveryPolicy"], "wake_agent");
    }

    #[test]
    fn default_reply_inference_does_not_wake_humans_or_explicit_notify() {
        assert_eq!(
            inferred_reply_audience(
                "#chan:msg_root",
                "actor_agent_worker",
                "done",
                None,
                Some("actor_human_owner"),
                true,
            ),
            None
        );
        assert_eq!(
            inferred_reply_audience(
                "#chan:msg_root",
                "actor_agent_worker",
                "done",
                None,
                Some("actor_agent_owner"),
                false,
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
    spill_long_message_bodies(&mut res.messages)?;
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

fn spill_long_message_bodies(messages: &mut [Message]) -> Result<()> {
    let dir = long_message_dir();
    spill_long_message_bodies_to_dir(messages, &dir)
}

fn spill_long_message_bodies_to_dir(messages: &mut [Message], dir: &Path) -> Result<()> {
    for message in messages {
        let char_count = message.body.chars().count();
        if char_count <= LONG_MESSAGE_BODY_CHAR_LIMIT {
            continue;
        }
        let byte_count = message.body.len();
        let path = save_long_message_body(message, dir)?;
        let path_text = path.display().to_string();
        message
            .metadata
            .insert("bodySavedTo".into(), json!(path_text));
        message
            .metadata
            .insert("bodySavedChars".into(), json!(char_count));
        message
            .metadata
            .insert("bodySavedBytes".into(), json!(byte_count));
        message.metadata.insert(
            "bodyOmittedReason".into(),
            json!("message body exceeded loom CLI query inline limit"),
        );
        message.body = format!(
            "[long message body omitted: {char_count} chars / {byte_count} bytes saved to {}]",
            path.display()
        );
    }
    Ok(())
}

fn save_long_message_body(message: &Message, dir: &Path) -> Result<PathBuf> {
    fs::create_dir_all(dir)
        .with_context(|| format!("create long message dir {}", dir.display()))?;
    let hash = Sha256::digest(message.body.as_bytes());
    let hash = hex::encode(hash);
    let filename = format!(
        "{}-{}.txt",
        sanitize_filename_component(&message.id),
        &hash[..12]
    );
    let path = dir.join(filename);
    fs::write(&path, &message.body)
        .with_context(|| format!("write long message body {}", path.display()))?;
    Ok(path)
}

fn long_message_dir() -> PathBuf {
    if let Some(value) = std::env::var_os(ENV_LONG_MESSAGE_DIR).filter(|value| !value.is_empty()) {
        return PathBuf::from(value);
    }
    config::config_dir().join("message-bodies")
}

fn sanitize_filename_component(raw: &str) -> String {
    let sanitized: String = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() {
        "message".into()
    } else {
        sanitized
    }
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

    #[test]
    fn long_message_bodies_are_saved_and_replaced() {
        let dir = tempfile::tempdir().expect("temp dir");
        let original_body = "x".repeat(LONG_MESSAGE_BODY_CHAR_LIMIT + 1);
        let mut message = sample_read_message();
        message.body = original_body.clone();

        spill_long_message_bodies_to_dir(std::slice::from_mut(&mut message), dir.path())
            .expect("spill long message");

        assert!(message.body.contains("long message body omitted"));
        let saved_to = message
            .metadata
            .get("bodySavedTo")
            .and_then(Value::as_str)
            .expect("bodySavedTo");
        assert!(Path::new(saved_to).starts_with(dir.path()));
        assert_eq!(
            fs::read_to_string(saved_to).expect("saved body"),
            original_body
        );
        assert_eq!(
            message
                .metadata
                .get("bodySavedChars")
                .and_then(Value::as_u64),
            Some((LONG_MESSAGE_BODY_CHAR_LIMIT + 1) as u64)
        );
    }

    #[test]
    fn short_message_bodies_stay_inline() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut message = sample_read_message();

        spill_long_message_bodies_to_dir(std::slice::from_mut(&mut message), dir.path())
            .expect("spill long message");

        assert_eq!(message.body, "hello");
        assert!(message.metadata.get("bodySavedTo").is_none());
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
    let mut res: InboxListResult = client.call(method::INBOX_LIST, params).await?;
    for entry in &mut res.deliveries {
        if let Some(message) = entry.message.as_mut() {
            spill_long_message_bodies(std::slice::from_mut(message))?;
        }
    }
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
    let mut res: MessageSearchResult = client
        .call(
            method::MESSAGE_SEARCH,
            json!({
                "query": query,
                "target": target,
                "limit": limit,
            }),
        )
        .await?;
    spill_long_message_bodies(&mut res.messages)?;
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
