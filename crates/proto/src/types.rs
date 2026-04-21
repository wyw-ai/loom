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
    #[serde(alias = "space")]
    Channel,
    #[serde(alias = "conversation")]
    Thread,
    Turn,
    Event,
    Artifact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ref {
    pub kind: RefKind,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Meta>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ScopeKind {
    #[serde(alias = "space")]
    Channel,
    #[serde(alias = "conversation")]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Meta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Channel {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Meta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub id: String,
    #[serde(alias = "spaceId")]
    pub channel_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    pub trigger_event_id: Option<String>,
    pub status: TurnStatus,
    pub opened_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<Timestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Meta>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    RepliesTo,
    /// "X is meant for actor Y." Used for both explicit handoffs (`/handoff`,
    /// `@mention`) and reply-induced targeting. The legacy distinction
    /// between `targets` (soft @ mention) and `hands_off_to` (must respond)
    /// collapsed once the policy became "@ always implies handoff" — old
    /// journals using the `"targets"` discriminator still load via the
    /// serde alias.
    #[serde(alias = "targets")]
    HandsOffTo,
    RespondsTo,
    AttachesArtifact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relation {
    pub kind: RelationKind,
    pub target: Ref,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Meta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: String,
    /// Event type, e.g. "content.add", "action.request", "action.response",
    /// "artifact.publish", "turn.close". Handoff is expressed as a
    /// `content.add` carrying a `HandsOffTo` relation rather than a
    /// dedicated event kind. Vendor extensions
    /// allowed. Agent tool calls and internal status changes are NOT events;
    /// they are turn-private trace frames carried by `turn/trace.update`
    /// (see `proto::types::trace`).
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    pub last_read_event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    pub event_id: String,
    pub actor_id: String,
    pub state: DeliveryState,
    pub updated_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Meta>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ReceiptKind {
    Seen,
    Read,
    Accepted,
    Declined,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Receipt {
    pub event_id: String,
    pub actor_id: String,
    pub kind: ReceiptKind,
    pub recorded_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Meta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub id: String,
    pub actor_id: String,
    pub endpoint_id: String,
    pub opened_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Meta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageInfo {
    pub has_more: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Meta>,
}

// ---- common payload helpers ----

pub mod payload {
    use super::Meta;
    use serde::{Deserialize, Serialize};

    /// Payload for `content.add`.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ContentAdd {
        #[serde(default = "default_content_type")]
        pub content_type: String,
        pub text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
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

    /// Payload for `turn.close`.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct TurnCloseEvent {
        pub status: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub stop_reason: Option<String>,
    }
}

// ---- turn-private trace ----
//
// Trace frames live on a `Turn` and are visible only to the turn's owner
// actor (the agent that runs the turn). They are NOT events: they have no
// global event id, are not stored in `events_by_scope`, are never returned by
// `scope/read`, and cannot be addressed by `replies_to` / `responds_to` /
// `references`. They are delivered through the dedicated `turn/trace.update`
// notification and can be re-read by the owner via `turn/trace.read`.

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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub _meta: Option<Meta>,
    }
}
