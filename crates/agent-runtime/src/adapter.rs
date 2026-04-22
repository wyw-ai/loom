//! Cross-transport adapter abstraction.
//!
//! v0 had a single `AcpAdapter` whose concrete API leaked out into `RuntimeManager`
//! and `wakeup`. v1 (per docs/architecture-v1-agent-client.md §4) introduces the
//! `Adapter` trait so additional transports — first up `CommandAdapter`, see
//! docs/command-transport-v0.md — can plug in without changing call sites.
//!
//! Every variant of `AdapterEvent` carries an optional `scope`: per-scope events
//! (Text, ToolUse, ActionRequest, Finished) MUST set it so the runtime can map
//! the event back to the right open turn when the same agent is active in
//! multiple channels concurrently. Agent-wide events (StatusChange / Error from
//! a process-level failure) may leave it `None`.

use async_trait::async_trait;
use proto::types::ScopeRef;
use serde_json::Value;
use tokio::sync::mpsc;

#[async_trait]
pub trait Adapter: Send + Sync {
    /// Spawn the underlying agent (ACP child, command supervisor, ...) and start
    /// pushing translated events into `events`. Returns once the agent is ready
    /// to accept its first prompt.
    async fn start(
        &self,
        events: mpsc::UnboundedSender<AdapterEvent>,
    ) -> Result<AdapterStartInfo, String>;

    /// Forward a single prompt to the agent in the given scope. Each distinct
    /// `scope` is conceptually its own conversation: ACP allocates one
    /// `session/new` per scope; command transport keys per-scope resume tokens
    /// at `~/.local/share/joi/agent-client/sessions/<actor>/<scope>.json`.
    async fn send_prompt(&self, scope: ScopeRef, prompt: String) -> Result<(), String>;

    /// Reply to an `AdapterEvent::ActionRequest` previously emitted by the agent.
    /// Transports without permission prompts (e.g. command/v0) may treat this as
    /// a no-op or return an error if called.
    async fn respond_action(&self, request_id: String, option_id: String) -> Result<(), String>;

    /// Stop the agent. Implementations should be idempotent.
    async fn stop(&self) -> Result<(), String>;
}

/// Emitted by every adapter back into the runtime.
#[derive(Debug, Clone)]
pub enum AdapterEvent {
    Text {
        scope: Option<ScopeRef>,
        content: String,
        is_partial: bool,
    },
    ToolUse {
        scope: Option<ScopeRef>,
        tool_name: String,
        input: Value,
    },
    ActionRequest {
        scope: Option<ScopeRef>,
        id: String,
        request_type: String,
        title: String,
        description: String,
        choices: Vec<ActionChoice>,
    },
    StatusChange {
        scope: Option<ScopeRef>,
        status: String,
    },
    Finished {
        scope: Option<ScopeRef>,
        success: bool,
        summary: String,
    },
    Error {
        scope: Option<ScopeRef>,
        message: String,
    },
}

impl AdapterEvent {
    /// Convenience accessor used by the runtime to route the event back to the
    /// scope's open turn. Returns `None` for agent-wide events.
    pub fn scope(&self) -> Option<&ScopeRef> {
        match self {
            AdapterEvent::Text { scope, .. }
            | AdapterEvent::ToolUse { scope, .. }
            | AdapterEvent::ActionRequest { scope, .. }
            | AdapterEvent::StatusChange { scope, .. }
            | AdapterEvent::Finished { scope, .. }
            | AdapterEvent::Error { scope, .. } => scope.as_ref(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ActionChoice {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct AdapterStartInfo {
    pub pid: Option<u32>,
    /// Some adapters allocate a single long-lived id at start (command:
    /// `cmd:<actor>`); ACP allocates sessions lazily per-scope so it returns
    /// `None` here. Treated as informational metadata only.
    pub session_id: Option<String>,
}
