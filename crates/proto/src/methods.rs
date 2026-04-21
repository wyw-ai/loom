use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::*;

// ---- method names (canonical, slash-form) ----

pub mod method {
    pub const INITIALIZE: &str = "initialize";
    pub const CONNECTION_OPEN: &str = "connection/open";
    pub const CONNECTION_CLOSE: &str = "connection/close";
    pub const SCOPE_SUBSCRIBE: &str = "scope/subscribe";
    pub const SCOPE_UNSUBSCRIBE: &str = "scope/unsubscribe";
    pub const SCOPE_READ: &str = "scope/read";
    pub const CHANNEL_CREATE: &str = "channel/create";
    pub const CHANNEL_LIST: &str = "channel/list";
    pub const THREAD_CREATE: &str = "thread/create";
    pub const THREAD_LIST: &str = "thread/list";
    pub const TURN_OPEN: &str = "turn/open";
    pub const TURN_CLOSE: &str = "turn/close";
    pub const TURN_TRACE_READ: &str = "turn/trace.read";
    pub const TURN_TRACE_UPDATE: &str = "turn/trace.update";
    pub const EVENT_APPEND: &str = "event/append";
    pub const HANDOFF_CREATE: &str = "handoff/create";
    pub const ARTIFACT_PUBLISH: &str = "artifact/publish";
    pub const ARTIFACT_GET: &str = "artifact/get";
    pub const ARTIFACT_READ: &str = "artifact/read";
    pub const RECEIPT_RECORD: &str = "receipt/record";
    pub const ACTOR_LIST: &str = "actor/list";

    // local extensions
    pub const AGENT_LIST: &str = "agent/list";
    pub const AGENT_REGISTER: &str = "agent/register";
    pub const AGENT_UNREGISTER: &str = "agent/unregister";
    pub const AGENT_START: &str = "agent/start";
    pub const AGENT_STOP: &str = "agent/stop";
    pub const AGENT_LOG: &str = "agent/log";
    pub const AGENT_INSTALL: &str = "agent/install";
    pub const AGENT_LIST_MARKETPLACE: &str = "agent/listMarketplace";

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
pub struct ChannelCreateParams {
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelCreateResult {
    pub channel: Channel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelListResult {
    pub channels: Vec<Channel>,
}

// ---- thread/create / list ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadCreateParams {
    #[serde(alias = "spaceId")]
    pub channel_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_event_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadCreateResult {
    pub thread: Thread,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(alias = "spaceId")]
    pub channel_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadListResult {
    pub threads: Vec<Thread>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
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

// ---- handoff/create ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoffCreateParams {
    pub source_actor_id: String,
    pub target_actor_id: String,
    pub scope: ScopeRef,
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffCreateResult {
    pub event: Event,
}

// ---- artifact/publish / get / read ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ArtifactIngress {
    InlineText(InlineTextIngress),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlineTextIngress {
    pub name: String,
    #[serde(default = "default_text_media_type")]
    pub media_type: String,
    pub text: String,
}

fn default_text_media_type() -> String {
    "text/markdown".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactPublishParams {
    pub scope: ScopeRef,
    pub ingress: ArtifactIngress,
    pub created_by: String,
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

// ---- actor/list ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorListResult {
    pub actors: Vec<Actor>,
}

// ---- agent/* ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTransport {
    pub kind: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "authMethod")]
    pub auth_method: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSpec {
    pub actor: Actor,
    pub transport: AgentTransport,
    #[serde(default)]
    pub autostart: bool,
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
pub struct AgentRegisterParams {
    pub spec: AgentSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRegisterResult {
    pub agent: AgentInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentByIdParams {
    pub actor_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSimpleResult {
    pub agent: AgentInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentOkResult {
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentLogParams {
    pub actor_id: String,
    #[serde(default = "default_tail")]
    pub tail: u32,
}

fn default_tail() -> u32 {
    100
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentLogResult {
    pub lines: Vec<String>,
}

// ---- agent/install + agent/listMarketplace ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInstallParams {
    pub marketplace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_actor_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Optional preference: "auto" (default), "npx", "uvx", or "binary".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefer: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInstallResult {
    pub agent: AgentInfo,
    pub source: String,
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
    pub const TURN_OPENED: &str = "turn.opened";
    pub const TURN_CLOSED: &str = "turn.closed";
    pub const EVENT_CREATED: &str = "event.created";
    pub const ARTIFACT_PUBLISHED: &str = "artifact.published";
    pub const DELIVERY_UPDATED: &str = "delivery.updated";
    pub const RECEIPT_RECORDED: &str = "receipt.recorded";
    pub const HANDOFF_CREATED: &str = "handoff.created";
}
