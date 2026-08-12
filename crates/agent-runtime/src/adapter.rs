//! Cross-transport adapter abstraction.
//!
//! v0 had a single `AcpAdapter` whose concrete API leaked out into `RuntimeManager`
//! and `wakeup`. v1 (per docs/architecture.md §4) introduces the
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
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
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

    /// Forward a single prompt to the agent in the given scope. Returns only
    /// after the provider has accepted the prompt execution boundary, not when
    /// the provider eventually finishes it. Each distinct `scope` is
    /// conceptually its own conversation: ACP allocates one `session/new` per
    /// scope with the supplied `cwd`; command transport starts its one-shot
    /// subprocess in the supplied `cwd`.
    async fn send_prompt(&self, prompt: AdapterPrompt) -> Result<(), String>;

    /// Reply to an `AdapterEvent::ActionRequest` previously emitted by the agent.
    /// Transports without permission prompts (e.g. command/v0) may treat this as
    /// a no-op or return an error if called.
    async fn respond_action(&self, request_id: String, option_id: String) -> Result<(), String>;

    /// Return runtime-provided model options for this prompt scope, if the
    /// transport exposes them. ACP surfaces these through `session/new`
    /// configOptions; transports without a runtime model picker return `None`.
    async fn list_model_options(
        &self,
        _prompt: AdapterPrompt,
    ) -> Result<Option<AdapterModelOptions>, String> {
        Ok(None)
    }

    /// Set a runtime-provided model option for an existing prompt scope. ACP
    /// implements this as `session/set_config_option`; unsupported transports
    /// return `None`.
    async fn set_model_option(
        &self,
        _scope: ScopeRef,
        _config_id: String,
        _value: String,
    ) -> Result<Option<AdapterModelOptions>, String> {
        Ok(None)
    }

    /// Cancel any in-flight prompt for `scope`. Idempotent — calling on a
    /// scope with no active prompt is a no-op. Implementations should NOT
    /// block on the cancellation completing; the eventual
    /// `AdapterEvent::Finished { success: false, .. }` will arrive on the
    /// event stream just like a normal completion.
    async fn cancel(&self, scope: ScopeRef) -> Result<(), String>;

    /// Reset the provider session for `scope_id`, discarding conversation
    /// history so the next `send_prompt` starts a fresh session. Used by the
    /// Warm-summary session-reset flow (ARCH §C): after a summary is
    /// generated and persisted, the session is reset and the original
    /// trigger is re-queued with the summary injected as Warm context.
    ///
    /// Default implementation is a no-op (transports without persistent
    /// sessions have nothing to reset).
    async fn reset_session(&self, _scope_id: &str) -> Result<(), String> {
        Ok(())
    }

    /// Stop the agent. Implementations should be idempotent.
    async fn stop(&self) -> Result<(), String>;
}

#[derive(Debug, Clone)]
pub struct AdapterPrompt {
    pub scope: ScopeRef,
    pub content: String,
    pub parts: Vec<PromptPart>,
    pub outputs: BTreeMap<String, String>,
    pub model: Option<String>,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    pub template_vars: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptPart {
    pub key: String,
    pub title: String,
    /// Provider-facing body without the Loom section title. Provider manifests
    /// decide whether to render `title` via `renderTitle`.
    pub content: String,
    /// Legacy/full-prompt body as it appeared in the composed Loom envelope.
    /// This lets callers preserve the old envelope string while exposing raw
    /// parts to provider prompt rendering.
    pub rendered_content: String,
    pub role_hint: PromptRoleHint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptRoleHint {
    System,
    User,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterModelOptions {
    pub config_id: String,
    pub current_value: Option<String>,
    pub choices: Vec<AdapterModelChoice>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterModelChoice {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TokenUsage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub estimated: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
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
        usage: Option<TokenUsage>,
    },
    /// Streaming token-usage snapshot emitted during an in-flight turn.
    ///
    /// Each `UsageUpdate` carries the current cumulative snapshot for `scope`
    /// as reported by the provider (or estimated, in which case
    /// `usage.estimated == true`). Consumers MUST treat it as a snapshot
    /// replace, NOT a delta — providers may revise figures mid-turn
    /// (e.g. cache_read tokens surfacing on the second segment).
    ///
    /// `Finished{usage}` is the authoritative final value; UsageUpdate is
    /// best-effort progress for UX. If `Finished.usage` arrives while
    /// UsageUpdates are still in-flight, the Finished value wins.
    UsageUpdate {
        scope: Option<ScopeRef>,
        usage: TokenUsage,
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
            | AdapterEvent::UsageUpdate { scope, .. }
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
