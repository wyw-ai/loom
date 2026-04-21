//! Cross-transport adapter abstraction.
//!
//! v0 had a single `AcpAdapter` whose concrete API leaked out into `RuntimeManager`
//! and `wakeup`. v1 (per docs/architecture-v1-agent-client.md §4) introduces the
//! `Adapter` trait so additional transports — first up `CommandAdapter`, see
//! docs/command-transport-v0.md — can plug in without changing call sites.
//!
//! E1 only extracts the trait in-place; ACP behavior is unchanged. The trait is
//! deliberately minimal (no per-call `scope`, no error enum) to keep this phase a
//! pure refactor.

use async_trait::async_trait;
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

    /// Forward a single prompt to the agent. For long-lived transports (ACP) this
    /// is a `session/prompt`; for command-style transports each call corresponds
    /// to a fresh subprocess.
    async fn send_prompt(&self, prompt: String) -> Result<(), String>;

    /// Reply to an `AdapterEvent::ActionRequest` previously emitted by the agent.
    /// Transports without permission prompts (e.g. command/v0) may treat this as
    /// a no-op or return an error if called.
    async fn respond_action(
        &self,
        request_id: String,
        option_id: String,
    ) -> Result<(), String>;

    /// Stop the agent. Implementations should be idempotent.
    async fn stop(&self) -> Result<(), String>;
}

/// Emitted by every adapter back into the runtime. v0's `AgentEvent` lived on
/// `acp.rs`; renaming + relocating here is the only change in E1.
#[derive(Debug, Clone)]
pub enum AdapterEvent {
    Text {
        content: String,
        is_partial: bool,
    },
    ToolUse {
        tool_name: String,
        input: Value,
    },
    ActionRequest {
        id: String,
        request_type: String,
        title: String,
        description: String,
        choices: Vec<ActionChoice>,
    },
    StatusChange {
        status: String,
    },
    Finished {
        success: bool,
        summary: String,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone)]
pub struct ActionChoice {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct AdapterStartInfo {
    pub pid: Option<u32>,
    pub session_id: String,
}
