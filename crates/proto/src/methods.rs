use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::*;

// ---- method names ----

pub mod method {
    pub const INITIALIZE: &str = "initialize";
    pub const CONNECTION_OPEN: &str = "connection/open";
    pub const CONNECTION_CLOSE: &str = "connection/close";
    pub const CONNECTION_LIST: &str = "connection/list";
    pub const SCOPE_SUBSCRIBE: &str = "scope/subscribe";
    pub const SCOPE_UNSUBSCRIBE: &str = "scope/unsubscribe";
    pub const CHANNEL_CREATE: &str = "channel/create";
    pub const CHANNEL_LIST: &str = "channel/list";
    pub const CHANNEL_UPDATE: &str = "channel/update";
    pub const CHANNEL_DELETE: &str = "channel/delete";
    pub const CHANNEL_INVITE: &str = "channel/invite";
    pub const CHANNEL_REVOKE: &str = "channel/revoke";
    pub const CHANNEL_MEMBERS: &str = "channel/members";
    pub const CHANNEL_MEMBER_CONFIG_GET: &str = "channel/member_config.get";
    pub const CHANNEL_MEMBER_CONFIG_LIST: &str = "channel/member_config.list";
    pub const CHANNEL_MEMBER_CONFIG_SET: &str = "channel/member_config.set";
    pub const CHANNEL_MEMBER_CONFIG_CLEAR: &str = "channel/member_config.clear";
    pub const CHANNEL_SET_INSTRUCTION: &str = "channel/set_instruction";
    pub const CHANNEL_GET_INSTRUCTION: &str = "channel/get_instruction";
    pub const CHANNEL_CLEAR_INSTRUCTION: &str = "channel/clear_instruction";
    pub const THREAD_CREATE: &str = "thread/create";
    pub const THREAD_LIST: &str = "thread/list";
    pub const THREAD_UPDATE: &str = "thread/update";
    pub const THREAD_ARCHIVE: &str = "thread/archive";
    pub const THREAD_DELETE: &str = "thread/delete";
    pub const THREAD_FOLLOW: &str = "thread.follow";
    pub const THREAD_UNFOLLOW: &str = "thread.unfollow";
    pub const THREAD_SET_INSTRUCTION: &str = "thread/set_instruction";
    pub const THREAD_GET_INSTRUCTION: &str = "thread/get_instruction";
    pub const THREAD_CLEAR_INSTRUCTION: &str = "thread/clear_instruction";
    pub const TASK_CREATE: &str = "task.create";
    pub const TASK_GET: &str = "task.get";
    pub const TASK_LIST: &str = "task.list";
    pub const TASK_UPDATE: &str = "task.update";
    pub const TASK_CLAIM: &str = "task.claim";
    pub const TASK_ASSIGN: &str = "task.assign";
    pub const TASK_COMPLETE: &str = "task.complete";
    pub const TASK_REOPEN: &str = "task.reopen";
    pub const TASK_CANCEL: &str = "task.cancel";
    pub const TASK_REF_ATTACH: &str = "task/ref.attach";
    pub const TASK_REF_FIND: &str = "task/ref.find";
    pub const TASK_REF_LIST: &str = "task/ref.list";
    pub const TASK_ARTIFACT_ATTACH: &str = "task/artifact.attach";
    pub const TASK_ARTIFACT_ACTIVATE: &str = "task/artifact.activate";
    pub const TASK_ARTIFACT_LIST: &str = "task/artifact.list";
    pub const TASK_FACT_APPEND: &str = "task/fact.append";
    pub const TASK_FACT_LIST: &str = "task/fact.list";
    pub const TASK_PROJECTION_PUT: &str = "task/projection.put";
    pub const TASK_PROJECTION_GET: &str = "task/projection.get";
    pub const TASK_PROJECTION_LIST: &str = "task/projection.list";
    pub const TASK_ASSIGNMENT_CREATE: &str = "task/assignment.create";
    pub const TASK_ASSIGNMENT_UPDATE: &str = "task/assignment.update";
    pub const TASK_ASSIGNMENT_CONTEXT: &str = "task/assignment.context";
    pub const TASK_ASSIGNMENT_PREFLIGHT: &str = "task/assignment.preflight";
    pub const TASK_CHANGE_LIST: &str = "task/change.list";
    pub const TASK_CHANGE_ACK: &str = "task/change.ack";
    pub const TASK_WORKSPACE_LEASE_ACQUIRE: &str = "task/workspace.lease.acquire";
    pub const TASK_WORKSPACE_LEASE_RELEASE: &str = "task/workspace.lease.release";
    pub const TASK_WORKSPACE_LEASE_LIST: &str = "task/workspace.lease.list";
    pub const RUN_OPEN: &str = "run.open";
    pub const RUN_APPEND: &str = "run.append";
    pub const RUN_CLOSE: &str = "run.close";
    pub const RUN_CANCEL: &str = "run.cancel";
    pub const RUN_LIST: &str = "run.list";
    pub const RUN_GET: &str = "run.get";
    pub const COORDINATION_PROPOSE: &str = "coordination.propose";
    pub const COORDINATION_COMMIT: &str = "coordination.commit";
    pub const COORDINATION_RESPOND: &str = "coordination.respond";
    pub const COORDINATION_STEP: &str = "coordination.step";
    pub const COORDINATION_SKIP: &str = "coordination.skip";
    pub const COORDINATION_REASSIGN: &str = "coordination.reassign";
    pub const AGENT_CONFIG_PUBLISH: &str = "agent_config.publish";
    pub const AGENT_CONFIG_ACTIVATE: &str = "agent_config.activate";
    pub const MESSAGE_SEND: &str = "message.send";
    pub const MESSAGE_LIST: &str = "message.list";
    pub const MESSAGE_READ: &str = "message.read";
    pub const MESSAGE_REACTION_TOGGLE: &str = "message.reaction.toggle";
    pub const MESSAGE_SEARCH: &str = "message.search";
    pub const MESSAGE_CONTEXT: &str = "message.context";
    pub const ARTIFACT_PUBLISH: &str = "artifact/publish";
    pub const ARTIFACT_GET: &str = "artifact/get";
    pub const ARTIFACT_READ: &str = "artifact/read";
    pub const REMINDER_SCHEDULE: &str = "reminder/schedule";
    pub const REMINDER_LIST: &str = "reminder/list";
    pub const REMINDER_CANCEL: &str = "reminder/cancel";
    pub const REMINDER_SNOOZE: &str = "reminder/snooze";
    pub const REMINDER_UPDATE: &str = "reminder/update";
    /// §9.2 Durable actor inbox. Caller (must be bound to `actorId`) lists
    /// deliveries pending against its inbox, with cursor pagination so a
    /// host can resume after restart without losing directed events.
    pub const INBOX_LIST: &str = "inbox.list";
    pub const DELIVERY_ACK: &str = "delivery.ack";
    /// Compatibility create-and-wait wrapper around the durable command API.
    pub const MACHINE_COMMAND: &str = "machine/command";
    pub const MACHINE_COMMAND_CREATE: &str = "machine/command.create";
    pub const MACHINE_COMMAND_GET: &str = "machine/command.get";
    pub const MACHINE_COMMAND_LIST: &str = "machine/command.list";
    pub const MACHINE_COMMAND_ACK: &str = "machine/command.ack";
    pub const MACHINE_COMMAND_RESULT: &str = "machine/command.result";
    pub const MACHINE_COMMAND_CANCEL: &str = "machine/command.cancel";
    pub const ACTOR_LIST: &str = "actor/list";
    pub const ACTOR_UPSERT: &str = "actor/upsert";
    pub const ACTOR_DELETE: &str = "actor/delete";
    pub const ACTOR_GROUP_CREATE: &str = "actor.group.create";
    pub const ACTOR_GROUP_LIST: &str = "actor.group.list";
    pub const ACTOR_GROUP_ADD_MEMBER: &str = "actor.group.add_member";
    pub const ACTOR_GROUP_REMOVE_MEMBER: &str = "actor.group.remove_member";

    // outbound notification
    pub const STREAM_UPDATE: &str = "stream/update";
    pub const MACHINE_COMMAND_NOTIFY: &str = "machine/command.notify";
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
    /// Whether this connection should become the actor-inbox owner.
    /// Long-lived runtimes keep the default `true`; observer clients can
    /// bind identity for authorization without preempting the runtime.
    #[serde(default = "default_true")]
    pub claim_inbox: bool,
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

fn default_limit() -> u32 {
    50
}

// ---- channel/create ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelCreateParams {
    pub title: String,
    #[serde(default)]
    pub topic: String,
    /// Force a public channel even when the connection is bound to an actor.
    /// This is useful for product-level shared spaces where membership is
    /// tracked for discovery but should not gate visibility.
    #[serde(default)]
    pub public: bool,
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

// ---- channel/member_config.* ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelMemberConfigGetParams {
    pub channel_id: String,
    pub actor_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMemberConfigGetResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<ChannelMemberConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelMemberConfigListParams {
    pub channel_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMemberConfigListResult {
    pub configs: Vec<ChannelMemberConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelMemberConfigSetParams {
    pub channel_id: String,
    pub actor_id: String,
    pub workspace_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMemberConfigSetResult {
    pub config: ChannelMemberConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelMemberConfigClearParams {
    pub channel_id: String,
    pub actor_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMemberConfigClearResult {
    pub cleared: bool,
}

// ---- channel/update / delete ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelUpdateParams {
    pub channel_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<ChannelVisibility>,
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
    /// channel itself is removed. Defaults to false so older clients that omit
    /// the field keep the non-destructive behavior.
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

// ---- channel/set_instruction / get_instruction / clear_instruction ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelSetInstructionParams {
    pub channel_id: String,
    pub instructions: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelSetInstructionResult {
    pub channel: Channel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelGetInstructionParams {
    pub channel_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelGetInstructionResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelClearInstructionParams {
    pub channel_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelClearInstructionResult {
    pub cleared: bool,
}

// ---- thread/create / list ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadCreateParams {
    pub channel_id: String,
    pub title: String,
    pub root_message_id: String,
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
    #[serde(default)]
    pub archived: bool,
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
pub struct ThreadArchiveParams {
    pub thread_id: String,
    #[serde(default = "default_archive_archived")]
    pub archived: bool,
}

fn default_archive_archived() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadArchiveResult {
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

// ---- thread/set_instruction / get_instruction / clear_instruction ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSetInstructionParams {
    pub thread_id: String,
    pub instructions: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadSetInstructionResult {
    pub thread: Thread,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadGetInstructionParams {
    pub thread_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadGetInstructionResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadClearInstructionParams {
    pub thread_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadClearInstructionResult {
    pub cleared: bool,
}

// ---- thread.follow / thread.unfollow ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadFollowParams {
    pub thread_id: String,
    #[serde(default)]
    pub muted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadFollowResult {
    pub presence: ActorPresence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadUnfollowParams {
    pub thread_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadUnfollowResult {
    pub presence: ActorPresence,
}

// ---- task/create / get / list / update ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskCreateParams {
    /// Top-level channel message that should own the task metadata.
    pub source_message_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requester_actor_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_actor_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<TaskStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_source_message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub practice_contract_epoch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskCreateResult {
    pub task: Task,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskGetParams {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskGetResult {
    pub task: Task,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assignments: Vec<TaskAssignment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<TaskRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_links: Vec<TaskArtifactLink>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub facts: Vec<TaskFact>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projections: Vec<TaskProjection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TaskListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_actor_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub statuses: Vec<TaskStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskListResult {
    pub tasks: Vec<Task>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskUpdateParams {
    pub task_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<TaskStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_actor_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_ids: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub append_artifact_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskUpdateResult {
    pub task: Task,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskClaimParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskAssignParams {
    pub task_id: String,
    pub owner_actor_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskCompleteParams {
    pub task_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskReopenParams {
    pub task_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_actor_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskCancelParams {
    pub task_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_summary: Option<String>,
}

// ---- task refs / artifacts / facts / projections ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRefAttachParams {
    pub task_id: String,
    pub kind: String,
    #[serde(default)]
    pub subtype: String,
    pub value: String,
    #[serde(default)]
    pub normalized: String,
    #[serde(default)]
    pub fields: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<TaskRefConfidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<TaskRefStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_message_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRefAttachResult {
    pub task_ref: TaskRef,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TaskRefFindParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,
    pub kind: String,
    #[serde(default)]
    pub subtype: String,
    pub normalized: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<TaskRefConfidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<TaskRefStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRefFindResult {
    pub refs: Vec<TaskRef>,
    pub tasks: Vec<Task>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRefListParams {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRefListResult {
    pub refs: Vec<TaskRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskArtifactAttachParams {
    pub task_id: String,
    pub artifact_id: String,
    #[serde(default)]
    pub schema: String,
    #[serde(default)]
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<TaskArtifactLinkStatus>,
    #[serde(default)]
    pub lineage: Value,
    #[serde(default)]
    pub binding: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskArtifactAttachResult {
    pub link: TaskArtifactLink,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskArtifactActivateParams {
    pub link_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supersede_link_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskArtifactActivateResult {
    pub link: TaskArtifactLink,
    pub superseded: Vec<TaskArtifactLink>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskArtifactListParams {
    pub task_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<TaskArtifactLinkStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskArtifactListResult {
    pub links: Vec<TaskArtifactLink>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskFactAppendParams {
    pub task_id: String,
    #[serde(default)]
    pub target_key: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fact_type: Option<TaskFactType>,
    #[serde(default)]
    pub subject: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<TaskFactStatus>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub replaces: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retracted_by: Option<String>,
    #[serde(default)]
    pub authority: String,
    #[serde(default)]
    pub authority_binding: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<Timestamp>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer_id: Option<String>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskFactAppendResult {
    pub fact: TaskFact,
    #[serde(default)]
    pub created: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskFactListParams {
    pub task_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<TaskFactStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskFactListResult {
    pub facts: Vec<TaskFact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskProjectionPutParams {
    pub task_id: String,
    #[serde(default)]
    pub projection_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer_actor_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health: Option<TaskProjectionHealth>,
    #[serde(default)]
    pub watermark: Value,
    #[serde(default)]
    pub payload_schema: String,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskProjectionPutResult {
    pub projection: TaskProjection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskProjectionGetParams {
    pub task_id: String,
    #[serde(default)]
    pub projection_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskProjectionGetResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projection: Option<TaskProjection>,
    pub health: TaskProjectionHealth,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskProjectionListParams {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskProjectionListResult {
    pub projections: Vec<TaskProjection>,
}

// ---- task assignment ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskAssignmentCreateParams {
    pub task_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_actor_id: Option<String>,
    pub to_actor_id: String,
    #[serde(rename = "type")]
    pub assignment_type: TaskAssignmentType,
    pub instruction: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskAssignmentCreateResult {
    pub assignment: TaskAssignment,
    /// Directed wake message written to the task's canonical thread.
    pub message: Message,
    pub task: Task,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskAssignmentUpdateParams {
    pub assignment_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<TaskAssignmentStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_envelope: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub result_artifact_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub result_fact_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskAssignmentUpdateResult {
    pub assignment: TaskAssignment,
    pub task: Task,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskAssignmentContextParams {
    pub assignment_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskAssignmentContextResult {
    pub task: Task,
    pub assignment: TaskAssignment,
    pub refs: Vec<TaskRef>,
    pub artifact_links: Vec<TaskArtifactLink>,
    pub facts: Vec<TaskFact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projection: Option<TaskProjection>,
    pub guards: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskAssignmentPreflightParams {
    pub assignment_id: String,
    #[serde(default)]
    pub target_key: String,
    #[serde(default)]
    pub head: String,
    #[serde(default)]
    pub effect: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskAssignmentPreflightResult {
    pub preflight: TaskPreflightResult,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TaskChangeListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipient_actor_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default)]
    pub include_handled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_cursor: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskChangeListResult {
    pub deliveries: Vec<TaskChangeDelivery>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskChangeAckParams {
    pub change_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipient_actor_id: Option<String>,
    pub disposition: TaskChangeAckDisposition,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub result_ref_ids: Vec<String>,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskChangeAckResult {
    pub delivery: TaskChangeDelivery,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceLeaseAcquireParams {
    pub assignment_id: String,
    pub resource_key: String,
    pub mode: WorkspaceLeaseMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl_seconds: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceLeaseAcquireResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease: Option<WorkspaceLease>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<WorkspaceLease>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceLeaseReleaseParams {
    pub lease_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceLeaseReleaseResult {
    pub lease: WorkspaceLease,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceLeaseListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignment_id: Option<String>,
    #[serde(default)]
    pub active_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceLeaseListResult {
    pub leases: Vec<WorkspaceLease>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_delete_defaults_to_non_cascade() {
        let params: ChannelDeleteParams =
            serde_json::from_value(serde_json::json!({ "channelId": "chan_123" })).unwrap();
        assert_eq!(params.channel_id, "chan_123");
        assert!(!params.cascade);
    }

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
        assert!(transport.model_args.is_empty());
        assert!(transport.interactive.is_none());
        assert!(transport.provider.is_none());
    }

    #[test]
    fn interactive_transport_deserializes_model_and_claude_settings() {
        let transport: AgentTransport = serde_json::from_str(
            r#"{
                "kind": "interactive_command",
                "command": "claude",
                "model": "claude-sonnet-4-6",
                "modelArgs": ["--model", "{model}"],
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
        assert_eq!(transport.model.as_deref(), Some("claude-sonnet-4-6"));
        assert_eq!(transport.model_args, vec!["--model", "{model}"]);
        let interactive = transport.interactive.unwrap();
        assert_eq!(interactive.session.new_args.len(), 3);
        let provider = transport.provider.unwrap();
        assert_eq!(provider.kind, "claude");
        assert_eq!(
            provider.settings.unwrap().mode,
            ClaudeSettingsMode::ActorProfile
        );
    }

    #[test]
    fn command_transport_deserializes_turn_timeouts() {
        let transport: AgentTransport = serde_json::from_str(
            r#"{
                "kind": "command",
                "command": "claude",
                "timeoutMs": 7200000,
                "idleTimeoutMs": 1200000
            }"#,
        )
        .unwrap();
        assert_eq!(transport.timeout_ms, Some(7_200_000));
        assert_eq!(transport.idle_timeout_ms, Some(1_200_000));
    }

    #[test]
    fn command_transport_round_trips_unlimited_turn_timeouts() {
        let transport: AgentTransport = serde_json::from_str(
            r#"{
                "kind": "command",
                "command": "codex",
                "timeoutMs": -1,
                "idleTimeoutMs": -1
            }"#,
        )
        .unwrap();
        assert_eq!(transport.timeout_ms, Some(-1));
        assert_eq!(transport.idle_timeout_ms, Some(-1));

        let serialized = serde_json::to_value(transport).unwrap();
        assert_eq!(serialized["timeoutMs"], -1);
        assert_eq!(serialized["idleTimeoutMs"], -1);
    }

    #[test]
    fn run_list_params_rust_and_wire_defaults_match() {
        let rust_default = RunListParams::default();
        let wire_default: RunListParams =
            serde_json::from_value(serde_json::json!({})).expect("deserialize empty params");

        for params in [&rust_default, &wire_default] {
            assert!(params.statuses.is_none());
            assert!(params.actor_id.is_none());
            assert!(params.target.is_none());
            assert_eq!(params.limit, 50);
        }

        let serialized = serde_json::to_value(rust_default).expect("serialize default params");
        assert_eq!(serialized, serde_json::json!({ "limit": 50 }));
    }
}

// ---- run.* ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunOpenParams {
    pub actor_id: String,
    pub scope: ScopeRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_reason: Option<String>,
    pub agent_config_version_id: String,
    #[serde(default)]
    pub metadata: Meta,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunOpenResult {
    pub run: Run,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunAppendParams {
    pub run_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<RunStatus>,
    #[serde(default = "default_run_frame_kind")]
    pub frame_kind: String,
    #[serde(default)]
    pub payload: Value,
}

fn default_run_frame_kind() -> String {
    "log".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunAppendResult {
    pub run: Run,
    pub frame: RunFrame,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunCloseParams {
    pub run_id: String,
    #[serde(default = "default_run_close_status")]
    pub status: RunStatus,
}

fn default_run_close_status() -> RunStatus {
    RunStatus::Completed
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunCloseResult {
    pub run: Run,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunCancelParams {
    pub run_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunCancelResult {
    pub run: Run,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel_message: Option<Message>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statuses: Option<Vec<RunStatus>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

impl Default for RunListParams {
    fn default() -> Self {
        Self {
            statuses: None,
            actor_id: None,
            target: None,
            limit: default_limit(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunListResult {
    pub runs: Vec<Run>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunGetParams {
    pub run_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunGetResult {
    pub run: Run,
}

// ---- coordination.* ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoordinationProposeParams {
    pub target: String,
    pub mode: CoordinationMode,
    #[serde(default = "default_coordination_decision_rule")]
    pub decision_rule: CoordinationDecisionRule,
    #[serde(default)]
    pub participants: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_root_message_id: Option<String>,
    #[serde(default)]
    pub plan: Value,
    #[serde(default)]
    pub metadata: Meta,
}

fn default_coordination_decision_rule() -> CoordinationDecisionRule {
    CoordinationDecisionRule::OwnerDecides
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinationProposeResult {
    pub session: CoordinationSession,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoordinationCommitParams {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinationCommitResult {
    pub session: CoordinationSession,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoordinationRespondParams {
    pub session_id: String,
    pub accept: bool,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinationRespondResult {
    pub session: CoordinationSession,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoordinationStepParams {
    pub session_id: String,
    pub base_revision: u64,
    #[serde(default = "default_coordination_step_type")]
    pub step_type: CoordinationStepType,
    #[serde(default)]
    pub output: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_body: Option<String>,
}

fn default_coordination_step_type() -> CoordinationStepType {
    CoordinationStepType::Work
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinationStepResult {
    pub session: CoordinationSession,
    pub step: CoordinationStep,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<Message>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoordinationSkipParams {
    pub session_id: String,
    pub base_revision: u64,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinationSkipResult {
    pub session: CoordinationSession,
    pub step: CoordinationStep,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoordinationReassignParams {
    pub session_id: String,
    pub from_actor_id: String,
    pub to_actor_id: String,
    pub base_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinationReassignResult {
    pub session: CoordinationSession,
    pub step: CoordinationStep,
}

// ---- agent_config.* ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfigPublishParams {
    pub actor_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub adapter: String,
    #[serde(default)]
    pub tools: Value,
    #[serde(default)]
    pub capability_tags: Vec<String>,
    #[serde(default)]
    pub attention_policy: Value,
    #[serde(default)]
    pub context_policy: Value,
    #[serde(default)]
    pub reply_policy: Value,
    #[serde(default)]
    pub metadata: Meta,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfigPublishResult {
    pub version: AgentConfigVersion,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfigActivateParams {
    pub actor_id: String,
    pub version_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<ScopeRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfigActivateResult {
    pub activation: AgentConfigActivation,
    pub version: AgentConfigVersion,
}

// ---- message.* ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSendParams {
    pub target: String,
    pub body: String,
    #[serde(default)]
    pub mentions: Vec<MessageMention>,
    #[serde(default)]
    pub audience: Vec<AudienceRef>,
    #[serde(default = "default_message_send_intent")]
    pub intent: MessageIntent,
    #[serde(default = "default_message_send_delivery_policy")]
    pub delivery_policy: DeliveryPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_root_message_id: Option<String>,
    #[serde(default)]
    pub attachments: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    /// Optional optimistic-send guard. When present, the server appends only
    /// if the resolved target scope's latest message id is exactly this value.
    /// Agents use it for send-time rebase: read latest, adjust content, then
    /// send with this base id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub if_latest_message_id: Option<String>,
    #[serde(default)]
    pub metadata: Meta,
}

fn default_message_send_intent() -> MessageIntent {
    MessageIntent::Chat
}

fn default_message_send_delivery_policy() -> DeliveryPolicy {
    DeliveryPolicy::NotifyOnly
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSendResult {
    pub message: Message,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageListParams {
    pub target: String,
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_message_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageListResult {
    pub messages: Vec<Message>,
    pub page_info: PageInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageReadParams {
    pub message_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageReadResult {
    pub message: Message,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageReactionToggleParams {
    pub message_id: String,
    pub emoji: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageReactionToggleResult {
    pub message: Message,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSearchParams {
    pub query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Inclusive lower bound on `createdAt` (absolute timestamp, compared in UTC).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_after: Option<Timestamp>,
    /// Exclusive upper bound on `createdAt` (absolute timestamp, compared in UTC).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_before: Option<Timestamp>,
    #[serde(default = "default_search_limit")]
    pub limit: u32,
}

fn default_search_limit() -> u32 {
    20
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSearchResult {
    pub messages: Vec<Message>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageContextParams {
    pub message_id: String,
    #[serde(default = "default_message_context_window")]
    pub before: u32,
    #[serde(default = "default_message_context_window")]
    pub after: u32,
}

fn default_message_context_window() -> u32 {
    20
}

/// Server-side hard cap for each `message.context` window side.
pub const MESSAGE_CONTEXT_MAX_WINDOW: u32 = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageContextResult {
    pub before: Vec<Message>,
    pub anchor: Message,
    pub after: Vec<Message>,
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
    #[serde(default)]
    pub offset: u64,
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
    pub offset: u64,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<u64>,
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
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub _meta: Option<Meta>,
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

// ---- inbox.list (§9.2 durable actor inbox) ----

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InboxListParams {
    /// Caller must be bound to this actor (via `connection/open`); cross-actor
    /// inbox reads are refused.
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
pub struct InboxListEntry {
    pub delivery: Delivery,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<Message>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<Event>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InboxListResult {
    pub deliveries: Vec<InboxListEntry>,
    /// Opaque cursor for the next page. Absent when the result set was fully
    /// drained.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryAckParams {
    pub actor_id: String,
    pub source_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryAckResult {
    pub delivery: Delivery,
}

// ---- machine command durable lifecycle ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineCommandParams {
    pub machine_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine_actor_id: Option<String>,
    #[serde(default)]
    pub command: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub if_inventory_revision: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineCommandCreateParams {
    pub machine_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine_actor_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    pub operation: String,
    #[serde(default)]
    pub payload: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub if_inventory_revision: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineCommandCreateResult {
    pub command: MachineCommand,
    pub delivered: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineCommandGetParams {
    pub command_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineCommandGetResult {
    pub command: MachineCommand,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MachineCommandListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine_actor_id: Option<String>,
    #[serde(default)]
    pub statuses: Vec<MachineCommandStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MachineCommandListResult {
    pub commands: Vec<MachineCommand>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineCommandAckParams {
    pub command_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine_actor_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineCommandAckResult {
    pub command: MachineCommand,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineCommandResult {
    pub command_id: String,
    pub machine_id: String,
    pub machine_actor_id: String,
    pub ok: bool,
    #[serde(default)]
    pub output: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineCommandResultParams {
    pub command_id: String,
    pub machine_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine_actor_id: Option<String>,
    pub ok: bool,
    #[serde(default)]
    pub output: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_error: Option<MachineCommandError>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<MachineCommandStatus>,
}

pub type MachineCommandResponse = MachineCommandResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineCommandCancelParams {
    pub command_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineCommandCancelResult {
    pub command: MachineCommand,
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

// ---- actor.group.* ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorGroupCreateParams {
    pub channel_id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default)]
    pub member_actor_ids: Vec<String>,
    #[serde(default)]
    pub wake_agents: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorGroupCreateResult {
    pub group: ActorGroup,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ActorGroupListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorGroupListResult {
    pub groups: Vec<ActorGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorGroupMemberParams {
    pub group_id: String,
    pub actor_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorGroupMemberResult {
    pub group: ActorGroup,
}

// ---- agent/* ----

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentTransport {
    /// `"acp_stdio"` (default) or `"command"` (see docs/command-transport-v0.md).
    pub kind: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Provider-manifest argv template preserving conditional arg fragments.
    /// When present, command runtime expands this instead of the legacy
    /// string-only `args` + `modelArgs` bridge.
    #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "argSpecs")]
    pub arg_specs: Vec<ProviderArgSpec>,
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "authMethod")]
    pub auth_method: Option<String>,

    // ---- command / interactive command transport only ----
    /// Optional default model for transports that expose a CLI-level model flag.
    /// `loom-daemon` may override this with the actor's selected runtime model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Argv template appended when a command-style transport has an active
    /// model. `{model}` expands to the selected model id. Empty means this
    /// transport does not receive a CLI model argument.
    #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "modelArgs")]
    pub model_args: Vec<String>,
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
    /// Full provider decoder spec retained after ProviderManifest resolution.
    /// `outputFormat` is the legacy adapter selector; decoder carries
    /// manifest-driven reducers such as JSONL finalText extraction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decoder: Option<ProviderDecoderSpec>,
    /// Optional stderr provider decoder retained after ProviderManifest
    /// resolution. This lets provider-owned stderr protocols emit trace events
    /// or capture sessions without overloading stdout semantics.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "stderr")]
    pub stderr_decoder: Option<ProviderDecoderSpec>,
    /// How the prompt text is delivered to the subprocess. Defaults to `args`
    /// (appended after `args` as the final argv token).
    #[serde(default, rename = "promptVia")]
    pub prompt_via: PromptVia,
    /// Provider-owned prompt rendering rules. When present, the runtime first
    /// composes Loom prompt parts, renders these named outputs, and then
    /// exposes them to argv/env/stdin templates as `{prompt.<name>}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<ProviderPromptSpec>,
    /// Optional stdin template for command transports. This is the manifest
    /// driven replacement for `promptVia = stdin`; it can reference any runtime
    /// variable including `{prompt.full}` or `{prompt.user}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdin: Option<String>,
    /// Optional hard timeout for one command-transport turn. Positive values
    /// are milliseconds; `-1` means unlimited. When a positive timeout is
    /// exceeded, the daemon cancels the subprocess and fails the turn.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "timeoutMs")]
    pub timeout_ms: Option<i64>,
    /// Optional idle timeout for one command-transport turn. Positive values
    /// are milliseconds; `-1` means unlimited. The timer resets whenever the
    /// subprocess emits output.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "idleTimeoutMs"
    )]
    pub idle_timeout_ms: Option<i64>,

    // ---- interactive_command only; ignored by other transports ----
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interactive: Option<InteractiveCommandSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<InteractiveProviderSpec>,
    /// Deprecated compatibility mirror from ProviderModeSpec. Current Loom
    /// runtime awareness is projected through workspace AGENTS.md plus skills.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "instructionsVia"
    )]
    pub instructions_via: Option<String>,
}

impl AgentTransport {
    pub fn is_empty(&self) -> bool {
        self.kind.is_empty()
            && self.command.is_empty()
            && self.args.is_empty()
            && self.arg_specs.is_empty()
            && self.env.is_empty()
            && self.auth_method.is_none()
            && self.model.is_none()
            && self.model_args.is_empty()
            && self.session.is_none()
            && self.output_format.is_none()
            && self.decoder.is_none()
            && self.stderr_decoder.is_none()
            && self.prompt_via == PromptVia::default()
            && self.prompt.is_none()
            && self.stdin.is_none()
            && self.timeout_ms.is_none()
            && self.idle_timeout_ms.is_none()
            && self.interactive.is_none()
            && self.provider.is_none()
    }
}

/// Bookkeeping rules for `transport.kind = "command"`. Both fields together let
/// the adapter resume an existing session (`first_run_capture` extracts a
/// session id from the very first invocation; `resume_args` is the argv
/// template used on subsequent calls).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandSession {
    /// How Loom obtains the session id before launching a command. `loom_uuid`
    /// means Loom generates and stores a stable UUID for the actor/scope.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "idSource")]
    pub id_source: Option<CommandSessionIdSource>,
    /// Session reuse boundary. Defaults to `actor_scope`, matching the legacy
    /// command transport behavior.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// DSL: `stdout_json:<jq-style-path>`, `stdout_json_any:<path>|<path>`,
    /// `stderr_regex:<re>`, `file:<path>`. `None` means this CLI does not
    /// expose a resumable session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_run_capture: Option<String>,
    /// Argv template substituted with `{session_id}` and (when `prompt_via=args`)
    /// `{prompt}`. `None` means resume is not supported (each call is a first run).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_args: Option<Vec<String>>,
    /// Provider-manifest resume argv template preserving conditional arg
    /// fragments. When present, command runtime expands this instead of
    /// string-only `resumeArgs`.
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        rename = "resumeArgSpecs"
    )]
    pub resume_arg_specs: Vec<ProviderArgSpec>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandSessionIdSource {
    LoomUuid,
    ProviderCapture,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderManifest {
    #[serde(default = "default_provider_schema_version")]
    pub schema_version: u32,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extends: Option<String>,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub detect: ProviderDetectSpec,
    #[serde(default)]
    pub modes: std::collections::BTreeMap<String, ProviderModeSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models: Option<AgentModelSpec>,
}

fn default_provider_schema_version() -> u32 {
    1
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderDetectSpec {
    #[serde(default)]
    pub candidates: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderModeSpec {
    #[serde(default = "default_provider_transport")]
    pub transport: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<ProviderArgSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "modelArgs")]
    pub model_args: Vec<String>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub env: std::collections::BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<ProviderPromptSpec>,
    #[serde(default)]
    pub stdout: ProviderDecoderSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stderr: Option<ProviderDecoderSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<ProviderSessionSpec>,
    /// Hard turn timeout in milliseconds. `-1` means unlimited.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "timeoutMs")]
    pub timeout_ms: Option<i64>,
    /// Output-idle timeout in milliseconds. `-1` means unlimited.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "idleTimeoutMs"
    )]
    pub idle_timeout_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interactive: Option<InteractiveCommandSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<InteractiveProviderSpec>,
    /// Deprecated compatibility field. Current default providers no longer use
    /// this to split Loom runtime guidance between prompt and AGENTS.md.
    #[serde(default = "default_instructions_via", rename = "instructionsVia")]
    pub instructions_via: String,
}

pub fn default_instructions_via() -> String {
    "agents_md".into()
}

fn default_provider_transport() -> String {
    "command".into()
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum ProviderArgSpec {
    Literal(String),
    Conditional(ProviderConditionalArgSpec),
}

impl<'de> Deserialize<'de> for ProviderArgSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::String(value) => Ok(Self::Literal(value)),
            serde_json::Value::Object(_) => ProviderConditionalArgSpec::deserialize(value)
                .map(Self::Conditional)
                .map_err(serde::de::Error::custom),
            _ => Err(serde::de::Error::custom(
                "provider arg must be a string or conditional object",
            )),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderConditionalArgSpec {
    pub when: String,
    #[serde(default)]
    pub args: Vec<ProviderArgSpec>,
}

impl Default for ProviderArgSpec {
    fn default() -> Self {
        Self::Literal(String::new())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderPromptSpec {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspace_files: Vec<ProviderWorkspaceFileSpec>,
    #[serde(default)]
    pub outputs: std::collections::BTreeMap<String, ProviderPromptOutputSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderWorkspaceFileSpec {
    pub key: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "roleHint")]
    pub role_hint: Option<ProviderPromptRoleHint>,
    #[serde(default = "default_provider_workspace_file_optional")]
    pub optional: bool,
    #[serde(default = "default_provider_workspace_file_max_bytes")]
    pub max_bytes: u64,
}

fn default_provider_workspace_file_optional() -> bool {
    true
}

fn default_provider_workspace_file_max_bytes() -> u64 {
    32 * 1024
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderPromptOutputSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suffix: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "renderTitle"
    )]
    pub render_title: Option<ProviderRenderTitle>,
    #[serde(default)]
    pub required: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderRenderTitle {
    Always,
    Never,
    #[default]
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderPromptRoleHint {
    System,
    User,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentPromptAssemblySpec {
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub vars: std::collections::BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<AgentPromptFileSpec>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub outputs: std::collections::BTreeMap<String, AgentPromptOutputSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentPromptFileSpec {
    pub key: String,
    pub root: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "roleHint")]
    pub role_hint: Option<AgentPromptRoleHint>,
    #[serde(default = "default_agent_prompt_file_optional")]
    pub optional: bool,
    #[serde(default = "default_agent_prompt_file_max_bytes")]
    pub max_bytes: u64,
}

fn default_agent_prompt_file_optional() -> bool {
    true
}

fn default_agent_prompt_file_max_bytes() -> u64 {
    32 * 1024
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentPromptRoleHint {
    System,
    User,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentPromptOutputSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suffix: Option<String>,
    #[serde(default)]
    pub required: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderDecoderSpec {
    #[serde(default = "default_provider_decoder_format")]
    pub format: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub events: Vec<ProviderDecoderEventSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reduce: Option<ProviderJsonlReduceSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture: Option<ProviderDecoderCaptureSpec>,
}

fn default_provider_decoder_format() -> String {
    "text".into()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderDecoderEventSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<ProviderJsonConditionSpec>,
    #[serde(default)]
    pub emit: ProviderDecoderEmitSpec,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderDecoderCaptureSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<ProviderJsonlTextReducerSpec>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderDecoderEmitSpec {
    #[serde(default, rename = "type")]
    pub emit_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partial: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "toolName")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub success: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderJsonlReduceSpec {
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "finalText")]
    pub final_text: Option<ProviderJsonlTextReducerSpec>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderJsonlTextReducerSpec {
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<ProviderJsonConditionSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<Box<ProviderJsonlTextReducerSpec>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderJsonConditionSpec {
    #[serde(default)]
    pub all: Vec<ProviderJsonConditionSpec>,
    #[serde(default)]
    pub any: Vec<ProviderJsonConditionSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not: Option<Box<ProviderJsonConditionSpec>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equals: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "notEquals")]
    pub not_equals: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exists: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "absentOrNull"
    )]
    pub absent_or_null: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "notEmpty")]
    pub not_empty: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "in")]
    pub in_values: Option<Vec<serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "notIn")]
    pub not_in: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderSessionSpec {
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "idSource")]
    pub id_source: Option<ProviderSessionIdSource>,
    #[serde(default)]
    pub resume_args: Vec<ProviderArgSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderSessionIdSource {
    LoomUuid,
    ProviderCapture,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractiveSessionIdStrategy {
    #[default]
    LoomUuidPerScope,
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
    "{loom_envelope}".into()
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
    "__LOOM_DONE__".into()
}

fn default_interactive_instruction() -> String {
    "When your final user-visible answer is complete, output __LOOM_DONE__ on a line by itself. Do not output anything after it.".into()
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
    /// Claude Code / Qoder CLI `--output-format stream-json` framing.
    ClaudeStreamJson,
    /// GitHub Copilot CLI `--output-format json` JSONL session events.
    CopilotJson,
    /// OpenAI codex CLI `--json` event stream.
    CodexStreamJson,
    /// OpenCode `run --format json` raw JSON events.
    OpencodeJson,
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
    /// Inject prompt as the env var `LOOM_PROMPT`.
    Env,
}

/// Controls whether Loom teaches its native runtime protocol to the provider.
/// Hidden mode is intended for host applications that expose their own agent-facing tools.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeAwareness {
    #[default]
    Native,
    Hidden,
}

impl RuntimeAwareness {
    pub fn is_native(value: &Self) -> bool {
        matches!(value, Self::Native)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSpec {
    pub actor: Actor,
    /// Static instructions for this agent actor. Native awareness projects
    /// these into the workspace AGENTS.md block; hidden awareness delegates
    /// instruction delivery to the host application's provider integration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// Provider-catalog based runtime selection. The host resolves this into a
    /// runtime transport plan before starting the worker.
    #[serde(rename = "providerRef")]
    pub provider_ref: AgentProviderRef,
    #[serde(default)]
    pub autostart: bool,
    /// Whether Loom projects its AGENTS.md rules, default skill, turn contract,
    /// and runtime environment into the provider. Defaults to native behavior.
    #[serde(
        default,
        rename = "runtimeAwareness",
        skip_serializing_if = "RuntimeAwareness::is_native"
    )]
    pub runtime_awareness: RuntimeAwareness,
    /// Optional model menu for this actor. Loom treats these as runtime-level
    /// model ids: `loom-daemon` can surface them through `/models` and
    /// pass the selected id to transports that support model selection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models: Option<AgentModelSpec>,
    /// Optional actor-local bundle configuration. When present, the runtime
    /// ensures a skill / tool bundle is available under the actor home before
    /// the transport is started. Each turn projects the current scope's
    /// bundles into provider-native skill directories under the agent
    /// workspace, then exposes the resolved paths through template variables /
    /// env injection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle: Option<AgentBundleSpec>,
    /// Optional per-actor memory configuration. Defines where records live
    /// under `{agent.profile}/memory/`, how they are selected each turn, and
    /// how they reach the agent (prompt section and/or MCP bridge).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemorySpec>,
    /// Optional opt-in for the pinned-announcement MCP. When present and
    /// `mcp = true`, the runtime auto-injects a `loom-announcement` stdio
    /// server into the ACP session, giving the agent two tools:
    /// `announcement.set` and `announcement.clear` to publish a recap to
    /// the right-side panel of any chat client subscribed to the scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub announcement: Option<AnnouncementSpec>,
    /// Optional trigger metadata. Lets the agent declare a slash-command
    /// prefix that the runtime auto-injects into triggered message content,
    /// so callers don't have to know about skill-activation conventions like
    /// `/delivery` or `/discovery`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<TriggerSpec>,
    /// Optional wake / turn-intake policy: burst coalescing, dispatch
    /// debounce, and reply-reminder frequency. Absent fields fall back to
    /// runtime defaults (coalesce on, no debounce, full reminder on the
    /// first scope turn then a one-line pointer).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wake: Option<WakeSpec>,
    /// Optional per-agent prompt assembly. This is the agent-owned rule that
    /// turns Loom prompt parts and controlled profile/workspace files into the
    /// named outputs providers consume through `{prompt.system}`,
    /// `{prompt.user}`, and `{prompt.full}`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "promptAssembly"
    )]
    pub prompt_assembly: Option<AgentPromptAssemblySpec>,
    /// Optional per-actor prompt template (design §5). Wraps the trigger
    /// event content with `everyTurnPrefix`, `firstTurnPrefix` (first
    /// turn per scope only), and `everyTurnSuffix` lines, with template
    /// variable substitution. When absent the runtime falls back to the
    /// bare envelope shape used before the migration.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "promptTemplate"
    )]
    pub prompt_template: Option<PromptTemplateSpec>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProviderRef {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    /// Per-agent environment variables injected into the provider child process.
    /// Keys here override same-named keys from the provider manifest's mode.env.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub env: std::collections::BTreeMap<String, String>,
}

/// Callee-described trigger metadata. See `AgentSpec.trigger`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerSpec {
    /// Text prepended to the trigger message content (the user message side
    /// of the prompt). Typically a slash command like `"/delivery\n"` so
    /// the underlying provider activates the right skill.
    #[serde(default)]
    pub trigger_prompt_prefix: String,
    /// Whether the prefix applies on the first turn of a scope only or
    /// on every trigger. Default: `every-turn`.
    #[serde(default)]
    pub apply_on: TriggerPrefixApplyOn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum TriggerPrefixApplyOn {
    FirstTurn,
    #[default]
    EveryTurn,
}

/// Wake / turn-intake policy. See `AgentSpec.wake`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WakeSpec {
    /// Merge triggers that queued up behind a busy scope into a single turn
    /// instead of replaying them one full turn per message. Only plain
    /// messages with the same reply target and visibility are merged;
    /// task/assignment triggers always run alone. Default: true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coalesce: Option<bool>,
    /// Milliseconds to wait after a scope becomes free before composing the
    /// prompt, so a burst of quick messages lands in one turn instead of
    /// several. Default: 0 (dispatch immediately).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debounce_ms: Option<u64>,
    /// How often the response-delivery reminder is appended to the turn
    /// input. The full rules always live in the workspace `AGENTS.md`;
    /// this only controls the per-turn repetition. Default: `first-turn`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_reminder: Option<ReplyReminderMode>,
    /// What to do when a human message arrives while the same scope already
    /// has an in-flight provider turn. Default: `queue`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_human_message_while_busy: Option<OnHumanMessageWhileBusy>,
    /// Approximate token budget for bootstrap / pending-delivery context
    /// injected outside the structured wake body. Default is runtime-defined.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_token_budget: Option<u64>,
}

/// Reply-reminder frequency. See `WakeSpec.reply_reminder`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ReplyReminderMode {
    /// Append the full reminder block to every turn input.
    EveryTurn,
    /// Full reminder on the first turn of a scope (per worker run), then a
    /// one-line pointer to AGENTS.md on later turns.
    #[default]
    FirstTurn,
    /// Never append reminder text; rely on `AGENTS.md` / skills only.
    Off,
}

/// Busy-scope policy for newly arriving human messages. See `WakeSpec`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OnHumanMessageWhileBusy {
    /// Preserve current behavior: enqueue the new trigger behind the active
    /// turn, subject to wake coalescing when it eventually dispatches.
    #[default]
    Queue,
    /// Cancel the active provider turn, requeue its trigger batch, and let the
    /// cancelled batch coalesce with the new human message.
    CancelAndRequeue,
    /// Reserved for transports that can append input to an existing provider
    /// process. Unsupported transports treat this as `queue`.
    Inject,
}

/// Per-actor prompt template (design §5). All three lists are joined with
/// newlines after template-variable substitution. Variables not bound by
/// the runtime are left as literal `{var}` text (so missing vars never
/// collapse the prompt).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptTemplateSpec {
    /// Optional active-skill marker. Reserved for future use; runtime
    /// currently only echoes it back as `{prompt.activeSkill}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_skill: Option<String>,
    /// Lines prepended to *every* turn's prompt.
    #[serde(default)]
    pub every_turn_prefix: Vec<String>,
    /// Lines prepended only on the first turn per scope (after
    /// `everyTurnPrefix`, before the user message).
    #[serde(default)]
    pub first_turn_prefix: Vec<String>,
    /// Lines appended to *every* turn's prompt (after the user message).
    #[serde(default)]
    pub every_turn_suffix: Vec<String>,
    /// Extra variables surfaced as `{vars.<key>}`. Static per-actor
    /// constants the spec author wants without polluting global names.
    #[serde(default)]
    pub vars: std::collections::BTreeMap<String, String>,
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
    /// When true, the ACP `session/new` call synthesizes a `loom-announcement`
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
    /// Additional actor-local skills to project into each scope workspace's
    /// provider-native skill directories. Each source is one skill directory.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<AgentBundleSkillSpec>,
}

impl Default for AgentBundleSpec {
    fn default() -> Self {
        Self {
            source: String::new(),
            version: String::new(),
            install_mode: BundleInstallMode::default(),
            root: default_bundle_root(),
            current: default_bundle_current(),
            skills: Vec::new(),
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentBundleSkillSpec {
    /// Stable skill directory name under provider-native skills roots.
    /// Empty means derive from the source basename.
    #[serde(default)]
    pub id: String,
    /// Source skill directory. Supports the same agent path templates as
    /// bundle source/current, including `{agent.bundle}`.
    #[serde(default)]
    pub source: String,
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
    /// When true, the ACP `session/new` call synthesizes a `loom-memory`
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

// ---- service spec (parallel to AgentSpec; runs under `loom-daemon`) ----

/// On-disk spec for a service actor (kind = Service). Mirrors `AgentSpec`
/// for the host process: `loom-daemon` loads `*.json` from a specs
/// directory, instantiates the named plugin (`kind`), and binds it to a
/// long-lived service actor connection. See
/// `docs/service-plugin-system-design.md` §6.1.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceSpec {
    /// Unique within a specs directory. Used for state-dir naming
    /// (`~/.local/share/loom/service-host/services/<id>/`) and for the
    /// `--allow-services` filter on `loom-daemon`. Distinct from
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
    /// Start this service automatically when `loom-daemon` boots.
    /// Default true — services exist to run continuously, the override is
    /// for staged rollout / debugging.
    #[serde(default = "default_true")]
    pub autostart: bool,
    /// Primary channel the service writes into. Optional because some
    /// plugins compute scope per-event from external context (e.g., am's
    /// `auto_thread` mode in §7.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,
    /// Default directed-message target for service output. Plugins are not
    /// forced to honor this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_agent: Option<String>,
    /// Lifecycle of this service. `channel_singleton` (default) means
    /// one instance per host bound to `channel_id`; `thread_bound` means
    /// the service is launched per-thread with state stored under
    /// `instances/<thread_id>/`. See design §4.7.3.
    #[serde(default)]
    pub lifecycle: ServiceLifecycle,
    /// Optional binding constraints for `lifecycle = thread_bound`. The
    /// host reads `bind.auto_stop_on` to decide which events tear down
    /// the per-thread instance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bind: Option<ServiceBind>,
    /// Optional JSON-Schema fragment describing `--params` accepted at
    /// `loom service start --in <thread> --params {...}`. The CLI validates
    /// start params against the shallow subset it supports.
    #[serde(
        default,
        alias = "params_schema",
        skip_serializing_if = "Option::is_none"
    )]
    pub params_schema: Option<Value>,
    /// Plugin-specific configuration. Parsed by the plugin itself, not by
    /// the host. Schema is the plugin's contract (see §7 for am, §8 for
    /// scheduler).
    #[serde(default)]
    pub config: Value,
}

/// Lifecycle of a [`ServiceSpec`]. See design §4.7.3.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ServiceLifecycle {
    /// Single channel-level instance per host. Default.
    #[default]
    ChannelSingleton,
    /// One instance per bound thread. Multiple instances of the same
    /// spec coexist; each owns its own state dir.
    ThreadBound,
}

/// Binding info for a [`ServiceLifecycle::ThreadBound`] service.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct ServiceBind {
    /// Scope kind the instance is bound to. v1: only `"thread"`.
    #[serde(default = "default_bind_scope")]
    pub scope: String,
    /// Event kinds that auto-stop the instance. Typical values:
    /// `"thread.closed"`, `"service.self_complete"`.
    #[serde(default)]
    pub auto_stop_on: Vec<String>,
}

fn default_bind_scope() -> String {
    "thread".to_string()
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
    /// Hoist the legacy `config.{lifecycle,bind,paramsSchema}` shape into
    /// the typed top-level fields. Pre-`p4a` ServiceSpec carried these
    /// under `config{}` because the typed fields didn't exist; once
    /// migrated, hoisting is a no-op so it's safe to call repeatedly.
    /// Top-level fields win when both positions are populated.
    pub fn normalize(&mut self) {
        let cfg = match self.config.as_object_mut() {
            Some(map) => map,
            None => return,
        };
        if matches!(self.lifecycle, ServiceLifecycle::ChannelSingleton) {
            if let Some(v) = cfg.remove("lifecycle") {
                if let Ok(lc) = serde_json::from_value::<ServiceLifecycle>(v) {
                    self.lifecycle = lc;
                }
            }
        } else {
            cfg.remove("lifecycle");
        }
        if self.bind.is_none() {
            if let Some(v) = cfg.remove("bind") {
                if let Ok(b) = serde_json::from_value::<ServiceBind>(v) {
                    self.bind = Some(b);
                }
            }
        } else {
            cfg.remove("bind");
        }
        if self.params_schema.is_none() {
            if let Some(v) = cfg.remove("params_schema") {
                self.params_schema = Some(v);
            } else if let Some(v) = cfg.remove("paramsSchema") {
                self.params_schema = Some(v);
            }
        } else {
            cfg.remove("params_schema");
            cfg.remove("paramsSchema");
        }
    }

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
    pub const THREAD_UPDATED: &str = "thread.updated";
    pub const TASK_CHANGED: &str = "task.changed";
    pub const TASK_ASSIGNMENT_CHANGED: &str = "task_assignment.changed";
    /// Broadcast when a new channel is created. Public channels go to
    /// all connections; private channels go to the creator only.
    pub const CHANNEL_CREATED: &str = "channel.created";
    pub const CHANNEL_UPDATED: &str = "channel.updated";
    pub const CHANNEL_DELETED: &str = "channel.deleted";
    pub const RUN_UPDATED: &str = "run.updated";
    pub const MESSAGE_CREATED: &str = "message.created";
    pub const MESSAGE_UPDATED: &str = "message.updated";
    pub const EVENT_CREATED: &str = "event.created";
    pub const ARTIFACT_PUBLISHED: &str = "artifact.published";
    pub const DELIVERY_UPDATED: &str = "delivery.updated";
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
mod service_spec_tests {
    use super::*;
    use serde_json::json;

    fn service_actor() -> Actor {
        Actor {
            id: "svc_webhook_bridge".into(),
            kind: ActorKind::Service,
            display_name: "Webhook Bridge".into(),
            capabilities: None,
            _meta: None,
        }
    }

    fn base_spec() -> ServiceSpec {
        ServiceSpec {
            id: "webhook_bridge".into(),
            kind: "webhook".into(),
            actor: service_actor(),
            autostart: true,
            channel_id: Some("chan_x".into()),
            target_agent: Some("actor_qa".into()),
            lifecycle: ServiceLifecycle::ChannelSingleton,
            bind: None,
            params_schema: None,
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
                spec_id: "webhook_bridge".into()
            })
        );
    }

    #[test]
    fn round_trips_through_full_example() {
        // A fully-populated ServiceSpec must deserialize cleanly and keep
        // the opaque plugin config intact.
        let raw = json!({
            "id": "webhook_bridge",
            "kind": "webhook",
            "actor": {
                "id": "svc_webhook_bridge",
                "kind": "service",
                "displayName": "Webhook Bridge"
            },
            "channelId": "chan_x",
            "targetAgent": "actor_qa",
            "config": {
                "endpoint": "https://example.com/hook",
                "scope": "auto_thread",
                "replyMode": "async_send"
            }
        });
        let spec: ServiceSpec = serde_json::from_value(raw.clone()).expect("deserialize");
        spec.validate().expect("valid");
        assert_eq!(spec.id, "webhook_bridge");
        assert_eq!(spec.kind, "webhook");
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

    #[test]
    fn lifecycle_defaults_channel_singleton() {
        let spec = base_spec();
        assert_eq!(spec.lifecycle, ServiceLifecycle::ChannelSingleton);
        assert!(spec.bind.is_none());
        assert!(spec.params_schema.is_none());
    }

    #[test]
    fn normalize_hoists_legacy_config_lifecycle() {
        // mr-detector-shaped spec — typed fields living under config{}
        // before p4a. After normalize() they must move to the top level
        // and config{} loses them so plugin parsers don't see noise.
        let raw = json!({
            "id": "mr-detector",
            "kind": "scheduler",
            "actor": {"id": "svc_mr_detector", "kind": "service"},
            "autostart": false,
            "config": {
                "lifecycle": "thread_bound",
                "bind": {
                    "scope": "thread",
                    "auto_stop_on": ["thread.closed", "service.self_complete"]
                },
                "params_schema": {"required": ["mr_url"]},
                "jobs": []
            }
        });
        let mut spec: ServiceSpec = serde_json::from_value(raw).expect("parse");
        spec.normalize();
        assert_eq!(spec.lifecycle, ServiceLifecycle::ThreadBound);
        let bind = spec.bind.as_ref().expect("bind hoisted");
        assert_eq!(bind.scope, "thread");
        assert_eq!(
            bind.auto_stop_on,
            vec!["thread.closed".to_string(), "service.self_complete".into()]
        );
        assert!(spec.params_schema.is_some());
        // jobs is a SchedulerConfig field — must remain inside config.
        assert!(spec.config.get("jobs").is_some());
        assert!(spec.config.get("lifecycle").is_none());
        assert!(spec.config.get("bind").is_none());
        assert!(spec.config.get("params_schema").is_none());
    }

    #[test]
    fn normalize_accepts_camelcase_params_schema() {
        let raw = json!({
            "id": "x",
            "kind": "scheduler",
            "actor": {"id": "svc_x", "kind": "service"},
            "config": {"paramsSchema": {"required": ["a"]}}
        });
        let mut spec: ServiceSpec = serde_json::from_value(raw).expect("parse");
        spec.normalize();
        assert!(spec.params_schema.is_some());
    }

    #[test]
    fn deserialize_accepts_top_level_snake_case_params_schema() {
        let raw = json!({
            "id": "x",
            "kind": "scheduler",
            "actor": {"id": "svc_x", "kind": "service"},
            "params_schema": {"required": ["mr_url"]},
            "config": {"jobs": []}
        });
        let spec: ServiceSpec = serde_json::from_value(raw).expect("parse");
        assert_eq!(
            spec.params_schema
                .as_ref()
                .and_then(|schema| schema.get("required"))
                .and_then(|required| required.as_array())
                .map(|required| required.len()),
            Some(1)
        );
    }

    #[test]
    fn normalize_top_level_wins_over_legacy_config() {
        // If both positions carry a value, the typed top-level wins and
        // the legacy duplicate is dropped from config{} to keep specs
        // canonical.
        let raw = json!({
            "id": "x",
            "kind": "scheduler",
            "actor": {"id": "svc_x", "kind": "service"},
            "lifecycle": "thread_bound",
            "config": {"lifecycle": "channel_singleton"}
        });
        let mut spec: ServiceSpec = serde_json::from_value(raw).expect("parse");
        spec.normalize();
        assert_eq!(spec.lifecycle, ServiceLifecycle::ThreadBound);
        assert!(spec.config.get("lifecycle").is_none());
    }

    #[test]
    fn normalize_is_idempotent() {
        let raw = json!({
            "id": "x",
            "kind": "scheduler",
            "actor": {"id": "svc_x", "kind": "service"},
            "config": {
                "lifecycle": "thread_bound",
                "bind": {"scope": "thread", "auto_stop_on": []}
            }
        });
        let mut spec: ServiceSpec = serde_json::from_value(raw).expect("parse");
        spec.normalize();
        let after_first = (spec.lifecycle, spec.bind.clone());
        spec.normalize();
        assert_eq!(after_first, (spec.lifecycle, spec.bind.clone()));
    }
}
