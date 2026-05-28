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
//!     `CopilotJson`. CodexStreamJson is wired through but its translation
//!     table is a placeholder per the doc.
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
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

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
use crate::usage::extract_token_usage_from_text;

#[cfg(unix)]
use std::os::unix::process::CommandExt;

/// Per-scope handle to an in-flight subprocess. The PID is set after spawn
/// and cleared on wait; `cancel_requested` is flipped on by `cancel()` so
/// the post-wait flow can label the Finished summary as "cancelled" rather
/// than the raw exit code.
#[derive(Default)]
struct InFlight {
    pid: Option<u32>,
    cancel_requested: bool,
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
        // Reset cancel flag for this scope's new prompt; old PID is already
        // gone (cleared after the previous wait).
        slot.lock().cancel_requested = false;
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

/// Send SIGTERM to `pid`. Unix only — Windows builds get a stub error so
/// callers know cancel isn't wired there yet (loom-server's audience is Unix).
#[cfg(unix)]
fn signal_child(pid: u32) -> Result<(), String> {
    signal_child_with(pid, libc::SIGTERM)
}

#[cfg(unix)]
fn signal_child_with(pid: u32, signal: libc::c_int) -> Result<(), String> {
    let rc = unsafe { libc::kill(-(pid as libc::pid_t), signal) };
    let rc = if rc == 0 {
        rc
    } else {
        // Older processes may not have been spawned into their own process
        // group. Fall back to the direct PID for compatibility.
        unsafe { libc::kill(pid as libc::pid_t, signal) }
    };
    if rc == 0 {
        Ok(())
    } else {
        let err = std::io::Error::last_os_error();
        // ESRCH (no such process) means the child already exited — racy but
        // harmless; treat as a successful no-op.
        if err.raw_os_error() == Some(libc::ESRCH) {
            Ok(())
        } else {
            Err(format!("kill({pid}, SIGTERM) failed: {err}"))
        }
    }
}

#[cfg(not(unix))]
fn signal_child(_pid: u32) -> Result<(), String> {
    Err("command transport cancel is not implemented for this platform".into())
}

#[cfg(unix)]
fn force_kill_child(pid: u32) -> Result<(), String> {
    signal_child_with(pid, libc::SIGKILL)
}

#[cfg(not(unix))]
fn force_kill_child(_pid: u32) -> Result<(), String> {
    Err("command transport timeout kill is not implemented for this platform".into())
}

fn run_prompt(
    cfg: CommandConfig,
    prompt: AdapterPrompt,
    sender: mpsc::UnboundedSender<AdapterEvent>,
    slot: Arc<Mutex<InFlight>>,
) -> Result<(), String> {
    let scope = prompt.scope.clone();
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
        } else if let Some(sid) = capture_decoder_session_id(cfg.decoder.as_ref(), &outcome.stdout)
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

fn spawn_and_collect(
    cfg: &CommandConfig,
    prompt: &AdapterPrompt,
    argv: &[String],
    session_id: Option<&str>,
    sender: &mpsc::UnboundedSender<AdapterEvent>,
    slot: &Arc<Mutex<InFlight>>,
) -> Result<SpawnOutcome, String> {
    std::fs::create_dir_all(&prompt.cwd).map_err(|e| {
        format!(
            "failed to create command cwd `{}`: {}",
            prompt.cwd.display(),
            e
        )
    })?;
    let mut cmd = Command::new(&cfg.command);
    let stdin = if cfg.stdin_template.is_some() || matches!(cfg.prompt_via, PromptVia::Stdin) {
        Stdio::piped()
    } else {
        Stdio::null()
    };
    cmd.args(argv)
        .current_dir(&prompt.cwd)
        .stdin(stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in expanded_env(cfg, prompt, session_id) {
        cmd.env(k, v);
    }
    if matches!(cfg.prompt_via, PromptVia::Env) {
        cmd.env("LOOM_PROMPT", &prompt.content);
    }
    configure_process_group(&mut cmd);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to spawn `{}`: {}", cfg.command, e))?;
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

    if cfg.stdin_template.is_some() || matches!(cfg.prompt_via, PromptVia::Stdin) {
        if let Some(mut stdin) = child.stdin.take() {
            let stdin_body = cfg
                .stdin_template
                .as_ref()
                .map(|template| expand_template(template, cfg, prompt, session_id, &prompt.content))
                .unwrap_or_else(|| prompt.content.clone());
            stdin
                .write_all(stdin_body.as_bytes())
                .map_err(|e| format!("failed to write prompt to stdin: {e}"))?;
        }
    }
    // Drop unused stdin so the child doesn't block on read.
    drop(child.stdin.take());

    let stdout = child.stdout.take().ok_or("failed to open child stdout")?;
    let stderr = child.stderr.take().ok_or("failed to open child stderr")?;

    // Stream stdout on a helper thread so the foreground loop can enforce a
    // hard timeout even if the child is silent or never closes stdout.
    let (stdout_tx, stdout_rx) = std::sync::mpsc::channel::<String>();
    let stdout_handle = std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    let _ = stdout_tx.send(line.clone());
                }
                Err(_) => break,
            }
        }
    });
    // Collect stderr on a thread purely so it doesn't fill its pipe and
    // deadlock the child.
    let stderr_handle = std::thread::spawn(move || {
        let mut buf = String::new();
        let mut r = BufReader::new(stderr);
        let _ = r.read_to_string(&mut buf);
        buf
    });

    let mut collected_stdout = String::new();
    let mut emitted_text = false;
    let mut emitted_finish = false;
    let deadline = cfg
        .timeout_ms
        .filter(|ms| *ms > 0)
        .map(|ms| Instant::now() + Duration::from_millis(ms));
    let idle_timeout = cfg
        .idle_timeout_ms
        .filter(|ms| *ms > 0)
        .map(Duration::from_millis);
    let mut last_stdout_at = Instant::now();
    let mut exit: Option<ExitStatus> = None;
    let mut timed_out = false;
    let mut idle_timed_out = false;

    loop {
        while let Ok(line) = stdout_rx.try_recv() {
            last_stdout_at = Instant::now();
            let events = collect_stdout_line(cfg, prompt, sender, &line, &mut collected_stdout);
            emitted_text |= events.emitted_text;
            emitted_finish |= events.emitted_finish;
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
            if last_stdout_at.elapsed() >= idle_timeout {
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
                    .saturating_sub(last_stdout_at.elapsed())
                    .min(Duration::from_millis(50))
            }))
            .min()
            .unwrap_or_else(|| Duration::from_millis(50));
        match stdout_rx.recv_timeout(wait_for) {
            Ok(line) => {
                last_stdout_at = Instant::now();
                let events = collect_stdout_line(cfg, prompt, sender, &line, &mut collected_stdout);
                emitted_text |= events.emitted_text;
                emitted_finish |= events.emitted_finish;
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
    for line in stdout_rx.try_iter() {
        let events = collect_stdout_line(cfg, prompt, sender, &line, &mut collected_stdout);
        emitted_text |= events.emitted_text;
        emitted_finish |= events.emitted_finish;
    }
    let collected_stderr = stderr_handle.join().unwrap_or_default();
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
    } else if !collected_stderr.is_empty() {
        truncate_for_summary(&collected_stderr)
    } else {
        format!("exited with code {exit_code}")
    };

    if let Some(content) = extract_decoder_final_text(cfg.decoder.as_ref(), &collected_stdout) {
        let _ = sender.send(AdapterEvent::Text {
            scope: Some(prompt.scope.clone()),
            content,
            is_partial: false,
        });
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

#[cfg(unix)]
fn configure_process_group(cmd: &mut Command) {
    cmd.process_group(0);
}

#[cfg(not(unix))]
fn configure_process_group(_cmd: &mut Command) {}

fn collect_stdout_line(
    cfg: &CommandConfig,
    prompt: &AdapterPrompt,
    sender: &mpsc::UnboundedSender<AdapterEvent>,
    line: &str,
    collected_stdout: &mut String,
) -> OutputLineEvents {
    collected_stdout.push_str(line);
    let parsed_line = line.trim_end_matches(&['\r', '\n'][..]);
    if let Some(events) =
        translate_decoder_event_line(cfg.decoder.as_ref(), parsed_line, &prompt.scope, sender)
    {
        return events;
    }
    match cfg.output_format {
        CommandOutputFormat::NdjsonLines => OutputLineEvents {
            emitted_text: translate_ndjson_line(parsed_line, &prompt.scope, sender),
            emitted_finish: false,
        },
        CommandOutputFormat::ClaudeStreamJson => OutputLineEvents {
            emitted_text: translate_claude_stream_line(parsed_line, &prompt.scope, sender),
            emitted_finish: false,
        },
        CommandOutputFormat::CodexStreamJson => OutputLineEvents {
            emitted_text: translate_codex_event_line(parsed_line, &prompt.scope, sender),
            emitted_finish: false,
        },
        CommandOutputFormat::Text | CommandOutputFormat::CopilotJson => OutputLineEvents::default(),
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

// ---------------- output_format translators ----------------

#[derive(Debug, Clone, Copy, Default)]
struct OutputLineEvents {
    emitted_text: bool,
    emitted_finish: bool,
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
    let v: Value = serde_json::from_str(line).ok()?;
    let mut events = OutputLineEvents::default();
    for event in &decoder.events {
        if !event
            .when
            .as_ref()
            .map(|condition| json_condition_matches(&v, condition))
            .unwrap_or(true)
        {
            continue;
        }
        let emitted = emit_decoder_event(&event.emit, &v, scope, sender);
        events.emitted_text |= emitted.emitted_text;
        events.emitted_finish |= emitted.emitted_finish;
    }
    Some(events)
}

fn emit_decoder_event(
    emit: &ProviderDecoderEmitSpec,
    root: &Value,
    scope: &ScopeRef,
    sender: &mpsc::UnboundedSender<AdapterEvent>,
) -> OutputLineEvents {
    match emit.emit_type.as_str() {
        "text" => {
            let Some(content) = emit
                .text
                .as_deref()
                .and_then(|template| decoder_template_value(root, template))
            else {
                return OutputLineEvents::default();
            };
            let _ = sender.send(AdapterEvent::Text {
                scope: Some(scope.clone()),
                content,
                is_partial: emit.partial.unwrap_or(true),
            });
            OutputLineEvents {
                emitted_text: true,
                emitted_finish: false,
            }
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
            let _ = sender.send(AdapterEvent::ToolUse {
                scope: Some(scope.clone()),
                tool_name,
                input,
            });
            OutputLineEvents::default()
        }
        "status" => {
            if let Some(status) = emit
                .status
                .as_deref()
                .or(emit.text.as_deref())
                .and_then(|template| decoder_template_value(root, template))
            {
                let _ = sender.send(AdapterEvent::StatusChange {
                    scope: Some(scope.clone()),
                    status,
                });
            }
            OutputLineEvents::default()
        }
        "error" => {
            let message = emit
                .message
                .as_deref()
                .or(emit.text.as_deref())
                .and_then(|template| decoder_template_value(root, template))
                .unwrap_or_else(|| "provider error frame".into());
            let _ = sender.send(AdapterEvent::Error {
                scope: Some(scope.clone()),
                message,
            });
            OutputLineEvents::default()
        }
        "finish" | "finished" => {
            let summary = emit
                .summary
                .as_deref()
                .or(emit.message.as_deref())
                .and_then(|template| decoder_template_value(root, template))
                .unwrap_or_default();
            let _ = sender.send(AdapterEvent::Finished {
                scope: Some(scope.clone()),
                success: emit.success.unwrap_or(true),
                summary,
                usage: None,
            });
            OutputLineEvents {
                emitted_text: false,
                emitted_finish: true,
            }
        }
        _ => OutputLineEvents::default(),
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
    let v: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let kind = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
    let mut emitted_text = false;
    match kind {
        "text" => {
            if let Some(t) = v.get("text").and_then(|x| x.as_str()) {
                let _ = sender.send(AdapterEvent::Text {
                    scope: Some(scope.clone()),
                    content: t.to_string(),
                    is_partial: true,
                });
                emitted_text = true;
            }
        }
        "tool" => {
            let name = v
                .get("name")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let input = v.get("input").cloned().unwrap_or(Value::Null);
            let _ = sender.send(AdapterEvent::ToolUse {
                scope: Some(scope.clone()),
                tool_name: name,
                input,
            });
        }
        "status" => {
            if let Some(s) = v.get("status").and_then(|x| x.as_str()) {
                let _ = sender.send(AdapterEvent::StatusChange {
                    scope: Some(scope.clone()),
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
            let _ = sender.send(AdapterEvent::Error {
                scope: Some(scope.clone()),
                message: msg,
            });
        }
        // "done" and unknown kinds: caller handles the final flush + Finished
        // outside the per-line loop, so nothing to do here.
        _ => {}
    }
    emitted_text
}

fn translate_claude_stream_line(
    line: &str,
    scope: &ScopeRef,
    sender: &mpsc::UnboundedSender<AdapterEvent>,
) -> bool {
    let v: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let outer = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
    let mut emitted_text = false;
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
                            let _ = sender.send(AdapterEvent::Text {
                                scope: Some(scope.clone()),
                                content: t.to_string(),
                                is_partial: false,
                            });
                            emitted_text = true;
                        }
                    }
                    "tool_use" => {
                        let name = b
                            .get("name")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string();
                        let input = b.get("input").cloned().unwrap_or(Value::Null);
                        let _ = sender.send(AdapterEvent::ToolUse {
                            scope: Some(scope.clone()),
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
    emitted_text
}

fn translate_codex_event_line(
    line: &str,
    scope: &ScopeRef,
    sender: &mpsc::UnboundedSender<AdapterEvent>,
) -> bool {
    let v: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let mut emitted_text = false;
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
                    let _ = sender.send(AdapterEvent::Text {
                        scope: Some(scope.clone()),
                        content: text,
                        is_partial: false,
                    });
                    emitted_text = true;
                }
            }
            "agent_message" | "agent.message" => {
                if let Some(text) = codex_message_event_text(&v).and_then(non_blank) {
                    let _ = sender.send(AdapterEvent::Text {
                        scope: Some(scope.clone()),
                        content: text,
                        is_partial: false,
                    });
                    emitted_text = true;
                }
            }
            "item_completed" | "item.completed" | "raw_response_item" | "raw.response_item" => {
                if let Some(text) = v
                    .get("item")
                    .and_then(codex_response_item_text)
                    .and_then(non_blank)
                {
                    let _ = sender.send(AdapterEvent::Text {
                        scope: Some(scope.clone()),
                        content: text,
                        is_partial: false,
                    });
                    emitted_text = true;
                }
            }
            "tool_call" => {
                let name = v
                    .get("name")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                let input = v.get("arguments").cloned().unwrap_or(Value::Null);
                let _ = sender.send(AdapterEvent::ToolUse {
                    scope: Some(scope.clone()),
                    tool_name: name,
                    input,
                });
            }
            "stream_error" => {
                let message =
                    string_at_paths(&v, &["/message", "/error/message", "/error", "/details"])
                        .unwrap_or_else(|| "codex stream error".into());
                let _ = sender.send(AdapterEvent::Error {
                    scope: Some(scope.clone()),
                    message,
                });
            }
            _ => {}
        }
    }
    emitted_text
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
        std::fs::create_dir_all(parent)?;
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
        return expand_arg_specs(&cfg.arg_specs, cfg, request, session_id, prompt);
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
    argv
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
            ProviderArgSpec::Conditional { when, args } => {
                if arg_condition_matches(when, request) {
                    append_arg_specs(out, args, cfg, request, session_id, prompt);
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
    let scope_kind = match request.scope.kind {
        proto::types::ScopeKind::Thread => "thread",
        proto::types::ScopeKind::Channel => "channel",
    };
    let mut out = input
        .replace("{actor.id}", &cfg.actor_id)
        .replace("{scope.id}", &request.scope.id)
        .replace("{scope.kind}", scope_kind)
        .replace("{model}", active_model(request).as_deref().unwrap_or(""))
        .replace("{prompt}", prompt)
        .replace(
            "{prompt.full}",
            request
                .outputs
                .get("full")
                .map(String::as_str)
                .unwrap_or(prompt),
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
            prompt_via: PromptVia::Args,
            prompt: None,
            stdin_template: None,
            sessions_dir: PathBuf::from("/tmp/loom-test-sessions"),
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
            cwd: PathBuf::from("/tmp"),
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
            ProviderArgSpec::Conditional {
                when: "model".into(),
                args: vec![
                    ProviderArgSpec::Literal("--model".into()),
                    ProviderArgSpec::Literal("{model}".into()),
                ],
            },
            ProviderArgSpec::Conditional {
                when: "reasoningEffort".into(),
                args: vec![
                    ProviderArgSpec::Literal("--effort".into()),
                    ProviderArgSpec::Literal("{reasoningEffort}".into()),
                ],
            },
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
            ProviderArgSpec::Conditional {
                when: "model".into(),
                args: vec![
                    ProviderArgSpec::Literal("--model".into()),
                    ProviderArgSpec::Literal("{model}".into()),
                ],
            },
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
