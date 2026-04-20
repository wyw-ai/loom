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
    Space,
    Conversation,
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
    Space,
    Conversation,
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
pub struct Space {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Meta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: String,
    pub space_id: String,
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
    Targets,
    HandsOffTo,
    RespondsTo,
    AttachesArtifact,
    References,
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
    /// Event type, e.g. "content.add", "tool.report", "action.request",
    /// "action.response", "handoff.offer", "artifact.publish", "status.report",
    /// "turn.close". Vendor extensions allowed.
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
    Bundle,
    Blob,
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
    use serde_json::Value;

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

    /// Payload for `tool.report`.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ToolReport {
        pub tool_name: String,
        #[serde(default)]
        pub input: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub status: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub output: Option<Value>,
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

    /// Payload for `handoff.offer`.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct HandoffOffer {
        #[serde(default)]
        pub message: String,
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
