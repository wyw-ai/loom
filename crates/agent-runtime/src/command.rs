//! Command transport adapter (per docs/command-transport-v0.md).
//!
//! Unlike [`AcpAdapter`](super::acp::AcpAdapter) — which keeps a single long-lived
//! child process and streams every prompt through one ACP session — this adapter
//! spawns a fresh subprocess for each prompt. State persists across prompts by
//! delegating to the underlying CLI's own session/resume mechanism (`claude
//! --resume <id>`, `codex resume`, etc.); loom only bookkeeps the
//! `(actor_id, scope_id) -> session_id` mapping in
//! `<agent-client-data>/sessions/<actor>/<scope_id>.json`.
//!
//! E2 scope (initial implementation):
//!   * `output_format`: `Text`, `NdjsonLines`, `ClaudeStreamJson`,
//!     `CopilotJson`, `CodexStreamJson`, `OpencodeJson`.
//!   * `prompt_via`: `Args`, `Stdin`, `Env`.
//!   * `decoder.capture.session` for provider-owned session ids. Legacy
//!     `first_run_capture` remains available to direct command transports.
//!   * Session bookkeeping: written to disk, read back on next prompt; signature
//!     mismatch invalidates and forces a first-run path.
//!
//! Per the trait, `Adapter::start` only sets the adapter up — it does NOT spawn
//! anything, because a command-transport agent is not "running" between prompts.
//! All work happens in `send_prompt`, which spawns a child, drains it, and emits
//! `AdapterEvent`s synchronously.

use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::TokenUsage;

use proto::ansi::strip_ansi;

use loom_platform::process::Command;

use async_trait::async_trait;
use parking_lot::Mutex;
use proto::methods::{
    CommandOutputFormat, CommandSessionIdSource, PromptVia, ProviderArgSpec,
    ProviderDecoderEmitSpec, ProviderDecoderSpec, ProviderJsonConditionSpec,
    ProviderJsonlTextReducerSpec, ProviderPromptSpec,
};
use proto::types::ScopeRef;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;

use super::adapter::{Adapter, AdapterEvent, AdapterPrompt, AdapterStartInfo};
use crate::acp::create_dir_all_unc;
use crate::provider::ProviderRuntimeEvent;
use crate::usage::{extract_token_usage_from_text, observe_usage_line};

#[cfg(unix)]
use std::os::unix::io::AsRawFd;

enum ProcessOutput {
    Stdout(String),
    Stderr(String),
}

/// Per-scope handle to an in-flight subprocess. The PID is set after spawn
/// and cleared on wait; `cancel_requested` is flipped on by `cancel()` so
/// the post-wait flow can label the Finished summary as "cancelled" rather
/// than the raw exit code.
#[derive(Default)]
struct InFlight {
    pid: Option<u32>,
    cancel_requested: bool,
    running: bool,
}

struct RunSlotGuard {
    slot: Arc<Mutex<InFlight>>,
}

impl RunSlotGuard {
    fn acquire(slot: Arc<Mutex<InFlight>>, scope: &ScopeRef) -> Result<Self, String> {
        let mut state = slot.lock();
        if state.running {
            return Err(format!(
                "command transport session already in flight for {}",
                scope_label(scope)
            ));
        }
        state.running = true;
        state.pid = None;
        state.cancel_requested = false;
        drop(state);
        Ok(Self { slot })
    }
}

impl Drop for RunSlotGuard {
    fn drop(&mut self) {
        let mut state = self.slot.lock();
        state.pid = None;
        state.cancel_requested = false;
        state.running = false;
    }
}

/// Snapshot of the bits of `AgentTransport` the command adapter cares about.
/// Templates are expanded per prompt because scope and channel paths are
/// dispatch-specific.
#[derive(Debug, Clone)]
pub struct CommandConfig {
    pub actor_id: String,
    pub command: String,
    /// First-run argv (template — `{prompt}` may appear when `prompt_via=args`).
    pub args: Vec<String>,
    /// Provider-manifest argv template preserving conditional fragments.
    pub arg_specs: Vec<ProviderArgSpec>,
    pub env: BTreeMap<String, String>,
    /// Argv template appended when the prompt selects a model. `{model}` is
    /// expanded only after a non-empty selected model exists.
    pub model_args: Vec<String>,
    pub session_id_source: Option<CommandSessionIdSource>,
    pub session_scope: Option<String>,
    pub first_run_capture: Option<String>,
    pub resume_args: Option<Vec<String>>,
    pub resume_arg_specs: Vec<ProviderArgSpec>,
    pub output_format: CommandOutputFormat,
    pub decoder: Option<ProviderDecoderSpec>,
    pub stderr_decoder: Option<ProviderDecoderSpec>,
    pub prompt_via: PromptVia,
    pub prompt: Option<ProviderPromptSpec>,
    pub stdin_template: Option<String>,
    /// Where to keep `<actor>/<scope_id>.json` session bookkeeping files. The
    /// adapter creates subdirs lazily on first write.
    pub sessions_dir: PathBuf,
    /// Optional per-turn wall-clock timeout in milliseconds.
    pub timeout_ms: Option<u64>,
    /// Optional per-turn stdout idle timeout in milliseconds.
    pub idle_timeout_ms: Option<u64>,
    /// Hash of provider command/session templates before per-turn runtime
    /// values are expanded. When the spec changes, saved sessions are
    /// invalidated.
    pub command_signature: String,
}

impl CommandConfig {
    pub fn from_transport(
        actor_id: String,
        command: String,
        args: Vec<String>,
        env: BTreeMap<String, String>,
        spec: &proto::methods::AgentTransport,
        sessions_dir: PathBuf,
    ) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(command.as_bytes());
        if spec.arg_specs.is_empty() {
            for a in &args {
                hasher.update(b"\x00arg\x00");
                hasher.update(a.as_bytes());
            }
        } else {
            hasher.update(b"\x00arg_specs\x00");
            if let Ok(json) = serde_json::to_vec(&spec.arg_specs) {
                hasher.update(json);
            }
        }
        for a in &spec.model_args {
            hasher.update(b"\x00model_arg\x00");
            hasher.update(a.as_bytes());
        }
        let session = spec.session.clone();
        if let Some(session) = session.as_ref() {
            let scope = session
                .scope
                .as_deref()
                .map(str::trim)
                .filter(|scope| !scope.is_empty())
                .unwrap_or("actor_scope");
            if scope != "actor_scope" {
                hasher.update(b"\x00session_scope\x00");
                hasher.update(scope.as_bytes());
            }
            if session.resume_arg_specs.is_empty() {
                if let Some(args) = session.resume_args.as_ref() {
                    for a in args {
                        hasher.update(b"\x00resume_arg\x00");
                        hasher.update(a.as_bytes());
                    }
                }
            } else {
                hasher.update(b"\x00resume_arg_specs\x00");
                if let Ok(json) = serde_json::to_vec(&session.resume_arg_specs) {
                    hasher.update(json);
                }
            }
        }
        let command_signature = format!("sha256:{}", hex::encode(hasher.finalize()));
        Self {
            actor_id,
            command,
            args,
            arg_specs: spec.arg_specs.clone(),
            env,
            model_args: spec.model_args.clone(),
            session_id_source: session.as_ref().and_then(|s| s.id_source),
            session_scope: session.as_ref().and_then(|s| s.scope.clone()),
            first_run_capture: session.as_ref().and_then(|s| s.first_run_capture.clone()),
            resume_args: session.as_ref().and_then(|s| s.resume_args.clone()),
            resume_arg_specs: session
                .as_ref()
                .map(|s| s.resume_arg_specs.clone())
                .unwrap_or_default(),
            output_format: spec.output_format.unwrap_or_default(),
            decoder: spec.decoder.clone(),
            stderr_decoder: spec.stderr_decoder.clone(),
            prompt_via: spec.prompt_via,
            prompt: spec.prompt.clone(),
            stdin_template: spec.stdin.clone(),
            sessions_dir,
            timeout_ms: spec.timeout_ms,
            idle_timeout_ms: spec.idle_timeout_ms,
            command_signature,
        }
    }
}

pub struct CommandAdapter {
    cfg: CommandConfig,
    inner: Mutex<CommandInner>,
}

struct CommandInner {
    event_sender: Option<mpsc::UnboundedSender<AdapterEvent>>,
    /// scope.id → in-flight subprocess slot. Lives across the spawn_blocking
    /// boundary so `cancel()` (called from the async runtime) can read the
    /// PID and signal it.
    in_flight: HashMap<String, Arc<Mutex<InFlight>>>,
}

impl CommandAdapter {
    pub fn new(cfg: CommandConfig) -> Self {
        Self {
            cfg,
            inner: Mutex::new(CommandInner {
                event_sender: None,
                in_flight: HashMap::new(),
            }),
        }
    }

    fn sender(&self) -> Result<mpsc::UnboundedSender<AdapterEvent>, String> {
        self.inner
            .lock()
            .event_sender
            .clone()
            .ok_or_else(|| "command adapter not started".to_string())
    }

    /// Get or create the per-scope in-flight slot. Reused across resume calls
    /// for the same scope so we don't leak entries.
    fn slot_for(&self, scope_id: &str) -> Arc<Mutex<InFlight>> {
        let mut inner = self.inner.lock();
        inner
            .in_flight
            .entry(scope_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(InFlight::default())))
            .clone()
    }
}

#[async_trait]
impl Adapter for CommandAdapter {
    /// "Start" for command transport just stashes the event sender — there is
    /// no long-lived child to spawn yet. The first `send_prompt` call does the
    /// actual work.
    async fn start(
        &self,
        events: mpsc::UnboundedSender<AdapterEvent>,
    ) -> Result<AdapterStartInfo, String> {
        self.inner.lock().event_sender = Some(events);
        Ok(AdapterStartInfo {
            pid: None,
            session_id: Some(format!("cmd:{}", self.cfg.actor_id)),
        })
    }

    async fn send_prompt(&self, prompt: AdapterPrompt) -> Result<(), String> {
        if prompt.content.is_empty() {
            let _ = self.sender()?.send(AdapterEvent::Error {
                scope: Some(prompt.scope.clone()),
                message: "empty prompt".into(),
            });
            let _ = self.sender()?.send(AdapterEvent::Finished {
                scope: Some(prompt.scope),
                success: false,
                summary: "empty prompt".into(),
                usage: None,
            });
            return Ok(());
        }
        let sender = self.sender()?;
        let cfg = self.cfg.clone();
        let slot = self.slot_for(&prompt.scope.id);
        // Detach the command turn so the actor worker can keep processing
        // other scopes while a long-running provider command is active. The
        // blocking worker reports completion through AdapterEvent::Finished.
        tokio::task::spawn_blocking(move || {
            if let Err(e) = run_prompt(cfg, prompt, sender, slot) {
                tracing::debug!(error = %e, "command run_prompt returned err (already reported via AdapterEvent)");
            }
        });
        Ok(())
    }

    async fn respond_action(&self, _request_id: String, _option_id: String) -> Result<(), String> {
        // Command transport does not surface permission prompts (no reverse
        // channel from the one-shot subprocess back into Loom). Anyone calling
        // this for a command adapter has a bug elsewhere; report it loudly.
        Err("command transport does not support action requests".into())
    }

    async fn cancel(&self, scope: ScopeRef) -> Result<(), String> {
        let slot = {
            let inner = self.inner.lock();
            inner.in_flight.get(&scope.id).cloned()
        };
        let Some(slot) = slot else {
            // No prompt has ever run for this scope — nothing to cancel.
            return Ok(());
        };
        let pid = {
            let mut s = slot.lock();
            s.cancel_requested = true;
            s.pid
        };
        let Some(pid) = pid else {
            // Slot exists but no PID — either we're between prompts, or the
            // child hasn't been spawned yet. Setting cancel_requested is
            // enough; if a spawn is in flight it'll be killed once it lands
            // (handled by send_prompt's spawn_and_collect on the next
            // wait iteration only if we extend that — for now, no-op).
            return Ok(());
        };
        signal_child(pid)
    }

    async fn stop(&self) -> Result<(), String> {
        // Nothing to stop — each prompt's subprocess exits on its own. Drop the
        // sender so the runtime's event consumer can close cleanly.
        self.inner.lock().event_sender = None;
        self.inner.lock().in_flight.clear();
        Ok(())
    }
}

/// Request graceful termination of `pid`. Cross-platform via
/// `loom_platform::signal` (`SIGTERM` on Unix, `TerminateProcess` on
/// Windows). Children spawned through `loom_platform::process::Command` are
/// their own process-group leader (`process_group(0)` is applied
/// automatically), so the leader PID is the correct signal target on both
/// platforms.
fn signal_child(pid: u32) -> Result<(), String> {
    loom_platform::signal::signal_child(pid, loom_platform::signal::Signal::Term)
        .map_err(|e| format!("signal_child({pid}, SIGTERM) failed: {e}"))
}

/// Force-kill `pid`. Cross-platform via `loom_platform::signal`
/// (`SIGKILL` on Unix, `TerminateProcess` on Windows). Previously the
/// Windows arm was an unconditional `Err` no-op, which left timed-out /
/// cancelled agent subprocesses (and their grandchildren) running.
fn force_kill_child(pid: u32) -> Result<(), String> {
    loom_platform::signal::force_kill_pid(pid)
        .map_err(|e| format!("force_kill({pid}) failed: {e}"))
}

fn run_prompt(
    cfg: CommandConfig,
    prompt: AdapterPrompt,
    sender: mpsc::UnboundedSender<AdapterEvent>,
    slot: Arc<Mutex<InFlight>>,
) -> Result<(), String> {
    let scope = prompt.scope.clone();
    let _run_slot = match RunSlotGuard::acquire(slot.clone(), &scope) {
        Ok(guard) => guard,
        Err(e) => {
            let _ = sender.send(AdapterEvent::Error {
                scope: Some(scope.clone()),
                message: e.clone(),
            });
            let _ = sender.send(AdapterEvent::Finished {
                scope: Some(scope.clone()),
                success: false,
                summary: e.clone(),
                usage: None,
            });
            return Ok(());
        }
    };
    let _session_lock = match acquire_session_lock(&cfg, &scope) {
        Ok(lock) => lock,
        Err(e) => {
            let message = format!("command adapter session lock error: {e}");
            let _ = sender.send(AdapterEvent::Error {
                scope: Some(scope.clone()),
                message: message.clone(),
            });
            let _ = sender.send(AdapterEvent::Finished {
                scope: Some(scope.clone()),
                success: false,
                summary: message.clone(),
                usage: None,
            });
            return Err(message);
        }
    };
    let content = prompt.content.clone();
    let command_signature = command_signature_for_prompt(&cfg, &prompt);
    let session = load_session(&cfg, &scope);
    let resume_session_id = session
        .as_ref()
        .filter(|s| s.command_signature == command_signature)
        .map(|s| s.session_id.clone());

    // First-run vs resume: if a usable session is on disk AND the spec supports
    // resume, build the argv from `resume_args`; otherwise build the first-run
    // argv from `args`.
    let first_run_session_id = if resume_session_id.is_none()
        && matches!(
            cfg.session_id_source,
            Some(CommandSessionIdSource::LoomUuid)
        ) {
        Some(uuid::Uuid::new_v4().to_string())
    } else {
        None
    };

    let (argv, is_first_run) = match resume_session_id.as_deref() {
        Some(sid) if !cfg.resume_arg_specs.is_empty() => (
            expand_arg_specs(&cfg.resume_arg_specs, &cfg, &prompt, Some(sid), &content),
            false,
        ),
        Some(sid) if cfg.resume_args.is_some() => (
            expand_argv(
                cfg.resume_args.as_deref().unwrap_or_default(),
                &cfg,
                &prompt,
                Some(sid),
                &content,
            ),
            false,
        ),
        _ => (
            expand_first_run_argv(&cfg, &prompt, first_run_session_id.as_deref(), &content),
            true,
        ),
    };

    let active_session_id = resume_session_id
        .as_deref()
        .or(first_run_session_id.as_deref());
    let result = spawn_and_collect(&cfg, &prompt, &argv, active_session_id, &sender, &slot);
    let mut outcome = match result {
        Ok(o) => o,
        Err(e) => {
            let _ = sender.send(AdapterEvent::Error {
                scope: Some(scope.clone()),
                message: format!("command adapter spawn error: {e}"),
            });
            let _ = sender.send(AdapterEvent::Finished {
                scope: Some(scope.clone()),
                success: false,
                summary: e.clone(),
                usage: None,
            });
            return Err(e);
        }
    };

    let mut retried_as_first_run = false;
    if !is_first_run && looks_like_session_lost(&outcome.stderr, &outcome.stdout) {
        let _ = delete_session(&cfg, &scope);
        tracing::info!(actor = %cfg.actor_id, scope = %scope.id,
            "command transport: dropped stale session and retrying first-run prompt");
        let retry_session_id = if matches!(
            cfg.session_id_source,
            Some(CommandSessionIdSource::LoomUuid)
        ) {
            Some(uuid::Uuid::new_v4().to_string())
        } else {
            None
        };
        let first_run_argv =
            expand_first_run_argv(&cfg, &prompt, retry_session_id.as_deref(), &content);
        outcome = match spawn_and_collect(
            &cfg,
            &prompt,
            &first_run_argv,
            retry_session_id.as_deref(),
            &sender,
            &slot,
        ) {
            Ok(o) => o,
            Err(e) => {
                let _ = sender.send(AdapterEvent::Error {
                    scope: Some(scope.clone()),
                    message: format!("command adapter retry spawn error: {e}"),
                });
                let _ = sender.send(AdapterEvent::Finished {
                    scope: Some(scope.clone()),
                    success: false,
                    summary: e.clone(),
                    usage: None,
                });
                return Err(e);
            }
        };
        retried_as_first_run = true;
        if outcome.exit_code == 0 {
            if let Some(sid) = retry_session_id.as_deref() {
                if let Err(e) = save_session(&cfg, &scope, sid, &command_signature) {
                    tracing::warn!(actor = %cfg.actor_id, %e, "failed to save generated command session");
                }
            }
        }
    }

    // First-run capture: try once, save to disk on success.
    if (is_first_run || retried_as_first_run) && outcome.exit_code == 0 {
        if let Some(sid) = first_run_session_id.as_deref() {
            if let Err(e) = save_session(&cfg, &scope, sid, &command_signature) {
                tracing::warn!(actor = %cfg.actor_id, %e, "failed to save generated command session");
            }
        } else if let Some(sid) = capture_configured_decoder_session_event(&cfg, &outcome)
            .and_then(ProviderRuntimeEvent::into_session_id)
        {
            if let Err(e) = save_session(&cfg, &scope, &sid, &command_signature) {
                tracing::warn!(actor = %cfg.actor_id, %e, "failed to save decoder-captured command session");
            }
        } else if let Some(rule) = cfg.first_run_capture.as_ref() {
            match capture_session_id(rule, &outcome, &cfg, &prompt) {
                Ok(Some(sid)) => {
                    if let Err(e) = save_session(&cfg, &scope, &sid, &command_signature) {
                        tracing::warn!(actor = %cfg.actor_id, %e, "failed to save command session");
                    }
                }
                Ok(None) => {
                    let stdout = truncate_for_summary(&outcome.stdout);
                    let stderr = truncate_for_summary(&outcome.stderr);
                    tracing::warn!(actor = %cfg.actor_id, rule = %rule,
                        stdout = %stdout, stderr = %stderr,
                        "command transport: first_run_capture matched no session id");
                }
                Err(e) => {
                    tracing::warn!(actor = %cfg.actor_id, %e,
                        "command transport: first_run_capture failed");
                }
            }
        }
    } else if outcome.exit_code != 0 && looks_like_session_lost(&outcome.stderr, &outcome.stdout) {
        // Resume failed in a way that suggests the underlying session is gone.
        // Drop the bookkeeping so the next call retries as a first run. We do
        // NOT auto-retry inside this call — the user's prompt has already been
        // reported as failed; re-running it silently could double-charge LLM
        // calls.
        let _ = delete_session(&cfg, &scope);
        tracing::info!(actor = %cfg.actor_id, scope = %scope.id,
            "command transport: dropped stale session after resume failure");
    } else if !is_first_run
        && !retried_as_first_run
        && outcome.exit_code != 0
        && looks_like_signed_thinking_replay_error(&outcome.stderr, &outcome.stdout)
    {
        // Claude Code can occasionally persist a transcript tail whose signed
        // thinking blocks cannot be replayed into Anthropic's next request. We
        // do not edit Claude's JSONL; we only stop reusing this session id.
        let _ = delete_session(&cfg, &scope);
        tracing::warn!(actor = %cfg.actor_id, scope = %scope.id,
            "command transport: dropped Claude session after signed-thinking replay error");
    } else if !is_first_run && !retried_as_first_run && outcome.exit_code == 0 {
        if let Some(sid) = resume_session_id.as_deref() {
            if let Err(e) = save_session(&cfg, &scope, sid, &command_signature) {
                tracing::warn!(actor = %cfg.actor_id, %e, "failed to update command session");
            }
        }
    }

    Ok(())
}

#[derive(Debug)]
struct SpawnOutcome {
    exit_code: i32,
    stdout: String,
    stderr: String,
}

/// Observes provider subprocess output and emits periodic health-status
/// `AdapterEvent::StatusChange` events.  Pure observation — never kills.
struct HealthObserver {
    scope: ScopeRef,
    last_activity: Instant,
    last_health_report: Instant,
    /// Sliding window of recent line hashes for repetition detection.
    recent_hashes: std::collections::VecDeque<u64>,
    /// How many times the same hash can appear in the window before a
    /// repetition warning fires.
    repetition_threshold: usize,
    /// Maximum recent-window size.
    window_size: usize,
    /// How long without activity before emitting `"health: idle [N s]"`.
    idle_warning: Duration,
    /// Set to Some when a repetition warning has already fired for the current
    /// dominant hash (avoids spam).
    repetition_fired_for: Option<u64>,
    /// Minimum line length for repetition detection (ignore blank/very short lines).
    min_line_len: usize,
}

impl HealthObserver {
    fn new(scope: ScopeRef) -> Self {
        Self {
            scope,
            last_activity: Instant::now(),
            last_health_report: Instant::now(),
            recent_hashes: std::collections::VecDeque::with_capacity(128),
            repetition_threshold: 20,
            window_size: 100,
            idle_warning: Duration::from_secs(60),
            repetition_fired_for: None,
            min_line_len: 4,
        }
    }

    /// Call after each stdout line is processed.  If the line produced
    /// meaningful output (text/finish), reset the idle timer.
    fn observe_stdout(&mut self, raw_line: &str, had_output: bool) {
        if had_output {
            self.last_activity = Instant::now();
        }
        let trimmed = raw_line.trim();
        if trimmed.len() >= self.min_line_len {
            let hash = hash_line(trimmed);
            if self.recent_hashes.len() >= self.window_size {
                self.recent_hashes.pop_front();
            }
            self.recent_hashes.push_back(hash);
        }
    }

    /// Call after each stderr line.
    fn observe_stderr(&mut self, _raw_line: &str) {
        // Stderr is informational; we track it but it doesn't affect liveness
        // directly.  Specific error-flood detection can be added here later.
    }

    /// Call once per read-loop iteration.  Emits `StatusChange` when the
    /// health picture changes meaningfully, and persists health events to
    /// the tracing log for audit and diagnostics.
    fn tick(&mut self, sender: &mpsc::UnboundedSender<AdapterEvent>) {
        let now = Instant::now();
        let idle_secs = now.duration_since(self.last_activity).as_secs();

        // 1. Repetition detection (check every tick)
        if let Some(repeat_msg) = self.check_repetition() {
            let status = format!("health: warning [{}]", repeat_msg);
            tracing::warn!(
                scope = %self.scope.id,
                "health: warning [{}]", repeat_msg
            );
            let _ = sender.send(AdapterEvent::StatusChange {
                scope: Some(self.scope.clone()),
                status,
            });
            self.last_health_report = now;
            return;
        }

        // 2. Idle escalation — only report when crossing the idle threshold
        // for the first time, then periodically.
        if idle_secs >= 300 {
            if now.duration_since(self.last_health_report) >= Duration::from_secs(120) {
                tracing::info!(
                    scope = %self.scope.id,
                    idle_secs,
                    "health: idle [{idle_secs}s]"
                );
                let _ = sender.send(AdapterEvent::StatusChange {
                    scope: Some(self.scope.clone()),
                    status: format!("health: idle [{idle_secs}s]"),
                });
                self.last_health_report = now;
            }
            return;
        }
        if idle_secs >= self.idle_warning.as_secs()
            && now.duration_since(self.last_health_report) >= Duration::from_secs(60)
        {
            tracing::info!(
                scope = %self.scope.id,
                idle_secs,
                "health: idle [{idle_secs}s]"
            );
            let _ = sender.send(AdapterEvent::StatusChange {
                scope: Some(self.scope.clone()),
                status: format!("health: idle [{idle_secs}s]"),
            });
            self.last_health_report = now;
            return;
        }

        // 3. Periodic active reaffirmation (every ~30s when active)
        if idle_secs == 0 && now.duration_since(self.last_health_report) >= Duration::from_secs(30)
        {
            tracing::info!(
                scope = %self.scope.id,
                "health: active"
            );
            let _ = sender.send(AdapterEvent::StatusChange {
                scope: Some(self.scope.clone()),
                status: "health: active".to_string(),
            });
            self.last_health_report = now;
        }
    }

    fn check_repetition(&mut self) -> Option<String> {
        if self.recent_hashes.len() < self.repetition_threshold {
            return None;
        }
        let mut counts: std::collections::HashMap<u64, u32> = std::collections::HashMap::new();
        for &h in &self.recent_hashes {
            *counts.entry(h).or_insert(0) += 1;
        }
        let (dominant, count) = counts.into_iter().max_by_key(|(_, c)| *c).unwrap_or((0, 0));
        if count as usize >= self.repetition_threshold {
            let already_fired = self.repetition_fired_for == Some(dominant);
            if !already_fired {
                self.repetition_fired_for = Some(dominant);
                return Some(format!(
                    "repeating output x{count} in last {} lines",
                    self.recent_hashes.len()
                ));
            }
        } else {
            self.repetition_fired_for = None;
        }
        None
    }
}

/// Fast non-crypto hash for output-line deduplication.
fn hash_line(line: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    line.hash(&mut h);
    h.finish()
}

fn spawn_and_collect(
    cfg: &CommandConfig,
    prompt: &AdapterPrompt,
    argv: &[String],
    session_id: Option<&str>,
    sender: &mpsc::UnboundedSender<AdapterEvent>,
    slot: &Arc<Mutex<InFlight>>,
) -> Result<SpawnOutcome, String> {
    crate::acp::create_dir_all_unc(&prompt.cwd).map_err(|e| {
        format!(
            "failed to create command cwd `{}`: {}",
            prompt.cwd.display(),
            e
        )
    })?;
    // On Windows, prefix the cwd with UNC prefix to bypass MAX_PATH (260
    // char) limit (Bug #2 Phase 2).  Do NOT UNC-prefix the command path —
    // `\\?\` bypasses PATHEXT resolution in CreateProcessW, so e.g.
    // `\\?\D:\nodejs\copilot` would fail to resolve to `copilot.cmd`.
    // `unc_prefix_path` is a no-op on Unix so the call site stays cfg-free.
    let command_path = PathBuf::from(&cfg.command);
    let spawn_cwd = crate::acp::unc_prefix_path(prompt.cwd.clone());

    // On Windows, `cmd.exe` treats newlines as command separators when
    // wrapping `.CMD`/`.BAT` file invocations via `cmd.exe /c`. Arguments
    // containing newlines (e.g. multi-line prompt content) cause "batch file
    // arguments are invalid" (CreateProcessW error 0xC1). Replace newlines
    // with spaces to prevent this.
    let mut sanitized_argv: Vec<String>;
    let final_argv: &[String] = if cfg!(windows) && is_batch_file(&command_path) {
        sanitized_argv = argv.to_vec();
        sanitize_batch_args(&mut sanitized_argv);
        &sanitized_argv
    } else {
        argv
    };

    let mut cmd = Command::new(&command_path);
    let stdin = if cfg.stdin_template.is_some() || matches!(cfg.prompt_via, PromptVia::Stdin) {
        Stdio::piped()
    } else {
        Stdio::null()
    };
    cmd.args(final_argv)
        .current_dir(&spawn_cwd)
        .stdin(stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in expanded_env(cfg, prompt, session_id) {
        cmd.env(k, v);
    }
    if matches!(cfg.prompt_via, PromptVia::Env) {
        cmd.env("LOOM_PROMPT", &prompt.content);
    }
    // Windows CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB |
    // CREATE_NEW_PROCESS_GROUP and Unix process_group(0) are applied by
    // `loom_platform::process::Command::new` automatically — no cfg block here.
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to spawn `{}`: {}", command_path.display(), e))?;
    // Register the PID so cancel() can find and signal it. If a cancel call
    // landed BEFORE we got here (cancel_requested already true), kill the
    // child immediately; the wait below will pick up the SIGTERM exit.
    {
        let mut s = slot.lock();
        s.pid = Some(child.id());
        if s.cancel_requested {
            let pid = child.id();
            drop(s);
            let _ = signal_child(pid);
        }
    }

    // Take stdout/stderr handles FIRST and start reader threads BEFORE writing
    // stdin.  This prevents a deadlock where the parent blocks on stdin write
    // (pipe buffer full because the child hasn't started reading yet) while the
    // child blocks on stdout/stderr write (pipe buffer full because nobody is
    // draining).  With readers already draining, the child can complete its
    // startup output, reach the point where it reads stdin, and unblock the
    // parent's write.
    let stdout = child.stdout.take().ok_or("failed to open child stdout")?;
    let stderr = child.stderr.take().ok_or("failed to open child stderr")?;

    // Stream child output on helper threads so the foreground loop can enforce
    // timeouts and surface provider errors even if the process never exits.
    let (output_tx, output_rx) = std::sync::mpsc::channel::<ProcessOutput>();
    let stdout_tx = output_tx.clone();
    let stdout_handle = std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    let _ = stdout_tx.send(ProcessOutput::Stdout(line.clone()));
                }
                Err(_) => break,
            }
        }
    });
    let stderr_tx = output_tx.clone();
    let stderr_handle = std::thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    let _ = stderr_tx.send(ProcessOutput::Stderr(line.clone()));
                }
                Err(_) => break,
            }
        }
    });
    drop(output_tx);

    // Write stdin AFTER reader threads are draining child output, preventing
    // the pipe-buffer deadlock described above.  On Windows, write_all for a
    // large prompt (> pipe buffer) can block; the readers keep the child from
    // deadlocking on full stdout/stderr pipes.
    let needs_stdin = cfg.stdin_template.is_some() || matches!(cfg.prompt_via, PromptVia::Stdin);
    let stdin_handle = if needs_stdin {
        if let Some(mut stdin_writer) = child.stdin.take() {
            let stdin_body = cfg
                .stdin_template
                .as_ref()
                .map(|template| {
                    expand_stdin_template(template, cfg, prompt, session_id, &prompt.content)
                })
                .unwrap_or_else(|| prompt.content.clone());
            // Write stdin on a background thread so a slow write doesn't stall
            // the foreground polling loop.  The reader threads above guarantee
            // the child won't deadlock on its own output pipes.
            let (done_tx, done_rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let result = stdin_writer
                    .write_all(stdin_body.as_bytes())
                    .map_err(|e| format!("failed to write prompt to stdin: {e}"));
                // Drop the writer so the child sees EOF on stdin.
                drop(stdin_writer);
                let _ = done_tx.send(result);
            });
            Some(done_rx)
        } else {
            None
        }
    } else {
        None
    };
    // Drop unused stdin so the child doesn't block on read.
    drop(child.stdin.take());

    let mut collected_stdout = String::new();
    let mut collected_stderr = String::new();
    let mut emitted_text = false;
    let mut emitted_finish = false;
    let mut last_usage: Option<TokenUsage> = None;
    let deadline = cfg
        .timeout_ms
        .filter(|ms| *ms > 0)
        .map(|ms| Instant::now() + Duration::from_millis(ms));
    let idle_timeout = cfg
        .idle_timeout_ms
        .filter(|ms| *ms > 0)
        .map(Duration::from_millis);
    let mut last_output_at = Instant::now();
    let mut exit: Option<ExitStatus> = None;
    let mut timed_out = false;
    let mut idle_timed_out = false;
    let mut early_runtime_error: Option<String> = None;
    let mut health_observer = HealthObserver::new(prompt.scope.clone());

    loop {
        health_observer.tick(sender);
        while let Ok(output) = output_rx.try_recv() {
            last_output_at = Instant::now();
            let (raw_line, is_stdout) = match &output {
                ProcessOutput::Stdout(s) => (s.clone(), true),
                ProcessOutput::Stderr(s) => (s.clone(), false),
            };
            let events = collect_process_output(
                cfg,
                prompt,
                sender,
                output,
                &mut collected_stdout,
                &mut collected_stderr,
                &mut early_runtime_error,
                &mut last_usage,
            );
            emitted_text |= events.emitted_text;
            emitted_finish |= events.emitted_finish;
            if is_stdout {
                health_observer
                    .observe_stdout(&raw_line, events.emitted_text || events.emitted_finish);
            } else {
                health_observer.observe_stderr(&raw_line);
            }
        }
        if early_runtime_error.is_some() {
            if force_kill_child(child.id()).is_err() {
                let _ = child.kill();
            }
            break;
        }

        if let Some(status) = child
            .try_wait()
            .map_err(|e| format!("failed to poll child: {e}"))?
        {
            exit = Some(status);
            break;
        }

        if slot.lock().cancel_requested {
            let _ = signal_child(child.id());
        }

        if let Some(deadline) = deadline {
            if Instant::now() >= deadline {
                timed_out = true;
                if force_kill_child(child.id()).is_err() {
                    let _ = child.kill();
                }
                break;
            }
        }

        if let Some(idle_timeout) = idle_timeout {
            if last_output_at.elapsed() >= idle_timeout {
                idle_timed_out = true;
                if force_kill_child(child.id()).is_err() {
                    let _ = child.kill();
                }
                break;
            }
        }

        let wait_for = deadline
            .map(|deadline| {
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(50))
            })
            .into_iter()
            .chain(idle_timeout.map(|idle_timeout| {
                idle_timeout
                    .saturating_sub(last_output_at.elapsed())
                    .min(Duration::from_millis(50))
            }))
            .min()
            .unwrap_or_else(|| Duration::from_millis(50));
        match output_rx.recv_timeout(wait_for) {
            Ok(output) => {
                last_output_at = Instant::now();
                let (raw_line, is_stdout) = match &output {
                    ProcessOutput::Stdout(s) => (s.clone(), true),
                    ProcessOutput::Stderr(s) => (s.clone(), false),
                };
                let events = collect_process_output(
                    cfg,
                    prompt,
                    sender,
                    output,
                    &mut collected_stdout,
                    &mut collected_stderr,
                    &mut early_runtime_error,
                    &mut last_usage,
                );
                emitted_text |= events.emitted_text;
                emitted_finish |= events.emitted_finish;
                if is_stdout {
                    health_observer
                        .observe_stdout(&raw_line, events.emitted_text || events.emitted_finish);
                } else {
                    health_observer.observe_stderr(&raw_line);
                }
                if early_runtime_error.is_some() {
                    if force_kill_child(child.id()).is_err() {
                        let _ = child.kill();
                    }
                    break;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }

    let exit = match exit {
        Some(status) => status,
        None => child
            .wait()
            .map_err(|e| format!("failed to wait on child: {e}"))?,
    };
    let _ = stdout_handle.join();
    let _ = stderr_handle.join();
    // Check the background stdin write result.  If it failed, surface the error
    // as an early_runtime_error so the caller gets a clear message instead of
    // a confusing "No prompt provided" from the child.
    if let Some(done_rx) = stdin_handle {
        if let Ok(Err(e)) = done_rx.recv() {
            early_runtime_error = Some(e);
        }
    }
    for output in output_rx.try_iter() {
        let (raw_line, is_stdout) = match &output {
            ProcessOutput::Stdout(s) => (s.clone(), true),
            ProcessOutput::Stderr(s) => (s.clone(), false),
        };
        let events = collect_process_output(
            cfg,
            prompt,
            sender,
            output,
            &mut collected_stdout,
            &mut collected_stderr,
            &mut early_runtime_error,
            &mut last_usage,
        );
        emitted_text |= events.emitted_text;
        emitted_finish |= events.emitted_finish;
        if is_stdout {
            health_observer.observe_stdout(&raw_line, events.emitted_text || events.emitted_finish);
        } else {
            health_observer.observe_stderr(&raw_line);
        }
    }
    let exit_code = exit.code().unwrap_or(-1);
    // Snapshot + clear the cancel flag now that the child is reaped, before
    // building the Finished summary below.
    let was_cancelled = {
        let mut s = slot.lock();
        s.pid = None;
        let req = s.cancel_requested;
        s.cancel_requested = false;
        req
    };

    // Emit the closing events appropriate to the chosen format. Stream formats
    // already pushed partial Text frames inline; we only need the final flush
    // + Finished here. The Text format never pushed anything, so we emit the
    // whole stdout as a single Text and then Finished.
    let success = exit_code == 0 && !was_cancelled && !timed_out && !idle_timed_out;
    let runtime_error = if success {
        None
    } else {
        early_runtime_error
            .or_else(|| extract_runtime_error_from_text(&collected_stdout))
            .or_else(|| extract_runtime_error_from_text(&collected_stderr))
    };
    let summary = if was_cancelled {
        "cancelled".into()
    } else if timed_out {
        match cfg.timeout_ms {
            Some(ms) => format!("timed out after {ms}ms"),
            None => "timed out".into(),
        }
    } else if idle_timed_out {
        match cfg.idle_timeout_ms {
            Some(ms) => format!("idle timed out after {ms}ms"),
            None => "idle timed out".into(),
        }
    } else if success {
        String::new()
    } else if let Some(message) = runtime_error {
        message
    } else if !collected_stderr.is_empty() {
        truncate_for_summary(&collected_stderr)
    } else {
        format!("exited with code {exit_code}")
    };

    if let Some(event) =
        configured_decoder_final_text_event(cfg, &collected_stdout, &collected_stderr)
    {
        let _ = emit_provider_runtime_event(event, &prompt.scope, sender);
    } else {
        match cfg.output_format {
            CommandOutputFormat::Text => {
                if !collected_stdout.is_empty() {
                    let _ = sender.send(AdapterEvent::Text {
                        scope: Some(prompt.scope.clone()),
                        content: collected_stdout.clone(),
                        is_partial: false,
                    });
                }
            }
            CommandOutputFormat::CopilotJson => {
                if let Some(content) = extract_copilot_json_final_text(&collected_stdout) {
                    let _ = sender.send(AdapterEvent::Text {
                        scope: Some(prompt.scope.clone()),
                        content,
                        is_partial: false,
                    });
                }
            }
            CommandOutputFormat::OpencodeJson => {
                if let Some(content) = extract_opencode_json_final_text(&collected_stdout) {
                    let _ = sender.send(AdapterEvent::Text {
                        scope: Some(prompt.scope.clone()),
                        content,
                        is_partial: false,
                    });
                }
            }
            CommandOutputFormat::CodexStreamJson if !emitted_text => {
                if let Some(content) = extract_codex_json_final_text(&collected_stdout) {
                    let _ = sender.send(AdapterEvent::Text {
                        scope: Some(prompt.scope.clone()),
                        content,
                        is_partial: false,
                    });
                }
            }
            CommandOutputFormat::CodexStreamJson => {}
            _ => {
                // Force a buffer flush downstream by emitting an empty
                // is_partial=false Text frame; the runtime's `take_text_buffer`
                // will turn whatever was accumulated into a single content.add.
                let _ = sender.send(AdapterEvent::Text {
                    scope: Some(prompt.scope.clone()),
                    content: String::new(),
                    is_partial: false,
                });
            }
        }
    }
    let usage = extract_token_usage_from_text(&collected_stdout)
        .or_else(|| extract_token_usage_from_text(&collected_stderr));
    if !emitted_finish || !success {
        let _ = sender.send(AdapterEvent::Finished {
            scope: Some(prompt.scope.clone()),
            success,
            summary,
            usage,
        });
    }

    Ok(SpawnOutcome {
        exit_code,
        stdout: collected_stdout,
        stderr: collected_stderr,
    })
}

/// Check if the command path is a Windows batch file (.CMD or .BAT).
/// Windows wraps `.CMD`/`.BAT` invocations with `cmd.exe /c`, which
/// treats newlines as command separators.
fn is_batch_file(path: &Path) -> bool {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let lower = ext.to_ascii_lowercase();
        lower == "cmd" || lower == "bat"
    } else {
        false
    }
}

/// Replace newlines in arguments with spaces to prevent `cmd.exe` from
/// treating them as command separators when wrapping `.CMD`/`.BAT`
/// invocations with `cmd.exe /c`. Without this, multi-line prompt
/// content triggers "batch file arguments are invalid" (error 0xC1).
fn sanitize_batch_args(args: &mut [String]) {
    for arg in args.iter_mut() {
        if arg.contains('\n') || arg.contains('\r') {
            *arg = arg.replace('\r', " ").replace('\n', " ");
        }
    }
}

fn collect_stdout_line(
    cfg: &CommandConfig,
    prompt: &AdapterPrompt,
    sender: &mpsc::UnboundedSender<AdapterEvent>,
    line: &str,
    collected_stdout: &mut String,
    last_emitted_usage: &mut Option<TokenUsage>,
) -> OutputLineEvents {
    let clean = strip_ansi(line);
    collected_stdout.push_str(&clean);
    let parsed_line = clean.trim_end_matches(&['\r', '\n'][..]);
    if let Some(usage) = observe_usage_line(parsed_line, last_emitted_usage) {
        let _ = sender.send(AdapterEvent::UsageUpdate {
            scope: Some(prompt.scope.clone()),
            usage,
        });
    }
    if let Some(decoder) = cfg.decoder.as_ref() {
        return collect_provider_decoder_line(decoder, parsed_line, &prompt.scope, sender);
    }
    collect_legacy_output_line(cfg.output_format, parsed_line, &prompt.scope, sender)
}

fn collect_process_output(
    cfg: &CommandConfig,
    prompt: &AdapterPrompt,
    sender: &mpsc::UnboundedSender<AdapterEvent>,
    output: ProcessOutput,
    collected_stdout: &mut String,
    collected_stderr: &mut String,
    early_runtime_error: &mut Option<String>,
    last_emitted_usage: &mut Option<TokenUsage>,
) -> OutputLineEvents {
    match output {
        ProcessOutput::Stdout(line) => collect_stdout_line(
            cfg,
            prompt,
            sender,
            &line,
            collected_stdout,
            last_emitted_usage,
        ),
        ProcessOutput::Stderr(line) => {
            collected_stderr.push_str(&line);
            if early_runtime_error.is_none() {
                *early_runtime_error = extract_runtime_error_from_text(&line);
            }
            let parsed_line = line.trim_end_matches(&['\r', '\n'][..]);
            cfg.stderr_decoder
                .as_ref()
                .map(|decoder| {
                    collect_provider_decoder_line(decoder, parsed_line, &prompt.scope, sender)
                })
                .unwrap_or_default()
        }
    }
}

fn collect_legacy_output_line(
    output_format: CommandOutputFormat,
    parsed_line: &str,
    scope: &ScopeRef,
    sender: &mpsc::UnboundedSender<AdapterEvent>,
) -> OutputLineEvents {
    match output_format {
        CommandOutputFormat::NdjsonLines => OutputLineEvents {
            emitted_text: translate_ndjson_line(parsed_line, scope, sender),
            emitted_finish: false,
        },
        CommandOutputFormat::ClaudeStreamJson => OutputLineEvents {
            emitted_text: translate_claude_stream_line(parsed_line, scope, sender),
            emitted_finish: false,
        },
        CommandOutputFormat::CodexStreamJson => OutputLineEvents {
            emitted_text: translate_codex_event_line(parsed_line, scope, sender),
            emitted_finish: false,
        },
        CommandOutputFormat::Text
        | CommandOutputFormat::CopilotJson
        | CommandOutputFormat::OpencodeJson => OutputLineEvents::default(),
    }
}

fn collect_provider_decoder_line(
    decoder: &ProviderDecoderSpec,
    parsed_line: &str,
    scope: &ScopeRef,
    sender: &mpsc::UnboundedSender<AdapterEvent>,
) -> OutputLineEvents {
    if !decoder.events.is_empty() {
        return translate_decoder_event_line(Some(decoder), parsed_line, scope, sender)
            .unwrap_or_default();
    }
    match (decoder.format.as_str(), decoder.name.as_deref()) {
        ("builtin", Some("claude_stream_json" | "qoder_stream_json")) => OutputLineEvents {
            emitted_text: translate_claude_stream_line(parsed_line, scope, sender),
            emitted_finish: false,
        },
        ("builtin", Some("codex_stream_json")) => OutputLineEvents {
            emitted_text: translate_codex_event_line(parsed_line, scope, sender),
            emitted_finish: false,
        },
        ("builtin", Some("ndjson_lines")) => OutputLineEvents {
            emitted_text: translate_ndjson_line(parsed_line, scope, sender),
            emitted_finish: false,
        },
        _ => OutputLineEvents::default(),
    }
}

fn truncate_for_summary(s: &str) -> String {
    const MAX: usize = 500;
    if s.len() <= MAX {
        s.trim().to_string()
    } else {
        let mut t = s[..MAX].trim().to_string();
        t.push('…');
        t
    }
}

fn extract_runtime_error_from_text(text: &str) -> Option<String> {
    let mut found = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            if let Some(value) = extract_embedded_error_json(trimmed) {
                if let Some(message) = runtime_error_from_json(&value).and_then(non_blank) {
                    found = Some(truncate_for_summary(&message));
                }
            }
            continue;
        };
        if let Some(message) = runtime_error_from_json(&value).and_then(non_blank) {
            found = Some(truncate_for_summary(&message));
        }
    }
    found
}

fn runtime_error_from_json(value: &Value) -> Option<String> {
    let embedded_error = json_string_at_paths(
        value,
        &["/data/errorMessage", "/errorMessage", "/error/errorMessage"],
    );
    let message = string_at_paths(
        value,
        &[
            "/error/message",
            "/error/data/error/message",
            "/error/data/message",
            "/error/error/message",
            "/properties/error/message",
            "/data/error/message",
            "/data/message",
            "/message/content/0/text",
            "/message",
            "/result",
            "/details",
        ],
    )
    .or_else(|| {
        embedded_error
            .as_ref()
            .and_then(|error| string_at_paths(error, &["/error/message", "/message", "/details"]))
    })
    .or_else(|| string_at_paths(value, &["/data/errorMessage", "/errorMessage"]))
    .or_else(|| response_body_error_message(value))?;

    let status = string_or_number_at_paths(
        value,
        &[
            "/statusCode",
            "/status",
            "/properties/statusCode",
            "/properties/status",
            "/data/statusCode",
            "/data/status",
            "/api_error_status",
            "/error/statusCode",
            "/error/status",
            "/error/api_error_status",
        ],
    )
    .or_else(|| {
        embedded_error.as_ref().and_then(|error| {
            string_or_number_at_paths(
                error,
                &[
                    "/statusCode",
                    "/status",
                    "/error/statusCode",
                    "/error/status",
                ],
            )
        })
    })
    .or_else(|| response_body_error_status(value));
    let code = string_at_paths(
        value,
        &[
            "/error/type",
            "/error/code",
            "/properties/error/type",
            "/properties/error/code",
            "/data/errorType",
            "/data/errorCode",
            "/data/code",
            "/code",
            "/error",
        ],
    )
    .or_else(|| {
        embedded_error.as_ref().and_then(|error| {
            string_at_paths(
                error,
                &[
                    "/error/type",
                    "/error/code",
                    "/error/name",
                    "/code",
                    "/name",
                ],
            )
        })
    })
    .or_else(|| response_body_error_code(value))
    .or_else(|| string_at_paths(value, &["/error/name", "/properties/error/name", "/name"]));
    let retry_after = string_or_number_at_paths(
        value,
        &[
            "/responseHeaders/retry-after",
            "/responseHeaders/Retry-After",
            "/error/responseHeaders/retry-after",
            "/error/responseHeaders/Retry-After",
            "/properties/responseHeaders/retry-after",
            "/properties/responseHeaders/Retry-After",
            "/headers/retry-after",
            "/headers/Retry-After",
        ],
    );

    let mut prefix = Vec::new();
    if let Some(status) = status.filter(|status| !message.contains(status)) {
        prefix.push(status);
    }
    if let Some(code) = code.filter(|code| !message.contains(code)) {
        prefix.push(code);
    }
    let mut rendered = if prefix.is_empty() {
        message
    } else {
        format!("{}: {message}", prefix.join(" "))
    };
    if let Some(retry_after) = retry_after {
        rendered.push_str(&format!(" (retry-after: {retry_after}s)"));
    }
    Some(rendered)
}

fn response_body_error_message(value: &Value) -> Option<String> {
    let parsed = response_body_json(value)?;
    string_at_paths(
        &parsed,
        &["/error/message", "/message", "/error/details", "/details"],
    )
}

fn response_body_error_status(value: &Value) -> Option<String> {
    let parsed = response_body_json(value)?;
    string_or_number_at_paths(
        &parsed,
        &[
            "/statusCode",
            "/status",
            "/error/statusCode",
            "/error/status",
        ],
    )
}

fn response_body_error_code(value: &Value) -> Option<String> {
    let parsed = response_body_json(value)?;
    string_at_paths(
        &parsed,
        &[
            "/error/type",
            "/error/code",
            "/error/name",
            "/code",
            "/name",
        ],
    )
    .or_else(|| {
        let kind = parsed.get("type").and_then(Value::as_str)?;
        (kind != "error").then(|| kind.to_string())
    })
}

fn response_body_json(value: &Value) -> Option<Value> {
    let body = string_at_paths(
        value,
        &[
            "/responseBody",
            "/error/responseBody",
            "/properties/responseBody",
            "/data/responseBody",
        ],
    )?;
    serde_json::from_str::<Value>(&body).ok()
}

fn json_string_at_paths(value: &Value, paths: &[&str]) -> Option<Value> {
    paths.iter().find_map(|path| {
        let raw = value.pointer(path).and_then(Value::as_str)?.trim();
        if !raw.starts_with('{') {
            return None;
        }
        serde_json::from_str::<Value>(raw).ok()
    })
}

fn extract_embedded_error_json(line: &str) -> Option<Value> {
    let start = line.find("error=")?;
    let after_marker = &line[start + "error=".len()..];
    let brace_offset = after_marker.find('{')?;
    let json_text = balanced_json_object_prefix(&after_marker[brace_offset..])?;
    serde_json::from_str::<Value>(json_text).ok()
}

fn balanced_json_object_prefix(input: &str) -> Option<&str> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (idx, ch) in input.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(&input[..=idx]);
                }
            }
            _ => {}
        }
    }
    None
}

// ---------------- provider and legacy output translators ----------------

#[derive(Debug, Clone, Copy, Default)]
struct OutputLineEvents {
    emitted_text: bool,
    emitted_finish: bool,
}

fn emit_provider_runtime_events(
    events: impl IntoIterator<Item = ProviderRuntimeEvent>,
    scope: &ScopeRef,
    sender: &mpsc::UnboundedSender<AdapterEvent>,
) -> OutputLineEvents {
    let mut emitted = OutputLineEvents::default();
    for event in events {
        let next = emit_provider_runtime_event(event, scope, sender);
        emitted.emitted_text |= next.emitted_text;
        emitted.emitted_finish |= next.emitted_finish;
    }
    emitted
}

fn emit_provider_runtime_event(
    event: ProviderRuntimeEvent,
    scope: &ScopeRef,
    sender: &mpsc::UnboundedSender<AdapterEvent>,
) -> OutputLineEvents {
    match event {
        ProviderRuntimeEvent::Text {
            content,
            is_partial,
        } => {
            let _ = sender.send(AdapterEvent::Text {
                scope: Some(scope.clone()),
                content,
                is_partial,
            });
            OutputLineEvents {
                emitted_text: true,
                emitted_finish: false,
            }
        }
        ProviderRuntimeEvent::ToolUse { tool_name, input } => {
            let _ = sender.send(AdapterEvent::ToolUse {
                scope: Some(scope.clone()),
                tool_name,
                input,
            });
            OutputLineEvents::default()
        }
        ProviderRuntimeEvent::Status { status } => {
            let _ = sender.send(AdapterEvent::StatusChange {
                scope: Some(scope.clone()),
                status,
            });
            OutputLineEvents::default()
        }
        ProviderRuntimeEvent::Error { message } => {
            let _ = sender.send(AdapterEvent::Error {
                scope: Some(scope.clone()),
                message,
            });
            OutputLineEvents::default()
        }
        ProviderRuntimeEvent::Finished { success, summary } => {
            let _ = sender.send(AdapterEvent::Finished {
                scope: Some(scope.clone()),
                success,
                summary,
                usage: None,
            });
            OutputLineEvents {
                emitted_text: false,
                emitted_finish: true,
            }
        }
        ProviderRuntimeEvent::Session { .. } => OutputLineEvents::default(),
    }
}

fn translate_decoder_event_line(
    decoder: Option<&ProviderDecoderSpec>,
    line: &str,
    scope: &ScopeRef,
    sender: &mpsc::UnboundedSender<AdapterEvent>,
) -> Option<OutputLineEvents> {
    let decoder = decoder?;
    if decoder.events.is_empty() {
        return None;
    }
    let events = decode_decoder_event_line(decoder, line)?;
    Some(emit_provider_runtime_events(events, scope, sender))
}

fn decode_decoder_event_line(
    decoder: &ProviderDecoderSpec,
    line: &str,
) -> Option<Vec<ProviderRuntimeEvent>> {
    if decoder.events.is_empty() {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;
    let mut decoded = Vec::new();
    for event in &decoder.events {
        if !event
            .when
            .as_ref()
            .map(|condition| json_condition_matches(&v, condition))
            .unwrap_or(true)
        {
            continue;
        }
        if let Some(event) = decode_decoder_emit(&event.emit, &v) {
            decoded.push(event);
        }
    }
    Some(decoded)
}

fn decode_decoder_emit(
    emit: &ProviderDecoderEmitSpec,
    root: &Value,
) -> Option<ProviderRuntimeEvent> {
    match emit.emit_type.as_str() {
        "text" => {
            let content = emit
                .text
                .as_deref()
                .and_then(|template| decoder_template_value(root, template))?;
            Some(ProviderRuntimeEvent::Text {
                content,
                is_partial: emit.partial.unwrap_or(true),
            })
        }
        "tool_use" | "toolUse" | "tool" => {
            let tool_name = emit
                .tool_name
                .as_deref()
                .and_then(|template| decoder_template_value(root, template))
                .unwrap_or_default();
            let input = emit
                .input
                .as_deref()
                .and_then(|template| decoder_template_json_value(root, template))
                .unwrap_or(Value::Null);
            Some(ProviderRuntimeEvent::ToolUse { tool_name, input })
        }
        "status" => emit
            .status
            .as_deref()
            .or(emit.text.as_deref())
            .and_then(|template| decoder_template_value(root, template))
            .map(|status| ProviderRuntimeEvent::Status { status }),
        "error" => {
            let message = emit
                .message
                .as_deref()
                .or(emit.text.as_deref())
                .and_then(|template| decoder_template_value(root, template))
                .unwrap_or_else(|| "provider error frame".into());
            Some(ProviderRuntimeEvent::Error { message })
        }
        "finish" | "finished" => {
            let summary = emit
                .summary
                .as_deref()
                .or(emit.message.as_deref())
                .and_then(|template| decoder_template_value(root, template))
                .unwrap_or_default();
            Some(ProviderRuntimeEvent::Finished {
                success: emit.success.unwrap_or(true),
                summary,
            })
        }
        _ => None,
    }
}

fn decoder_template_value(root: &Value, template: &str) -> Option<String> {
    if template.starts_with('$') || template.starts_with('.') {
        json_path_lookup(root, template)
    } else {
        Some(template.to_string())
    }
}

fn decoder_template_json_value(root: &Value, template: &str) -> Option<Value> {
    if template.starts_with('$') || template.starts_with('.') {
        json_path_lookup_value(root, template).cloned()
    } else {
        serde_json::from_str(template)
            .ok()
            .or_else(|| Some(Value::String(template.to_string())))
    }
}

fn translate_ndjson_line(
    line: &str,
    scope: &ScopeRef,
    sender: &mpsc::UnboundedSender<AdapterEvent>,
) -> bool {
    let events = decode_ndjson_line_events(line);
    emit_provider_runtime_events(events, scope, sender).emitted_text
}

fn decode_ndjson_line_events(line: &str) -> Vec<ProviderRuntimeEvent> {
    let v: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let kind = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
    let mut events = Vec::new();
    match kind {
        "text" => {
            if let Some(t) = v.get("text").and_then(|x| x.as_str()) {
                events.push(ProviderRuntimeEvent::Text {
                    content: t.to_string(),
                    is_partial: true,
                });
            }
        }
        "tool" => {
            let name = v
                .get("name")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let input = v.get("input").cloned().unwrap_or(Value::Null);
            events.push(ProviderRuntimeEvent::ToolUse {
                tool_name: name,
                input,
            });
        }
        "status" => {
            if let Some(s) = v.get("status").and_then(|x| x.as_str()) {
                events.push(ProviderRuntimeEvent::Status {
                    status: s.to_string(),
                });
            }
        }
        "error" => {
            let msg = v
                .get("message")
                .and_then(|x| x.as_str())
                .unwrap_or("ndjson error frame")
                .to_string();
            events.push(ProviderRuntimeEvent::Error { message: msg });
        }
        // "done" and unknown kinds: caller handles the final flush + Finished
        // outside the per-line loop, so nothing to do here.
        _ => {}
    }
    events
}

fn translate_claude_stream_line(
    line: &str,
    scope: &ScopeRef,
    sender: &mpsc::UnboundedSender<AdapterEvent>,
) -> bool {
    let events = decode_claude_stream_line_events(line);
    emit_provider_runtime_events(events, scope, sender).emitted_text
}

fn decode_claude_stream_line_events(line: &str) -> Vec<ProviderRuntimeEvent> {
    let v: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let outer = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
    let mut events = Vec::new();
    match outer {
        "assistant" => {
            let blocks = v
                .pointer("/message/content")
                .and_then(|c| c.as_array())
                .cloned()
                .unwrap_or_default();
            for b in blocks {
                let kind = b.get("type").and_then(|x| x.as_str()).unwrap_or("");
                match kind {
                    "text" => {
                        if let Some(t) = b.get("text").and_then(|x| x.as_str()) {
                            events.push(ProviderRuntimeEvent::Text {
                                content: t.to_string(),
                                is_partial: false,
                            });
                        }
                    }
                    "tool_use" => {
                        let name = b
                            .get("name")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string();
                        let input = b.get("input").cloned().unwrap_or(Value::Null);
                        events.push(ProviderRuntimeEvent::ToolUse {
                            tool_name: name,
                            input,
                        });
                    }
                    _ => {}
                }
            }
        }
        // tool_result frames are internal to claude — the user already has
        // text turn output; don't surface them as their own events.
        "user" | "system" | "result" => {}
        _ => {}
    }
    events
}

fn translate_codex_event_line(
    line: &str,
    scope: &ScopeRef,
    sender: &mpsc::UnboundedSender<AdapterEvent>,
) -> bool {
    let events = decode_codex_event_line_events(line);
    emit_provider_runtime_events(events, scope, sender).emitted_text
}

fn decode_codex_event_line_events(line: &str) -> Vec<ProviderRuntimeEvent> {
    let v: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let mut events = Vec::new();
    if let Some(t) = v.get("type").and_then(|x| x.as_str()) {
        match t {
            "task_complete" | "task.completed" => {
                if let Some(text) = string_at_paths(
                    &v,
                    &["/last_agent_message", "/lastAgentMessage", "/message"],
                )
                .or_else(|| {
                    v.get("last_agent_message")
                        .and_then(codex_response_item_text)
                })
                .or_else(|| codex_content_text(v.get("last_agent_message")?))
                .and_then(non_blank)
                {
                    events.push(ProviderRuntimeEvent::Text {
                        content: text,
                        is_partial: false,
                    });
                }
            }
            "agent_message" | "agent.message" => {
                if let Some(text) = codex_message_event_text(&v).and_then(non_blank) {
                    events.push(ProviderRuntimeEvent::Text {
                        content: text,
                        is_partial: false,
                    });
                }
            }
            "item_completed" | "item.completed" | "raw_response_item" | "raw.response_item" => {
                if let Some(text) = v
                    .get("item")
                    .and_then(codex_response_item_text)
                    .and_then(non_blank)
                {
                    events.push(ProviderRuntimeEvent::Text {
                        content: text,
                        is_partial: false,
                    });
                }
            }
            "tool_call" => {
                let name = v
                    .get("name")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                let input = v.get("arguments").cloned().unwrap_or(Value::Null);
                events.push(ProviderRuntimeEvent::ToolUse {
                    tool_name: name,
                    input,
                });
            }
            "stream_error" => {
                let message =
                    string_at_paths(&v, &["/message", "/error/message", "/error", "/details"])
                        .unwrap_or_else(|| "codex stream error".into());
                events.push(ProviderRuntimeEvent::Error { message });
            }
            _ => {}
        }
    }
    events
}

fn extract_codex_json_final_text(stdout: &str) -> Option<String> {
    let mut final_text: Option<String> = None;
    let mut streamed_text = String::new();

    for line in stdout.lines() {
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let kind = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
        match kind {
            "task_complete" | "task.completed" | "turn_complete" | "turn.completed" => {
                if let Some(text) = string_at_paths(
                    &v,
                    &["/last_agent_message", "/lastAgentMessage", "/message"],
                )
                .or_else(|| {
                    v.get("last_agent_message")
                        .and_then(codex_response_item_text)
                })
                .or_else(|| codex_content_text(v.get("last_agent_message")?))
                .and_then(non_blank)
                {
                    final_text = Some(text);
                }
            }
            "agent_message" | "agent.message" => {
                if let Some(text) = codex_message_event_text(&v).and_then(non_blank) {
                    final_text = Some(text);
                }
            }
            "item_completed" | "item.completed" | "raw_response_item" | "raw.response_item" => {
                if let Some(text) = v
                    .get("item")
                    .and_then(codex_response_item_text)
                    .and_then(non_blank)
                {
                    final_text = Some(text);
                }
            }
            "agent_message_content_delta" | "output_text.delta" => {
                if let Some(delta) = string_at_paths(
                    &v,
                    &["/delta", "/text", "/content", "/data/delta", "/data/text"],
                ) {
                    streamed_text.push_str(&delta);
                }
            }
            _ => {}
        }
    }

    final_text.or_else(|| non_blank(streamed_text))
}

fn codex_message_event_text(v: &Value) -> Option<String> {
    string_at_paths(v, &["/message", "/text", "/content"])
        .or_else(|| v.get("message").and_then(codex_response_item_text))
        .or_else(|| v.pointer("/message/content").and_then(codex_content_text))
        .or_else(|| v.get("content").and_then(codex_content_text))
        .or_else(|| v.get("message").and_then(codex_content_text))
}

fn codex_response_item_text(item: &Value) -> Option<String> {
    let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
    if item_type != "message" && item_type != "agent_message" {
        return None;
    }
    if let Some(role) = item.get("role").and_then(Value::as_str) {
        if role != "assistant" {
            return None;
        }
    }
    string_at_paths(item, &["/text", "/message"])
        .or_else(|| item.get("content").and_then(codex_content_text))
}

fn codex_content_text(value: &Value) -> Option<String> {
    if let Some(s) = value.as_str() {
        return Some(s.to_string());
    }
    let arr = value.as_array()?;
    let mut out = String::new();
    for item in arr {
        let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
        if !matches!(
            item_type,
            "" | "output_text" | "final_answer" | "commentary" | "text"
        ) {
            continue;
        }
        if let Some(text) = string_at_paths(item, &["/text", "/content", "/delta"]) {
            out.push_str(&text);
        } else if let Some(content) = item.get("content").and_then(codex_content_text) {
            out.push_str(&content);
        }
    }
    non_blank(out)
}

fn extract_copilot_json_final_text(stdout: &str) -> Option<String> {
    let mut final_text: Option<String> = None;
    let mut streamed_text = String::new();

    for line in stdout.lines() {
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        // Copilot includes `agentId` only for sub-agent events. Those are
        // timeline details, not the user-visible answer for the current actor.
        let has_subagent_id = v.get("agentId").is_some_and(|agent_id| !agent_id.is_null());
        let has_parent_tool_call = v
            .pointer("/data/parentToolCallId")
            .is_some_and(|call_id| !call_id.is_null());
        if has_subagent_id || has_parent_tool_call {
            continue;
        }
        let kind = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
        match kind {
            "assistant.message" => {
                if copilot_message_phase_is_hidden(&v) {
                    continue;
                }
                if let Some(content) =
                    string_at_paths(&v, &["/data/content", "/message/content", "/content"])
                        .and_then(non_blank)
                {
                    final_text = Some(content);
                }
            }
            "assistant.message_delta" => {
                if let Some(delta) =
                    string_at_paths(&v, &["/data/deltaContent", "/deltaContent", "/delta"])
                {
                    streamed_text.push_str(&delta);
                }
            }
            _ => {}
        }
    }

    final_text.or_else(|| non_blank(streamed_text))
}

fn extract_opencode_json_final_text(stdout: &str) -> Option<String> {
    let mut final_text: Option<String> = None;
    let mut part_texts: Vec<(String, String)> = Vec::new();
    let mut streamed_text = String::new();

    for line in stdout.lines() {
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let kind = v.get("type").and_then(Value::as_str).unwrap_or("");
        match kind {
            "message.updated" => {
                let info = v.pointer("/properties/info").or_else(|| v.get("info"));
                if let Some(info) = info {
                    if !opencode_message_role_allows_assistant(info) {
                        continue;
                    }
                    if let Some(text) = opencode_message_text(info).and_then(non_blank) {
                        final_text = Some(text);
                    }
                }
            }
            "message.part.updated" | "message.part.delta" => {
                let part = v.pointer("/properties/part").or_else(|| v.get("part"));
                if let Some(part) = part {
                    if let Some(delta) = string_at_paths(part, &["/delta", "/textDelta"]) {
                        streamed_text.push_str(&delta);
                    } else if let Some(text) = opencode_part_text(part).and_then(non_blank) {
                        let id = string_at_paths(part, &["/id", "/partID", "/partId"])
                            .unwrap_or_else(|| format!("part_{}", part_texts.len()));
                        upsert_part_text(&mut part_texts, id, text);
                    }
                }
            }
            _ => {}
        }
    }

    final_text
        .or_else(|| {
            let joined = part_texts
                .into_iter()
                .map(|(_, text)| text)
                .collect::<Vec<_>>()
                .join("");
            non_blank(joined)
        })
        .or_else(|| non_blank(streamed_text))
}

fn opencode_message_role_allows_assistant(info: &Value) -> bool {
    let Some(role) = info.get("role").and_then(Value::as_str) else {
        return true;
    };
    role == "assistant"
}

fn opencode_message_text(info: &Value) -> Option<String> {
    string_at_paths(info, &["/text", "/content", "/message"])
        .or_else(|| info.get("parts").and_then(opencode_parts_text))
        .or_else(|| info.get("content").and_then(opencode_content_text))
}

fn opencode_parts_text(value: &Value) -> Option<String> {
    let parts = value.as_array()?;
    let mut out = String::new();
    for part in parts {
        if let Some(text) = opencode_part_text(part) {
            out.push_str(&text);
        }
    }
    non_blank(out)
}

fn opencode_part_text(part: &Value) -> Option<String> {
    let part_type = part.get("type").and_then(Value::as_str).unwrap_or("");
    if !part_type.is_empty()
        && !matches!(
            part_type,
            "text" | "markdown" | "content" | "assistant_message"
        )
    {
        return None;
    }
    string_at_paths(part, &["/text", "/content", "/message"])
        .or_else(|| part.get("content").and_then(opencode_content_text))
}

fn opencode_content_text(value: &Value) -> Option<String> {
    if let Some(s) = value.as_str() {
        return Some(s.to_string());
    }
    let arr = value.as_array()?;
    let mut out = String::new();
    for item in arr {
        if let Some(text) = opencode_part_text(item) {
            out.push_str(&text);
        }
    }
    non_blank(out)
}

fn upsert_part_text(parts: &mut Vec<(String, String)>, id: String, text: String) {
    if let Some((_, existing)) = parts.iter_mut().find(|(existing_id, _)| *existing_id == id) {
        *existing = text;
        return;
    }
    parts.push((id, text));
}

fn extract_decoder_final_text(
    decoder: Option<&ProviderDecoderSpec>,
    stdout: &str,
) -> Option<String> {
    let decoder = decoder?;
    let reducer = decoder
        .reduce
        .as_ref()
        .and_then(|reduce| reduce.final_text.as_ref())?;
    reduce_text_with_fallback(decoder.format.as_str(), stdout, reducer)
}

fn extract_configured_decoder_final_text(
    cfg: &CommandConfig,
    stdout: &str,
    stderr: &str,
) -> Option<String> {
    extract_decoder_final_text(cfg.decoder.as_ref(), stdout)
        .or_else(|| extract_decoder_final_text(cfg.stderr_decoder.as_ref(), stderr))
}

fn configured_decoder_final_text_event(
    cfg: &CommandConfig,
    stdout: &str,
    stderr: &str,
) -> Option<ProviderRuntimeEvent> {
    extract_configured_decoder_final_text(cfg, stdout, stderr).map(|content| {
        ProviderRuntimeEvent::Text {
            content,
            is_partial: false,
        }
    })
}

fn capture_decoder_session_id(
    decoder: Option<&ProviderDecoderSpec>,
    stdout: &str,
) -> Option<String> {
    let decoder = decoder?;
    let reducer = decoder
        .capture
        .as_ref()
        .and_then(|capture| capture.session.as_ref())?;
    reduce_text_with_fallback(decoder.format.as_str(), stdout, reducer)
}

fn capture_configured_decoder_session_id(
    cfg: &CommandConfig,
    outcome: &SpawnOutcome,
) -> Option<String> {
    capture_decoder_session_id(cfg.decoder.as_ref(), &outcome.stdout)
        .or_else(|| capture_decoder_session_id(cfg.stderr_decoder.as_ref(), &outcome.stderr))
}

fn capture_configured_decoder_session_event(
    cfg: &CommandConfig,
    outcome: &SpawnOutcome,
) -> Option<ProviderRuntimeEvent> {
    capture_configured_decoder_session_id(cfg, outcome)
        .map(|session_id| ProviderRuntimeEvent::Session { session_id })
}

fn reduce_text_with_fallback(
    format: &str,
    stdout: &str,
    reducer: &ProviderJsonlTextReducerSpec,
) -> Option<String> {
    reduce_text(format, stdout, reducer).or_else(|| {
        reducer
            .fallback
            .as_deref()
            .and_then(|fallback| reduce_text_with_fallback(format, stdout, fallback))
    })
}

fn reduce_text(
    format: &str,
    stdout: &str,
    reducer: &ProviderJsonlTextReducerSpec,
) -> Option<String> {
    if format.trim() == "json" {
        let root = serde_json::from_str::<Value>(stdout).ok()?;
        return reduce_json_values(std::iter::once(root), reducer);
    }
    reduce_jsonl_text(stdout, reducer)
}

fn reduce_jsonl_text(stdout: &str, reducer: &ProviderJsonlTextReducerSpec) -> Option<String> {
    reduce_json_values(
        stdout
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok()),
        reducer,
    )
}

fn reduce_json_values(
    values: impl IntoIterator<Item = Value>,
    reducer: &ProviderJsonlTextReducerSpec,
) -> Option<String> {
    let mode = reducer.mode.trim();
    let path = reducer.path.trim();
    if path.is_empty() {
        return None;
    }
    let mut last: Option<String> = None;
    let mut concat = String::new();
    for v in values {
        if !reducer
            .when
            .as_ref()
            .map(|condition| json_condition_matches(&v, condition))
            .unwrap_or(true)
        {
            continue;
        }
        for text in json_path_lookup_strings(&v, path) {
            match mode {
                "firstNonEmpty" | "first_non_empty" => {
                    if let Some(text) = non_blank(text) {
                        return Some(text);
                    }
                }
                "concat" | "joinText" | "join_text" => concat.push_str(&text),
                _ => {
                    if let Some(text) = non_blank(text) {
                        last = Some(text);
                    }
                }
            }
        }
    }
    match mode {
        "concat" | "joinText" | "join_text" => non_blank(concat),
        _ => last,
    }
}

fn json_condition_matches(root: &Value, condition: &ProviderJsonConditionSpec) -> bool {
    if !condition
        .all
        .iter()
        .all(|item| json_condition_matches(root, item))
    {
        return false;
    }
    if !condition.any.is_empty()
        && !condition
            .any
            .iter()
            .any(|item| json_condition_matches(root, item))
    {
        return false;
    }
    if condition
        .not
        .as_deref()
        .is_some_and(|item| json_condition_matches(root, item))
    {
        return false;
    }
    let actual = condition
        .path
        .as_deref()
        .and_then(|path| json_path_lookup_value(root, path));
    if let Some(expected) = condition.exists {
        if actual.is_some() != expected {
            return false;
        }
    }
    if let Some(expected) = condition.absent_or_null {
        let absent_or_null = actual.is_none_or(Value::is_null);
        if absent_or_null != expected {
            return false;
        }
    }
    if let Some(expected) = condition.not_empty {
        let not_empty = actual.is_some_and(value_not_empty);
        if not_empty != expected {
            return false;
        }
    }
    if let Some(expected) = condition.equals.as_ref() {
        if actual != Some(expected) {
            return false;
        }
    }
    if let Some(unexpected) = condition.not_equals.as_ref() {
        if actual == Some(unexpected) {
            return false;
        }
    }
    if let Some(values) = condition.in_values.as_ref() {
        if !actual.is_some_and(|actual| values.iter().any(|value| value == actual)) {
            return false;
        }
    }
    if let Some(values) = condition.not_in.as_ref() {
        if actual.is_some_and(|actual| values.iter().any(|value| value == actual)) {
            return false;
        }
    }
    true
}

fn value_not_empty(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::String(value) => !value.trim().is_empty(),
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
        _ => true,
    }
}

fn copilot_message_phase_is_hidden(v: &Value) -> bool {
    let Some(phase) = v.pointer("/data/phase").and_then(Value::as_str) else {
        return false;
    };
    matches!(
        phase.to_ascii_lowercase().as_str(),
        "thinking" | "reasoning"
    )
}

fn string_at_paths(v: &Value, paths: &[&str]) -> Option<String> {
    paths
        .iter()
        .find_map(|path| v.pointer(path).and_then(Value::as_str).map(str::to_string))
}

fn string_or_number_at_paths(v: &Value, paths: &[&str]) -> Option<String> {
    paths.iter().find_map(|path| {
        v.pointer(path).and_then(|value| {
            value
                .as_str()
                .map(str::to_string)
                .or_else(|| value.as_i64().map(|n| n.to_string()))
                .or_else(|| value.as_u64().map(|n| n.to_string()))
        })
    })
}

fn non_blank(s: String) -> Option<String> {
    let trimmed = s.trim_end().to_string();
    if trimmed.trim().is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

// ---------------- session bookkeeping ----------------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SessionRecord {
    actor_id: String,
    scope: ScopeRef,
    session_id: String,
    created_at: String,
    last_used_at: String,
    command_signature: String,
}

fn session_path(cfg: &CommandConfig, scope: &ScopeRef) -> Option<PathBuf> {
    match session_scope(cfg) {
        "turn" => return None,
        "actor" => return Some(cfg.sessions_dir.join(&cfg.actor_id).join("actor.json")),
        _ => {}
    }
    let kind = match scope.kind {
        proto::types::ScopeKind::Thread => "thread",
        proto::types::ScopeKind::Channel => "channel",
    };
    Some(
        cfg.sessions_dir
            .join(&cfg.actor_id)
            .join(format!("{kind}-{}.json", scope.id)),
    )
}

fn session_scope(cfg: &CommandConfig) -> &str {
    cfg.session_scope
        .as_deref()
        .map(str::trim)
        .filter(|scope| !scope.is_empty())
        .unwrap_or("actor_scope")
}

fn scope_label(scope: &ScopeRef) -> String {
    let kind = match scope.kind {
        proto::types::ScopeKind::Thread => "thread",
        proto::types::ScopeKind::Channel => "channel",
    };
    format!("{kind}:{}", scope.id)
}

fn session_lock_path(cfg: &CommandConfig, scope: &ScopeRef) -> Option<PathBuf> {
    let path = session_path(cfg, scope)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("session.json");
    Some(path.with_file_name(format!("{file_name}.lock")))
}

#[cfg(unix)]
struct SessionLockGuard {
    file: Option<std::fs::File>,
}

#[cfg(not(unix))]
struct SessionLockGuard;

#[cfg(unix)]
fn acquire_session_lock(cfg: &CommandConfig, scope: &ScopeRef) -> Result<SessionLockGuard, String> {
    let Some(path) = session_lock_path(cfg, scope) else {
        return Ok(SessionLockGuard { file: None });
    };
    acquire_lock_file(&path)
}

#[cfg(unix)]
fn acquire_lock_file(path: &Path) -> Result<SessionLockGuard, String> {
    if let Some(parent) = path.parent() {
        create_dir_all_unc(parent).map_err(|e| {
            format!(
                "failed to create session lock dir `{}`: {e}",
                parent.display()
            )
        })?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| format!("failed to open session lock `{}`: {e}", path.display()))?;

    loop {
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        if rc == 0 {
            break;
        }
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::EINTR) {
            continue;
        }
        return Err(format!(
            "failed to lock session `{}`: {err}",
            path.display()
        ));
    }

    Ok(SessionLockGuard { file: Some(file) })
}

#[cfg(not(unix))]
fn acquire_session_lock(
    _cfg: &CommandConfig,
    _scope: &ScopeRef,
) -> Result<SessionLockGuard, String> {
    Ok(SessionLockGuard)
}

#[cfg(unix)]
impl Drop for SessionLockGuard {
    fn drop(&mut self) {
        if let Some(file) = self.file.as_ref() {
            let _ = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
        }
    }
}

fn load_session(cfg: &CommandConfig, scope: &ScopeRef) -> Option<SessionRecord> {
    let path = session_path(cfg, scope)?;
    let text = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&text).ok()
}

fn save_session(
    cfg: &CommandConfig,
    scope: &ScopeRef,
    session_id: &str,
    command_signature: &str,
) -> std::io::Result<()> {
    let Some(path) = session_path(cfg, scope) else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        create_dir_all_unc(parent)?;
    }
    let now = chrono::Utc::now().to_rfc3339();
    let existing = load_session(cfg, scope);
    let record = SessionRecord {
        actor_id: cfg.actor_id.clone(),
        scope: scope.clone(),
        session_id: session_id.to_string(),
        created_at: existing
            .map(|e| e.created_at)
            .unwrap_or_else(|| now.clone()),
        last_used_at: now,
        command_signature: command_signature.to_string(),
    };
    let json = serde_json::to_string_pretty(&record)?;
    std::fs::write(path, json)
}

fn command_signature_for_prompt(cfg: &CommandConfig, request: &AdapterPrompt) -> String {
    let model = request
        .model
        .as_ref()
        .map(|model| model.trim())
        .filter(|model| !model.is_empty());
    let reasoning_effort = request
        .template_vars
        .get("reasoningEffort")
        .or_else(|| request.template_vars.get("reasoning_effort"))
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());
    if model.is_none() && reasoning_effort.is_none() {
        return cfg.command_signature.clone();
    };
    let mut hasher = Sha256::new();
    hasher.update(cfg.command_signature.as_bytes());
    if let Some(model) = model {
        hasher.update(b"\x00model\x00");
        hasher.update(model.as_bytes());
    }
    if let Some(reasoning_effort) = reasoning_effort {
        hasher.update(b"\x00reasoning_effort\x00");
        hasher.update(reasoning_effort.as_bytes());
    }
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

fn delete_session(cfg: &CommandConfig, scope: &ScopeRef) -> std::io::Result<()> {
    let Some(path) = session_path(cfg, scope) else {
        return Ok(());
    };
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

fn looks_like_session_lost(stderr: &str, stdout: &str) -> bool {
    let s = format!("{stderr}\n{stdout}").to_ascii_lowercase();
    s.contains("session not found")
        || s.contains("unknown session")
        || s.contains("no such session")
        || s.contains("no conversation found with session id")
}

fn looks_like_signed_thinking_replay_error(stderr: &str, stdout: &str) -> bool {
    let s = format!("{stderr}\n{stdout}").to_ascii_lowercase();
    (s.contains("thinking") || s.contains("redacted_thinking"))
        && s.contains("latest assistant message")
        && s.contains("cannot be modified")
        && s.contains("original response")
}

// ---------------- first_run_capture ----------------

fn capture_session_id(
    rule: &str,
    outcome: &SpawnOutcome,
    cfg: &CommandConfig,
    prompt: &AdapterPrompt,
) -> Result<Option<String>, String> {
    if let Some(path) = rule.strip_prefix("stdout_json:") {
        return Ok(extract_json_path(&outcome.stdout, path));
    }
    if let Some(path) = rule
        .strip_prefix("stdout_jsonl:last(")
        .and_then(|value| value.strip_suffix(')'))
    {
        return Ok(extract_json_path(&outcome.stdout, path));
    }
    if let Some(paths) = rule.strip_prefix("stdout_json_any:") {
        let paths = paths
            .split('|')
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .collect::<Vec<_>>();
        return Ok(extract_json_path_any(&outcome.stdout, &paths));
    }
    if let Some(_re) = rule.strip_prefix("stderr_regex:") {
        return Err("stderr_regex first_run_capture not yet implemented".into());
    }
    if let Some(path) = rule.strip_prefix("file:") {
        let expanded = expand_template(path, cfg, prompt, None, "");
        let text = std::fs::read_to_string(&expanded)
            .map_err(|e| format!("failed to read capture file `{expanded}`: {e}"))?;
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        return Ok(Some(trimmed.to_string()));
    }
    Err(format!("unknown first_run_capture rule: {rule}"))
}

fn extract_json_path(stdout: &str, path: &str) -> Option<String> {
    // Try the whole stdout as one JSON document first; fall back to
    // line-by-line so we handle ndjson-style streams too.
    if let Ok(v) = serde_json::from_str::<Value>(stdout) {
        if let Some(s) = json_path_lookup(&v, path) {
            return Some(s);
        }
    }
    let mut found: Option<String> = None;
    for line in stdout.lines() {
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            if let Some(s) = json_path_lookup(&v, path) {
                // Per the doc: "find the last line that matches" — keep
                // overwriting so the loop ends with the most recent value.
                found = Some(s);
            }
        }
    }
    found
}

fn extract_json_path_any(stdout: &str, paths: &[&str]) -> Option<String> {
    if let Ok(v) = serde_json::from_str::<Value>(stdout) {
        if let Some(s) = paths.iter().find_map(|path| json_path_lookup(&v, path)) {
            return Some(s);
        }
    }
    let mut found: Option<String> = None;
    for line in stdout.lines() {
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            if let Some(s) = paths.iter().find_map(|path| json_path_lookup(&v, path)) {
                found = Some(s);
            }
        }
    }
    found
}

/// Tiny jq-style accessor: only `.field.sub`, `.items[3].id`,
/// `.items[*].id`. No filters, pipes, or functions.
fn json_path_lookup(root: &Value, path: &str) -> Option<String> {
    json_path_lookup_strings(root, path).into_iter().next()
}

fn json_path_lookup_strings(root: &Value, path: &str) -> Vec<String> {
    json_path_lookup_values(root, path)
        .into_iter()
        .filter_map(json_value_to_string)
        .collect()
}

fn json_value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn json_path_lookup_value<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    json_path_lookup_values(root, path).into_iter().next()
}

fn json_path_lookup_values<'a>(root: &'a Value, path: &str) -> Vec<&'a Value> {
    let path = path
        .strip_prefix("$.")
        .or_else(|| path.strip_prefix('.'))
        .unwrap_or(path);
    let mut current = vec![root];
    for raw in path.split('.') {
        if raw.is_empty() {
            continue;
        }
        // Parse `name[3]` → key + indices.
        let (key, indices) = parse_segment(raw);
        let mut next = Vec::new();
        for value in current {
            let Some(value) = (if key.is_empty() {
                Some(value)
            } else {
                value.get(key)
            }) else {
                continue;
            };
            push_indexed_values(value, &indices, &mut next);
        }
        current = next;
        if current.is_empty() {
            break;
        }
    }
    current
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JsonPathIndex {
    Index(usize),
    Wildcard,
}

fn parse_segment(seg: &str) -> (&str, Vec<JsonPathIndex>) {
    let mut indices = Vec::new();
    let key_end = seg.find('[').unwrap_or(seg.len());
    let key = &seg[..key_end];
    let mut rest = &seg[key_end..];
    while let Some(open) = rest.find('[') {
        let close = match rest.find(']') {
            Some(c) if c > open => c,
            _ => break,
        };
        let index = &rest[open + 1..close];
        if index == "*" {
            indices.push(JsonPathIndex::Wildcard);
        } else if let Ok(n) = index.parse::<usize>() {
            indices.push(JsonPathIndex::Index(n));
        }
        rest = &rest[close + 1..];
    }
    (key, indices)
}

fn push_indexed_values<'a>(value: &'a Value, indices: &[JsonPathIndex], out: &mut Vec<&'a Value>) {
    let Some((first, rest)) = indices.split_first() else {
        out.push(value);
        return;
    };
    match first {
        JsonPathIndex::Index(idx) => {
            if let Some(value) = value.get(*idx) {
                push_indexed_values(value, rest, out);
            }
        }
        JsonPathIndex::Wildcard => {
            if let Some(items) = value.as_array() {
                for item in items {
                    push_indexed_values(item, rest, out);
                }
            }
        }
    }
}

// ---------------- argv & template expansion ----------------

fn expand_first_run_argv(
    cfg: &CommandConfig,
    request: &AdapterPrompt,
    session_id: Option<&str>,
    prompt: &str,
) -> Vec<String> {
    if !cfg.arg_specs.is_empty() {
        let mut argv = expand_arg_specs(&cfg.arg_specs, cfg, request, session_id, prompt);
        if matches!(cfg.prompt_via, PromptVia::Stdin) {
            strip_prompt_args(&mut argv);
        }
        return argv;
    }
    let mut argv: Vec<String> = cfg
        .args
        .iter()
        .map(|a| expand_template(a, cfg, request, session_id, prompt))
        .collect();
    if let Some(pos) = prompt_ref_insert_pos(&cfg.args, &argv) {
        let mut model_args = Vec::new();
        append_model_args(&mut model_args, cfg, request, session_id, prompt);
        argv.splice(pos..pos, model_args);
        if matches!(cfg.prompt_via, PromptVia::Stdin) {
            strip_prompt_args(&mut argv);
        }
        return argv;
    }
    if matches!(cfg.prompt_via, PromptVia::Args) {
        let already = cfg.args.iter().any(|a| contains_prompt_ref(a));
        if !already {
            if let Some(pos) = prompt_flag_without_value(&argv) {
                let mut model_args = Vec::new();
                append_model_args(&mut model_args, cfg, request, session_id, prompt);
                let model_len = model_args.len();
                argv.splice(pos..pos, model_args);
                argv.insert(pos + model_len + 1, prompt.to_string());
                return argv;
            }
        }
    }
    append_model_args(&mut argv, cfg, request, session_id, prompt);
    if matches!(cfg.prompt_via, PromptVia::Args) {
        // Only append when the template didn't already place {prompt} itself.
        let already =
            cfg.args.iter().any(|a| contains_prompt_ref(a)) || argv.iter().any(|a| a == prompt);
        if !already {
            argv.push(prompt.to_string());
        }
    }
    if matches!(cfg.prompt_via, PromptVia::Stdin) {
        strip_prompt_args(&mut argv);
    }
    argv
}

/// Remove `-p <prompt>` or `--prompt <prompt>` from argv when prompt is
/// delivered via stdin to avoid hitting the Windows command-line length limit.
fn strip_prompt_args(argv: &mut Vec<String>) {
    let mut i = 0;
    while i < argv.len() {
        if matches!(argv[i].as_str(), "-p" | "--prompt") {
            if i + 1 < argv.len() {
                argv.remove(i); // remove -p
                argv.remove(i); // remove its value
            } else {
                argv.remove(i); // trailing -p with no value
            }
        } else {
            i += 1;
        }
    }
}

fn expand_arg_specs(
    specs: &[ProviderArgSpec],
    cfg: &CommandConfig,
    request: &AdapterPrompt,
    session_id: Option<&str>,
    prompt: &str,
) -> Vec<String> {
    let mut out = Vec::new();
    append_arg_specs(&mut out, specs, cfg, request, session_id, prompt);
    out
}

fn append_arg_specs(
    out: &mut Vec<String>,
    specs: &[ProviderArgSpec],
    cfg: &CommandConfig,
    request: &AdapterPrompt,
    session_id: Option<&str>,
    prompt: &str,
) {
    for spec in specs {
        match spec {
            ProviderArgSpec::Literal(value) => {
                out.push(expand_template(value, cfg, request, session_id, prompt));
            }
            ProviderArgSpec::Conditional(spec) => {
                if arg_condition_matches(&spec.when, request) {
                    append_arg_specs(out, &spec.args, cfg, request, session_id, prompt);
                }
            }
        }
    }
}

fn arg_condition_matches(when: &str, request: &AdapterPrompt) -> bool {
    let when = when.trim();
    if when == "model" {
        return active_model(request).is_some();
    }
    if matches!(when, "reasoningEffort" | "reasoning_effort") {
        return request
            .template_vars
            .get("reasoningEffort")
            .or_else(|| request.template_vars.get("reasoning_effort"))
            .is_some_and(|value| !value.trim().is_empty());
    }
    request
        .template_vars
        .get(when)
        .is_some_and(|value| !value.trim().is_empty())
}

fn expand_argv(
    template: &[String],
    cfg: &CommandConfig,
    request: &AdapterPrompt,
    session_id: Option<&str>,
    prompt: &str,
) -> Vec<String> {
    let mut argv: Vec<String> = template
        .iter()
        .map(|a| expand_template(a, cfg, request, session_id, prompt))
        .collect();
    if let Some(pos) = prompt_ref_insert_pos(template, &argv) {
        let mut model_args = Vec::new();
        append_model_args(&mut model_args, cfg, request, session_id, prompt);
        argv.splice(pos..pos, model_args);
        if matches!(cfg.prompt_via, PromptVia::Stdin) {
            strip_prompt_args(&mut argv);
        }
        return argv;
    }
    if matches!(cfg.prompt_via, PromptVia::Args) {
        let already = template.iter().any(|a| contains_prompt_ref(a));
        if !already {
            if let Some(pos) = prompt_flag_without_value(&argv) {
                let mut model_args = Vec::new();
                append_model_args(&mut model_args, cfg, request, session_id, prompt);
                let model_len = model_args.len();
                argv.splice(pos..pos, model_args);
                argv.insert(pos + model_len + 1, prompt.to_string());
                return argv;
            }
        }
    }
    append_model_args(&mut argv, cfg, request, session_id, prompt);
    if matches!(cfg.prompt_via, PromptVia::Args) {
        let already = template.iter().any(|a| contains_prompt_ref(a));
        if !already {
            argv.push(prompt.to_string());
        }
    }
    if matches!(cfg.prompt_via, PromptVia::Stdin) {
        strip_prompt_args(&mut argv);
    }
    argv
}

fn prompt_flag_without_value(argv: &[String]) -> Option<usize> {
    argv.iter()
        .rposition(|arg| matches!(arg.as_str(), "-p" | "--prompt"))
        .filter(|pos| *pos + 1 == argv.len())
}

fn contains_prompt_ref(input: &str) -> bool {
    input.contains("{prompt") || input.contains("{loom_envelope}")
}

fn prompt_ref_insert_pos(template: &[String], argv: &[String]) -> Option<usize> {
    let pos = template.iter().position(|arg| contains_prompt_ref(arg))?;
    if pos > 0
        && matches!(
            argv[pos - 1].as_str(),
            "-p" | "--prompt" | "--append-system-prompt" | "--system-prompt"
        )
    {
        Some(pos - 1)
    } else {
        Some(pos)
    }
}

fn expand_template(
    input: &str,
    cfg: &CommandConfig,
    request: &AdapterPrompt,
    session_id: Option<&str>,
    prompt: &str,
) -> String {
    expand_template_inner(input, cfg, request, session_id, prompt, false)
}

/// Like [`expand_template`] but preserves `{prompt}` / `{prompt.full}` even
/// when [`PromptVia::Stdin`] is set.  Used when expanding the stdin body
/// template so the prompt content actually reaches the child process.
fn expand_stdin_template(
    input: &str,
    cfg: &CommandConfig,
    request: &AdapterPrompt,
    session_id: Option<&str>,
    prompt: &str,
) -> String {
    expand_template_inner(input, cfg, request, session_id, prompt, true)
}

fn expand_template_inner(
    input: &str,
    cfg: &CommandConfig,
    request: &AdapterPrompt,
    session_id: Option<&str>,
    prompt: &str,
    for_stdin: bool,
) -> String {
    let scope_kind = match request.scope.kind {
        proto::types::ScopeKind::Thread => "thread",
        proto::types::ScopeKind::Channel => "channel",
    };
    let strip_prompt = !for_stdin && matches!(cfg.prompt_via, PromptVia::Stdin);
    let mut out = input
        .replace("{actor.id}", &cfg.actor_id)
        .replace("{scope.id}", &request.scope.id)
        .replace("{scope.kind}", scope_kind)
        .replace("{model}", active_model(request).as_deref().unwrap_or(""))
        .replace("{prompt}", if strip_prompt { "" } else { prompt })
        .replace(
            "{prompt.full}",
            if strip_prompt {
                ""
            } else {
                request
                    .outputs
                    .get("full")
                    .map(String::as_str)
                    .unwrap_or(prompt)
            },
        )
        .replace(
            "{loom_envelope}",
            request
                .outputs
                .get("full")
                .map(String::as_str)
                .unwrap_or(prompt),
        );
    if let Some(sid) = session_id {
        out = out
            .replace("{session_id}", sid)
            .replace("{session.id}", sid);
    }
    for (name, value) in &request.outputs {
        out = out.replace(&format!("{{prompt.{name}}}"), value);
    }
    for (key, value) in &request.template_vars {
        out = out.replace(&format!("{{{key}}}"), value);
    }
    out
}

fn active_model(request: &AdapterPrompt) -> Option<String> {
    request
        .model
        .as_ref()
        .map(|model| model.trim())
        .filter(|model| !model.is_empty())
        .map(ToOwned::to_owned)
}

fn append_model_args(
    argv: &mut Vec<String>,
    cfg: &CommandConfig,
    request: &AdapterPrompt,
    session_id: Option<&str>,
    prompt: &str,
) {
    if active_model(request).is_none() {
        return;
    }
    argv.extend(
        cfg.model_args
            .iter()
            .map(|arg| expand_template(arg, cfg, request, session_id, prompt)),
    );
}

fn expanded_env(
    cfg: &CommandConfig,
    request: &AdapterPrompt,
    session_id: Option<&str>,
) -> BTreeMap<String, String> {
    let mut env: BTreeMap<String, String> = cfg
        .env
        .iter()
        .map(|(k, v)| {
            (
                k.clone(),
                expand_template(v, cfg, request, session_id, &request.content),
            )
        })
        .collect();
    for (k, v) in &request.env {
        env.entry(k.clone()).or_insert_with(|| v.clone());
    }
    if let Some(model) = active_model(request) {
        env.entry("LOOM_AGENT_MODEL".into()).or_insert(model);
    }
    env
}

// Silences the `Arc` import in modules that wrap CommandAdapter behind
// `Arc<dyn Adapter>` without using anything else from this module yet.
#[allow(dead_code)]
fn _arc_keepalive(_: Arc<CommandAdapter>) {}

#[cfg(test)]
mod tests {
    use super::*;
    use proto::methods::ProviderConditionalArgSpec;
    use proto::methods::{ProviderDecoderEventSpec, ProviderJsonlReduceSpec};
    use proto::types::ScopeKind;

    fn cfg() -> CommandConfig {
        CommandConfig {
            actor_id: "actor_demo".into(),
            command: "echo".into(),
            args: vec!["-n".into()],
            arg_specs: Vec::new(),
            env: BTreeMap::new(),
            model_args: Vec::new(),
            session_id_source: None,
            session_scope: None,
            first_run_capture: None,
            resume_args: None,
            resume_arg_specs: Vec::new(),
            output_format: CommandOutputFormat::Text,
            decoder: None,
            stderr_decoder: None,
            prompt_via: PromptVia::Args,
            prompt: None,
            stdin_template: None,
            sessions_dir: std::env::temp_dir().join("loom-test-sessions"),
            timeout_ms: None,
            idle_timeout_ms: None,
            command_signature: "sha256:test".into(),
        }
    }

    fn scope() -> ScopeRef {
        ScopeRef {
            kind: ScopeKind::Thread,
            id: "thr_xyz".into(),
        }
    }

    fn named_scope(kind: ScopeKind, id: &str) -> ScopeRef {
        ScopeRef {
            kind,
            id: id.into(),
        }
    }

    fn prompt(content: &str) -> AdapterPrompt {
        AdapterPrompt {
            scope: scope(),
            content: content.into(),
            parts: Vec::new(),
            outputs: BTreeMap::from([("full".into(), content.into())]),
            model: None,
            cwd: std::env::temp_dir(),
            env: BTreeMap::new(),
            template_vars: BTreeMap::new(),
        }
    }

    #[test]
    fn command_signature_includes_prompt_model() {
        let cfg = cfg();
        let mut request = prompt("hello");
        assert_eq!(
            command_signature_for_prompt(&cfg, &request),
            cfg.command_signature
        );

        request.model = Some("model_a".into());
        let model_a = command_signature_for_prompt(&cfg, &request);
        request.model = Some("model_b".into());
        let model_b = command_signature_for_prompt(&cfg, &request);

        assert_ne!(model_a, cfg.command_signature);
        assert_ne!(model_a, model_b);

        request.model = None;
        request
            .template_vars
            .insert("reasoningEffort".into(), "high".into());
        let high = command_signature_for_prompt(&cfg, &request);
        request
            .template_vars
            .insert("reasoningEffort".into(), "low".into());
        let low = command_signature_for_prompt(&cfg, &request);

        assert_ne!(high, cfg.command_signature);
        assert_ne!(high, low);
    }

    #[test]
    fn argv_inserts_model_args_before_prompt_argument() {
        let mut cfg = cfg();
        cfg.model_args = vec!["--model".into(), "{model}".into()];
        let mut request = prompt("hello");
        request.model = Some("model_a".into());

        let argv = expand_first_run_argv(&cfg, &request, None, "hello");

        assert_eq!(
            argv,
            vec![
                "-n".to_string(),
                "--model".to_string(),
                "model_a".to_string(),
                "hello".to_string(),
            ]
        );
    }

    #[test]
    fn argv_inserts_model_args_before_prompt_flag_value() {
        let mut cfg = cfg();
        cfg.args = vec!["--json".into(), "-p".into()];
        cfg.model_args = vec!["--model".into(), "{model}".into()];
        let mut request = prompt("hello");
        request.model = Some("model_a".into());

        let argv = expand_first_run_argv(&cfg, &request, None, "hello");

        assert_eq!(
            argv,
            vec![
                "--json".to_string(),
                "--model".to_string(),
                "model_a".to_string(),
                "-p".to_string(),
                "hello".to_string(),
            ]
        );
    }

    #[test]
    fn argv_omits_model_args_without_selected_model() {
        let mut cfg = cfg();
        cfg.model_args = vec!["--model".into(), "{model}".into()];

        let argv = expand_first_run_argv(&cfg, &prompt("hello"), None, "hello");

        assert_eq!(argv, vec!["-n".to_string(), "hello".to_string()]);
    }

    #[test]
    fn provider_arg_specs_expand_conditionals_in_manifest_order() {
        let mut cfg = cfg();
        cfg.args = vec!["legacy".into()];
        cfg.model_args = vec!["--legacy-model".into(), "{model}".into()];
        cfg.arg_specs = vec![
            ProviderArgSpec::Literal("run".into()),
            ProviderArgSpec::Conditional(ProviderConditionalArgSpec {
                when: "model".into(),
                args: vec![
                    ProviderArgSpec::Literal("--model".into()),
                    ProviderArgSpec::Literal("{model}".into()),
                ],
            }),
            ProviderArgSpec::Conditional(ProviderConditionalArgSpec {
                when: "reasoningEffort".into(),
                args: vec![
                    ProviderArgSpec::Literal("--effort".into()),
                    ProviderArgSpec::Literal("{reasoningEffort}".into()),
                ],
            }),
            ProviderArgSpec::Literal("{prompt.full}".into()),
        ];
        let mut request = prompt("fallback prompt");
        request.model = Some("model_a".into());
        request
            .template_vars
            .insert("reasoningEffort".into(), "high".into());
        request
            .outputs
            .insert("full".into(), "rendered full prompt".into());

        let argv = expand_first_run_argv(&cfg, &request, None, "fallback prompt");

        assert_eq!(
            argv,
            vec![
                "run".to_string(),
                "--model".to_string(),
                "model_a".to_string(),
                "--effort".to_string(),
                "high".to_string(),
                "rendered full prompt".to_string(),
            ]
        );
    }

    #[test]
    fn provider_arg_specs_omit_false_conditionals_without_implicit_prompt_append() {
        let mut cfg = cfg();
        cfg.arg_specs = vec![
            ProviderArgSpec::Literal("run".into()),
            ProviderArgSpec::Conditional(ProviderConditionalArgSpec {
                when: "model".into(),
                args: vec![
                    ProviderArgSpec::Literal("--model".into()),
                    ProviderArgSpec::Literal("{model}".into()),
                ],
            }),
            ProviderArgSpec::Literal("{prompt.full}".into()),
        ];
        let mut request = prompt("fallback prompt");
        request.outputs.insert("full".into(), "full".into());

        let argv = expand_first_run_argv(&cfg, &request, None, "fallback prompt");

        assert_eq!(argv, vec!["run".to_string(), "full".to_string()]);
    }

    #[test]
    fn json_path_top_level_string() {
        let v: Value = serde_json::from_str(r#"{"session_id":"abc"}"#).unwrap();
        assert_eq!(json_path_lookup(&v, ".session_id"), Some("abc".into()));
    }

    #[test]
    fn json_path_nested_with_index() {
        let v: Value =
            serde_json::from_str(r#"{"items":[{"id":"first"},{"id":"second"}]}"#).unwrap();
        assert_eq!(json_path_lookup(&v, ".items[1].id"), Some("second".into()));
    }

    #[test]
    fn json_path_wildcard_returns_values_in_order() {
        let v: Value =
            serde_json::from_str(r#"{"items":[{"id":"first"},{"id":"second"}]}"#).unwrap();
        assert_eq!(
            json_path_lookup_strings(&v, "$.items[*].id"),
            vec!["first".to_string(), "second".to_string()]
        );
        assert_eq!(json_path_lookup(&v, "$.items[*].id"), Some("first".into()));
    }

    #[test]
    fn json_path_missing_returns_none() {
        let v: Value = serde_json::from_str(r#"{"a":1}"#).unwrap();
        assert!(json_path_lookup(&v, ".b").is_none());
    }

    #[test]
    fn provider_jsonl_events_emit_text_status_tool_error_and_finish() {
        let decoder = ProviderDecoderSpec {
            format: "jsonl".into(),
            name: None,
            events: vec![
                ProviderDecoderEventSpec {
                    when: Some(ProviderJsonConditionSpec {
                        path: Some("$.type".into()),
                        equals: Some(Value::String("text".into())),
                        ..Default::default()
                    }),
                    emit: ProviderDecoderEmitSpec {
                        emit_type: "text".into(),
                        text: Some("$.text".into()),
                        partial: Some(false),
                        ..Default::default()
                    },
                },
                ProviderDecoderEventSpec {
                    when: Some(ProviderJsonConditionSpec {
                        path: Some("$.type".into()),
                        equals: Some(Value::String("status".into())),
                        ..Default::default()
                    }),
                    emit: ProviderDecoderEmitSpec {
                        emit_type: "status".into(),
                        status: Some("$.status".into()),
                        ..Default::default()
                    },
                },
                ProviderDecoderEventSpec {
                    when: Some(ProviderJsonConditionSpec {
                        path: Some("$.type".into()),
                        equals: Some(Value::String("tool".into())),
                        ..Default::default()
                    }),
                    emit: ProviderDecoderEmitSpec {
                        emit_type: "tool_use".into(),
                        tool_name: Some("$.name".into()),
                        input: Some("$.input".into()),
                        ..Default::default()
                    },
                },
                ProviderDecoderEventSpec {
                    when: Some(ProviderJsonConditionSpec {
                        path: Some("$.type".into()),
                        equals: Some(Value::String("error".into())),
                        ..Default::default()
                    }),
                    emit: ProviderDecoderEmitSpec {
                        emit_type: "error".into(),
                        message: Some("$.message".into()),
                        ..Default::default()
                    },
                },
                ProviderDecoderEventSpec {
                    when: Some(ProviderJsonConditionSpec {
                        path: Some("$.type".into()),
                        equals: Some(Value::String("done".into())),
                        ..Default::default()
                    }),
                    emit: ProviderDecoderEmitSpec {
                        emit_type: "finish".into(),
                        success: Some(true),
                        summary: Some("$.summary".into()),
                        ..Default::default()
                    },
                },
            ],
            reduce: None,
            capture: None,
        };
        let (tx, rx) = mpsc::unbounded_channel();
        let scope = scope();

        let text = translate_decoder_event_line(
            Some(&decoder),
            r#"{"type":"text","text":"hello"}"#,
            &scope,
            &tx,
        )
        .expect("text events");
        let status = translate_decoder_event_line(
            Some(&decoder),
            r#"{"type":"status","status":"working"}"#,
            &scope,
            &tx,
        )
        .expect("status events");
        let tool = translate_decoder_event_line(
            Some(&decoder),
            r#"{"type":"tool","name":"shell","input":{"cmd":"ls"}}"#,
            &scope,
            &tx,
        )
        .expect("tool events");
        let error = translate_decoder_event_line(
            Some(&decoder),
            r#"{"type":"error","message":"bad"}"#,
            &scope,
            &tx,
        )
        .expect("error events");
        let finish = translate_decoder_event_line(
            Some(&decoder),
            r#"{"type":"done","summary":"ok"}"#,
            &scope,
            &tx,
        )
        .expect("finish events");

        assert!(text.emitted_text);
        assert!(!status.emitted_text && !tool.emitted_text && !error.emitted_text);
        assert!(finish.emitted_finish);

        let mut rx = rx;
        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }
        assert_eq!(events.len(), 5);
        assert!(matches!(
            &events[0],
            AdapterEvent::Text {
                content,
                is_partial: false,
                ..
            } if content == "hello"
        ));
        assert!(matches!(
            &events[1],
            AdapterEvent::StatusChange { status, .. } if status == "working"
        ));
        assert!(matches!(
            &events[2],
            AdapterEvent::ToolUse {
                tool_name,
                input,
                ..
            } if tool_name == "shell" && input.pointer("/cmd").and_then(Value::as_str) == Some("ls")
        ));
        assert!(matches!(
            &events[3],
            AdapterEvent::Error { message, .. } if message == "bad"
        ));
        assert!(matches!(
            &events[4],
            AdapterEvent::Finished {
                success: true,
                summary,
                ..
            } if summary == "ok"
        ));
    }

    #[test]
    fn provider_decoder_line_decodes_runtime_event_before_adapter_mapping() {
        let decoder = ProviderDecoderSpec {
            format: "jsonl".into(),
            name: None,
            events: vec![ProviderDecoderEventSpec {
                when: Some(ProviderJsonConditionSpec {
                    path: Some("$.type".into()),
                    equals: Some(Value::String("text".into())),
                    ..Default::default()
                }),
                emit: ProviderDecoderEmitSpec {
                    emit_type: "text".into(),
                    text: Some("$.text".into()),
                    partial: Some(false),
                    ..Default::default()
                },
            }],
            reduce: None,
            capture: None,
        };

        let events =
            decode_decoder_event_line(&decoder, r#"{"type":"text","text":"runtime event"}"#)
                .expect("runtime events");
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            ProviderRuntimeEvent::Text {
                content,
                is_partial: false,
            } if content == "runtime event"
        ));

        let (tx, mut rx) = mpsc::unbounded_channel();
        let emitted = emit_provider_runtime_events(events, &scope(), &tx);
        assert!(emitted.emitted_text);
        assert!(matches!(
            rx.try_recv().expect("adapter event"),
            AdapterEvent::Text { content, is_partial: false, .. } if content == "runtime event"
        ));
    }

    #[test]
    fn builtin_claude_decoder_produces_runtime_events_before_adapter_mapping() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hello"},{"type":"tool_use","name":"shell","input":{"cmd":"pwd"}}]}}"#;

        let events = decode_claude_stream_line_events(line);
        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0],
            ProviderRuntimeEvent::Text {
                content,
                is_partial: false,
            } if content == "hello"
        ));
        assert!(matches!(
            &events[1],
            ProviderRuntimeEvent::ToolUse { tool_name, input }
                if tool_name == "shell" && input.pointer("/cmd").and_then(Value::as_str) == Some("pwd")
        ));

        let (tx, mut rx) = mpsc::unbounded_channel();
        let emitted = emit_provider_runtime_events(events, &scope(), &tx);
        assert!(emitted.emitted_text);
        assert!(matches!(
            rx.try_recv().expect("text event"),
            AdapterEvent::Text { content, is_partial: false, .. } if content == "hello"
        ));
        assert!(matches!(
            rx.try_recv().expect("tool event"),
            AdapterEvent::ToolUse { tool_name, input, .. }
                if tool_name == "shell" && input.pointer("/cmd").and_then(Value::as_str) == Some("pwd")
        ));
    }

    #[test]
    fn provider_builtin_decoder_streams_without_legacy_output_format() {
        let mut cfg = cfg();
        cfg.output_format = CommandOutputFormat::Text;
        cfg.decoder = Some(ProviderDecoderSpec {
            format: "builtin".into(),
            name: Some("claude_stream_json".into()),
            events: Vec::new(),
            reduce: None,
            capture: None,
        });
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut collected = String::new();

        let events = collect_stdout_line(
            &cfg,
            &prompt("ignored"),
            &tx,
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"from decoder"}]}}"#,
            &mut collected,
            &mut None,
        );

        assert!(events.emitted_text);
        match rx.try_recv().expect("text event") {
            AdapterEvent::Text {
                content,
                is_partial,
                ..
            } => {
                assert_eq!(content, "from decoder");
                assert!(!is_partial);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn provider_decoder_without_events_does_not_fall_back_to_legacy_ndjson() {
        let mut cfg = cfg();
        cfg.output_format = CommandOutputFormat::NdjsonLines;
        cfg.decoder = Some(ProviderDecoderSpec {
            format: "jsonl".into(),
            name: None,
            events: Vec::new(),
            reduce: Some(ProviderJsonlReduceSpec {
                final_text: Some(ProviderJsonlTextReducerSpec {
                    mode: "lastNonEmpty".into(),
                    path: "$.text".into(),
                    when: None,
                    fallback: None,
                }),
            }),
            capture: None,
        });
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut collected = String::new();

        let events = collect_stdout_line(
            &cfg,
            &prompt("ignored"),
            &tx,
            r#"{"type":"text","text":"legacy ndjson would stream this"}"#,
            &mut collected,
            &mut None,
        );

        assert!(!events.emitted_text);
        assert!(rx.try_recv().is_err());
        assert!(collected.contains("legacy ndjson would stream this"));
        assert_eq!(
            extract_configured_decoder_final_text(&cfg, &collected, ""),
            Some("legacy ndjson would stream this".into())
        );
    }

    #[test]
    fn extract_session_id_picks_last_matching_line() {
        let stdout = "{\"type\":\"system\",\"session_id\":\"first\"}\n\
                      {\"type\":\"result\",\"session_id\":\"second\"}\n";
        assert_eq!(
            extract_json_path(stdout, ".session_id"),
            Some("second".into())
        );
    }

    #[test]
    fn decoder_capture_session_picks_last_matching_jsonl_value() {
        let decoder = ProviderDecoderSpec {
            format: "jsonl".into(),
            name: None,
            events: Vec::new(),
            reduce: None,
            capture: Some(proto::methods::ProviderDecoderCaptureSpec {
                session: Some(ProviderJsonlTextReducerSpec {
                    mode: "lastNonEmpty".into(),
                    path: "$.session_id".into(),
                    when: Some(ProviderJsonConditionSpec {
                        path: Some("$.type".into()),
                        in_values: Some(vec![
                            Value::String("system".into()),
                            Value::String("result".into()),
                        ]),
                        ..Default::default()
                    }),
                    fallback: None,
                }),
            }),
        };
        let stdout = "{\"type\":\"debug\",\"session_id\":\"ignored\"}\n\
                      {\"type\":\"system\",\"session_id\":\"first\"}\n\
                      {\"type\":\"result\",\"session_id\":\"second\"}\n";

        assert_eq!(
            capture_decoder_session_id(Some(&decoder), stdout),
            Some("second".into())
        );
    }

    #[test]
    fn copilot_json_final_text_picks_last_root_assistant_message() {
        let stdout = r#"{"type":"assistant.message","data":{"messageId":"m1","content":"I will inspect it.","toolRequests":[{"name":"shell"}]}}
{"type":"tool.completed","data":{"toolCallId":"t1","content":"done"}}
{"type":"assistant.message","data":{"messageId":"m2","content":"Final answer\n"}}
"#;

        assert_eq!(
            extract_copilot_json_final_text(stdout),
            Some("Final answer".into())
        );
    }

    #[test]
    fn copilot_json_final_text_ignores_subagent_messages() {
        let stdout = r#"{"agentId":"sub_1","type":"assistant.message","data":{"messageId":"m1","content":"Sub-agent detail"}}
{"type":"assistant.message","data":{"messageId":"m2","content":"Root answer"}}
"#;

        assert_eq!(
            extract_copilot_json_final_text(stdout),
            Some("Root answer".into())
        );
    }

    #[test]
    fn copilot_json_final_text_ignores_hidden_phases() {
        let stdout = r#"{"type":"assistant.message","data":{"messageId":"m1","phase":"thinking","content":"Private reasoning"}}
{"agentId":null,"type":"assistant.message","data":{"messageId":"m2","content":"Visible answer"}}
"#;

        assert_eq!(
            extract_copilot_json_final_text(stdout),
            Some("Visible answer".into())
        );
    }

    #[test]
    fn copilot_json_final_text_falls_back_to_deltas() {
        let stdout = r#"{"type":"assistant.message_delta","data":{"messageId":"m1","deltaContent":"hello"}}
{"type":"assistant.message_delta","data":{"messageId":"m1","deltaContent":" world"}}
"#;

        assert_eq!(
            extract_copilot_json_final_text(stdout),
            Some("hello world".into())
        );
    }

    #[test]
    fn provider_jsonl_reducer_matches_copilot_final_text_rules() {
        let decoder = crate::provider::builtin_provider_manifests()
            .into_iter()
            .find(|manifest| manifest.id == "copilot")
            .and_then(|manifest| manifest.modes.get("print").map(|mode| mode.stdout.clone()))
            .expect("copilot decoder");
        let stdout = r#"{"agentId":"sub_1","type":"assistant.message","data":{"messageId":"m1","content":"Sub-agent detail"}}
{"type":"assistant.message","data":{"messageId":"m2","phase":"thinking","content":"Private reasoning"}}
{"type":"assistant.message","data":{"messageId":"m3","content":"Root answer"}}
"#;

        assert_eq!(
            extract_decoder_final_text(Some(&decoder), stdout),
            Some("Root answer".into())
        );
    }

    #[test]
    fn provider_jsonl_reducer_uses_fallback_delta_text() {
        let decoder = crate::provider::builtin_provider_manifests()
            .into_iter()
            .find(|manifest| manifest.id == "copilot")
            .and_then(|manifest| manifest.modes.get("print").map(|mode| mode.stdout.clone()))
            .expect("copilot decoder");
        let stdout = r#"{"type":"assistant.message_delta","data":{"messageId":"m1","deltaContent":"hello"}}
{"type":"assistant.message_delta","data":{"messageId":"m1","deltaContent":" world"}}
"#;

        assert_eq!(
            extract_decoder_final_text(Some(&decoder), stdout),
            Some("hello world".into())
        );
    }

    #[test]
    fn provider_jsonl_reducer_concats_wildcard_values() {
        let reducer = ProviderJsonlTextReducerSpec {
            mode: "concat".into(),
            path: "$.items[*].text".into(),
            when: None,
            fallback: None,
        };
        let stdout = r#"{"items":[{"text":"hello "},{"text":"world"}]}"#;

        assert_eq!(
            reduce_text_with_fallback("jsonl", stdout, &reducer),
            Some("hello world".into())
        );
    }

    #[test]
    fn provider_json_decoder_reads_pretty_json_final_text() {
        let decoder = ProviderDecoderSpec {
            format: "json".into(),
            name: None,
            events: Vec::new(),
            reduce: Some(ProviderJsonlReduceSpec {
                final_text: Some(ProviderJsonlTextReducerSpec {
                    mode: "lastNonEmpty".into(),
                    path: "$.result.message".into(),
                    when: None,
                    fallback: None,
                }),
            }),
            capture: None,
        };
        let stdout = r#"{
  "result": {
    "message": "JSON answer"
  }
}"#;

        assert_eq!(
            extract_decoder_final_text(Some(&decoder), stdout),
            Some("JSON answer".into())
        );
    }

    #[test]
    fn provider_json_decoder_captures_session_from_pretty_json() {
        let decoder = ProviderDecoderSpec {
            format: "json".into(),
            name: None,
            events: Vec::new(),
            reduce: None,
            capture: Some(proto::methods::ProviderDecoderCaptureSpec {
                session: Some(ProviderJsonlTextReducerSpec {
                    mode: "lastNonEmpty".into(),
                    path: "$.meta.session_id".into(),
                    when: None,
                    fallback: None,
                }),
            }),
        };
        let stdout = r#"{
  "meta": {
    "session_id": "sid_json"
  }
}"#;

        assert_eq!(
            capture_decoder_session_id(Some(&decoder), stdout),
            Some("sid_json".into())
        );
    }

    #[test]
    fn codex_json_final_text_picks_task_complete_message() {
        let stdout = r#"{"type":"session_configured","session_id":"s1"}
{"type":"agent_message_content_delta","delta":"draft"}
{"type":"task_complete","last_agent_message":"Final answer\n"}
"#;

        assert_eq!(
            extract_codex_json_final_text(stdout),
            Some("Final answer".into())
        );
    }

    #[test]
    fn codex_json_final_text_reads_completed_message_item() {
        let stdout = r#"{"type":"item_completed","item":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Hello"},{"type":"output_text","text":" world"}]}}
"#;

        assert_eq!(
            extract_codex_json_final_text(stdout),
            Some("Hello world".into())
        );
    }

    #[test]
    fn codex_json_final_text_reads_codex_0130_item_completed() {
        let stdout = r#"Reading additional input from stdin...
{"type":"thread.started","thread_id":"019e1a84-3d53-7512-b9ef-c7cfb437ae6b"}
{"type":"turn.started"}
{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"OK"}}
{"type":"turn.completed","usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1,"reasoning_output_tokens":0}}
"#;

        assert_eq!(extract_codex_json_final_text(stdout), Some("OK".into()));
    }

    #[test]
    fn codex_json_final_text_reads_nested_agent_message() {
        let stdout = r#"{"type":"agent_message","message":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Nested answer"}]}}
"#;

        assert_eq!(
            extract_codex_json_final_text(stdout),
            Some("Nested answer".into())
        );
    }

    #[test]
    fn codex_json_final_text_falls_back_to_deltas() {
        let stdout = r#"{"type":"agent_message_content_delta","delta":"hello"}
{"type":"agent_message_content_delta","delta":" world"}
"#;

        assert_eq!(
            extract_codex_json_final_text(stdout),
            Some("hello world".into())
        );
    }

    #[test]
    fn claude_stream_assistant_text_is_a_complete_message() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"I'll inspect it."}]}}"#;

        assert!(translate_claude_stream_line(line, &scope(), &tx));

        match rx.try_recv().expect("text event") {
            AdapterEvent::Text {
                content,
                is_partial,
                ..
            } => {
                assert_eq!(content, "I'll inspect it.");
                assert!(!is_partial);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn codex_stream_item_completed_emits_complete_messages() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let first = r#"{"type":"item.completed","item":{"id":"item_1","type":"agent_message","text":"First"}}"#;
        let final_msg = r#"{"type":"item.completed","item":{"id":"item_2","type":"agent_message","text":"Final"}}"#;

        assert!(translate_codex_event_line(first, &scope(), &tx));
        assert!(translate_codex_event_line(final_msg, &scope(), &tx));

        let mut messages = Vec::new();
        while let Ok(event) = rx.try_recv() {
            match event {
                AdapterEvent::Text {
                    content,
                    is_partial,
                    ..
                } => {
                    assert!(!is_partial);
                    messages.push(content);
                }
                other => panic!("unexpected event: {other:?}"),
            }
        }
        assert_eq!(messages, vec!["First", "Final"]);
    }

    #[cfg(unix)]
    #[test]
    fn codex_stream_does_not_duplicate_completed_messages_at_process_end() {
        let mut cfg = cfg();
        cfg.command = "/bin/sh".into();
        cfg.args = vec![
            "-c".into(),
            "printf '%s\\n' \
             '{\"type\":\"item.completed\",\"item\":{\"id\":\"item_1\",\"type\":\"agent_message\",\"text\":\"First\"}}' \
             '{\"type\":\"item.completed\",\"item\":{\"id\":\"item_2\",\"type\":\"agent_message\",\"text\":\"Final\"}}'"
                .into(),
        ];
        cfg.output_format = CommandOutputFormat::CodexStreamJson;
        cfg.prompt_via = PromptVia::Stdin;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let slot = Arc::new(Mutex::new(InFlight::default()));

        let outcome = spawn_and_collect(&cfg, &prompt("ignored"), &cfg.args, None, &tx, &slot)
            .expect("spawn sh");

        assert_eq!(outcome.exit_code, 0);
        let mut messages = Vec::new();
        while let Ok(event) = rx.try_recv() {
            if let AdapterEvent::Text {
                content,
                is_partial,
                ..
            } = event
            {
                assert!(!is_partial);
                messages.push(content);
            }
        }
        assert_eq!(messages, vec!["First", "Final"]);
    }

    #[test]
    fn template_expands_scope_and_session_and_prompt() {
        let cfg = cfg();
        let request = prompt("hello world");
        let out = expand_template(
            "--resume {session_id} --scope {scope.id} -- {prompt}",
            &cfg,
            &request,
            Some("sid_42"),
            "hello world",
        );
        assert_eq!(out, "--resume sid_42 --scope thr_xyz -- hello world");
    }

    #[test]
    fn first_run_argv_appends_prompt_when_args_mode() {
        let cfg = cfg();
        let request = prompt("what time is it");
        let argv = expand_first_run_argv(&cfg, &request, None, "what time is it");
        assert_eq!(argv, vec!["-n", "what time is it"]);
    }

    #[test]
    fn first_run_argv_does_not_double_append_when_template_has_prompt() {
        let mut cfg = cfg();
        cfg.args = vec!["--input".into(), "{prompt}".into()];
        let request = prompt("hi");
        let argv = expand_first_run_argv(&cfg, &request, None, "hi");
        assert_eq!(argv, vec!["--input", "hi"]);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn args_prompt_does_not_pipe_stdin_to_child() {
        let mut cfg = cfg();
        cfg.command = "sh".into();
        cfg.args = vec![
            "-c".into(),
            "if read -r _; then echo stdin-open; else echo stdin-closed; fi".into(),
        ];
        cfg.prompt_via = PromptVia::Args;
        let (tx, _rx) = mpsc::unbounded_channel();
        let slot = Arc::new(Mutex::new(InFlight::default()));
        let outcome = spawn_and_collect(&cfg, &prompt("ignored"), &cfg.args, None, &tx, &slot)
            .expect("spawn sh");

        assert_eq!(outcome.stdout.trim(), "stdin-closed");
    }

    #[cfg(unix)]
    #[test]
    fn timeout_kills_running_child_and_reports_failed_turn() {
        let mut cfg = cfg();
        cfg.command = "sh".into();
        cfg.args = vec!["-c".into(), "sleep 30".into()];
        cfg.prompt_via = PromptVia::Stdin;
        cfg.timeout_ms = Some(100);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let slot = Arc::new(Mutex::new(InFlight::default()));

        let started = std::time::Instant::now();
        let outcome = spawn_and_collect(&cfg, &prompt("ignored"), &cfg.args, None, &tx, &slot)
            .expect("spawn sh");

        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "timeout did not reap the child promptly"
        );
        assert_ne!(outcome.exit_code, 0);

        let mut got_timeout = false;
        while let Ok(event) = rx.try_recv() {
            if let AdapterEvent::Finished {
                success, summary, ..
            } = event
            {
                assert!(!success);
                assert_eq!(summary, "timed out after 100ms");
                got_timeout = true;
                break;
            }
        }
        assert!(got_timeout, "missing timeout Finished event");
    }

    #[cfg(unix)]
    #[test]
    fn idle_timeout_kills_quiet_child_and_reports_failed_turn() {
        let mut cfg = cfg();
        cfg.command = "sh".into();
        cfg.args = vec!["-c".into(), "echo started; sleep 30".into()];
        cfg.prompt_via = PromptVia::Stdin;
        cfg.idle_timeout_ms = Some(500);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let slot = Arc::new(Mutex::new(InFlight::default()));

        let started = std::time::Instant::now();
        let outcome = spawn_and_collect(&cfg, &prompt("ignored"), &cfg.args, None, &tx, &slot)
            .expect("spawn sh");

        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "idle timeout did not reap the child promptly"
        );
        assert_ne!(outcome.exit_code, 0);
        assert!(outcome.stdout.contains("started"));

        let mut got_idle_timeout = false;
        while let Ok(event) = rx.try_recv() {
            if let AdapterEvent::Finished {
                success, summary, ..
            } = event
            {
                assert!(!success);
                assert_eq!(summary, "idle timed out after 500ms");
                got_idle_timeout = true;
                break;
            }
        }
        assert!(got_idle_timeout, "missing idle timeout Finished event");
    }

    #[test]
    fn looks_like_session_lost_matches_common_phrases() {
        assert!(looks_like_session_lost("error: Session not found", ""));
        assert!(looks_like_session_lost("UNKNOWN session abc", ""));
        assert!(looks_like_session_lost(
            "",
            r#"{"errors":["No conversation found with session ID: abc"]}"#
        ));
        assert!(!looks_like_session_lost("everything is fine", ""));
    }

    #[test]
    fn looks_like_signed_thinking_replay_error_matches_claude_400() {
        let stderr = "API Error: 400 messages.7.content.3: thinking or \
                      redacted_thinking blocks in the latest assistant message \
                      cannot be modified. These blocks must remain as they were \
                      in the original response.";

        assert!(looks_like_signed_thinking_replay_error(stderr, ""));
        assert!(!looks_like_signed_thinking_replay_error(
            "API Error: 400 unrelated provider error",
            ""
        ));
    }

    #[test]
    fn run_slot_guard_rejects_concurrent_prompt_for_same_scope() {
        let slot = Arc::new(Mutex::new(InFlight::default()));
        let scope = scope();
        let guard = RunSlotGuard::acquire(slot.clone(), &scope).expect("first acquire");

        assert!(slot.lock().running);
        assert!(RunSlotGuard::acquire(slot.clone(), &scope).is_err());

        drop(guard);
        assert!(!slot.lock().running);
    }

    #[test]
    fn session_path_distinguishes_thread_and_channel() {
        let cfg = cfg();
        let thread = session_path(&cfg, &named_scope(ScopeKind::Thread, "same")).unwrap();
        let channel = session_path(&cfg, &named_scope(ScopeKind::Channel, "same")).unwrap();

        assert_ne!(thread, channel);
        assert!(thread.ends_with("thread-same.json"));
        assert!(channel.ends_with("channel-same.json"));
    }

    #[test]
    fn session_scope_actor_reuses_one_path_across_scopes() {
        let mut cfg = cfg();
        cfg.session_scope = Some("actor".into());
        let thread = session_path(&cfg, &named_scope(ScopeKind::Thread, "thread-a")).unwrap();
        let channel = session_path(&cfg, &named_scope(ScopeKind::Channel, "channel-b")).unwrap();

        assert_eq!(thread, channel);
        assert!(thread.ends_with("actor.json"));
    }

    #[test]
    fn session_scope_turn_does_not_persist() {
        let mut cfg = cfg();
        let root = std::env::temp_dir().join(format!("loom-command-{}", uuid::Uuid::new_v4()));
        cfg.sessions_dir = root.join("sessions");
        cfg.session_scope = Some("turn".into());
        let scope = named_scope(ScopeKind::Channel, "chan");

        assert!(session_path(&cfg, &scope).is_none());
        save_session(&cfg, &scope, "sid", "sig").unwrap();
        assert!(load_session(&cfg, &scope).is_none());
        assert!(!root.exists());
    }

    #[test]
    fn save_session_preserves_created_at_and_updates_last_used() {
        let mut cfg = cfg();
        let root = std::env::temp_dir().join(format!("loom-command-{}", uuid::Uuid::new_v4()));
        cfg.sessions_dir = root.join("sessions");
        let scope = named_scope(ScopeKind::Channel, "chan");

        save_session(&cfg, &scope, "sid", "sig").unwrap();
        let first = load_session(&cfg, &scope).expect("first session");
        std::thread::sleep(std::time::Duration::from_millis(20));
        save_session(&cfg, &scope, "sid", "sig").unwrap();
        let second = load_session(&cfg, &scope).expect("second session");

        assert_eq!(second.scope.kind, ScopeKind::Channel);
        assert_eq!(second.created_at, first.created_at);
        assert!(second.last_used_at > first.last_used_at);
        std::fs::remove_dir_all(root).ok();
    }

    #[cfg(unix)]
    #[test]
    fn run_prompt_saves_decoder_captured_provider_session() {
        let mut cfg = cfg();
        let root = std::env::temp_dir().join(format!("loom-command-{}", uuid::Uuid::new_v4()));
        cfg.sessions_dir = root.join("sessions");
        cfg.command = "/bin/sh".into();
        cfg.args = vec![
            "-c".into(),
            "printf '%s\\n' '{\"type\":\"system\",\"session_id\":\"sid_decoder\"}'".into(),
        ];
        cfg.prompt_via = PromptVia::Stdin;
        cfg.session_id_source = Some(CommandSessionIdSource::ProviderCapture);
        cfg.resume_args = Some(vec![
            "--resume".into(),
            "{session_id}".into(),
            "{prompt}".into(),
        ]);
        cfg.decoder = Some(ProviderDecoderSpec {
            format: "jsonl".into(),
            name: None,
            events: Vec::new(),
            reduce: None,
            capture: Some(proto::methods::ProviderDecoderCaptureSpec {
                session: Some(ProviderJsonlTextReducerSpec {
                    mode: "lastNonEmpty".into(),
                    path: "$.session_id".into(),
                    when: None,
                    fallback: None,
                }),
            }),
        });
        let (tx, _rx) = mpsc::unbounded_channel();
        let slot = Arc::new(Mutex::new(InFlight::default()));

        run_prompt(cfg.clone(), prompt("ignored"), tx, slot).expect("run prompt");

        let saved = load_session(&cfg, &scope()).expect("saved session");
        assert_eq!(saved.session_id, "sid_decoder");
        std::fs::remove_dir_all(root).ok();
    }

    #[cfg(unix)]
    #[test]
    fn run_prompt_saves_stderr_decoder_captured_provider_session() {
        let mut cfg = cfg();
        let root = std::env::temp_dir().join(format!("loom-command-{}", uuid::Uuid::new_v4()));
        cfg.sessions_dir = root.join("sessions");
        cfg.command = "/bin/sh".into();
        cfg.args = vec![
            "-c".into(),
            "printf '%s\\n' '{\"type\":\"system\",\"session_id\":\"sid_stderr\"}' >&2".into(),
        ];
        cfg.prompt_via = PromptVia::Stdin;
        cfg.session_id_source = Some(CommandSessionIdSource::ProviderCapture);
        cfg.resume_args = Some(vec![
            "--resume".into(),
            "{session_id}".into(),
            "{prompt}".into(),
        ]);
        cfg.stderr_decoder = Some(ProviderDecoderSpec {
            format: "jsonl".into(),
            name: None,
            events: Vec::new(),
            reduce: None,
            capture: Some(proto::methods::ProviderDecoderCaptureSpec {
                session: Some(ProviderJsonlTextReducerSpec {
                    mode: "lastNonEmpty".into(),
                    path: "$.session_id".into(),
                    when: None,
                    fallback: None,
                }),
            }),
        });
        let (tx, _rx) = mpsc::unbounded_channel();
        let slot = Arc::new(Mutex::new(InFlight::default()));

        run_prompt(cfg.clone(), prompt("ignored"), tx, slot).expect("run prompt");

        let saved = load_session(&cfg, &scope()).expect("saved session");
        assert_eq!(saved.session_id, "sid_stderr");
        std::fs::remove_dir_all(root).ok();
    }

    #[cfg(unix)]
    #[test]
    fn stderr_decoder_final_text_is_emitted() {
        let mut cfg = cfg();
        cfg.command = "/bin/sh".into();
        cfg.args = vec![
            "-c".into(),
            "printf '%s\\n' '{\"text\":\"from stderr\"}' >&2".into(),
        ];
        cfg.stderr_decoder = Some(ProviderDecoderSpec {
            format: "jsonl".into(),
            name: None,
            events: Vec::new(),
            reduce: Some(ProviderJsonlReduceSpec {
                final_text: Some(ProviderJsonlTextReducerSpec {
                    mode: "lastNonEmpty".into(),
                    path: "$.text".into(),
                    when: None,
                    fallback: None,
                }),
            }),
            capture: None,
        });
        let (tx, mut rx) = mpsc::unbounded_channel();
        let slot = Arc::new(Mutex::new(InFlight::default()));

        spawn_and_collect(&cfg, &prompt("ignored"), &cfg.args, None, &tx, &slot).expect("spawn sh");

        let mut texts = Vec::new();
        while let Ok(event) = rx.try_recv() {
            if let AdapterEvent::Text { content, .. } = event {
                texts.push(content);
            }
        }
        assert_eq!(texts, vec!["from stderr"]);
    }

    #[cfg(unix)]
    #[test]
    fn run_prompt_resumes_with_arg_specs_without_legacy_resume_args() {
        let mut cfg = cfg();
        let root = std::env::temp_dir().join(format!("loom-command-{}", uuid::Uuid::new_v4()));
        cfg.sessions_dir = root.join("sessions");
        cfg.command = "/bin/sh".into();
        cfg.args = vec!["-c".into(), "printf '%s\\n' first-run".into()];
        cfg.resume_args = None;
        cfg.resume_arg_specs = vec![
            ProviderArgSpec::Literal("-c".into()),
            ProviderArgSpec::Literal("printf '%s\\n' \"$1\"".into()),
            ProviderArgSpec::Literal("resume".into()),
            ProviderArgSpec::Literal("{session_id}".into()),
        ];
        let request = prompt("ignored");
        let signature = command_signature_for_prompt(&cfg, &request);
        save_session(&cfg, &request.scope, "sid_arg_specs", &signature).expect("save session");
        let (tx, mut rx) = mpsc::unbounded_channel();
        let slot = Arc::new(Mutex::new(InFlight::default()));

        run_prompt(cfg.clone(), request, tx, slot).expect("run prompt");

        let mut texts = Vec::new();
        while let Ok(event) = rx.try_recv() {
            if let AdapterEvent::Text { content, .. } = event {
                texts.push(content);
            }
        }
        assert_eq!(texts, vec!["sid_arg_specs\n"]);
        std::fs::remove_dir_all(root).ok();
    }

    #[cfg(unix)]
    #[test]
    fn signed_thinking_replay_error_drops_saved_resume_session() {
        let mut cfg = cfg();
        let root = std::env::temp_dir().join(format!("loom-command-{}", uuid::Uuid::new_v4()));
        cfg.sessions_dir = root.join("sessions");
        cfg.command = "sh".into();
        cfg.prompt_via = PromptVia::Stdin;
        cfg.resume_args = Some(vec![
            "-c".into(),
            "cat >/dev/null; printf '%s\\n' 'API Error: 400 messages.7.content.3: thinking or redacted_thinking blocks in the latest assistant message cannot be modified. These blocks must remain as they were in the original response.' >&2; exit 1".into(),
        ]);
        let request = prompt("ignored");
        let signature = command_signature_for_prompt(&cfg, &request);
        save_session(&cfg, &request.scope, "bad_sid", &signature).expect("save session");
        let (tx, _rx) = mpsc::unbounded_channel();
        let slot = Arc::new(Mutex::new(InFlight::default()));

        run_prompt(cfg.clone(), request.clone(), tx, slot).expect("run prompt");

        assert!(load_session(&cfg, &request.scope).is_none());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn template_expands_request_vars_late() {
        let cfg = cfg();
        let mut request = prompt("hello");
        request.template_vars.insert(
            "agent.workspace".into(),
            "/tmp/channel/actor/workspace".into(),
        );
        request
            .template_vars
            .insert("agent.bundle".into(), "/tmp/actor/bundles/current".into());
        let out = expand_template(
            "{agent.workspace}:{agent.bundle}:{scope.id}",
            &cfg,
            &request,
            None,
            "",
        );
        assert_eq!(
            out,
            "/tmp/channel/actor/workspace:/tmp/actor/bundles/current:thr_xyz"
        );
    }

    #[test]
    fn template_expands_provider_runtime_aliases_from_request_vars() {
        let cfg = cfg();
        let mut request = prompt("hello");
        request
            .template_vars
            .insert("loom.server".into(), "ws://server/rpc".into());
        request
            .template_vars
            .insert("loom.run.id".into(), "run_1".into());
        request
            .template_vars
            .insert("loom.trigger.actor".into(), "actor_human".into());
        request
            .template_vars
            .insert("paths.cwd".into(), "/tmp/workspace".into());

        let out = expand_template(
            "{loom.server}|{loom.run.id}|{loom.trigger.actor}|{paths.cwd}",
            &cfg,
            &request,
            None,
            "",
        );

        assert_eq!(out, "ws://server/rpc|run_1|actor_human|/tmp/workspace");
    }

    #[test]
    fn expanded_env_keeps_spec_values_and_adds_request_defaults() {
        let mut cfg = cfg();
        cfg.env.insert("LOOM_SERVER".into(), "ws://spec".into());
        cfg.env
            .insert("WORKSPACE".into(), "{agent.workspace}".into());
        let mut request = prompt("hello");
        request
            .env
            .insert("LOOM_SERVER".into(), "ws://runtime".into());
        request
            .env
            .insert("AGENTX_CHANNEL_ID".into(), "channel_1".into());
        request
            .template_vars
            .insert("agent.workspace".into(), "/tmp/channel/workspace".into());

        let env = expanded_env(&cfg, &request, None);

        assert_eq!(
            env.get("LOOM_SERVER").map(String::as_str),
            Some("ws://spec")
        );
        assert_eq!(
            env.get("WORKSPACE").map(String::as_str),
            Some("/tmp/channel/workspace")
        );
        assert_eq!(
            env.get("AGENTX_CHANNEL_ID").map(String::as_str),
            Some("channel_1")
        );
    }

    #[test]
    fn expanded_env_expands_prompt_outputs_and_session_id() {
        let mut cfg = cfg();
        cfg.env.insert("SESSION_ID".into(), "{session.id}".into());
        cfg.env.insert("LEGACY_PROMPT".into(), "{prompt}".into());
        cfg.env.insert("USER_PROMPT".into(), "{prompt.user}".into());
        let mut request = prompt("full prompt");
        request.outputs.insert("user".into(), "user prompt".into());

        let env = expanded_env(&cfg, &request, Some("sid_123"));

        assert_eq!(env.get("SESSION_ID").map(String::as_str), Some("sid_123"));
        assert_eq!(
            env.get("LEGACY_PROMPT").map(String::as_str),
            Some("full prompt")
        );
        assert_eq!(
            env.get("USER_PROMPT").map(String::as_str),
            Some("user prompt")
        );
    }

    /// Spawning a long-lived child and cancelling it should reap quickly with
    /// the cancelled label set, instead of waiting for the natural exit.
    /// Unix-only because `signal_child` is gated on cfg(unix).
    #[cfg(unix)]
    #[tokio::test]
    async fn send_prompt_returns_before_command_finishes() {
        let mut cfg = cfg();
        cfg.command = "sleep".into();
        cfg.args = vec!["30".into()];
        cfg.prompt_via = PromptVia::Stdin;
        let adapter = Arc::new(CommandAdapter::new(cfg));
        let (tx, mut rx) = mpsc::unbounded_channel();
        adapter.start(tx).await.expect("start");

        let started = std::time::Instant::now();
        tokio::time::timeout(
            std::time::Duration::from_millis(250),
            adapter.send_prompt(prompt("ignored")),
        )
        .await
        .expect("send_prompt should return promptly")
        .expect("send_prompt ok");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(1),
            "send_prompt blocked on the child command"
        );

        let scope = scope();
        let slot = adapter.slot_for(&scope.id);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if slot.lock().pid.is_some() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "child never registered a PID"
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        adapter.cancel(scope).await.expect("cancel");

        let mut got_cancelled = false;
        while let Ok(Some(event)) =
            tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv()).await
        {
            if let AdapterEvent::Finished {
                success, summary, ..
            } = event
            {
                assert!(!success);
                assert_eq!(summary, "cancelled");
                got_cancelled = true;
                break;
            }
        }
        assert!(got_cancelled, "did not receive cancelled Finished event");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cancel_kills_running_child_and_labels_summary() {
        let mut cfg = cfg();
        cfg.command = "sleep".into();
        cfg.args = vec!["30".into()];
        // Use Stdin so the prompt body does NOT get appended to argv (which
        // would make BSD `sleep` reject the extra non-numeric token and exit
        // immediately, racing with the test's PID poll).
        cfg.prompt_via = PromptVia::Stdin;
        let adapter = Arc::new(CommandAdapter::new(cfg));
        let (tx, mut rx) = mpsc::unbounded_channel();
        adapter.start(tx).await.expect("start");

        let scope = scope();
        let send = {
            let adapter = adapter.clone();
            tokio::spawn(async move {
                adapter.send_prompt(prompt("ignored")).await.unwrap();
            })
        };

        // Wait until the slot has a PID, then cancel.
        let slot = adapter.slot_for(&scope.id);
        let start = std::time::Instant::now();
        loop {
            if slot.lock().pid.is_some() {
                break;
            }
            if start.elapsed() > std::time::Duration::from_secs(5) {
                panic!("child never registered a PID");
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        adapter.cancel(scope.clone()).await.expect("cancel");

        // The Finished event should arrive promptly with summary "cancelled".
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut got_cancelled = false;
        while std::time::Instant::now() < deadline {
            match tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv()).await {
                Ok(Some(AdapterEvent::Finished {
                    success, summary, ..
                })) => {
                    assert!(!success, "cancelled finish should not be success");
                    assert_eq!(summary, "cancelled");
                    got_cancelled = true;
                    break;
                }
                Ok(Some(_)) => continue,
                Ok(None) | Err(_) => break,
            }
        }
        assert!(got_cancelled, "did not receive cancelled Finished event");
        send.await.unwrap();
    }
}
