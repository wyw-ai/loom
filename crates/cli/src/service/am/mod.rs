// Public S2 surface; first user is `loom service am-handler` (added in
// S2-5). Silence dead-code warnings until then so Stage-by-stage
// commits stay clean.
#![allow(dead_code)]

//! `am` plugin entry point. S2 ships AM as a short-lived CLI handler
//! (`loom service am-handler --service-id <id>`) that replaces the
//! removed Python bridge. Each
//! `am listen --script` invocation spawns one handler, which:
//!
//! 1. Reads the message JSON from stdin.
//! 2. Loads the `ServiceSpec` for `<id>` and parses `spec.config` as
//!    [`AmConfig`].
//! 3. Connects to loom-server as the service actor, writes a directed
//!    message into the resolved scope, optionally awaits the agent reply,
//!    and either prints a callback JSON to stdout or invokes `am`.
//!
//! No long-lived plugin process in S2 — see the design doc §12 S2 entry
//! and the AM-shape architectural decision (`A` in the S2 dialog). The
//! `ServiceRuntime` substrate from S1 is used per-invocation; thread
//! map / dedupe live in files under
//! `~/.local/share/loom/service-host/services/<id>/`.

pub mod extract;
pub mod handler;
pub mod reply;
pub mod scope;
pub mod text;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use self::reply::{AmSendConfig, ReplyMode};
use self::scope::ScopeMode;

pub use handler::run_handler;

/// Plugin-specific deserialized form of `ServiceSpec.config` for
/// `kind = "am"`. Defaults match the Python reference so a freshly
/// migrated operator with no `config` overrides still gets the same
/// behaviour as before.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AmConfig {
    /// Path to the `am` binary. Default: `am` (looked up on PATH).
    pub am_bin: String,
    /// Path to `am`'s `config.properties`. Default: rely on `am`'s own
    /// resolution (none passed through).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub am_config_path: Option<PathBuf>,
    /// DingTalk topic. Default: `/v1.0/im/bot/messages/get`. Listener-side
    /// only — the handler doesn't subscribe; it's documentation for the
    /// `am listen --topic` flag operators wire up themselves.
    pub topic: String,
    /// Where to write each directed message (§7.2).
    pub scope: ScopeMode,
    /// Fixed thread id used by `scope = "thread"` (and as override
    /// when `scope = "auto_thread"` and `threadId` is set, matching
    /// Python behaviour).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    /// Whether to wait for the agent reply at all. When `false`, the
    /// handler exits after the directed message and emits an empty `{}` to the
    /// listener.
    pub reply: bool,
    /// Which §7.4 reply path to take when `reply = true`.
    pub reply_mode: ReplyMode,
    /// Wait timeout per agent reply (seconds). Default 120.
    pub reply_timeout_secs: u64,
    /// On first run, attempt to invite the service actor + target
    /// agent into `channelId`. Best-effort: failures are logged, not
    /// fatal.
    pub auto_invite: bool,
    /// Flatten markdown into single-line plain text before sending via
    /// `am`. Default true (DingTalk renders best as plain).
    pub send_plain_text: bool,
    /// Max chars in flattened reply (truncated with ellipsis).
    pub send_max_chars: usize,
    /// `am send` retry attempts.
    pub send_attempts: u32,
    /// Initial retry delay for `am send` (seconds).
    pub send_retry_delay_secs: f64,
    /// Multiplier applied to delay between attempts.
    pub send_retry_backoff: f64,
    /// On group-send failure, fall back to `am chat <sender>`.
    pub send_fallback_chat: bool,
    /// In `Send` / `AsyncSend` modes, propagate send failures back to
    /// the caller (the listener callback) instead of swallowing them.
    pub reply_strict: bool,
    /// Pending callback text in `AsyncSend` mode.
    pub pending_text: String,
    /// Print outbound text instead of invoking `am`. For local testing.
    pub dry_run: bool,
    /// Override the log path for the `AsyncSend` detached child.
    /// Default: `<state_dir>/logs/async-reply.log`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub async_log_path: Option<PathBuf>,
}

impl Default for AmConfig {
    fn default() -> Self {
        Self {
            am_bin: "am".into(),
            am_config_path: None,
            topic: "/v1.0/im/bot/messages/get".into(),
            scope: ScopeMode::default(),
            thread_id: None,
            reply: false,
            reply_mode: ReplyMode::default(),
            reply_timeout_secs: 120,
            auto_invite: false,
            send_plain_text: true,
            send_max_chars: 1800,
            send_attempts: 4,
            send_retry_delay_secs: 1.0,
            send_retry_backoff: 1.8,
            send_fallback_chat: true,
            reply_strict: false,
            pending_text: "Received, processing...".into(),
            dry_run: false,
            async_log_path: None,
        }
    }
}

impl AmConfig {
    /// Project the AM-config knobs that `reply::send_via_am` needs into
    /// its own struct. Keeps the `reply` module independent of the full
    /// AmConfig surface — only knowing about the send path.
    pub fn am_send_config(&self) -> AmSendConfig {
        AmSendConfig {
            am_bin: self.am_bin.clone(),
            am_config_path: self.am_config_path.clone(),
            send_plain_text: self.send_plain_text,
            send_max_chars: self.send_max_chars,
            send_attempts: self.send_attempts,
            send_retry_delay_secs: self.send_retry_delay_secs,
            send_retry_backoff: self.send_retry_backoff,
            send_fallback_chat: self.send_fallback_chat,
            reply_strict: self.reply_strict,
            dry_run: self.dry_run,
        }
    }
}
