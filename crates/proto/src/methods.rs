use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::*;

// ---- method names (canonical, slash-form) ----

pub mod method {
    pub const INITIALIZE: &str = "initialize";
    pub const CONNECTION_OPEN: &str = "connection/open";
    pub const CONNECTION_CLOSE: &str = "connection/close";
    pub const CONNECTION_LIST: &str = "connection/list";
    pub const SCOPE_SUBSCRIBE: &str = "scope/subscribe";
    pub const SCOPE_UNSUBSCRIBE: &str = "scope/unsubscribe";
    pub const SCOPE_READ: &str = "scope/read";
    pub const CHANNEL_CREATE: &str = "channel/create";
    pub const CHANNEL_LIST: &str = "channel/list";
    pub const CHANNEL_UPDATE: &str = "channel/update";
    pub const CHANNEL_DELETE: &str = "channel/delete";
    pub const CHANNEL_INVITE: &str = "channel/invite";
    pub const CHANNEL_REVOKE: &str = "channel/revoke";
    pub const CHANNEL_MEMBERS: &str = "channel/members";
    pub const THREAD_CREATE: &str = "thread/create";
    pub const THREAD_LIST: &str = "thread/list";
    pub const THREAD_UPDATE: &str = "thread/update";
    pub const THREAD_DELETE: &str = "thread/delete";
    pub const TURN_OPEN: &str = "turn/open";
    pub const TURN_CLOSE: &str = "turn/close";
    pub const TURN_TRACE_READ: &str = "turn/trace.read";
    pub const TURN_TRACE_UPDATE: &str = "turn/trace.update";
    pub const TURN_TRACE_APPEND: &str = "turn/trace.append";
    pub const EVENT_APPEND: &str = "event/append";
    pub const MESSAGE_SEARCH: &str = "message/search";
    pub const ARTIFACT_PUBLISH: &str = "artifact/publish";
    pub const ARTIFACT_GET: &str = "artifact/get";
    pub const ARTIFACT_READ: &str = "artifact/read";
    pub const RECEIPT_RECORD: &str = "receipt/record";
    pub const REMINDER_SCHEDULE: &str = "reminder/schedule";
    pub const REMINDER_LIST: &str = "reminder/list";
    pub const REMINDER_CANCEL: &str = "reminder/cancel";
    pub const REMINDER_SNOOZE: &str = "reminder/snooze";
    pub const REMINDER_UPDATE: &str = "reminder/update";
    /// §9.2 Durable actor inbox. Caller (must be bound to `actorId`) lists
    /// deliveries pending against its inbox, with cursor pagination so a
    /// host can resume after restart without losing directed events.
    pub const DELIVERY_LIST: &str = "delivery/list";
    pub const ACTOR_LIST: &str = "actor/list";
    pub const ACTOR_UPSERT: &str = "actor/upsert";
    pub const ACTOR_DELETE: &str = "actor/delete";

    // outbound notification
    pub const STREAM_UPDATE: &str = "stream/update";
}

// ---- initialize ----

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    pub name: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    #[serde(default)]
    pub protocol_version: String,
    #[serde(default)]
    pub client_info: Option<ClientInfo>,
    #[serde(default)]
    pub client_capabilities: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub name: String,
    pub title: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub protocol_version: String,
    pub server_info: ServerInfo,
    pub server_capabilities: Value,
}

// ---- connection/open ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionOpenParams {
    pub actor_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_kind: Option<ActorKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<Endpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionOpenResult {
    pub connection: Connection,
    pub actor: Actor,
}

// ---- connection/close ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionCloseParams {
    pub connection_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionCloseResult {
    pub closed: bool,
}

// ---- connection/list ----

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionListParams {
    /// Optional actor id filter. Empty means every currently-bound actor.
    #[serde(default)]
    pub actor_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionListResult {
    /// Actor ids that currently own a live actor inbox connection.
    pub actor_ids: Vec<String>,
}

// ---- scope/subscribe ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeOnlyParams {
    pub scope: ScopeRef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeSubscribeResult {
    pub mode: String,
    pub created_at: Timestamp,
    pub actor_id: String,
    pub scope: ScopeRef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeUnsubscribeResult {
    pub unsubscribed: bool,
}

// ---- scope/read ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeReadParams {
    pub scope: ScopeRef,
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_event_id: Option<String>,
}

fn default_limit() -> u32 {
    50
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeReadResult {
    pub events: Vec<Event>,
    pub page_info: PageInfo,
}

// ---- channel/create ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelCreateParams {
    pub title: String,
    /// When provided, the new channel is created `Private` and the creator
    /// is its sole initial member. When omitted, the channel is created
    /// `Public`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelCreateResult {
    pub channel: Channel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelListResult {
    pub channels: Vec<Channel>,
}

// ---- channel/invite + channel/revoke + channel/members ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelInviteParams {
    pub channel_id: String,
    pub actor_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelInviteResult {
    pub channel: Channel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelRevokeParams {
    pub channel_id: String,
    pub actor_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelRevokeResult {
    pub channel: Channel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelMembersParams {
    pub channel_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMembersResult {
    /// Resolved actor rows, not just ids — the chat sidebar needs the
    /// display name + kind to render rows.
    pub members: Vec<Actor>,
}

// ---- channel/update / delete ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelUpdateParams {
    pub channel_id: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelUpdateResult {
    pub channel: Channel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelDeleteParams {
    pub channel_id: String,
    /// When true, child threads are deleted in the same call before the
    /// channel itself is removed. Default false preserves the safe-by-default
    /// behavior: server refuses to delete a non-empty channel.
    #[serde(default)]
    pub cascade: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelDeleteResult {
    pub deleted: bool,
    /// Number of child threads removed when `cascade=true`. Zero on the
    /// non-cascade path (the empty-channel happy case) and on no-op deletes.
    #[serde(default)]
    pub deleted_threads: u32,
}

// ---- thread/create / list ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadCreateParams {
    pub channel_id: String,
    pub title: String,
    pub root_event_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadCreateResult {
    pub thread: Thread,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadListResult {
    pub threads: Vec<Thread>,
}

// ---- thread/update / delete ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadUpdateParams {
    pub thread_id: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadUpdateResult {
    pub thread: Thread,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadDeleteParams {
    pub thread_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadDeleteResult {
    pub deleted: bool,
}

// ---- turn/open / close ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnOpenParams {
    pub actor_id: String,
    pub scope: ScopeRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_event_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnOpenResult {
    pub turn: Turn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnCloseParams {
    pub turn_id: String,
    #[serde(default = "default_close_status")]
    pub status: TurnStatus,
}

fn default_close_status() -> TurnStatus {
    TurnStatus::Closed
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnCloseResult {
    pub turn: Turn,
}

// ---- turn/trace.read ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnTraceReadParams {
    pub turn_id: String,
    #[serde(default = "default_trace_limit")]
    pub limit: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_seq: Option<u64>,
}

fn default_trace_limit() -> u32 {
    100
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnTraceReadResult {
    pub frames: Vec<crate::types::trace::TraceFrame>,
    pub page_info: PageInfo,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acp_transport_deserializes_without_interactive_fields() {
        let transport: AgentTransport = serde_json::from_str(
            r#"{
                "kind": "acp_stdio",
                "command": "claude-acp",
                "args": [],
                "env": {}
            }"#,
        )
        .unwrap();
        assert_eq!(transport.kind, "acp_stdio");
        assert!(transport.model.is_none());
        assert!(transport.interactive.is_none());
        assert!(transport.provider.is_none());
    }

    #[test]
    fn interactive_transport_deserializes_model_and_claude_settings() {
        let transport: AgentTransport = serde_json::from_str(
            r#"{
                "kind": "interactive_command",
                "command": "claude",
                "model": "claude-sonnet-4.6",
                "interactive": {
                    "session": {
                        "newArgs": ["{prompt}", "--session-id", "{session_id}"],
                        "resumeArgs": ["{prompt}", "--resume", "{session_id}"]
                    }
                },
                "provider": {
                    "kind": "claude",
                    "settings": { "mode": "actor_profile" }
                }
            }"#,
        )
        .unwrap();
        assert_eq!(transport.kind, "interactive_command");
        assert_eq!(transport.model.as_deref(), Some("claude-sonnet-4.6"));
        let interactive = transport.interactive.unwrap();
        assert_eq!(interactive.session.new_args.len(), 3);
        let provider = transport.provider.unwrap();
        assert_eq!(provider.kind, "claude");
        assert_eq!(
            provider.settings.unwrap().mode,
            ClaudeSettingsMode::ActorProfile
        );
    }
}

// ---- turn/trace.append (external client → server) ----

/// Append a turn-private trace frame from an external agent client. v0 wrote
/// trace frames directly through the in-server runtime; v1 lets `joi agent
/// serve` push them via this RPC instead. Server fans the new frame out as a
/// `turn/trace.update` notification to the turn owner just like the embedded
/// path.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnTraceAppendParams {
    pub turn_id: String,
    pub kind: crate::types::trace::TraceKind,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnTraceAppendResult {
    pub frame: crate::types::trace::TraceFrame,
}

// ---- turn/trace.update notification ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnTraceUpdate {
    pub turn_id: String,
    pub frame: crate::types::trace::TraceFrame,
}

// ---- event/append ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventAppendInput {
    #[serde(rename = "type")]
    pub kind: String,
    pub actor_id: String,
    pub scope: ScopeRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default)]
    pub payload: Value,
    #[serde(default)]
    pub relations: Vec<Relation>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Meta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventAppendParams {
    pub event: EventAppendInput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventAppendResult {
    pub event: Event,
}

// ---- message/search ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSearchParams {
    pub query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<ScopeRef>,
    #[serde(default = "default_search_limit")]
    pub limit: u32,
}

fn default_search_limit() -> u32 {
    20
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSearchResult {
    pub events: Vec<Event>,
}

// ---- artifact/publish / get / read ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ArtifactIngress {
    InlineText(InlineTextIngress),
    FileBytes(FileBytesIngress),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlineTextIngress {
    pub name: String,
    #[serde(default = "default_text_media_type")]
    pub media_type: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileBytesIngress {
    pub name: String,
    #[serde(default = "default_octet_stream_media_type")]
    pub media_type: String,
    pub bytes: Vec<u8>,
}

fn default_text_media_type() -> String {
    "text/markdown".into()
}

fn default_octet_stream_media_type() -> String {
    "application/octet-stream".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactPublishParams {
    pub ingress: ArtifactIngress,
    pub created_by: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<ScopeRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactPublishResult {
    pub artifact: Artifact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactGetParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_uri: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactGetResult {
    pub artifact: Artifact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactReadParams {
    pub artifact_id: String,
    #[serde(default = "default_max_bytes")]
    pub max_bytes: u64,
}

fn default_max_bytes() -> u64 {
    65536
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactReadResult {
    pub artifact_id: String,
    pub media_type: String,
    pub truncated: bool,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bytes: Vec<u8>,
}

// ---- reminder/schedule / list / cancel / snooze / update ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReminderScheduleParams {
    pub actor_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<ScopeRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msg_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delay_seconds: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fire_at: Option<Timestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReminderScheduleResult {
    pub reminder: Reminder,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReminderListParams {
    pub actor_id: String,
    #[serde(default)]
    pub statuses: Vec<ReminderStatus>,
    #[serde(default)]
    pub all: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReminderListResult {
    pub reminders: Vec<Reminder>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReminderIdParams {
    pub actor_id: String,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReminderCancelResult {
    pub reminder: Reminder,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReminderSnoozeParams {
    pub actor_id: String,
    pub id: String,
    pub by_seconds: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReminderSnoozeResult {
    pub reminder: Reminder,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReminderUpdateParams {
    pub actor_id: String,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delay_seconds: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fire_at: Option<Timestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReminderUpdateResult {
    pub reminder: Reminder,
}

// ---- receipt/record ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptRecordParams {
    pub event_id: String,
    pub actor_id: String,
    pub kind: ReceiptKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiptRecordResult {
    pub receipt: Receipt,
}

// ---- delivery/list (§9.2 durable actor inbox) ----

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryListParams {
    /// Caller must be bound to this actor (via `connection/open`); cross-actor
    /// inbox reads are refused. Mirrors the `actorId`-scoped contract that
    /// `service-plugin-system-design.md` §9.2 calls out.
    pub actor_id: String,
    /// Optional state filter. `None` = all states.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<DeliveryState>,
    /// Page size. Server default 50, hard cap 200.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// Opaque cursor returned from a previous call. Pass as-is to fetch the
    /// next page; omit on the first call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryListEntry {
    pub delivery: Delivery,
    /// Inline event payload so plugins don't need a follow-up `scope/read`.
    /// `None` only when the event row has been compacted away (defensive —
    /// the current store keeps events forever).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<Event>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryListResult {
    pub deliveries: Vec<DeliveryListEntry>,
    /// Opaque cursor for the next page. Absent when the result set was
    /// fully drained.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

// ---- actor/list + actor/upsert ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorListResult {
    pub actors: Vec<Actor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorUpsertParams {
    pub actor: Actor,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorUpsertResult {
    pub actor: Actor,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorDeleteParams {
    pub actor_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorDeleteResult {
    pub deleted: bool,
}

// ---- agent/* ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTransport {
    /// `"acp_stdio"` (default) or `"command"` (see docs/command-transport-v0.md).
    pub kind: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "authMethod")]
    pub auth_method: Option<String>,

    // ---- command / interactive command transport only ----
    /// Optional default model for transports that expose a CLI-level model flag.
    /// `joi agent serve` may override this with the actor's selected runtime
    /// model; when an interactive command has an active model, the runtime
    /// appends `--model=<model>` to the provider argv.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// How to capture and re-use the underlying CLI's session id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<CommandSession>,
    /// Tells the adapter how to translate subprocess output into AdapterEvent.
    /// Defaults to `text` (whole stdout → one content.add at finish).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "outputFormat"
    )]
    pub output_format: Option<CommandOutputFormat>,
    /// How the prompt text is delivered to the subprocess. Defaults to `args`
    /// (appended after `args` as the final argv token).
    #[serde(default, rename = "promptVia")]
    pub prompt_via: PromptVia,

    // ---- interactive_command only; ignored by other transports ----
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interactive: Option<InteractiveCommandSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<InteractiveProviderSpec>,
}

/// Bookkeeping rules for `transport.kind = "command"`. Both fields together let
/// the adapter resume an existing session (`first_run_capture` extracts a
/// session id from the very first invocation; `resume_args` is the argv
/// template used on subsequent calls).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandSession {
    /// DSL: `stdout_json:<jq-style-path>`, `stderr_regex:<re>`, `file:<path>`.
    /// `None` means this CLI does not expose a resumable session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_run_capture: Option<String>,
    /// Argv template substituted with `{session_id}` and (when `prompt_via=args`)
    /// `{prompt}`. `None` means resume is not supported (each call is a first run).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_args: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractiveCommandSpec {
    #[serde(default)]
    pub session: InteractiveSessionSpec,
    #[serde(default)]
    pub prompt: InteractivePromptSpec,
    #[serde(default)]
    pub completion: InteractiveCompletionSpec,
    #[serde(default)]
    pub output: InteractiveOutputSpec,
    #[serde(default)]
    pub kill: InteractiveKillSpec,
}

impl Default for InteractiveCommandSpec {
    fn default() -> Self {
        Self {
            session: InteractiveSessionSpec::default(),
            prompt: InteractivePromptSpec::default(),
            completion: InteractiveCompletionSpec::default(),
            output: InteractiveOutputSpec::default(),
            kill: InteractiveKillSpec::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractiveSessionSpec {
    #[serde(default)]
    pub id_strategy: InteractiveSessionIdStrategy,
    #[serde(default)]
    pub new_args: Vec<String>,
    #[serde(default)]
    pub resume_args: Vec<String>,
    #[serde(default)]
    pub on_missing: InteractiveSessionMissingPolicy,
    #[serde(default)]
    pub on_signature_changed: InteractiveSignatureChangedPolicy,
    #[serde(default)]
    pub on_resume_failed: InteractiveResumeFailedPolicy,
}

impl Default for InteractiveSessionSpec {
    fn default() -> Self {
        Self {
            id_strategy: InteractiveSessionIdStrategy::default(),
            new_args: Vec::new(),
            resume_args: Vec::new(),
            on_missing: InteractiveSessionMissingPolicy::default(),
            on_signature_changed: InteractiveSignatureChangedPolicy::default(),
            on_resume_failed: InteractiveResumeFailedPolicy::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractiveSessionIdStrategy {
    #[default]
    JoiUuidPerScope,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractiveSessionMissingPolicy {
    #[default]
    Create,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractiveSignatureChangedPolicy {
    #[default]
    Create,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractiveResumeFailedPolicy {
    #[default]
    Fail,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractivePromptSpec {
    #[serde(default = "default_interactive_prompt_template")]
    pub template: String,
    #[serde(default)]
    pub completion_contract: InteractiveCompletionContractSpec,
}

impl Default for InteractivePromptSpec {
    fn default() -> Self {
        Self {
            template: default_interactive_prompt_template(),
            completion_contract: InteractiveCompletionContractSpec::default(),
        }
    }
}

fn default_interactive_prompt_template() -> String {
    "{joi_envelope}".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractiveCompletionContractSpec {
    #[serde(default = "default_interactive_sentinel")]
    pub sentinel: String,
    #[serde(default = "default_interactive_instruction")]
    pub instruction: String,
}

impl Default for InteractiveCompletionContractSpec {
    fn default() -> Self {
        Self {
            sentinel: default_interactive_sentinel(),
            instruction: default_interactive_instruction(),
        }
    }
}

fn default_interactive_sentinel() -> String {
    "__JOI_DONE__".into()
}

fn default_interactive_instruction() -> String {
    "When your final user-visible answer is complete, output __JOI_DONE__ on a line by itself. Do not output anything after it.".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractiveCompletionSpec {
    #[serde(default)]
    pub detect: InteractiveCompletionDetector,
    #[serde(default = "default_true")]
    pub strip_sentinel: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_timeout_ms: Option<u64>,
    #[serde(default = "default_interactive_max_turn_ms")]
    pub max_turn_ms: u64,
}

impl Default for InteractiveCompletionSpec {
    fn default() -> Self {
        Self {
            detect: InteractiveCompletionDetector::default(),
            strip_sentinel: true,
            idle_timeout_ms: None,
            max_turn_ms: default_interactive_max_turn_ms(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractiveCompletionDetector {
    #[default]
    Sentinel,
}

fn default_interactive_max_turn_ms() -> u64 {
    900_000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractiveOutputSpec {
    #[serde(default = "default_true")]
    pub strip_ansi: bool,
    #[serde(default)]
    pub stderr: InteractiveStderrPolicy,
    #[serde(default = "default_true")]
    pub stream_partial: bool,
}

impl Default for InteractiveOutputSpec {
    fn default() -> Self {
        Self {
            strip_ansi: true,
            stderr: InteractiveStderrPolicy::default(),
            stream_partial: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractiveStderrPolicy {
    #[default]
    Trace,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractiveKillSpec {
    #[serde(default = "default_complete_kill")]
    pub on_complete: InteractiveKillAction,
    #[serde(default = "default_cancel_kill")]
    pub on_cancel: InteractiveKillAction,
    #[serde(default = "default_timeout_kill")]
    pub on_timeout: InteractiveKillAction,
}

impl Default for InteractiveKillSpec {
    fn default() -> Self {
        Self {
            on_complete: default_complete_kill(),
            on_cancel: default_cancel_kill(),
            on_timeout: default_timeout_kill(),
        }
    }
}

fn default_complete_kill() -> InteractiveKillAction {
    InteractiveKillAction {
        action: InteractiveKillKind::Sigterm,
        grace_ms: Some(3000),
        fallback: Some(InteractiveKillKind::Sigkill),
    }
}

fn default_cancel_kill() -> InteractiveKillAction {
    InteractiveKillAction {
        action: InteractiveKillKind::Sigterm,
        grace_ms: Some(1000),
        fallback: Some(InteractiveKillKind::Sigkill),
    }
}

fn default_timeout_kill() -> InteractiveKillAction {
    InteractiveKillAction {
        action: InteractiveKillKind::Sigkill,
        grace_ms: None,
        fallback: None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractiveKillAction {
    #[serde(default)]
    pub action: InteractiveKillKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grace_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<InteractiveKillKind>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractiveKillKind {
    StdinEof,
    CtrlD,
    CtrlC,
    #[default]
    Sigterm,
    Sigkill,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractiveProviderSpec {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<ClaudeSettingsSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeSettingsSpec {
    #[serde(default)]
    pub mode: ClaudeSettingsMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeSettingsMode {
    #[default]
    Global,
    ActorProfile,
    Custom,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandOutputFormat {
    /// Whole stdout collected → single `content.add` event at process exit.
    #[default]
    Text,
    /// Anthropic Claude Code `--output-format stream-json` framing.
    ClaudeStreamJson,
    /// OpenAI codex CLI `--output-format stream-json` framing (placeholder).
    CodexStreamJson,
    /// Generic line-delimited JSON (each line carries `{"type": "...", ...}`).
    NdjsonLines,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptVia {
    /// Append prompt as the final argv token (default). Safe — no shell parse.
    #[default]
    Args,
    /// Write prompt to subprocess stdin, then close stdin.
    Stdin,
    /// Inject prompt as the env var `JOI_PROMPT`.
    Env,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSpec {
    pub actor: Actor,
    pub transport: AgentTransport,
    #[serde(default)]
    pub autostart: bool,
    /// Optional model menu for this actor. Joi treats these as runtime-level
    /// model ids: `joi agent serve` can surface them through `/models` and
    /// pass the selected id to transports that support model selection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models: Option<AgentModelSpec>,
    /// Optional actor-local bundle configuration. When present, the runtime
    /// ensures a skill / tool bundle is available under the actor home before
    /// the transport is started, then exposes its resolved paths through
    /// template variables / env injection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle: Option<AgentBundleSpec>,
    /// Optional per-actor persona configuration. When present, the runtime
    /// loads the referenced markdown files from `{agent.profile}` and injects
    /// them as labeled prompt sections on **every** turn. Absent means "no
    /// persona injection" and preserves pre-persona behavior.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<IdentitySpec>,
    /// Optional per-actor memory configuration. Defines where records live
    /// under `{agent.profile}/memory/`, how they are selected each turn, and
    /// how they reach the agent (prompt section and/or MCP bridge).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemorySpec>,
    /// Optional opt-in for the pinned-announcement MCP. When present and
    /// `mcp = true`, the runtime auto-injects a `joi-announcement` stdio
    /// server into the ACP session, giving the agent two tools:
    /// `announcement.set` and `announcement.clear` to publish a recap to
    /// the right-side panel of any chat client subscribed to the scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub announcement: Option<AnnouncementSpec>,
}

/// On-disk provider spec. A provider is one installed agent CLI/runtime
/// (Claude Code, Codex, Qoder, ...). It may expose multiple runtime actors,
/// each with its own identity, model, memory, profile, and workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProviderSpec {
    pub provider: AgentProviderInfo,
    pub transport: AgentTransport,
    #[serde(default)]
    pub defaults: AgentActorDefaults,
    pub actors: Vec<AgentActorSpec>,
}

impl AgentProviderSpec {
    pub fn into_agent_specs(self) -> Vec<AgentSpec> {
        let AgentProviderSpec {
            provider: _,
            transport,
            defaults,
            actors,
        } = self;
        actors
            .into_iter()
            .map(|actor| {
                let mut actor_models = actor.models.or_else(|| defaults.models.clone());
                if let Some(model) = actor
                    .model
                    .as_deref()
                    .map(str::trim)
                    .filter(|model| !model.is_empty())
                {
                    actor_models
                        .get_or_insert_with(AgentModelSpec::default)
                        .default = Some(model.to_string());
                }
                let display_name = actor
                    .display_name
                    .as_deref()
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .unwrap_or(&actor.id)
                    .to_string();
                AgentSpec {
                    actor: Actor {
                        id: actor.id,
                        kind: ActorKind::Agent,
                        display_name,
                        capabilities: actor.capabilities,
                        _meta: actor.meta,
                    },
                    transport: actor.transport.unwrap_or_else(|| transport.clone()),
                    autostart: actor.autostart.unwrap_or(defaults.autostart),
                    models: actor_models,
                    bundle: actor.bundle.or_else(|| defaults.bundle.clone()),
                    identity: merge_identity(defaults.identity.as_ref(), actor.identity),
                    memory: actor.memory.or_else(|| defaults.memory.clone()),
                    announcement: actor.announcement.or_else(|| defaults.announcement.clone()),
                }
            })
            .collect()
    }
}

fn merge_identity(
    base: Option<&IdentitySpec>,
    actor: Option<IdentitySpec>,
) -> Option<IdentitySpec> {
    match (base.cloned(), actor) {
        (None, None) => None,
        (Some(base), None) => Some(base),
        (None, Some(actor)) => Some(actor),
        (Some(base), Some(actor)) => Some(IdentitySpec {
            files: if actor.files == IdentityFiles::default() {
                base.files
            } else {
                actor.files
            },
            description: actor.description.or(base.description),
            scaffold: merge_identity_scaffold(base.scaffold, actor.scaffold),
        }),
    }
}

fn merge_identity_scaffold(
    base: Option<IdentityScaffoldSpec>,
    actor: Option<IdentityScaffoldSpec>,
) -> Option<IdentityScaffoldSpec> {
    match (base, actor) {
        (None, None) => None,
        (Some(base), None) => Some(base),
        (None, Some(actor)) => Some(actor),
        (Some(base), Some(actor)) => Some(IdentityScaffoldSpec {
            identity: actor.identity.or(base.identity),
            soul: actor.soul.or(base.soul),
        }),
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProviderInfo {
    pub id: String,
    #[serde(default)]
    pub display_name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentActorDefaults {
    #[serde(default)]
    pub autostart: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models: Option<AgentModelSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle: Option<AgentBundleSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<IdentitySpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemorySpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub announcement: Option<AnnouncementSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentActorSpec {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub meta: Option<Meta>,
    /// Optional transport override for this actor. Omit to share the provider
    /// transport, which is the common "same CLI, multiple actors" path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<AgentTransport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub autostart: Option<bool>,
    /// Shorthand for setting `models.default` on this actor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models: Option<AgentModelSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle: Option<AgentBundleSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<IdentitySpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemorySpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub announcement: Option<AnnouncementSpec>,
}

// ---- models ----

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentModelSpec {
    /// Actor-level default model id. Used when no local selection has been
    /// persisted yet. If `choices` is empty this still acts as a single
    /// selectable model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// User-facing menu. `id` is what gets sent to the runtime; `label` is only
    /// display text and may be omitted.
    #[serde(default)]
    pub choices: Vec<AgentModelChoice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentModelChoice {
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

// ---- announcement ----

/// Per-actor announcement configuration. v0 has only a single `mcp` switch;
/// future fields might constrain which scopes the agent can pin to or
/// require an extra approval step.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnouncementSpec {
    /// When true, the ACP `session/new` call synthesizes a `joi-announcement`
    /// stdio MCP server. Default false — opt-in.
    #[serde(default)]
    pub mcp: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentBundleSpec {
    /// Source directory or file to install into the actor home. Relative paths
    /// should be resolved by the caller before registration.
    #[serde(default)]
    pub source: String,
    /// Logical bundle version. Used to create a stable installed path under the
    /// actor home. Empty means derive from the source basename.
    #[serde(default)]
    pub version: String,
    /// Install mode for the versioned bundle directory.
    #[serde(default)]
    pub install_mode: BundleInstallMode,
    /// Resolved with the same template variables as transport paths. Default
    /// `{agent.root}/bundles`.
    #[serde(default = "default_bundle_root")]
    pub root: String,
    /// Resolved with the same template variables as transport paths. Default
    /// `{agent.root}/bundles/current`.
    #[serde(default = "default_bundle_current")]
    pub current: String,
}

impl Default for AgentBundleSpec {
    fn default() -> Self {
        Self {
            source: String::new(),
            version: String::new(),
            install_mode: BundleInstallMode::default(),
            root: default_bundle_root(),
            current: default_bundle_current(),
        }
    }
}

fn default_bundle_root() -> String {
    "{agent.root}/bundles".into()
}

fn default_bundle_current() -> String {
    "{agent.root}/bundles/current".into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BundleInstallMode {
    #[default]
    Copy,
    Symlink,
}

// ---- identity ----

/// Persona config: which markdown files under `{agent.profile}` carry the
/// agent's role definition (identity) and operating style (soul).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentitySpec {
    #[serde(default)]
    pub files: IdentityFiles,
    /// Optional short role description used when scaffolding a missing
    /// identity file. Existing files are never overwritten.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional first-run file contents for `identity.md` / `soul.md`.
    /// Existing files still win, so operators can edit profiles safely.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scaffold: Option<IdentityScaffoldSpec>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityScaffoldSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soul: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityFiles {
    /// Markdown path relative to `{agent.profile}` (or absolute). Default
    /// `identity.md`.
    #[serde(default = "default_identity_file")]
    pub identity: String,
    /// Markdown path relative to `{agent.profile}` (or absolute). Default
    /// `soul.md`.
    #[serde(default = "default_soul_file")]
    pub soul: String,
}

impl Default for IdentityFiles {
    fn default() -> Self {
        Self {
            identity: default_identity_file(),
            soul: default_soul_file(),
        }
    }
}

fn default_identity_file() -> String {
    "identity.md".into()
}
fn default_soul_file() -> String {
    "soul.md".into()
}

// ---- memory ----

/// Per-actor memory: storage layout + per-turn selection + delivery channels.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySpec {
    #[serde(default)]
    pub store: MemoryStoreSpec,
    #[serde(default)]
    pub query: MemoryQuerySpec,
    #[serde(default)]
    pub delivery: MemoryDeliverySpec,
    /// Reserved: auto-extract records from conversation. Off by default.
    #[serde(default)]
    pub extraction: FeatureMode,
    /// Reserved: auto-compact old records. Off by default.
    #[serde(default)]
    pub compaction: FeatureMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryStoreSpec {
    /// Only `"jsonl"` is implemented. Future: `"sqlite"`, `"qdrant"`.
    #[serde(default = "default_memory_store_type")]
    pub store_type: String,
    /// Path relative to `{agent.profile}` (or absolute). Default
    /// `./memory/records`.
    #[serde(default = "default_memory_root")]
    pub root: String,
    /// Shard JSONL files by time bucket. `"month"` → `YYYY-MM.jsonl`,
    /// `"day"` → `YYYY-MM-DD.jsonl`. Default `"month"`.
    #[serde(default = "default_shard_by")]
    pub shard_by: String,
}

impl Default for MemoryStoreSpec {
    fn default() -> Self {
        Self {
            store_type: default_memory_store_type(),
            root: default_memory_root(),
            shard_by: default_shard_by(),
        }
    }
}

fn default_memory_store_type() -> String {
    "jsonl".into()
}
fn default_memory_root() -> String {
    "./memory/records".into()
}
fn default_shard_by() -> String {
    "month".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryQuerySpec {
    /// Only `"heuristic"` is implemented (token-overlap). Reserved for future
    /// embedding-based modes.
    #[serde(default = "default_query_mode")]
    pub mode: String,
    /// Top-K recent / high-confidence records always injected at the top of
    /// the memory section. Default 8.
    #[serde(default = "default_bootstrap_top_k")]
    pub bootstrap_top_k: usize,
    /// Top-K records selected by keyword overlap with the current prompt +
    /// thread context. Default 4.
    #[serde(default = "default_turn_top_k")]
    pub turn_top_k: usize,
    /// When true (default), queries filter out records whose
    /// `source.channelId` differs from the current turn's channel, preventing
    /// cross-channel leakage of private memory. Set false only for "journal"
    /// style agents whose memories are not channel-sensitive.
    #[serde(default = "default_true")]
    pub per_channel: bool,
}

impl Default for MemoryQuerySpec {
    fn default() -> Self {
        Self {
            mode: default_query_mode(),
            bootstrap_top_k: default_bootstrap_top_k(),
            turn_top_k: default_turn_top_k(),
            per_channel: true,
        }
    }
}

fn default_query_mode() -> String {
    "heuristic".into()
}
fn default_bootstrap_top_k() -> usize {
    8
}
fn default_turn_top_k() -> usize {
    4
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryDeliverySpec {
    /// When true, the runtime renders bootstrap + turn records as a labeled
    /// "Memory" section and prepends it to every `session/prompt`. Default
    /// false — opt-in.
    #[serde(default)]
    pub prompt: bool,
    /// When true, the ACP `session/new` call synthesizes a `joi-memory`
    /// stdio MCP server so the agent can `memory.query` / `memory.append` /
    /// `memory.get` on demand. Default false — opt-in.
    #[serde(default)]
    pub mcp: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeatureMode {
    /// `"disabled"` (default), or reserved future modes like `"auto"` /
    /// `"manual"`.
    #[serde(default = "default_disabled_mode")]
    pub mode: String,
}

impl Default for FeatureMode {
    fn default() -> Self {
        Self {
            mode: default_disabled_mode(),
        }
    }
}

fn default_disabled_mode() -> String {
    "disabled".into()
}

// ---- service spec (parallel to AgentSpec; runs under `joi service serve`) ----

/// On-disk spec for a service actor (kind = Service). Mirrors `AgentSpec`
/// for the host process: `joi service serve` loads `*.json` from a specs
/// directory, instantiates the named plugin (`kind`), and binds it to a
/// long-lived service actor connection. See
/// `docs/service-plugin-system-design.md` §6.1.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceSpec {
    /// Unique within a specs directory. Used for state-dir naming
    /// (`~/.local/share/joi/service-host/services/<id>/`) and for the
    /// `--allow-services` filter on `joi service serve`. Distinct from
    /// `actor.id` because one actor may be reachable from multiple specs
    /// (e.g., a default and a test variant).
    pub id: String,
    /// Plugin kind discriminator. Service-host looks up the registered
    /// plugin factory by this name (`"am"`, `"scheduler"`, ...). v1 only
    /// permits built-in kinds (see §13 Q5).
    pub kind: String,
    /// The service actor this spec runs as. `actor.kind` MUST equal
    /// `Service`; loaders reject anything else (see `validate_kind`). §6.1
    /// is explicit that `ServiceSpec` does not inherit kind by convention
    /// the way `AgentSpec` does — the field is mandatory because a host
    /// process can mount multiple actor kinds and the spec is the only
    /// signal of intent.
    pub actor: Actor,
    /// Start this service automatically when `joi service serve` boots.
    /// Default true — services exist to run continuously, the override is
    /// for staged rollout / debugging.
    #[serde(default = "default_true")]
    pub autostart: bool,
    /// Primary channel the service writes into. Optional because some
    /// plugins compute scope per-event from external context (e.g., am's
    /// `auto_thread` mode in §7.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,
    /// Default handoff target. When set, plugin-emitted events typically
    /// carry `hands_off_to -> actor:<targetAgent>` unless the plugin
    /// overrides per-event. Plugins are not forced to honor this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_agent: Option<String>,
    /// Plugin-specific configuration. Parsed by the plugin itself, not by
    /// the host. Schema is the plugin's contract (see §7 for am, §8 for
    /// scheduler).
    #[serde(default)]
    pub config: Value,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ServiceSpecError {
    #[error(
        "ServiceSpec `{spec_id}`: actor `{actor_id}` must declare kind = service, got {got:?}"
    )]
    WrongActorKind {
        spec_id: String,
        actor_id: String,
        got: ActorKind,
    },
    #[error("ServiceSpec `{spec_id}`: kind must be non-empty")]
    EmptyKind { spec_id: String },
    #[error("ServiceSpec id must be non-empty")]
    EmptyId,
}

impl ServiceSpec {
    /// Reject specs with a malformed actor or empty discriminators. §6.1's
    /// invariant is "actor.kind must be service"; the loader calls this
    /// after `serde_json::from_str` and surfaces the error to the operator.
    /// Other invariants beyond JSON-shape (e.g., does the plugin kind exist
    /// in this build?) belong to the host's plugin registry, not here.
    pub fn validate(&self) -> Result<(), ServiceSpecError> {
        if self.id.trim().is_empty() {
            return Err(ServiceSpecError::EmptyId);
        }
        if self.kind.trim().is_empty() {
            return Err(ServiceSpecError::EmptyKind {
                spec_id: self.id.clone(),
            });
        }
        if !matches!(self.actor.kind, ActorKind::Service) {
            return Err(ServiceSpecError::WrongActorKind {
                spec_id: self.id.clone(),
                actor_id: self.actor.id.clone(),
                got: self.actor.kind,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub spec: AgentSpec,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentListResult {
    pub agents: Vec<AgentInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMarketplaceListResult {
    pub entries: Vec<crate::marketplace::MarketplaceEntry>,
}

// ---- stream/update notification ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamUpdate {
    pub kind: String,
    pub scope: ScopeRef,
    pub data: Value,
}

pub mod stream_kind {
    pub const THREAD_CREATED: &str = "thread.created";
    /// Broadcast when a new channel is created. Public channels go to
    /// all connections; private channels go to the creator only.
    pub const CHANNEL_CREATED: &str = "channel.created";
    pub const TURN_OPENED: &str = "turn.opened";
    pub const TURN_CLOSED: &str = "turn.closed";
    pub const EVENT_CREATED: &str = "event.created";
    pub const ARTIFACT_PUBLISHED: &str = "artifact.published";
    pub const DELIVERY_UPDATED: &str = "delivery.updated";
    pub const RECEIPT_RECORDED: &str = "receipt.recorded";
    /// Direct-to-actor notification: the recipient was added to a channel
    /// and is now allowed to read/subscribe/append. Carries the full
    /// `Channel` so the receiving client can patch its sidebar cache
    /// without a follow-up RPC.
    pub const CHANNEL_INVITED: &str = "channel.invited";
    /// Mirror of `CHANNEL_INVITED`: the recipient was removed from a
    /// channel. Carries `{ channelId, actorId }`.
    pub const CHANNEL_REVOKED: &str = "channel.revoked";
}

#[cfg(test)]
mod agent_provider_spec_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn provider_spec_expands_shared_transport_with_actor_overrides() {
        let provider: AgentProviderSpec = serde_json::from_value(json!({
            "provider": { "id": "codex", "displayName": "Codex" },
            "transport": { "kind": "acp_stdio", "command": "npx", "args": ["-y", "codex-acp"] },
            "defaults": {
                "models": {
                    "choices": [
                        { "id": "gpt-5.5", "label": "GPT-5.5" },
                        { "id": "gpt-5.4-mini", "label": "GPT-5.4 Mini" }
                    ]
                },
                "identity": {
                    "scaffold": { "soul": "# Shared style" }
                }
            },
            "actors": [
                {
                    "id": "actor_codex_architect",
                    "displayName": "Codex Architect",
                    "model": "gpt-5.5",
                    "identity": {
                        "description": "System design reviewer",
                        "scaffold": { "identity": "# Architect" }
                    }
                },
                {
                    "id": "actor_codex_fast",
                    "displayName": "Codex Fast",
                    "model": "gpt-5.4-mini"
                }
            ]
        }))
        .expect("parse provider spec");

        let specs = provider.into_agent_specs();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].actor.id, "actor_codex_architect");
        assert_eq!(specs[0].actor.display_name, "Codex Architect");
        assert!(matches!(specs[0].actor.kind, ActorKind::Agent));
        assert_eq!(specs[1].actor.id, "actor_codex_fast");
        assert_eq!(specs[0].transport.args, specs[1].transport.args);
        assert_eq!(
            specs[0].models.as_ref().and_then(|m| m.default.as_deref()),
            Some("gpt-5.5")
        );
        assert_eq!(
            specs[1].models.as_ref().and_then(|m| m.default.as_deref()),
            Some("gpt-5.4-mini")
        );
        assert_eq!(
            specs[0]
                .identity
                .as_ref()
                .and_then(|i| i.description.as_deref()),
            Some("System design reviewer")
        );
        assert_eq!(
            specs[0]
                .identity
                .as_ref()
                .and_then(|i| i.scaffold.as_ref())
                .and_then(|s| s.soul.as_deref()),
            Some("# Shared style")
        );
        assert_eq!(
            specs[1]
                .identity
                .as_ref()
                .and_then(|i| i.scaffold.as_ref())
                .and_then(|s| s.soul.as_deref()),
            Some("# Shared style")
        );
    }
}

#[cfg(test)]
mod service_spec_tests {
    use super::*;
    use serde_json::json;

    fn service_actor() -> Actor {
        Actor {
            id: "svc_am_bridge".into(),
            kind: ActorKind::Service,
            display_name: "DingTalk QA Bridge".into(),
            capabilities: None,
            _meta: None,
        }
    }

    fn base_spec() -> ServiceSpec {
        ServiceSpec {
            id: "am_dingtalk_qa".into(),
            kind: "am".into(),
            actor: service_actor(),
            autostart: true,
            channel_id: Some("chan_x".into()),
            target_agent: Some("actor_qa".into()),
            config: json!({}),
        }
    }

    #[test]
    fn validate_passes_for_service_actor() {
        assert!(base_spec().validate().is_ok());
    }

    #[test]
    fn validate_rejects_human_actor() {
        let mut spec = base_spec();
        spec.actor.kind = ActorKind::Human;
        let err = spec.validate().expect_err("human kind must fail");
        assert!(matches!(
            err,
            ServiceSpecError::WrongActorKind {
                got: ActorKind::Human,
                ..
            }
        ));
    }

    #[test]
    fn validate_rejects_agent_actor() {
        // §6.1: an AgentSpec-shaped actor must not be loaded as a service —
        // catching the typo at load time stops a long-lived agent host from
        // accidentally claiming service-actor semantics.
        let mut spec = base_spec();
        spec.actor.kind = ActorKind::Agent;
        let err = spec.validate().expect_err("agent kind must fail");
        assert!(matches!(
            err,
            ServiceSpecError::WrongActorKind {
                got: ActorKind::Agent,
                ..
            }
        ));
    }

    #[test]
    fn validate_rejects_empty_id_or_kind() {
        let mut spec = base_spec();
        spec.id = "  ".into();
        assert_eq!(spec.validate(), Err(ServiceSpecError::EmptyId));

        let mut spec = base_spec();
        spec.kind = "".into();
        assert_eq!(
            spec.validate(),
            Err(ServiceSpecError::EmptyKind {
                spec_id: "am_dingtalk_qa".into()
            })
        );
    }

    #[test]
    fn round_trips_through_section_6_1_example() {
        // The doc's §6.1 am example must deserialize cleanly. If this test
        // breaks the doc and the type are out of sync — fix one or the other,
        // not the test.
        let raw = json!({
            "id": "am_dingtalk_qa",
            "kind": "am",
            "actor": {
                "id": "svc_am_bridge",
                "kind": "service",
                "displayName": "DingTalk QA Bridge"
            },
            "channelId": "chan_x",
            "targetAgent": "actor_qa",
            "config": {
                "amBin": "am",
                "topic": "/v1.0/im/bot/messages/get",
                "scope": "auto_thread",
                "replyMode": "async_send"
            }
        });
        let spec: ServiceSpec = serde_json::from_value(raw.clone()).expect("deserialize");
        spec.validate().expect("valid");
        assert_eq!(spec.id, "am_dingtalk_qa");
        assert_eq!(spec.kind, "am");
        assert!(spec.autostart, "autostart defaults to true when absent");
        assert_eq!(spec.config["scope"], "auto_thread");
        assert_eq!(spec.config["replyMode"], "async_send");
    }

    #[test]
    fn autostart_defaults_true_when_field_missing() {
        let raw = json!({
            "id": "x",
            "kind": "am",
            "actor": {
                "id": "svc_x",
                "kind": "service"
            }
        });
        let spec: ServiceSpec = serde_json::from_value(raw).expect("deserialize");
        assert!(spec.autostart);
    }
}
