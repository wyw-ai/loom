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
    /// Outbound notification: scope-broadcast partial text from an in-flight
    /// agent turn. Unlike `TURN_TRACE_UPDATE` (owner-only) this fans out to
    /// every scope subscriber so other channel members can watch the agent
    /// type. Carries `TurnStreamUpdate`. Never journaled — the canonical
    /// record is the `content.add` event written when the turn closes.
    pub const TURN_STREAM_UPDATE: &str = "turn/stream.update";
    pub const EVENT_APPEND: &str = "event/append";
    pub const ARTIFACT_PUBLISH: &str = "artifact/publish";
    pub const ARTIFACT_GET: &str = "artifact/get";
    pub const ARTIFACT_READ: &str = "artifact/read";
    pub const RECEIPT_RECORD: &str = "receipt/record";
    pub const ACTOR_LIST: &str = "actor/list";
    pub const ACTOR_UPSERT: &str = "actor/upsert";

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
#[serde(rename_all = "camelCase")]
pub struct ChannelCreateParams {
    pub title: String,
    /// When provided, the new channel is created `Private` and the creator
    /// is its sole initial member. When omitted, the channel is created
    /// `Public` (legacy behavior, for back-compat with old callers).
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

// ---- turn/stream.update notification ----

/// Scope-broadcast partial text from an in-flight agent turn. `seq` is
/// monotonic per-turn and lets clients detect dropped frames; ordering
/// across turns is not defined. Never persisted — the final `content.add`
/// event written at turn close is canonical.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnStreamUpdate {
    pub turn_id: String,
    pub scope: ScopeRef,
    pub actor_id: String,
    pub seq: u64,
    pub delta_text: String,
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
    #[serde(default)]
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "authMethod")]
    pub auth_method: Option<String>,

    // ---- command transport only; ignored when kind != "command" ----
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandOutputFormat {
    /// Whole stdout collected → single `content.add` event at process exit.
    Text,
    /// Anthropic Claude Code `--output-format stream-json` framing.
    ClaudeStreamJson,
    /// OpenAI codex CLI `--output-format stream-json` framing (placeholder).
    CodexStreamJson,
    /// Generic line-delimited JSON (each line carries `{"type": "...", ...}`).
    NdjsonLines,
}

impl Default for CommandOutputFormat {
    fn default() -> Self {
        Self::Text
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptVia {
    /// Append prompt as the final argv token (default). Safe — no shell parse.
    Args,
    /// Write prompt to subprocess stdin, then close stdin.
    Stdin,
    /// Inject prompt as the env var `JOI_PROMPT`.
    Env,
}

impl Default for PromptVia {
    fn default() -> Self {
        Self::Args
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSpec {
    pub actor: Actor,
    pub transport: AgentTransport,
    #[serde(default)]
    pub autostart: bool,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Direct-to-actor notification: the recipient was added to a channel
    /// and is now allowed to read/subscribe/append. Carries the full
    /// `Channel` so the receiving client can patch its sidebar cache
    /// without a follow-up RPC.
    pub const CHANNEL_INVITED: &str = "channel.invited";
    /// Mirror of `CHANNEL_INVITED`: the recipient was removed from a
    /// channel. Carries `{ channelId, actorId }`.
    pub const CHANNEL_REVOKED: &str = "channel.revoked";
}
