use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub type Timestamp = DateTime<Utc>;
pub type Meta = BTreeMap<String, Value>;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum RefKind {
    Actor,
    Channel,
    Thread,
    Turn,
    Event,
    Message,
    Artifact,
    Task,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ref {
    pub kind: RefKind,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Meta>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ScopeKind {
    Channel,
    Thread,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ScopeRef {
    pub kind: ScopeKind,
    pub id: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ActorKind {
    Human,
    Agent,
    Service,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Actor {
    pub id: String,
    pub kind: ActorKind,
    #[serde(default)]
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Meta>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelVisibility {
    /// Legacy / pre-ACL channels: any actor known to the server may read,
    /// subscribe, and append. Existing journals deserialize as `Public` so
    /// previously-created channels keep working with no migration step.
    Public,
    /// Explicit member set only — listed/readable/writable iff the caller
    /// is in `Channel.members`.
    Private,
}

fn default_visibility() -> ChannelVisibility {
    ChannelVisibility::Public
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Channel {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub topic: String,
    /// `Public` for back-compat (channels in journals predating the ACL
    /// roll-out deserialize via `default_visibility`); newly-created
    /// channels with a known creator are `Private`.
    #[serde(default = "default_visibility")]
    pub visibility: ChannelVisibility,
    /// Always present, even for `Public` channels (used by the chat TUI's
    /// Members pane). For `Public` channels this set is best-effort and
    /// the ACL gate is skipped; for `Private` channels it is authoritative.
    #[serde(default)]
    pub members: Vec<String>,
    /// Channel-level instructions projected into every member agent's
    /// AGENTS.md. Use this to declare the channel's purpose, rules, and
    /// shared context conventions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Meta>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChannelMemberConfig {
    pub channel_id: String,
    pub actor_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_dir: Option<String>,
    pub updated_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Meta>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActorGroup {
    pub id: String,
    pub channel_id: String,
    /// Stable mention handle without the leading `@`.
    pub name: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub member_actor_ids: Vec<String>,
    /// Default false: group mentions notify humans but do not wake agents.
    #[serde(default)]
    pub wake_agents: bool,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Meta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorPresence {
    pub actor_id: String,
    pub channel_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub following: bool,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub attention_policy: String,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Meta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub id: String,
    pub channel_id: String,
    pub title: String,
    pub root_message_id: String,
    /// Thread-level instructions. When present, these are appended after
    /// channel instructions in the AGENTS.md block, giving the thread
    /// scope-specific guidance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<Timestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Meta>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TurnStatus {
    Open,
    Closed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    pub id: String,
    pub actor_id: String,
    pub scope: ScopeRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_source_id: Option<String>,
    pub status: TurnStatus,
    pub opened_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<Timestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Meta>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    PreparingContext,
    Running,
    WaitingTool,
    Completed,
    Failed,
    Canceled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: String,
    pub actor_id: String,
    pub scope: ScopeRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_reason: Option<String>,
    pub agent_config_version_id: String,
    pub status: RunStatus,
    pub opened_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<Timestamp>,
    #[serde(default, rename = "metadata")]
    pub metadata: Meta,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunFrame {
    pub run_id: String,
    pub seq: u64,
    pub kind: String,
    pub payload: serde_json::Value,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfigVersion {
    pub id: String,
    pub actor_id: String,
    pub version: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub adapter: String,
    #[serde(default)]
    pub tools: serde_json::Value,
    #[serde(default)]
    pub capability_tags: Vec<String>,
    #[serde(default)]
    pub attention_policy: serde_json::Value,
    #[serde(default)]
    pub context_policy: serde_json::Value,
    #[serde(default)]
    pub reply_policy: serde_json::Value,
    pub created_by: String,
    pub created_at: Timestamp,
    #[serde(default, rename = "metadata")]
    pub metadata: Meta,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfigActivation {
    pub actor_id: String,
    pub version_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<ScopeRef>,
    pub activated_by: String,
    pub activated_at: Timestamp,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CoordinationMode {
    Sequential,
    ParallelReduce,
    Broadcast,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CoordinationDecisionRule {
    OwnerDecides,
    HumanApproval,
    AllAck,
    Majority,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CoordinationStatus {
    Planning,
    CollectingResponses,
    Committed,
    Executing,
    Done,
    Canceled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CoordinationResponseKind {
    Ack,
    Reject,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoordinationResponse {
    pub actor_id: String,
    pub kind: CoordinationResponseKind,
    #[serde(default)]
    pub reason: String,
    pub responded_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoordinationSession {
    pub id: String,
    pub target: String,
    pub scope: ScopeRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_root_message_id: Option<String>,
    pub owner_actor_id: String,
    pub mode: CoordinationMode,
    pub decision_rule: CoordinationDecisionRule,
    pub status: CoordinationStatus,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baton_holder_actor_id: Option<String>,
    #[serde(default)]
    pub participants: Vec<String>,
    #[serde(default)]
    pub responses: Vec<CoordinationResponse>,
    #[serde(default)]
    pub plan: serde_json::Value,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    #[serde(default, rename = "metadata")]
    pub metadata: Meta,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CoordinationStepType {
    Work,
    Handoff,
    Skip,
    Reassign,
    Reduce,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CoordinationStepStatus {
    Submitted,
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoordinationStep {
    pub id: String,
    pub session_id: String,
    pub actor_id: String,
    pub step_type: CoordinationStepType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot_key: Option<String>,
    pub base_revision: u64,
    pub status: CoordinationStepStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_message_id: Option<String>,
    #[serde(default)]
    pub output: serde_json::Value,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    RepliesTo,
    /// "X is meant for actor Y." Machine-routing semantics only exist when a
    /// binding explicitly writes this relation; raw `@handle` text has no
    /// protocol meaning by itself.
    DirectedTo,
    RespondsTo,
    AttachesArtifact,
    RelatesToTask,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relation {
    pub kind: RelationKind,
    pub target: Ref,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Meta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: String,
    /// Event type, e.g. "action.request", "action.response", or
    /// "artifact.publish". User-visible directed messages are represented by
    /// `Message`; `DirectedTo` remains only for internal control records.
    /// Agent tool calls and runtime status changes are NOT events; they are
    /// private run frames carried by `run.append`.
    #[serde(rename = "type")]
    pub kind: String,
    pub actor_id: String,
    pub scope: ScopeRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    pub seq: u64,
    pub occurred_at: Timestamp,
    #[serde(default)]
    pub payload: Value,
    #[serde(default)]
    pub relations: Vec<Relation>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Meta>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    Human,
    Agent,
    System,
    Attention,
    TaskUpdate,
    Artifact,
}

fn default_message_kind() -> MessageKind {
    MessageKind::Human
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MessageIntent {
    Chat,
    Ask,
    RequestAction,
    AssignTask,
    StatusUpdate,
    Review,
    Notify,
}

fn default_message_intent() -> MessageIntent {
    MessageIntent::Chat
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryPolicy {
    NotifyOnly,
    WakeAgent,
    RouteByIntent,
    Silent,
}

fn default_delivery_policy() -> DeliveryPolicy {
    DeliveryPolicy::NotifyOnly
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AudienceKind {
    Actor,
    Group,
    All,
    Agents,
    Humans,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AudienceRef {
    pub kind: AudienceKind,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MessageMentionKind {
    Actor,
    Group,
    All,
    Agents,
    Humans,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MessageMention {
    pub actor_or_group_id: String,
    pub kind: MessageMentionKind,
    pub source: String,
    pub byte_start: usize,
    pub byte_end: usize,
    pub display: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MessageReaction {
    pub emoji: String,
    #[serde(default)]
    pub actor_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub scope: ScopeRef,
    pub target: String,
    pub author_actor_id: String,
    pub created_at: Timestamp,
    #[serde(default = "default_message_kind")]
    pub kind: MessageKind,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub mentions: Vec<MessageMention>,
    #[serde(default)]
    pub audience: Vec<AudienceRef>,
    #[serde(default = "default_message_intent")]
    pub intent: MessageIntent,
    #[serde(default = "default_delivery_policy")]
    pub delivery_policy: DeliveryPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_root_message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reactions: Vec<MessageReaction>,
    /// Client-provided key used to make message creation idempotent within an
    /// author and resolved scope. Persisting it on the append record lets the
    /// server rebuild its deduplication index after a restart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    #[serde(default, rename = "metadata")]
    pub metadata: Meta,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Todo,
    Claimed,
    InProgress,
    WaitingReview,
    Done,
    Failed,
    Canceled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TaskAssignmentType {
    Generate,
    Review,
    Investigate,
    Fix,
    Verify,
    Other,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TaskAssignmentStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Canceled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TaskRefConfidence {
    Confirmed,
    Inferred,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TaskRefStatus {
    Active,
    Superseded,
    Retired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRef {
    pub id: String,
    pub task_id: String,
    pub channel_id: String,
    pub kind: String,
    #[serde(default)]
    pub subtype: String,
    pub value: String,
    pub normalized: String,
    #[serde(default)]
    pub fields: Value,
    pub confidence: TaskRefConfidence,
    pub status: TaskRefStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_message_id: Option<String>,
    pub created_by_actor_id: String,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Meta>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TaskArtifactLinkStatus {
    Active,
    Proposal,
    Superseded,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskArtifactLink {
    pub id: String,
    pub task_id: String,
    pub artifact_id: String,
    #[serde(default)]
    pub schema: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub sequence: u64,
    pub status: TaskArtifactLinkStatus,
    #[serde(default)]
    pub lineage: Value,
    #[serde(default)]
    pub binding: Value,
    pub created_by_actor_id: String,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Meta>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TaskFactType {
    Observation,
    Status,
    Decision,
    Action,
    UserDefined,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TaskFactStatus {
    Active,
    Superseded,
    Retracted,
    Conflict,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TaskSnapshotCompleteness {
    Complete,
    Partial,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskFact {
    pub id: String,
    pub task_id: String,
    #[serde(default)]
    pub target_key: String,
    pub kind: String,
    #[serde(default = "default_task_fact_type")]
    pub fact_type: TaskFactType,
    #[serde(default)]
    pub subject: Value,
    pub signature: String,
    pub status: TaskFactStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub replaces: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retracted_by: Option<String>,
    #[serde(default)]
    pub authority: String,
    #[serde(default)]
    pub authority_binding: Value,
    pub observed_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_snapshot_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_updated_at: Option<Timestamp>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub observed_fields: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unobserved_fields: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_completeness: Option<TaskSnapshotCompleteness>,
    pub producer_id: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub raw_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_id: Option<String>,
    #[serde(default)]
    pub payload_schema: String,
    #[serde(default)]
    pub payload: Value,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Meta>,
}

fn default_task_fact_type() -> TaskFactType {
    TaskFactType::UserDefined
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TaskProjectionHealth {
    Fresh,
    Stale,
    Missing,
    Invalid,
    RepairRequired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskProjection {
    pub id: String,
    pub task_id: String,
    #[serde(default)]
    pub projection_type: String,
    pub producer_actor_id: String,
    pub health: TaskProjectionHealth,
    #[serde(default)]
    pub watermark: Value,
    #[serde(default)]
    pub payload_schema: String,
    #[serde(default)]
    pub payload: Value,
    pub updated_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Meta>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TaskChangeType {
    Fact,
    Projection,
    Assignment,
    ArtifactLink,
    Lease,
    Action,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskChange {
    pub id: String,
    pub cursor: u64,
    pub task_id: String,
    pub change_type: TaskChangeType,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_ids: Vec<String>,
    pub signature: String,
    #[serde(default)]
    pub summary: String,
    pub occurred_at: Timestamp,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recipients: Vec<String>,
    #[serde(default)]
    pub requires_ack: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TaskChangeDeliveryStatus {
    Pending,
    Processing,
    Handled,
    Failed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TaskChangeAckDisposition {
    AssignmentCreated,
    AssignmentReused,
    ActionRequested,
    FactWritten,
    ArtifactWritten,
    ProjectionRepaired,
    Blocked,
    NoopRecorded,
    Escalated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskChangeDelivery {
    pub change: TaskChange,
    pub recipient_actor_id: String,
    pub status: TaskChangeDeliveryStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disposition: Option<TaskChangeAckDisposition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub result_ref_ids: Vec<String>,
    #[serde(default)]
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acked_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceLeaseMode {
    Read,
    Write,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceLeaseStatus {
    Active,
    Released,
    Expired,
    Canceled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceLease {
    pub id: String,
    pub resource_key: String,
    pub holder_assignment_id: String,
    pub holder_actor_id: String,
    pub mode: WorkspaceLeaseMode,
    pub status: WorkspaceLeaseStatus,
    pub expires_at: Timestamp,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Meta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskPreflightResult {
    pub assignment_id: String,
    pub allowed: bool,
    pub checked_at: Timestamp,
    #[serde(default)]
    pub target_key: String,
    #[serde(default)]
    pub head: String,
    #[serde(default)]
    pub effect: String,
    pub guards: Value,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    /// Monotonic, human-facing number scoped to `channel_id`.
    pub number: u64,
    pub channel_id: String,
    /// Top-level channel message that anchors this work item.
    pub source_message_id: String,
    /// Thread attached to `source_message_id`; all progress and directed
    /// discussion should return here.
    pub canonical_thread_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_source_message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_task_id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub requester_actor_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_actor_id: Option<String>,
    pub status: TaskStatus,
    #[serde(default)]
    pub result_summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assignment_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub practice_contract_epoch: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Meta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskAssignment {
    pub id: String,
    pub task_id: String,
    pub from_actor_id: String,
    pub to_actor_id: String,
    #[serde(rename = "type")]
    pub assignment_type: TaskAssignmentType,
    #[serde(default)]
    pub instruction: String,
    pub status: TaskAssignmentStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_message_id: Option<String>,
    #[serde(default)]
    pub result_summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub result_artifact_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub result_fact_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_envelope: Option<Value>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Meta>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactKind {
    File,
    Directory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub id: String,
    pub uri: String,
    pub kind: ArtifactKind,
    pub name: String,
    pub media_type: String,
    pub size: u64,
    pub checksum: String,
    pub created_by: String,
    pub created_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Meta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Membership {
    pub actor_id: String,
    pub scope: ScopeRef,
    pub joined_at: Timestamp,
    pub updated_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_read_source_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Meta>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DeliveryState {
    Pending,
    Delivered,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Delivery {
    pub source_id: String,
    pub actor_id: String,
    pub state: DeliveryState,
    pub updated_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Meta>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum MachineCommandStatus {
    Queued,
    Delivered,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Expired,
}

impl MachineCommandStatus {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            MachineCommandStatus::Succeeded
                | MachineCommandStatus::Failed
                | MachineCommandStatus::Cancelled
                | MachineCommandStatus::Expired
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineCommandError {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineCommand {
    pub command_id: String,
    pub machine_id: String,
    pub machine_actor_id: String,
    pub requested_by: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    pub operation: String,
    pub payload: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub if_inventory_revision: Option<u64>,
    pub status: MachineCommandStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline_at: Option<Timestamp>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<MachineCommandError>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<Timestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Meta>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ReminderStatus {
    Scheduled,
    Fired,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reminder {
    pub id: String,
    pub actor_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<ScopeRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msg_id: Option<String>,
    pub fire_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat: Option<String>,
    pub status: ReminderStatus,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_fired_at: Option<Timestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Meta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Endpoint {
    pub id: String,
    pub actor_id: String,
    pub kind: String,
    #[serde(default)]
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Meta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub id: String,
    pub actor_id: String,
    pub endpoint_id: String,
    pub opened_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Meta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageInfo {
    pub has_more: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Meta>,
}

// ---- common payload helpers ----

pub mod payload {
    use super::{Meta, Timestamp};
    use serde::{Deserialize, Serialize};

    /// Payload for `content.add`.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ContentAdd {
        #[serde(default = "default_content_type")]
        pub content_type: String,
        pub text: String,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "_meta")]
        pub _meta: Option<Meta>,
    }

    fn default_content_type() -> String {
        "text/markdown".into()
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ActionChoice {
        pub id: String,
        pub label: String,
    }

    /// Payload for `action.request`.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ActionRequest {
        pub request_type: String,
        pub title: String,
        #[serde(default)]
        pub description: String,
        #[serde(default)]
        pub choices: Vec<ActionChoice>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub task_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub target_key: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub decision_kind: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub blocks_assignment_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub conversion_owner_actor_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub expires_at: Option<Timestamp>,
    }

    /// Payload for `action.response`.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ActionResponse {
        pub option_id: String,
        #[serde(default = "default_kind")]
        pub kind: String,
    }

    fn default_kind() -> String {
        "accepted".into()
    }
}

// ---- run trace frame payloads ----
//
// Trace payloads live on `RunFrame` records. They are NOT events: they have no
// global event id, are not stored in `events_by_scope`, and cannot be addressed
// by message reply or event response relations.

pub mod trace {
    use super::{Meta, Timestamp};
    use serde::{Deserialize, Serialize};
    use serde_json::Value;

    /// Discriminator for trace frame payloads.
    ///
    /// - `tool.start` / `tool.update` / `tool.end`: agent tool invocations
    ///   reported by the runtime
    /// - `text.delta`: streaming partial chunks of the agent's reply, before
    ///   they are aggregated into a final `content.add` event at turn close
    /// - `status`: agent runtime state changes
    /// - `error`: agent runtime errors that are private to the agent
    #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
    pub enum TraceKind {
        #[serde(rename = "tool.start")]
        ToolStart,
        #[serde(rename = "tool.update")]
        ToolUpdate,
        #[serde(rename = "tool.end")]
        ToolEnd,
        #[serde(rename = "text.delta")]
        TextDelta,
        #[serde(rename = "status")]
        Status,
        #[serde(rename = "error")]
        Error,
    }

    /// A single trace frame attached to a turn. `seq` is monotonic per-turn.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct TraceFrame {
        pub turn_id: String,
        pub seq: u64,
        pub kind: TraceKind,
        pub occurred_at: Timestamp,
        #[serde(default)]
        pub payload: Value,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "_meta")]
        pub _meta: Option<Meta>,
    }
}
