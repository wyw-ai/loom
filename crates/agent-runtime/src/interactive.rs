//! Interactive command transport.
//!
//! This adapter is for provider CLIs that accept prompt + session id arguments
//! but do not use process exit as the turn completion boundary. Loom owns the
//! provider session id per `(actor, scope)` and uses an explicit completion
//! sentinel to decide when a turn is done.

use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Child, Stdio};

use loom_platform::process::Command;

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use parking_lot::Mutex;
use proto::methods::{
    ClaudeSettingsMode, InteractiveCommandSpec, InteractiveKillAction, InteractiveKillKind,
    InteractiveProviderSpec,
};
use proto::types::{ScopeKind, ScopeRef};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;
use uuid::Uuid;

use super::adapter::{Adapter, AdapterEvent, AdapterPrompt, AdapterStartInfo, TokenUsage};
use crate::acp::create_dir_all_unc;
use loom_platform::path::unc_prefix_path;
use crate::usage::extract_token_usage_from_text;

#[derive(Debug, Clone)]
pub struct InteractiveCommandConfig {
    pub actor_id: String,
    pub command: String,
    pub base_args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub model: Option<String>,
    pub model_args: Vec<String>,
    pub spec: InteractiveCommandSpec,
    pub provider: Option<InteractiveProviderSpec>,
    pub sessions_dir: PathBuf,
    pub profile_dir: PathBuf,
    pub command_signature: String,
}

impl InteractiveCommandConfig {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        actor_id: String,
        command: String,
        base_args: &[String],
        env: BTreeMap<String, String>,
        model: Option<String>,
        model_args: Vec<String>,
        spec: InteractiveCommandSpec,
        provider: Option<InteractiveProviderSpec>,
        sessions_dir: PathBuf,
        profile_dir: PathBuf,
    ) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(b"interactive_command");
        hasher.update(command.as_bytes());
        for a in base_args {
            hasher.update(b"\x00");
            hasher.update(a.as_bytes());
        }
        for a in &model_args {
            hasher.update(b"\x00model_arg\x00");
            hasher.update(a.as_bytes());
        }
        for a in &spec.session.new_args {
            hasher.update(b"\x00new\x00");
            hasher.update(a.as_bytes());
        }
        for a in &spec.session.resume_args {
            hasher.update(b"\x00resume\x00");
            hasher.update(a.as_bytes());
        }
        Self {
            actor_id,
            command,
            base_args: base_args.to_vec(),
            env,
            model,
            model_args,
            spec,
            provider,
            sessions_dir,
            profile_dir,
            command_signature: format!("sha256:{}", hex::encode(hasher.finalize())),
        }
    }
}

#[derive(Default)]
struct InFlight {
    pid: Option<u32>,
    cancel_requested: bool,
}

pub struct InteractiveCommandAdapter {
    cfg: InteractiveCommandConfig,
    inner: Mutex<InteractiveInner>,
}

struct InteractiveInner {
    event_sender: Option<mpsc::UnboundedSender<AdapterEvent>>,
    in_flight: HashMap<String, Arc<Mutex<InFlight>>>,
}

impl InteractiveCommandAdapter {
    pub fn new(cfg: InteractiveCommandConfig) -> Self {
        Self {
            cfg,
            inner: Mutex::new(InteractiveInner {
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
            .ok_or_else(|| "interactive command adapter not started".to_string())
    }

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
impl Adapter for InteractiveCommandAdapter {
    async fn start(
        &self,
        events: mpsc::UnboundedSender<AdapterEvent>,
    ) -> Result<AdapterStartInfo, String> {
        self.inner.lock().event_sender = Some(events);
        Ok(AdapterStartInfo {
            pid: None,
            session_id: Some(format!("interactive:{}", self.cfg.actor_id)),
        })
    }

    async fn send_prompt(&self, prompt: AdapterPrompt) -> Result<(), String> {
        if prompt.content.trim().is_empty() {
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
        slot.lock().cancel_requested = false;
        // Detach the provider turn onto a blocking worker. send_prompt must
        // return promptly so the agent worker's notification loop stays
        // free to process turn/close, action responses, and directed messages to
        // other scopes while a (potentially long) interactive turn runs.
        // Completion is driven entirely through AdapterEvent (Finished /
        // Error), and run_prompt always emits a Finished event in both
        // success and failure paths.
        tokio::task::spawn_blocking(move || {
            if let Err(e) = run_prompt(cfg, prompt, sender, slot) {
                tracing::debug!(error = %e, "interactive run_prompt returned err (already reported via AdapterEvent)");
            }
        });
        Ok(())
    }

    async fn respond_action(&self, _request_id: String, _option_id: String) -> Result<(), String> {
        Err("interactive command transport does not support action requests".into())
    }

    async fn cancel(&self, scope: ScopeRef) -> Result<(), String> {
        let slot = {
            let inner = self.inner.lock();
            inner.in_flight.get(&scope.id).cloned()
        };
        let Some(slot) = slot else {
            return Ok(());
        };
        let pid = {
            let mut s = slot.lock();
            s.cancel_requested = true;
            s.pid
        };
        if let Some(pid) = pid {
            apply_kill_kind(InteractiveKillKind::Sigterm, pid)?;
        }
        Ok(())
    }

    async fn stop(&self) -> Result<(), String> {
        let slots = {
            let mut inner = self.inner.lock();
            inner.event_sender = None;
            inner.in_flight.values().cloned().collect::<Vec<_>>()
        };
        for slot in slots {
            if let Some(pid) = slot.lock().pid {
                let _ = apply_kill_kind(InteractiveKillKind::Sigterm, pid);
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
struct RunOutcome {
    success: bool,
    summary: String,
    final_text: String,
    stdout: String,
    stderr: String,
    session_id: String,
    command_signature: String,
    used_first_run: bool,
    usage: Option<TokenUsage>,
}

fn run_prompt(
    cfg: InteractiveCommandConfig,
    prompt: AdapterPrompt,
    sender: mpsc::UnboundedSender<AdapterEvent>,
    slot: Arc<Mutex<InFlight>>,
) -> Result<(), String> {
    let outcome = run_prompt_inner(&cfg, &prompt, &slot);
    match outcome {
        Ok(outcome) => {
            if !outcome.stderr.trim().is_empty() {
                let _ = sender.send(AdapterEvent::StatusChange {
                    scope: Some(prompt.scope.clone()),
                    status: format!("stderr: {}", truncate(&outcome.stderr, 500)),
                });
            }
            if !outcome.success
                && !outcome.used_first_run
                && looks_like_signed_thinking_replay_error(&outcome.stderr, &outcome.stdout)
            {
                let _ = delete_session(&cfg, &prompt.scope);
                tracing::warn!(actor = %cfg.actor_id, scope = %prompt.scope.id,
                    "interactive command transport: dropped Claude session after signed-thinking replay error");
            }
            if outcome.success {
                save_session(
                    &cfg,
                    &prompt.scope,
                    &outcome.session_id,
                    &outcome.command_signature,
                )
                .map_err(|e| {
                    let msg = format!("failed to save interactive session: {e}");
                    let _ = sender.send(AdapterEvent::Error {
                        scope: Some(prompt.scope.clone()),
                        message: msg.clone(),
                    });
                    let _ = sender.send(AdapterEvent::Finished {
                        scope: Some(prompt.scope.clone()),
                        success: false,
                        summary: msg.clone(),
                        usage: None,
                    });
                    msg
                })?;
                if !outcome.final_text.is_empty() {
                    let _ = sender.send(AdapterEvent::Text {
                        scope: Some(prompt.scope.clone()),
                        content: outcome.final_text,
                        is_partial: false,
                    });
                }
            } else if !outcome.stdout.trim().is_empty() {
                let _ = sender.send(AdapterEvent::StatusChange {
                    scope: Some(prompt.scope.clone()),
                    status: format!("stdout: {}", truncate(&outcome.stdout, 500)),
                });
            }
            let _ = sender.send(AdapterEvent::Finished {
                scope: Some(prompt.scope.clone()),
                success: outcome.success,
                summary: outcome.summary,
                usage: outcome.usage,
            });
            Ok(())
        }
        Err(e) => {
            let _ = sender.send(AdapterEvent::Error {
                scope: Some(prompt.scope.clone()),
                message: e.clone(),
            });
            let _ = sender.send(AdapterEvent::Finished {
                scope: Some(prompt.scope),
                success: false,
                summary: e.clone(),
                usage: None,
            });
            Err(e)
        }
    }
}

fn run_prompt_inner(
    cfg: &InteractiveCommandConfig,
    prompt: &AdapterPrompt,
    slot: &Arc<Mutex<InFlight>>,
) -> Result<RunOutcome, String> {
    crate::acp::create_dir_all_unc(&prompt.cwd).map_err(|e| {
        format!(
            "failed to create interactive command cwd `{}`: {}",
            prompt.cwd.display(),
            e
        )
    })?;
    let command_signature = command_signature_for_prompt(cfg, prompt)?;
    let saved = load_session(cfg, &prompt.scope);
    let resume_session_id = saved
        .as_ref()
        .filter(|s| s.command_signature == command_signature)
        .map(|s| s.session_id.clone());
    let used_first_run = resume_session_id.is_none();
    let session_id = resume_session_id.unwrap_or_else(|| Uuid::new_v4().to_string());
    let prompt_text = build_interactive_prompt(cfg, prompt, &session_id);
    // Prepend transport.args (the agent spec's executable-level base args)
    // before the session-specific argv so wrapper / common flags actually
    // reach the child process. Without this, base_args only participated in
    // command_signature hashing and were silently dropped at spawn time,
    // which contradicts the documented AgentSpec contract.
    let mut argv = expand_argv(&cfg.base_args, cfg, prompt, Some(&session_id), &prompt_text);
    let session_argv = if used_first_run {
        expand_argv(
            &cfg.spec.session.new_args,
            cfg,
            prompt,
            Some(&session_id),
            &prompt_text,
        )
    } else {
        let template = if cfg.spec.session.resume_args.is_empty() {
            &cfg.spec.session.new_args
        } else {
            &cfg.spec.session.resume_args
        };
        expand_argv(template, cfg, prompt, Some(&session_id), &prompt_text)
    };
    argv.extend(session_argv);
    append_provider_args(cfg, prompt, &mut argv)?;

    let mut child = spawn_child(cfg, prompt, &argv)?;
    {
        let mut s = slot.lock();
        s.pid = Some(child.id());
        if s.cancel_requested {
            let pid = child.id();
            drop(s);
            let _ = apply_kill_policy(&cfg.spec.kill.on_cancel, pid);
        }
    }

    let stdout = child.stdout.take().ok_or("failed to open child stdout")?;
    let stderr = child.stderr.take().ok_or("failed to open child stderr")?;
    let (stdout_tx, stdout_rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
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
    let stderr_handle = std::thread::spawn(move || {
        let mut buf = String::new();
        let mut r = BufReader::new(stderr);
        let _ = r.read_to_string(&mut buf);
        buf
    });

    let sentinel = cfg
        .spec
        .prompt
        .completion_contract
        .sentinel
        .trim()
        .to_string();
    let deadline = Instant::now() + Duration::from_millis(cfg.spec.completion.max_turn_ms);
    let mut collected = String::new();
    let mut final_text = String::new();
    let mut found_done = false;
    let summary: String;
    let mut success = false;

    loop {
        if slot.lock().cancel_requested {
            let _ = apply_kill_policy(&cfg.spec.kill.on_cancel, child.id());
            summary = "cancelled".into();
            break;
        }
        if Instant::now() >= deadline {
            let _ = apply_kill_policy(&cfg.spec.kill.on_timeout, child.id());
            summary = "timeout waiting for interactive command sentinel".into();
            break;
        }
        match stdout_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(chunk) => {
                if append_stdout_chunk(&mut collected, chunk, cfg.spec.output.strip_ansi, &sentinel)
                {
                    final_text = text_before_sentinel(&collected, &sentinel);
                    if cfg.spec.completion.strip_sentinel {
                        final_text = final_text.trim_end().to_string();
                    }
                    found_done = true;
                    if let Err(e) = apply_kill_policy(&cfg.spec.kill.on_complete, child.id()) {
                        summary = format!("completion kill policy failed: {e}");
                        success = false;
                    } else {
                        summary = String::new();
                        success = true;
                    }
                    break;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {}
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|e| format!("failed to poll child: {e}"))?
        {
            drain_exited_stdout(
                &stdout_rx,
                &mut collected,
                cfg.spec.output.strip_ansi,
                &sentinel,
                &mut final_text,
                cfg.spec.completion.strip_sentinel,
                &mut found_done,
            );
            if found_done {
                summary = String::new();
                success = true;
            } else if status.success() {
                summary = "process exited before completion sentinel".into();
            } else {
                summary = format!(
                    "process exited before completion sentinel with code {}",
                    status.code().unwrap_or(-1)
                );
            }
            break;
        }
    }

    let _ = child.wait();
    {
        let mut s = slot.lock();
        s.pid = None;
        s.cancel_requested = false;
    }
    let stderr = stderr_handle.join().unwrap_or_default();
    let usage = extract_token_usage_from_text(&collected)
        .or_else(|| extract_token_usage_from_text(&stderr));
    Ok(RunOutcome {
        success: success && found_done,
        summary,
        final_text,
        stdout: collected,
        stderr,
        session_id,
        command_signature,
        used_first_run,
        usage,
    })
}

fn append_stdout_chunk(
    collected: &mut String,
    chunk: String,
    strip_output_ansi: bool,
    sentinel: &str,
) -> bool {
    let chunk = if strip_output_ansi {
        proto::ansi::strip_ansi(&chunk)
    } else {
        chunk
    };
    collected.push_str(&chunk);
    contains_sentinel_line(collected, sentinel)
}

fn drain_exited_stdout(
    stdout_rx: &std::sync::mpsc::Receiver<String>,
    collected: &mut String,
    strip_output_ansi: bool,
    sentinel: &str,
    final_text: &mut String,
    strip_sentinel: bool,
    found_done: &mut bool,
) {
    let deadline = Instant::now() + Duration::from_millis(250);
    loop {
        match stdout_rx.recv_timeout(Duration::from_millis(10)) {
            Ok(chunk) => {
                if append_stdout_chunk(collected, chunk, strip_output_ansi, sentinel) {
                    *found_done = true;
                    *final_text = text_before_sentinel(collected, sentinel);
                    if strip_sentinel {
                        *final_text = final_text.trim_end().to_string();
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) if Instant::now() >= deadline => break,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}

fn spawn_child(
    cfg: &InteractiveCommandConfig,
    prompt: &AdapterPrompt,
    argv: &[String],
) -> Result<Child, String> {
    // On Windows, prefix the cwd with UNC prefix to bypass MAX_PATH (260
    // char) limit.  Do NOT UNC-prefix the command path — `\\?\` bypasses
    // PATHEXT resolution in CreateProcessW, so e.g.
    // `\\?\D:\nodejs\claude` would fail to resolve to `claude.cmd`.
    // `unc_prefix_path` is a no-op on Unix so the call site stays cfg-free.
    let spawn_cwd = unc_prefix_path(prompt.cwd.clone());

    let mut cmd = Command::new(&cfg.command);
    cmd.args(argv)
        .current_dir(&spawn_cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in expanded_env(cfg, prompt) {
        cmd.env(k, v);
    }
    // Windows CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB |
    // CREATE_NEW_PROCESS_GROUP and Unix process_group(0) are applied by
    // `loom_platform::process::Command::new` automatically — no cfg block.
    cmd.spawn()
        .map_err(|e| format!("failed to spawn `{}`: {}", cfg.command, e))
}

fn build_interactive_prompt(
    cfg: &InteractiveCommandConfig,
    prompt: &AdapterPrompt,
    session_id: &str,
) -> String {
    let contract = completion_contract(cfg);
    let envelope = prompt.content.as_str();
    let with_contract = if let Some(idx) = envelope.find("\n\n=== User message ===") {
        let (head, tail) = envelope.split_at(idx);
        format!("{head}\n\n{contract}{tail}")
    } else {
        format!("{contract}\n\n=== User message ===\n{envelope}")
    };
    cfg.spec
        .prompt
        .template
        .replace("{loom_envelope}", &with_contract)
        .replace("{user_message}", &prompt.content)
        .replace("{session_id}", session_id)
}

fn completion_contract(cfg: &InteractiveCommandConfig) -> String {
    let sentinel = cfg.spec.prompt.completion_contract.sentinel.trim();
    let instruction = cfg
        .spec
        .prompt
        .completion_contract
        .instruction
        .replace("__LOOM_DONE__", sentinel);
    format!(
        "=== Loom interactive command completion contract ===\n{instruction}\nCompletion sentinel: {sentinel}"
    )
}

fn append_provider_args(
    cfg: &InteractiveCommandConfig,
    prompt: &AdapterPrompt,
    argv: &mut Vec<String>,
) -> Result<(), String> {
    if let Some(settings) = resolve_claude_settings(cfg, prompt)? {
        argv.push("--settings".into());
        argv.push(settings);
    }
    if let Some(model) = active_model(cfg, prompt) {
        let model_args = if cfg.model_args.is_empty() {
            vec![format!("--model={model}")]
        } else {
            cfg.model_args
                .iter()
                .map(|arg| expand_template(arg, cfg, prompt, None, "").replace("{model}", &model))
                .collect()
        };
        argv.extend(model_args);
    }
    Ok(())
}

fn active_model(cfg: &InteractiveCommandConfig, prompt: &AdapterPrompt) -> Option<String> {
    prompt
        .model
        .as_ref()
        .or(cfg.model.as_ref())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

fn resolve_claude_settings(
    cfg: &InteractiveCommandConfig,
    prompt: &AdapterPrompt,
) -> Result<Option<String>, String> {
    let Some(provider) = cfg.provider.as_ref() else {
        return Ok(None);
    };
    if provider.kind != "claude" {
        return Ok(None);
    }
    let Some(settings) = provider.settings.as_ref() else {
        return Ok(None);
    };
    match settings.mode {
        ClaudeSettingsMode::Global => Ok(None),
        ClaudeSettingsMode::ActorProfile => {
            let path = cfg.profile_dir.join("claude").join("settings.json");
            if let Some(parent) = path.parent() {
                create_dir_all_unc(parent).map_err(|e| {
                    format!(
                        "failed to create claude settings dir `{}`: {e}",
                        parent.display()
                    )
                })?;
            }
            Ok(Some(path.display().to_string()))
        }
        ClaudeSettingsMode::Custom => {
            let raw = settings
                .path
                .as_ref()
                .ok_or("claude custom settings mode requires provider.settings.path")?;
            Ok(Some(expand_template(raw, cfg, prompt, None, "")))
        }
    }
}

fn command_signature_for_prompt(
    cfg: &InteractiveCommandConfig,
    prompt: &AdapterPrompt,
) -> Result<String, String> {
    let mut hasher = Sha256::new();
    hasher.update(cfg.command_signature.as_bytes());
    if let Some(model) = active_model(cfg, prompt) {
        hasher.update(b"\x00model\x00");
        hasher.update(model.as_bytes());
    }
    hasher.update(b"\x00sentinel\x00");
    hasher.update(cfg.spec.prompt.completion_contract.sentinel.as_bytes());
    if let Some(settings) = resolve_settings_signature(cfg, prompt)? {
        hasher.update(b"\x00settings\x00");
        hasher.update(settings.as_bytes());
    }
    Ok(format!("sha256:{}", hex::encode(hasher.finalize())))
}

fn resolve_settings_signature(
    cfg: &InteractiveCommandConfig,
    prompt: &AdapterPrompt,
) -> Result<Option<String>, String> {
    let Some(provider) = cfg.provider.as_ref() else {
        return Ok(None);
    };
    let Some(settings) = provider.settings.as_ref() else {
        return Ok(None);
    };
    match settings.mode {
        ClaudeSettingsMode::Global => Ok(Some("claude:global".into())),
        ClaudeSettingsMode::ActorProfile => Ok(Some(format!(
            "claude:actor_profile:{}",
            cfg.profile_dir
                .join("claude")
                .join("settings.json")
                .display()
        ))),
        ClaudeSettingsMode::Custom => {
            let raw = settings
                .path
                .as_ref()
                .ok_or("claude custom settings mode requires provider.settings.path")?;
            Ok(Some(format!(
                "claude:custom:{}",
                expand_template(raw, cfg, prompt, None, "")
            )))
        }
    }
}

fn expand_argv(
    template: &[String],
    cfg: &InteractiveCommandConfig,
    request: &AdapterPrompt,
    session_id: Option<&str>,
    prompt: &str,
) -> Vec<String> {
    template
        .iter()
        .map(|a| expand_template(a, cfg, request, session_id, prompt))
        .collect()
}

fn expand_template(
    input: &str,
    cfg: &InteractiveCommandConfig,
    request: &AdapterPrompt,
    session_id: Option<&str>,
    prompt: &str,
) -> String {
    let scope_kind = match request.scope.kind {
        ScopeKind::Thread => "thread",
        ScopeKind::Channel => "channel",
    };
    let mut out = input
        .replace("{actor.id}", &cfg.actor_id)
        .replace("{scope.id}", &request.scope.id)
        .replace("{scope.kind}", scope_kind)
        .replace(
            "{model}",
            active_model(cfg, request).as_deref().unwrap_or(""),
        )
        .replace("{prompt}", prompt);
    if let Some(sid) = session_id {
        out = out.replace("{session_id}", sid);
    }
    for (key, value) in &request.template_vars {
        out = out.replace(&format!("{{{key}}}"), value);
    }
    out
}

fn expanded_env(
    cfg: &InteractiveCommandConfig,
    request: &AdapterPrompt,
) -> BTreeMap<String, String> {
    let mut env: BTreeMap<String, String> = cfg
        .env
        .iter()
        .map(|(k, v)| (k.clone(), expand_template(v, cfg, request, None, "")))
        .collect();
    for (k, v) in &request.env {
        env.entry(k.clone()).or_insert_with(|| v.clone());
    }
    if let Some(model) = active_model(cfg, request) {
        env.entry("LOOM_AGENT_MODEL".into()).or_insert(model);
    }
    env
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionRecord {
    actor_id: String,
    scope: ScopeRef,
    session_id: String,
    created_at: String,
    last_used_at: String,
    command_signature: String,
}

fn session_path(cfg: &InteractiveCommandConfig, scope: &ScopeRef) -> PathBuf {
    let kind = match scope.kind {
        ScopeKind::Thread => "thread",
        ScopeKind::Channel => "channel",
    };
    cfg.sessions_dir
        .join(&cfg.actor_id)
        .join(format!("{kind}-{}.json", scope.id))
}

fn load_session(cfg: &InteractiveCommandConfig, scope: &ScopeRef) -> Option<SessionRecord> {
    let path = session_path(cfg, scope);
    let text = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&text).ok()
}

fn save_session(
    cfg: &InteractiveCommandConfig,
    scope: &ScopeRef,
    session_id: &str,
    command_signature: &str,
) -> std::io::Result<()> {
    let path = session_path(cfg, scope);
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
    std::fs::write(path, serde_json::to_string_pretty(&record)?)
}

fn delete_session(cfg: &InteractiveCommandConfig, scope: &ScopeRef) -> std::io::Result<()> {
    let path = session_path(cfg, scope);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

fn looks_like_signed_thinking_replay_error(stderr: &str, stdout: &str) -> bool {
    let s = format!("{stderr}\n{stdout}").to_ascii_lowercase();
    (s.contains("thinking") || s.contains("redacted_thinking"))
        && s.contains("latest assistant message")
        && s.contains("cannot be modified")
        && s.contains("original response")
}

fn contains_sentinel_line(text: &str, sentinel: &str) -> bool {
    text.lines().any(|line| line.trim() == sentinel)
}

fn text_before_sentinel(text: &str, sentinel: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        if line.trim() == sentinel {
            break;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.trim().to_string()
    } else {
        let cut = s
            .char_indices()
            .map(|(idx, _)| idx)
            .take_while(|idx| *idx <= max)
            .last()
            .unwrap_or(0);
        let mut t = s[..cut].trim().to_string();
        t.push('…');
        t
    }
}

fn apply_kill_policy(policy: &InteractiveKillAction, pid: u32) -> Result<(), String> {
    apply_kill_kind(policy.action, pid)?;
    if let Some(grace_ms) = policy.grace_ms {
        std::thread::sleep(Duration::from_millis(grace_ms));
    }
    if let Some(fallback) = policy.fallback {
        apply_kill_kind(fallback, pid)?;
    }
    Ok(())
}

#[cfg(unix)]
fn apply_kill_kind(kind: InteractiveKillKind, pid: u32) -> Result<(), String> {
    let sig = match kind {
        InteractiveKillKind::Sigterm | InteractiveKillKind::CtrlC => Some(libc::SIGTERM),
        InteractiveKillKind::Sigkill => Some(libc::SIGKILL),
        InteractiveKillKind::StdinEof | InteractiveKillKind::CtrlD | InteractiveKillKind::None => {
            None
        }
    };
    let Some(sig) = sig else {
        return Ok(());
    };
    let rc = unsafe { libc::kill(pid as libc::pid_t, sig) };
    if rc == 0 {
        Ok(())
    } else {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ESRCH) {
            Ok(())
        } else {
            Err(format!("kill({pid}, {sig}) failed: {err}"))
        }
    }
}

#[cfg(not(unix))]
fn apply_kill_kind(_kind: InteractiveKillKind, _pid: u32) -> Result<(), String> {
    Err("interactive command kill policy is not implemented for this platform".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proto::methods::{InteractiveCompletionContractSpec, InteractiveCompletionSpec};

    fn cfg(tmp: PathBuf) -> InteractiveCommandConfig {
        InteractiveCommandConfig::new(
            "actor_demo".into(),
            "printf".into(),
            &[],
            BTreeMap::new(),
            None,
            Vec::new(),
            InteractiveCommandSpec {
                session: proto::methods::InteractiveSessionSpec {
                    new_args: vec![
                        "{prompt}".into(),
                        "--session-id".into(),
                        "{session_id}".into(),
                    ],
                    resume_args: vec!["{prompt}".into(), "--resume".into(), "{session_id}".into()],
                    ..Default::default()
                },
                prompt: proto::methods::InteractivePromptSpec {
                    completion_contract: InteractiveCompletionContractSpec {
                        sentinel: "__DONE__".into(),
                        instruction: "finish with __DONE__".into(),
                    },
                    ..Default::default()
                },
                completion: InteractiveCompletionSpec {
                    max_turn_ms: 5000,
                    ..Default::default()
                },
                ..Default::default()
            },
            None,
            tmp.join("sessions"),
            tmp.join("profile"),
        )
    }

    fn scope(kind: ScopeKind, id: &str) -> ScopeRef {
        ScopeRef {
            kind,
            id: id.into(),
        }
    }

    fn prompt(scope: ScopeRef, content: &str) -> AdapterPrompt {
        AdapterPrompt {
            scope,
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
    fn session_path_distinguishes_thread_and_channel() {
        let cfg = cfg(std::env::temp_dir().join(format!("loom-it-{}", Uuid::new_v4())));
        let thread = session_path(&cfg, &scope(ScopeKind::Thread, "abc"));
        let channel = session_path(&cfg, &scope(ScopeKind::Channel, "abc"));
        assert_ne!(thread, channel);
        assert!(thread.ends_with("thread-abc.json"));
        assert!(channel.ends_with("channel-abc.json"));
    }

    #[test]
    fn argv_appends_model_when_configured() {
        let mut cfg = cfg(std::env::temp_dir().join(format!("loom-it-{}", Uuid::new_v4())));
        cfg.model = Some("claude-sonnet".into());
        let req = prompt(scope(ScopeKind::Thread, "t"), "hello");
        let mut argv = expand_argv(&cfg.spec.session.new_args, &cfg, &req, Some("sid"), "hello");
        append_provider_args(&cfg, &req, &mut argv).unwrap();
        assert_eq!(
            argv.last().map(String::as_str),
            Some("--model=claude-sonnet")
        );
    }

    #[test]
    fn argv_uses_configured_model_args_when_present() {
        let mut cfg = cfg(std::env::temp_dir().join(format!("loom-it-{}", Uuid::new_v4())));
        cfg.model = Some("claude-sonnet".into());
        cfg.model_args = vec!["--model".into(), "{model}".into()];
        let req = prompt(scope(ScopeKind::Thread, "t"), "hello");
        let mut argv = expand_argv(&cfg.spec.session.new_args, &cfg, &req, Some("sid"), "hello");
        append_provider_args(&cfg, &req, &mut argv).unwrap();
        assert_eq!(
            &argv[argv.len() - 2..],
            ["--model".to_string(), "claude-sonnet".to_string()]
        );
    }

    #[test]
    fn argv_omits_model_when_missing() {
        let cfg = cfg(std::env::temp_dir().join(format!("loom-it-{}", Uuid::new_v4())));
        let req = prompt(scope(ScopeKind::Thread, "t"), "hello");
        let mut argv = expand_argv(&cfg.spec.session.new_args, &cfg, &req, Some("sid"), "hello");
        append_provider_args(&cfg, &req, &mut argv).unwrap();
        assert!(!argv.iter().any(|a| a.starts_with("--model=")));
    }

    #[test]
    fn prompt_injects_completion_contract_before_user_message() {
        let cfg = cfg(std::env::temp_dir().join(format!("loom-it-{}", Uuid::new_v4())));
        let req = prompt(
            scope(ScopeKind::Thread, "t"),
            "context\n\n=== User message ===\nhello",
        );
        let built = build_interactive_prompt(&cfg, &req, "sid");
        assert!(built.contains("=== Loom interactive command completion contract ==="));
        assert!(
            built.find("completion contract").unwrap()
                < built.find("=== User message ===").unwrap()
        );
    }

    #[test]
    fn sentinel_helpers_strip_final_marker() {
        let text = "hello\n__DONE__\nignored\n";
        assert!(contains_sentinel_line(text, "__DONE__"));
        assert_eq!(text_before_sentinel(text, "__DONE__"), "hello\n");
    }

    #[test]
    fn truncate_is_char_boundary_safe() {
        let text = "修复 delivery 中文 stderr 截断 panic";
        let truncated = truncate(text, 10);
        assert!(truncated.ends_with('…'));
        assert!(truncated.len() <= 13);
    }

    #[test]
    fn save_session_updates_last_used_on_resume() {
        let root = std::env::temp_dir().join(format!("loom-it-{}", Uuid::new_v4()));
        let cfg = cfg(root.clone());
        let scope = scope(ScopeKind::Thread, "thr");

        save_session(&cfg, &scope, "sid", "sig").unwrap();
        let first = load_session(&cfg, &scope).expect("first session record");
        std::thread::sleep(std::time::Duration::from_millis(20));

        save_session(&cfg, &scope, "sid", "sig").unwrap();
        let second = load_session(&cfg, &scope).expect("updated session record");

        assert_eq!(second.created_at, first.created_at);
        assert!(second.last_used_at > first.last_used_at);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn signed_thinking_replay_error_matches_claude_400() {
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

    #[cfg(unix)]
    #[test]
    fn signed_thinking_replay_error_drops_saved_interactive_session() {
        let root = std::env::temp_dir().join(format!("loom-it-{}", Uuid::new_v4()));
        let mut cfg = cfg(root.clone());
        cfg.command = "sh".into();
        cfg.spec.session.resume_args = vec![
            "-c".into(),
            "printf '%s\\n' 'API Error: 400 messages.7.content.3: thinking or redacted_thinking blocks in the latest assistant message cannot be modified. These blocks must remain as they were in the original response.' >&2; exit 1".into(),
        ];
        let scope = scope(ScopeKind::Thread, "thr");
        let req = prompt(scope.clone(), "ignored");
        let signature = command_signature_for_prompt(&cfg, &req).expect("signature");
        save_session(&cfg, &scope, "bad_sid", &signature).expect("save session");
        let (tx, _rx) = mpsc::unbounded_channel();
        let slot = Arc::new(Mutex::new(InFlight::default()));

        run_prompt(cfg.clone(), req, tx, slot).expect("run prompt");

        assert!(load_session(&cfg, &scope).is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn successful_done_requires_sentinel_and_saves_session() {
        let root = std::env::temp_dir().join(format!("loom-it-{}", Uuid::new_v4()));
        let mut cfg = cfg(root.clone());
        cfg.command = "sh".into();
        cfg.spec.session.new_args = vec!["-c".into(), "printf 'hello\\n__DONE__\\n'".into()];
        cfg.spec.session.resume_args = cfg.spec.session.new_args.clone();
        cfg.spec.kill.on_complete.grace_ms = Some(0);
        cfg.spec.kill.on_complete.fallback = None;
        let adapter = InteractiveCommandAdapter::new(cfg.clone());
        let (tx, mut rx) = mpsc::unbounded_channel();
        adapter.start(tx).await.unwrap();
        adapter
            .send_prompt(prompt(scope(ScopeKind::Thread, "thr"), "ignored"))
            .await
            .unwrap();

        let mut text = String::new();
        let mut finished = None;
        while let Some(event) = rx.recv().await {
            match event {
                AdapterEvent::Text { content, .. } => text.push_str(&content),
                AdapterEvent::Finished {
                    success, summary, ..
                } => {
                    finished = Some((success, summary));
                    break;
                }
                _ => {}
            }
        }
        assert_eq!(text, "hello");
        assert_eq!(finished, Some((true, String::new())));
        assert!(session_path(&cfg, &scope(ScopeKind::Thread, "thr")).exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn process_exit_without_sentinel_is_failed_done() {
        let root = std::env::temp_dir().join(format!("loom-it-{}", Uuid::new_v4()));
        let mut cfg = cfg(root.clone());
        cfg.command = "sh".into();
        cfg.spec.session.new_args = vec!["-c".into(), "printf 'hello\\n'".into()];
        cfg.spec.session.resume_args = cfg.spec.session.new_args.clone();
        let adapter = InteractiveCommandAdapter::new(cfg.clone());
        let (tx, mut rx) = mpsc::unbounded_channel();
        adapter.start(tx).await.unwrap();
        adapter
            .send_prompt(prompt(scope(ScopeKind::Thread, "thr"), "ignored"))
            .await
            .unwrap();

        let mut finished = None;
        while let Some(event) = rx.recv().await {
            if let AdapterEvent::Finished {
                success, summary, ..
            } = event
            {
                finished = Some((success, summary));
                break;
            }
        }
        let (success, summary) = finished.expect("finished event");
        assert!(!success);
        assert!(summary.contains("completion sentinel"));
        assert!(!session_path(&cfg, &scope(ScopeKind::Thread, "thr")).exists());
        let _ = std::fs::remove_dir_all(root);
    }
}
