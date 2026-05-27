use std::io::{self, IsTerminal, Read};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use proto::methods::{method, stream_kind, MessageListResult, MessageSendResult, ThreadListResult};
use proto::types::{
    AudienceKind, DeliveryPolicy, Message, MessageIntent, Meta, ScopeKind, ScopeRef,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::time::Instant;
use uuid::Uuid;

use crate::client::Client;
use crate::render;

const DEFAULT_TIMEOUT_SECONDS: u64 = 3600;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AskUserQuestionInput {
    #[serde(default)]
    header: Option<String>,
    #[serde(default)]
    title: Option<String>,
    question: String,
    #[serde(default)]
    choices: Vec<QuestionChoice>,
    #[serde(default)]
    allow_freeform: bool,
    #[serde(default)]
    target_actor: Option<String>,
    #[serde(default)]
    scope: Option<ScopeRef>,
    #[serde(default)]
    turn_id: Option<String>,
    #[serde(default)]
    timeout_seconds: Option<u64>,
    #[serde(default)]
    meta: Value,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RequestApprovalInput {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    approve_label: Option<String>,
    #[serde(default)]
    reject_label: Option<String>,
    #[serde(default)]
    target_actor: Option<String>,
    #[serde(default)]
    scope: Option<ScopeRef>,
    #[serde(default)]
    turn_id: Option<String>,
    #[serde(default)]
    timeout_seconds: Option<u64>,
    #[serde(default)]
    meta: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QuestionChoice {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AskUserQuestionResult {
    status: String,
    request_message_id: String,
    request_id: String,
    question: ReturnedQuestion,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    answered_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    option_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    answer: Option<ReturnedAnswer>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RequestApprovalResult {
    status: String,
    request_message_id: String,
    request_id: String,
    request: ReturnedApprovalRequest,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    approved: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    approved_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    option_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReturnedQuestion {
    title: String,
    text: String,
    choices: Vec<QuestionChoice>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReturnedAnswer {
    /// This is the human's answer to the question asked by this tool call.
    answered_by: String,
    option_id: Option<String>,
    label: Option<String>,
    kind: Option<String>,
    text: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReturnedApprovalRequest {
    title: String,
    reason: String,
}

pub struct RunArgs {
    pub timeout_seconds: Option<u64>,
    pub scope_id: Option<String>,
    pub is_channel: bool,
    pub to: Option<String>,
    pub turn_id: Option<String>,
    pub title: Option<String>,
    pub question: Option<String>,
    pub choices: Vec<String>,
    pub allow_freeform: bool,
}

pub struct ApprovalArgs {
    pub timeout_seconds: Option<u64>,
    pub scope_id: Option<String>,
    pub is_channel: bool,
    pub to: Option<String>,
    pub turn_id: Option<String>,
    pub title: Option<String>,
    pub reason: Option<String>,
    pub approve_label: Option<String>,
    pub reject_label: Option<String>,
}

pub async fn run(client: Arc<Client>, _actor_id: String, args: RunArgs) -> Result<()> {
    let input = read_input(&args)?;
    let scope = resolve_scope(args.scope_id, args.is_channel, input.scope)?;
    let target_actor = first_non_empty(args.to, input.target_actor)
        .or_else(|| std::env::var("LOOM_TRIGGER_ACTOR").ok())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            anyhow!("missing target actor; pass --to or run inside a daemon turn with LOOM_TRIGGER_ACTOR")
        })?;
    let turn_id = first_non_empty(args.turn_id, input.turn_id)
        .or_else(|| std::env::var("LOOM_RUN_ID").ok())
        .filter(|value| !value.trim().is_empty());
    let timeout_seconds = args
        .timeout_seconds
        .or(input.timeout_seconds)
        .unwrap_or(DEFAULT_TIMEOUT_SECONDS);
    let question_text = input.question;
    let choices = merge_choices(input.choices, &args.choices)?;
    if choices.is_empty() && !input.allow_freeform && !args.allow_freeform {
        bail!("question requires at least one choice; pass --choice id=label or provide choices in JSON");
    }

    let request_id = format!("loom:question:{}", Uuid::new_v4());
    let title = first_non_empty(args.title, input.header.or(input.title))
        .unwrap_or_else(|| "Question".into());
    let allow_freeform = input.allow_freeform || args.allow_freeform;
    let mut payload = json!({
        "requestId": request_id,
        "requestType": "question",
        "title": title,
        "description": question_text,
        "choices": choices,
        "allowFreeform": allow_freeform,
    });
    if !input.meta.is_null() {
        payload["_meta"] = input.meta;
    }

    let sent = append_action_request(&client, scope, turn_id, target_actor, payload).await?;

    let mut result = AskUserQuestionResult {
        status: "waiting".into(),
        request_message_id: sent.message.id.clone(),
        request_id: request_id.clone(),
        question: ReturnedQuestion {
            title: title.clone(),
            text: question_text.clone(),
            choices: choices.clone(),
        },
        response_message_id: None,
        answered_by: None,
        option_id: None,
        kind: None,
        text: None,
        answer: None,
    };

    match wait_for_response(
        &client,
        &sent.message.target,
        &sent.message.id,
        Duration::from_secs(timeout_seconds),
    )
    .await?
    {
        Some(response) => {
            result.status = "answered".into();
            result.response_message_id = Some(response.id.clone());
            result.answered_by = Some(response.author_actor_id.clone());
            let option_id = response
                .metadata
                .get("optionId")
                .and_then(|value| value.as_str())
                .map(ToOwned::to_owned);
            let kind = response
                .metadata
                .get("responseKind")
                .or_else(|| response.metadata.get("kind"))
                .and_then(|value| value.as_str())
                .map(ToOwned::to_owned);
            let text = response
                .metadata
                .get("text")
                .and_then(|value| value.as_str())
                .map(ToOwned::to_owned);
            let label = option_id
                .as_deref()
                .and_then(|id| choices.iter().find(|choice| choice.id == id))
                .map(|choice| choice.label.clone());
            result.option_id = option_id.clone();
            result.kind = kind.clone();
            result.text = text.clone();
            result.answer = Some(ReturnedAnswer {
                answered_by: response.author_actor_id,
                option_id,
                label,
                kind,
                text,
            });
        }
        None => {
            result.status = "timed_out".into();
        }
    }

    if render::is_json() {
        render::print_json(&result);
    } else {
        println!(
            "{}\t{}\t{}",
            result.status, result.request_message_id, result.request_id
        );
        if let Some(option_id) = result.option_id {
            println!("option = {option_id}");
        }
        if let Some(text) = result.text {
            println!("text = {text}");
        }
    }
    Ok(())
}

pub async fn run_approval(
    client: Arc<Client>,
    _actor_id: String,
    args: ApprovalArgs,
) -> Result<()> {
    let input = read_approval_input(&args)?;
    let scope = resolve_scope(args.scope_id, args.is_channel, input.scope)?;
    let target_actor = first_non_empty(args.to, input.target_actor)
        .or_else(|| std::env::var("LOOM_TRIGGER_ACTOR").ok())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            anyhow!("missing target actor; pass --to or run inside a daemon turn with LOOM_TRIGGER_ACTOR")
        })?;
    let turn_id = first_non_empty(args.turn_id, input.turn_id)
        .or_else(|| std::env::var("LOOM_RUN_ID").ok())
        .filter(|value| !value.trim().is_empty());
    let timeout_seconds = args
        .timeout_seconds
        .or(input.timeout_seconds)
        .unwrap_or(DEFAULT_TIMEOUT_SECONDS);
    let title =
        first_non_empty(args.title, input.title).unwrap_or_else(|| "Approval required".into());
    let reason = first_non_empty(args.reason, input.reason.or(input.description))
        .ok_or_else(|| anyhow!("approval request requires --reason or JSON reason/description"))?;
    let approve_label = first_non_empty(args.approve_label, input.approve_label)
        .unwrap_or_else(|| "Approve".into());
    let reject_label =
        first_non_empty(args.reject_label, input.reject_label).unwrap_or_else(|| "Reject".into());
    let choices = vec![
        QuestionChoice {
            id: "approve".into(),
            label: approve_label,
            description: None,
        },
        QuestionChoice {
            id: "reject".into(),
            label: reject_label,
            description: None,
        },
    ];

    let request_id = format!("loom:approval:{}", Uuid::new_v4());
    let mut payload = json!({
        "requestId": request_id,
        "requestType": "approval",
        "title": title,
        "description": reason,
        "choices": choices,
        "allowFreeform": false,
    });
    if !input.meta.is_null() {
        payload["_meta"] = input.meta;
    }

    let sent = append_action_request(&client, scope, turn_id, target_actor, payload).await?;

    let mut result = RequestApprovalResult {
        status: "waiting".into(),
        request_message_id: sent.message.id.clone(),
        request_id,
        request: ReturnedApprovalRequest { title, reason },
        response_message_id: None,
        approved: None,
        approved_by: None,
        option_id: None,
        kind: None,
    };

    match wait_for_response(
        &client,
        &sent.message.target,
        &sent.message.id,
        Duration::from_secs(timeout_seconds),
    )
    .await?
    {
        Some(response) => {
            let option_id = response
                .metadata
                .get("optionId")
                .and_then(|value| value.as_str())
                .map(ToOwned::to_owned);
            let kind = response
                .metadata
                .get("responseKind")
                .or_else(|| response.metadata.get("kind"))
                .and_then(|value| value.as_str())
                .map(ToOwned::to_owned);
            let approved =
                option_id.as_deref() == Some("approve") || kind.as_deref() == Some("accepted");
            result.status = if approved { "approved" } else { "rejected" }.into();
            result.response_message_id = Some(response.id);
            result.approved = Some(approved);
            result.approved_by = Some(response.author_actor_id);
            result.option_id = option_id;
            result.kind = kind;
        }
        None => {
            result.status = "timed_out".into();
        }
    }

    if render::is_json() {
        render::print_json(&result);
    } else {
        println!(
            "{}\t{}\t{}",
            result.status, result.request_message_id, result.request_id
        );
        if let Some(approved) = result.approved {
            println!("approved = {approved}");
        }
    }
    Ok(())
}

fn read_input(args: &RunArgs) -> Result<AskUserQuestionInput> {
    if let Some(question) = args
        .question
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        return Ok(AskUserQuestionInput {
            header: None,
            title: args.title.clone(),
            question: question.clone(),
            choices: Vec::new(),
            allow_freeform: args.allow_freeform,
            target_actor: None,
            scope: None,
            turn_id: None,
            timeout_seconds: args.timeout_seconds,
            meta: Value::Null,
        });
    }
    if io::stdin().is_terminal() {
        bail!("provide --question or pipe a JSON question payload on stdin");
    }
    let mut buf = String::new();
    io::stdin().read_to_string(&mut buf)?;
    let input: AskUserQuestionInput =
        serde_json::from_str(&buf).context("parse ask-user-question JSON input")?;
    Ok(input)
}

fn read_approval_input(args: &ApprovalArgs) -> Result<RequestApprovalInput> {
    if args
        .reason
        .as_ref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        return Ok(RequestApprovalInput {
            title: args.title.clone(),
            reason: args.reason.clone(),
            description: None,
            approve_label: args.approve_label.clone(),
            reject_label: args.reject_label.clone(),
            target_actor: None,
            scope: None,
            turn_id: None,
            timeout_seconds: args.timeout_seconds,
            meta: Value::Null,
        });
    }
    if io::stdin().is_terminal() {
        bail!("provide --reason or pipe a JSON approval payload on stdin");
    }
    let mut buf = String::new();
    io::stdin().read_to_string(&mut buf)?;
    let input: RequestApprovalInput =
        serde_json::from_str(&buf).context("parse request-approval JSON input")?;
    Ok(input)
}

async fn append_action_request(
    client: &Arc<Client>,
    scope: ScopeRef,
    turn_id: Option<String>,
    target_actor: String,
    payload: Value,
) -> Result<MessageSendResult> {
    let _: Value = client
        .call(method::SCOPE_SUBSCRIBE, json!({ "scope": scope.clone() }))
        .await
        .with_context(|| {
            format!(
                "scope/subscribe {}:{}",
                scope_kind_name(scope.kind),
                scope.id
            )
        })?;

    let mut metadata = action_request_metadata(payload);
    if let Some(run_id) = turn_id.filter(|value| !value.trim().is_empty()) {
        metadata.insert("runId".into(), json!(run_id));
    }
    let target = message_target_for_scope(client, &scope).await?;
    let body = action_request_body(&metadata);
    client
        .call(
            method::MESSAGE_SEND,
            json!({
                "target": target,
                "body": body,
                "audience": [{ "kind": AudienceKind::Actor, "id": target_actor }],
                "intent": MessageIntent::RequestAction,
                "deliveryPolicy": DeliveryPolicy::WakeAgent,
                "metadata": metadata,
            }),
        )
        .await
        .map_err(Into::into)
}

fn action_request_metadata(payload: Value) -> Meta {
    let mut metadata = Meta::default();
    metadata.insert("kind".into(), json!("action.request"));
    match payload {
        Value::Object(map) => {
            for (key, value) in map {
                metadata.insert(key, value);
            }
        }
        other => {
            metadata.insert("payload".into(), other);
        }
    }
    metadata
}

fn action_request_body(metadata: &Meta) -> String {
    let title = metadata
        .get("title")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("Question");
    let description = metadata
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let mut body = format!("Action requested: {title}");
    if !description.is_empty() {
        body.push_str("\n\n");
        body.push_str(description);
    }
    if let Some(choices) = metadata.get("choices").and_then(Value::as_array) {
        for choice in choices {
            let id = choice.get("id").and_then(Value::as_str).unwrap_or("");
            let label = choice.get("label").and_then(Value::as_str).unwrap_or("");
            if !id.is_empty() || !label.is_empty() {
                body.push_str(&format!("\n- {id}: {label}"));
            }
        }
    }
    body
}

fn resolve_scope(
    scope_id: Option<String>,
    is_channel: bool,
    input_scope: Option<ScopeRef>,
) -> Result<ScopeRef> {
    if let Some(scope_id) = scope_id.filter(|value| !value.trim().is_empty()) {
        return Ok(ScopeRef {
            kind: if is_channel {
                ScopeKind::Channel
            } else {
                ScopeKind::Thread
            },
            id: scope_id,
        });
    }
    if let Some(scope) = input_scope {
        return Ok(scope);
    }
    let id = std::env::var("LOOM_SCOPE_ID")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("missing scope; pass --in or run inside a daemon turn"))?;
    let kind = match std::env::var("LOOM_SCOPE_KIND")
        .unwrap_or_else(|_| "thread".into())
        .as_str()
    {
        "channel" => ScopeKind::Channel,
        "thread" => ScopeKind::Thread,
        other => bail!("invalid LOOM_SCOPE_KIND `{other}`; expected thread or channel"),
    };
    Ok(ScopeRef { kind, id })
}

fn merge_choices(
    mut input_choices: Vec<QuestionChoice>,
    flag_choices: &[String],
) -> Result<Vec<QuestionChoice>> {
    for raw in flag_choices {
        input_choices.push(parse_choice(raw)?);
    }
    Ok(input_choices)
}

fn parse_choice(raw: &str) -> Result<QuestionChoice> {
    let (id, label) = raw
        .split_once('=')
        .ok_or_else(|| anyhow!("choice `{raw}` must be formatted as id=label"))?;
    let id = id.trim();
    let label = label.trim();
    if id.is_empty() || label.is_empty() {
        bail!("choice `{raw}` must include non-empty id and label");
    }
    Ok(QuestionChoice {
        id: id.into(),
        label: label.into(),
        description: None,
    })
}

async fn wait_for_response(
    client: &Arc<Client>,
    target: &str,
    request_message_id: &str,
    timeout: Duration,
) -> Result<Option<Message>> {
    if let Some(response) = find_response_in_messages(client, target, request_message_id).await? {
        return Ok(Some(response));
    }
    let deadline = Instant::now() + timeout;
    let mut rx = client.notifications.lock().await;
    loop {
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            return Ok(None);
        };
        let notification = match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Some(notification)) => notification,
            Ok(None) => return Ok(None),
            Err(_) => return Ok(None),
        };
        if let Some(message) = response_message_from_notification(notification, request_message_id)
        {
            return Ok(Some(message));
        }
    }
}

async fn find_response_in_messages(
    client: &Arc<Client>,
    target: &str,
    request_message_id: &str,
) -> Result<Option<Message>> {
    // Best-effort race closer for very fast responders. The notification path
    // is primary; fall back to the bounded message history for this scope.
    let res: MessageListResult = client
        .call(
            method::MESSAGE_LIST,
            json!({ "target": target, "limit": 200 }),
        )
        .await?;
    Ok(res
        .messages
        .into_iter()
        .find(|message| is_response_to(message, request_message_id)))
}

fn response_message_from_notification(
    notification: proto::Notification,
    request_message_id: &str,
) -> Option<Message> {
    if notification.method != method::STREAM_UPDATE {
        return None;
    }
    let params = notification.params?;
    if params.get("kind").and_then(|value| value.as_str()) != Some(stream_kind::MESSAGE_CREATED) {
        return None;
    }
    let message: Message =
        serde_json::from_value(params.get("data")?.get("message")?.clone()).ok()?;
    is_response_to(&message, request_message_id).then_some(message)
}

fn is_response_to(message: &Message, request_message_id: &str) -> bool {
    message.metadata.get("kind").and_then(Value::as_str) == Some("action.response")
        && (message.parent_message_id.as_deref() == Some(request_message_id)
            || message
                .metadata
                .get("requestMessageId")
                .and_then(Value::as_str)
                == Some(request_message_id))
}

fn first_non_empty(left: Option<String>, right: Option<String>) -> Option<String> {
    left.filter(|value| !value.trim().is_empty())
        .or_else(|| right.filter(|value| !value.trim().is_empty()))
}

fn scope_kind_name(kind: ScopeKind) -> &'static str {
    match kind {
        ScopeKind::Channel => "channel",
        ScopeKind::Thread => "thread",
    }
}

async fn message_target_for_scope(client: &Arc<Client>, scope: &ScopeRef) -> Result<String> {
    match scope.kind {
        ScopeKind::Channel => Ok(format!("#{}", scope.id)),
        ScopeKind::Thread => {
            let res: ThreadListResult = client
                .call(method::THREAD_LIST, json!({ "archived": false }))
                .await
                .context("thread/list")?;
            let thread = res
                .threads
                .into_iter()
                .find(|thread| thread.id == scope.id)
                .ok_or_else(|| anyhow!("thread {} not found", scope.id))?;
            Ok(format!("#{}:{}", thread.channel_id, thread.root_message_id))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn parse_choice_requires_id_and_label() {
        let choice = parse_choice("approve=Approve").unwrap();
        assert_eq!(
            choice,
            QuestionChoice {
                id: "approve".into(),
                label: "Approve".into(),
                description: None,
            }
        );
        assert!(parse_choice("approve").is_err());
        assert!(parse_choice("=Approve").is_err());
    }

    #[test]
    fn response_notification_matches_parent_message() {
        let mut metadata = Meta::default();
        metadata.insert("kind".into(), json!("action.response"));
        metadata.insert("optionId".into(), json!("approve"));
        let message = Message {
            id: "msg_response".into(),
            scope: ScopeRef {
                kind: ScopeKind::Thread,
                id: "thr_1".into(),
            },
            target: "#chan:msg_root".into(),
            author_actor_id: "human_alice".into(),
            created_at: Utc::now(),
            kind: proto::types::MessageKind::Human,
            body: "approved".into(),
            mentions: Vec::new(),
            audience: Vec::new(),
            intent: MessageIntent::Notify,
            delivery_policy: DeliveryPolicy::WakeAgent,
            parent_message_id: Some("msg_request".into()),
            thread_root_message_id: None,
            task_id: None,
            attachments: Vec::new(),
            reactions: Vec::new(),
            metadata,
        };
        let notification = proto::Notification {
            jsonrpc: "2.0".into(),
            method: method::STREAM_UPDATE.into(),
            params: Some(json!({
                "kind": stream_kind::MESSAGE_CREATED,
                "scope": message.scope,
                "data": { "message": message },
            })),
        };

        let matched = response_message_from_notification(notification, "msg_request").unwrap();
        assert_eq!(matched.id, "msg_response");
        assert_eq!(
            matched
                .metadata
                .get("optionId")
                .and_then(|value| value.as_str()),
            Some("approve")
        );
    }
}
