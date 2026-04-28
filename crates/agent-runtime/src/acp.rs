//! Minimal ACP (Agent Client Protocol) stdio adapter.
//!
//! Lifted and slimmed from joi/src-tauri/src/agents/acp.rs — only the wire
//! handling we need:
//!   initialize → optional authenticate → (lazy per-scope session/new) →
//!   session/prompt loop, inbound session/update streams (text + tool_call),
//!   session/request_permission, stop via session/cancel + child kill.
//!
//! Sessions are allocated lazily on the first prompt for each `scope`, so the
//! same agent driving two channels concurrently uses two independent ACP
//! sessions — preventing the cross-talk that biting v0 had when only a single
//! `session_id` existed per child.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::ErrorKind;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{
    Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Output, Stdio,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::Mutex;
use proto::types::ScopeRef;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use uuid::Uuid;

use super::adapter::{ActionChoice, Adapter, AdapterEvent, AdapterPrompt, AdapterStartInfo};

const SHELL_ENV_CAPTURE_TIMEOUT: Duration = Duration::from_secs(8);
const TERMINAL_AUTH_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone)]
pub struct AcpConfig {
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub process_cwd: PathBuf,
    pub auth_method: Option<String>,
    /// Passed verbatim as `mcpServers` in every `session/new`. Callers
    /// synthesize this (e.g. from `memory.delivery.mcp = true` + the joi
    /// binary path). An empty vec reproduces the pre-envelope behavior of
    /// always sending `mcpServers: []`.
    #[allow(dead_code)]
    pub mcp_servers: Vec<Value>,
}

/// Internal start info. Sessions are no longer minted at start — they are
/// created on demand per scope inside `send_prompt`.
#[derive(Debug, Clone)]
struct AcpStartInfo {
    pid: Option<u32>,
    #[allow(dead_code)]
    agent_info: Option<Value>,
    #[allow(dead_code)]
    agent_capabilities: Option<Value>,
}

struct PendingPermission {
    request_id: Value,
    option_ids: HashSet<String>,
    #[allow(dead_code)]
    scope: ScopeRef,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AcpAuthMethod {
    id: String,
    terminal_auth: Option<TerminalAuthCommand>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TerminalAuthCommand {
    command: String,
    args: Vec<String>,
    env: BTreeMap<String, String>,
    label: Option<String>,
}

struct AcpShared {
    stdin: Mutex<ChildStdin>,
    next_request_id: AtomicU64,
    response_waiters: Mutex<HashMap<String, std::sync::mpsc::Sender<Result<Value, String>>>>,
    auth_method: Mutex<Option<AcpAuthMethod>>,
    auth_attempted: Mutex<bool>,
    process_cwd: PathBuf,
    process_env: BTreeMap<String, String>,
    /// Outstanding `session/prompt` requests we are waiting on. Maps request id
    /// → originating scope so the asynchronous `Finished` event can be tagged
    /// with the right scope when the response comes back.
    in_flight_prompts: Mutex<HashMap<String, ScopeRef>>,
    pending_permissions: Mutex<HashMap<String, PendingPermission>>,
    /// Reverse map populated when `send_prompt` mints a new ACP session for a
    /// scope. Inbound `session/update` and `session/request_permission` carry
    /// `params.sessionId`; the stdout reader uses this to tag the resulting
    /// `AdapterEvent` with the originating scope.
    sessions_by_id: Mutex<HashMap<String, ScopeRef>>,
    action_namespace: String,
    /// Forwarded verbatim as the `mcpServers` array on every `session/new`.
    /// Populated at start from `AcpConfig.mcp_servers`; immutable thereafter.
    mcp_servers: Vec<Value>,
    event_sender: mpsc::UnboundedSender<AdapterEvent>,
}

pub struct AcpAdapter {
    config: AcpConfig,
    inner: Mutex<AcpInner>,
}

struct AcpInner {
    child: Option<Child>,
    shared: Option<Arc<AcpShared>>,
    /// scope.id → ACP session id. Built up lazily by `send_prompt`.
    sessions: HashMap<String, String>,
}

impl AcpAdapter {
    pub fn new(config: AcpConfig) -> Self {
        Self {
            config,
            inner: Mutex::new(AcpInner {
                child: None,
                shared: None,
                sessions: HashMap::new(),
            }),
        }
    }

    async fn start_internal(
        &self,
        event_sender: mpsc::UnboundedSender<AdapterEvent>,
    ) -> Result<AcpStartInfo, String> {
        let cfg = self.config.clone();
        tokio::task::spawn_blocking(move || start_blocking(cfg, event_sender))
            .await
            .map_err(|e| e.to_string())?
            .map(|(info, child, shared)| {
                let mut inner = self.inner.lock();
                inner.child = Some(child);
                inner.shared = Some(shared);
                drop(inner);
                info
            })
    }

    async fn send_prompt_internal(&self, prompt: AdapterPrompt) -> Result<(), String> {
        let scope = prompt.scope.clone();
        // Snapshot what we need under the parking_lot guard, then drop it
        // before any spawn_blocking / await.
        let (shared, existing_sid) = {
            let inner = self.inner.lock();
            let shared = inner.shared.clone().ok_or("ACP agent not running")?;
            (shared, inner.sessions.get(&scope.id).cloned())
        };

        // Lazy session/new for this scope. The runtime layer serializes prompts
        // per scope, so we shouldn't see two concurrent send_prompt calls for
        // the same scope racing on this allocation.
        let session_id = match existing_sid {
            Some(sid) => sid,
            None => {
                let scope_for_new = scope.clone();
                let shared_for_new = shared.clone();
                let mcp_servers = shared.mcp_servers.clone();
                let cwd = prompt.cwd.clone();
                let new_sid = tokio::task::spawn_blocking(move || -> Result<String, String> {
                    std::fs::create_dir_all(&cwd).map_err(|e| {
                        format!(
                            "Failed to create ACP session cwd `{}`: {}",
                            cwd.display(),
                            e
                        )
                    })?;
                    let res =
                        request_new_session_with_auth_retry(&shared_for_new, &cwd, &mcp_servers)?;
                    let sid = res
                        .get("sessionId")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| "ACP agent did not return sessionId".to_string())?
                        .to_string();
                    shared_for_new
                        .sessions_by_id
                        .lock()
                        .insert(sid.clone(), scope_for_new);
                    Ok(sid)
                })
                .await
                .map_err(|e| e.to_string())??;
                self.inner
                    .lock()
                    .sessions
                    .insert(scope.id.clone(), new_sid.clone());
                new_sid
            }
        };

        let scope_for_prompt = scope.clone();
        let content = prompt.content;
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            let request_id = shared.next_request_id_string();
            shared
                .in_flight_prompts
                .lock()
                .insert(request_id.clone(), scope_for_prompt.clone());
            eprintln!(
                "[joi:acp] session/prompt sent id={} session={} scope={} (in_flight={})",
                request_id,
                session_id,
                scope_for_prompt.id,
                shared.in_flight_prompts.lock().len()
            );
            let request = json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "method": "session/prompt",
                "params": {
                    "sessionId": session_id,
                    "prompt": [
                        { "type": "text", "text": content }
                    ],
                },
            });
            if let Err(err) = shared.write_message(&request) {
                shared.in_flight_prompts.lock().remove(&request_id);
                return Err(err);
            }
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())?
    }

    async fn respond_permission_internal(
        &self,
        action_id: String,
        option_id: String,
    ) -> Result<(), String> {
        let shared = {
            let inner = self.inner.lock();
            inner.shared.clone().ok_or("ACP agent not running")?
        };
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            let pending = shared
                .pending_permissions
                .lock()
                .remove(&action_id)
                .ok_or_else(|| format!("No pending ACP action with id {}", action_id))?;
            if !pending.option_ids.contains(&option_id) {
                return Err(format!(
                    "Invalid ACP option `{}` for action {}",
                    option_id, action_id
                ));
            }
            let response = json!({
                "jsonrpc": "2.0",
                "id": pending.request_id,
                "result": {
                    "outcome": {
                        "outcome": "selected",
                        "optionId": option_id.clone(),
                    }
                }
            });
            shared.write_message(&response)?;
            eprintln!(
                "[joi:acp] -> permission response id={} option={}",
                response
                    .get("id")
                    .and_then(request_id_key)
                    .unwrap_or_else(|| "?".into()),
                option_id
            );
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())?
    }

    async fn cancel_internal(&self, scope: ScopeRef) -> Result<(), String> {
        // Look up the per-scope ACP session id. No session means no in-flight
        // prompt for this scope — silent no-op (cancel is idempotent).
        let (shared, session_id) = {
            let inner = self.inner.lock();
            let Some(shared) = inner.shared.clone() else {
                return Ok(());
            };
            let Some(sid) = inner.sessions.get(&scope.id).cloned() else {
                return Ok(());
            };
            (shared, sid)
        };
        // Fire-and-forget: the agent's pending session/prompt response will
        // come back with stopReason="cancelled" and run through the existing
        // Finished path. We don't wait on it here.
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            shared.write_message(&json!({
                "jsonrpc": "2.0",
                "method": "session/cancel",
                "params": { "sessionId": session_id },
            }))
        })
        .await
        .map_err(|e| e.to_string())?
    }

    async fn stop_internal(&self) -> Result<(), String> {
        let (shared, session_ids, mut child) = {
            let mut inner = self.inner.lock();
            let session_ids: Vec<String> = inner.sessions.values().cloned().collect();
            inner.sessions.clear();
            (inner.shared.take(), session_ids, inner.child.take())
        };
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            if let Some(shared) = shared.as_ref() {
                for session_id in &session_ids {
                    let _ = shared.write_message(&json!({
                        "jsonrpc": "2.0",
                        "method": "session/cancel",
                        "params": { "sessionId": session_id }
                    }));
                }
            }
            if let Some(ref mut c) = child {
                drop(c.stdin.take());
                match c.try_wait() {
                    Ok(Some(_)) => {}
                    _ => {
                        let _ = c.kill();
                        let _ = c.wait();
                    }
                }
            }
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())?
    }
}

#[async_trait]
impl Adapter for AcpAdapter {
    async fn start(
        &self,
        events: mpsc::UnboundedSender<AdapterEvent>,
    ) -> Result<AdapterStartInfo, String> {
        let info = self.start_internal(events).await?;
        Ok(AdapterStartInfo {
            pid: info.pid,
            // Sessions are lazy per scope; nothing useful to expose here.
            session_id: None,
        })
    }

    async fn send_prompt(&self, prompt: AdapterPrompt) -> Result<(), String> {
        self.send_prompt_internal(prompt).await
    }

    async fn respond_action(&self, request_id: String, option_id: String) -> Result<(), String> {
        self.respond_permission_internal(request_id, option_id)
            .await
    }

    async fn cancel(&self, scope: ScopeRef) -> Result<(), String> {
        self.cancel_internal(scope).await
    }

    async fn stop(&self) -> Result<(), String> {
        self.stop_internal().await
    }
}

fn start_blocking(
    cfg: AcpConfig,
    event_sender: mpsc::UnboundedSender<AdapterEvent>,
) -> Result<(AcpStartInfo, Child, Arc<AcpShared>), String> {
    let process_cwd = if cfg.process_cwd.is_absolute() {
        cfg.process_cwd.clone()
    } else {
        std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(&cfg.process_cwd)
    };
    std::fs::create_dir_all(&process_cwd).map_err(|e| {
        format!(
            "Failed to create ACP workdir `{}`: {}",
            process_cwd.display(),
            e
        )
    })?;
    let mut cmd = Command::new(&cfg.command);
    cmd.args(&cfg.args)
        .current_dir(&process_cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut path_for_error = std::env::var("PATH").unwrap_or_default();
    let mut process_env = BTreeMap::new();
    if should_capture_shell_env() {
        match capture_login_shell_env(&process_cwd, SHELL_ENV_CAPTURE_TIMEOUT) {
            Ok(env) => {
                eprintln!(
                    "[joi:acp] captured {} env vars from login shell for ACP child",
                    env.len()
                );
                if let Some(path) = env.get("PATH") {
                    path_for_error = path.clone();
                }
                process_env.extend(env);
            }
            Err(err) => {
                eprintln!("[joi:acp] login shell env capture skipped: {err}");
            }
        }
    }
    for (k, v) in &cfg.env {
        if k == "PATH" {
            path_for_error = v.clone();
        }
        process_env.insert(k.clone(), v.clone());
    }
    cmd.envs(&process_env);
    let mut child = cmd
        .spawn()
        .map_err(|e| spawn_error_message(&cfg.command, &process_cwd, &path_for_error, e))?;
    let stdin = child.stdin.take().ok_or("Failed to open agent stdin")?;
    let stdout = child.stdout.take().ok_or("Failed to open agent stdout")?;
    let stderr = child.stderr.take().ok_or("Failed to open agent stderr")?;
    let shared = Arc::new(AcpShared {
        stdin: Mutex::new(stdin),
        next_request_id: AtomicU64::new(1),
        response_waiters: Mutex::new(HashMap::new()),
        auth_method: Mutex::new(None),
        auth_attempted: Mutex::new(false),
        process_cwd: process_cwd.clone(),
        process_env,
        in_flight_prompts: Mutex::new(HashMap::new()),
        pending_permissions: Mutex::new(HashMap::new()),
        sessions_by_id: Mutex::new(HashMap::new()),
        action_namespace: Uuid::new_v4().to_string(),
        mcp_servers: cfg.mcp_servers.clone(),
        event_sender: event_sender.clone(),
    });
    spawn_stdout_reader(stdout, shared.clone());
    spawn_stderr_drain(stderr);

    // Cold-start budget: some agents (e.g. opencode) refresh remote model
    // registries and JIT large bundles before they answer `initialize`, so the
    // first round-trip can easily exceed 15 s. Use a generous handshake budget
    // here; per-prompt requests stay snappier.
    let initialize = shared.request_and_wait(
        "initialize",
        json!({
            "protocolVersion": 1,
            "clientInfo": {
                "name": "joi-server",
                "title": "Joi Server",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "clientCapabilities": {
                "fs": { "readTextFile": false, "writeTextFile": false },
                "terminal": false,
                "auth": { "terminal": true },
                "_meta": { "terminal-auth": true },
            },
        }),
        Duration::from_secs(60),
    )?;

    let configured_auth_method = cfg
        .auth_method
        .as_deref()
        .filter(|method| !method.trim().is_empty());
    let auth_method = select_initialize_auth_method(&initialize, configured_auth_method);
    *shared.auth_method.lock() = auth_method.clone();

    if let Some(method) = configured_auth_method {
        if let Some(auth_method) = auth_method
            .as_ref()
            .filter(|method| method.terminal_auth.is_some())
        {
            run_terminal_auth(&shared, auth_method)?;
        } else {
            *shared.auth_attempted.lock() = true;
            shared.request_and_wait(
                "authenticate",
                json!({ "methodId": method }),
                Duration::from_secs(30),
            )?;
        }
    }

    let _ = event_sender.send(AdapterEvent::StatusChange {
        scope: None,
        status: "idle".into(),
    });

    let pid = child.id();
    let info = AcpStartInfo {
        pid: Some(pid),
        agent_info: initialize.get("agentInfo").cloned(),
        agent_capabilities: initialize.get("agentCapabilities").cloned(),
    };
    Ok((info, child, shared))
}

fn request_new_session_with_auth_retry(
    shared: &Arc<AcpShared>,
    cwd: &Path,
    mcp_servers: &[Value],
) -> Result<Value, String> {
    let params = json!({
        "cwd": cwd.to_string_lossy(),
        "mcpServers": mcp_servers,
    });
    match shared.request_and_wait("session/new", params.clone(), Duration::from_secs(30)) {
        Ok(value) => Ok(value),
        Err(err) if is_auth_required_error(&err) => {
            let method = shared.auth_method.lock().clone();
            let Some(method) = method else {
                return Err(format!(
                    "{err}; agent requires authentication but did not advertise a single \
                     auth method. Set transport.authMethod in the agent spec."
                ));
            };
            {
                let mut attempted = shared.auth_attempted.lock();
                if *attempted {
                    return Err(err);
                }
                *attempted = true;
            }
            eprintln!(
                "[joi:acp] session/new requires authentication; trying auth method `{}`",
                method.id
            );
            if method.terminal_auth.is_some() {
                run_terminal_auth(shared, &method)?;
                match shared.request_and_wait(
                    "session/new",
                    params.clone(),
                    Duration::from_secs(30),
                ) {
                    Ok(value) => return Ok(value),
                    Err(retry_err) if is_auth_required_error(&retry_err) => {
                        eprintln!(
                            "[joi:acp] terminal auth completed but session/new still requires \
                             authentication; trying ACP authenticate `{}`",
                            method.id
                        );
                    }
                    Err(retry_err) => return Err(retry_err),
                }
            }
            shared.request_and_wait(
                "authenticate",
                json!({ "methodId": method.id }),
                Duration::from_secs(30),
            )?;
            shared.request_and_wait("session/new", params, Duration::from_secs(30))
        }
        Err(err) => Err(err),
    }
}

fn select_initialize_auth_method(
    initialize: &Value,
    configured: Option<&str>,
) -> Option<AcpAuthMethod> {
    let auth_methods = initialize.get("authMethods")?.as_array()?;
    if let Some(configured) = configured {
        return auth_methods
            .iter()
            .find_map(|method| {
                let parsed = parse_auth_method(method)?;
                (parsed.id == configured).then_some(parsed)
            })
            .or_else(|| {
                Some(AcpAuthMethod {
                    id: configured.to_string(),
                    terminal_auth: None,
                })
            });
    }
    if auth_methods.len() != 1 {
        return None;
    }
    parse_auth_method(&auth_methods[0])
}

fn parse_auth_method(method: &Value) -> Option<AcpAuthMethod> {
    let id = method.get("id").and_then(|id| id.as_str())?.to_string();
    Some(AcpAuthMethod {
        id,
        terminal_auth: parse_terminal_auth_command(method),
    })
}

fn parse_terminal_auth_command(method: &Value) -> Option<TerminalAuthCommand> {
    let terminal_auth = method.get("_meta")?.get("terminal-auth")?;
    let command = terminal_auth.get("command")?.as_str()?.to_string();
    let args = terminal_auth
        .get("args")
        .and_then(|args| args.as_array())
        .map(|args| {
            args.iter()
                .filter_map(|arg| arg.as_str().map(ToString::to_string))
                .collect()
        })
        .unwrap_or_default();
    let env = terminal_auth
        .get("env")
        .and_then(|env| env.as_object())
        .map(|env| {
            env.iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|value| (key.clone(), value.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();
    let label = terminal_auth
        .get("label")
        .and_then(|label| label.as_str())
        .map(ToString::to_string);
    Some(TerminalAuthCommand {
        command,
        args,
        env,
        label,
    })
}

fn run_terminal_auth(shared: &AcpShared, method: &AcpAuthMethod) -> Result<(), String> {
    let terminal = method
        .terminal_auth
        .as_ref()
        .ok_or_else(|| format!("auth method `{}` does not provide terminal auth", method.id))?;
    eprintln!(
        "[joi:acp] running terminal auth command `{}`{}",
        format_terminal_auth_command(terminal),
        terminal
            .label
            .as_deref()
            .map(|label| format!(" ({label})"))
            .unwrap_or_default()
    );
    let mut cmd = Command::new(&terminal.command);
    cmd.args(&terminal.args)
        .current_dir(&shared.process_cwd)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .envs(&shared.process_env)
        .envs(&terminal.env);
    let child = cmd.spawn().map_err(|err| {
        format!(
            "failed to start terminal auth command `{}`: {err}",
            format_terminal_auth_command(terminal)
        )
    })?;
    let status = wait_status_with_timeout(child, TERMINAL_AUTH_TIMEOUT)?;
    if status.success() {
        return Ok(());
    }
    Err(format!(
        "terminal auth command `{}` exited with {}",
        format_terminal_auth_command(terminal),
        status,
    ))
}

fn format_terminal_auth_command(terminal: &TerminalAuthCommand) -> String {
    let mut parts = vec![shell_quote(&terminal.command)];
    parts.extend(terminal.args.iter().map(|arg| shell_quote(arg)));
    parts.join(" ")
}

fn is_auth_required_error(err: &str) -> bool {
    if err.contains("Authentication required") {
        return true;
    }
    let Ok(value) = serde_json::from_str::<Value>(err) else {
        return false;
    };
    value
        .get("message")
        .and_then(|message| message.as_str())
        .is_some_and(|message| message == "Authentication required")
}

fn spawn_error_message(command: &str, cwd: &Path, path: &str, err: std::io::Error) -> String {
    if err.kind() == ErrorKind::NotFound {
        return format!(
            "Failed to start ACP command `{command}`: command not found on PATH \
             (cwd `{}`, PATH `{}`)",
            cwd.display(),
            truncate_for_log(&path, 500),
        );
    }
    format!(
        "Failed to start ACP command `{command}` in `{}`: {err}",
        cwd.display()
    )
}

fn should_capture_shell_env() -> bool {
    match std::env::var("JOI_ACP_SHELL_ENV") {
        Ok(value) => !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "no" | "off"
        ),
        Err(_) => true,
    }
}

#[cfg(unix)]
fn capture_login_shell_env(
    cwd: &Path,
    timeout: Duration,
) -> Result<BTreeMap<String, String>, String> {
    let shell = std::env::var_os("SHELL")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/bin/sh"));
    let start = format!("__JOI_ACP_ENV_START_{}__", Uuid::new_v4().simple());
    let end = format!("__JOI_ACP_ENV_END_{}__", Uuid::new_v4().simple());
    let cwd = cwd.to_string_lossy();
    let command = format!(
        "cd {}; printf '{}\\n'; env -0; printf '\\n{}\\n'",
        shell_quote(&cwd),
        start,
        end
    );

    let child = Command::new(&shell)
        .arg("-l")
        .arg("-i")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to start `{}`: {e}", shell.display()))?;

    let output = wait_with_timeout(child, timeout)?;
    if !output.status.success() {
        return Err(format!(
            "`{}` exited with {}; stderr: {}",
            shell.display(),
            output.status,
            truncate_for_log(&String::from_utf8_lossy(&output.stderr), 300)
        ));
    }

    parse_env_output(&output.stdout, &start, &end)
}

#[cfg(not(unix))]
fn capture_login_shell_env(
    _cwd: &Path,
    _timeout: Duration,
) -> Result<BTreeMap<String, String>, String> {
    Err("login shell env capture is only implemented on Unix".into())
}

fn wait_with_timeout(child: Child, timeout: Duration) -> Result<Output, String> {
    let pid = child.id();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });

    match rx.recv_timeout(timeout) {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(err)) => Err(format!("failed to read shell env output: {err}")),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            kill_process(pid);
            Err(format!("timed out after {}s", timeout.as_secs()))
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Err("shell env capture worker disconnected".into())
        }
    }
}

fn wait_status_with_timeout(mut child: Child, timeout: Duration) -> Result<ExitStatus, String> {
    let pid = child.id();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait());
    });

    match rx.recv_timeout(timeout) {
        Ok(Ok(status)) => Ok(status),
        Ok(Err(err)) => Err(format!("failed to wait for process: {err}")),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            kill_process(pid);
            Err(format!("timed out after {}s", timeout.as_secs()))
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Err("process wait worker disconnected".into())
        }
    }
}

#[cfg(unix)]
fn kill_process(pid: u32) {
    unsafe {
        libc::kill(pid as i32, libc::SIGKILL);
    }
}

#[cfg(not(unix))]
fn kill_process(_pid: u32) {}

fn parse_env_output(
    stdout: &[u8],
    start: &str,
    end: &str,
) -> Result<BTreeMap<String, String>, String> {
    let start_pos = find_bytes(stdout, start.as_bytes())
        .ok_or_else(|| "login shell env output did not contain start marker".to_string())?;
    let mut body_start = start_pos + start.len();
    if stdout.get(body_start) == Some(&b'\r') {
        body_start += 1;
    }
    if stdout.get(body_start) == Some(&b'\n') {
        body_start += 1;
    }
    let end_rel = find_bytes(&stdout[body_start..], end.as_bytes())
        .ok_or_else(|| "login shell env output did not contain end marker".to_string())?;
    let mut body_end = body_start + end_rel;
    while body_end > body_start && matches!(stdout[body_end - 1], b'\n' | b'\r') {
        body_end -= 1;
    }

    let mut env = BTreeMap::new();
    for raw in stdout[body_start..body_end].split(|b| *b == 0) {
        let raw = trim_ascii_newlines(raw);
        if raw.is_empty() {
            continue;
        }
        let Some(eq) = raw.iter().position(|b| *b == b'=') else {
            continue;
        };
        if eq == 0 {
            continue;
        }
        let key = String::from_utf8_lossy(&raw[..eq]).into_owned();
        let value = String::from_utf8_lossy(&raw[eq + 1..]).into_owned();
        env.insert(key, value);
    }
    if env.is_empty() {
        return Err("login shell env output contained no KEY=VALUE entries".into());
    }
    Ok(env)
}

fn trim_ascii_newlines(bytes: &[u8]) -> &[u8] {
    let mut start = 0;
    let mut end = bytes.len();
    while start < end && matches!(bytes[start], b'\n' | b'\r') {
        start += 1;
    }
    while end > start && matches!(bytes[end - 1], b'\n' | b'\r') {
        end -= 1;
    }
    &bytes[start..end]
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

impl AcpShared {
    fn next_request_id_string(&self) -> String {
        self.next_request_id
            .fetch_add(1, Ordering::Relaxed)
            .to_string()
    }

    fn request_and_wait(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, String> {
        let request_id = self.next_request_id_string();
        let (tx, rx) = std::sync::mpsc::channel();
        self.response_waiters.lock().insert(request_id.clone(), tx);
        let req = json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params,
        });
        if let Err(err) = self.write_message(&req) {
            self.response_waiters.lock().remove(&request_id);
            return Err(err);
        }
        rx.recv_timeout(timeout)
            .map_err(|_| format!("Timed out waiting for ACP response to `{}`", method))?
    }

    fn write_message(&self, message: &Value) -> Result<(), String> {
        let serialized = serde_json::to_string(message).map_err(|e| e.to_string())?;
        let mut stdin = self.stdin.lock();
        stdin
            .write_all(serialized.as_bytes())
            .map_err(|e| e.to_string())?;
        stdin.write_all(b"\n").map_err(|e| e.to_string())?;
        stdin.flush().map_err(|e| e.to_string())
    }
}

fn spawn_stdout_reader(stdout: ChildStdout, shared: Arc<AcpShared>) {
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(err) => {
                    let _ = shared.event_sender.send(AdapterEvent::Error {
                        scope: None,
                        message: format!("Failed to read ACP output: {}", err),
                    });
                    break;
                }
            };
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            eprintln!("[joi:acp] <- {}", truncate_for_log(trimmed, 400));
            let message: Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(err) => {
                    let _ = shared.event_sender.send(AdapterEvent::Error {
                        scope: None,
                        message: format!("Invalid ACP JSON: {}", err),
                    });
                    continue;
                }
            };
            handle_incoming_message(&shared, message);
        }
        // Stream closed: fail every in-flight synchronous request, AND every
        // outstanding session/prompt. Without the per-prompt fan-out the
        // runtime would never see Finished and would leave the turn open
        // forever.
        fail_pending_waiters(&shared, "ACP agent disconnected".into());
        {
            let mut prompts = shared.in_flight_prompts.lock();
            for (req_id, scope) in prompts.drain() {
                let _ = shared.event_sender.send(AdapterEvent::Error {
                    scope: Some(scope.clone()),
                    message: format!("ACP agent disconnected (pending request {req_id})"),
                });
                let _ = shared.event_sender.send(AdapterEvent::Finished {
                    scope: Some(scope),
                    success: false,
                    summary: "agent disconnected".into(),
                });
            }
        }
        shared.pending_permissions.lock().clear();
        shared.sessions_by_id.lock().clear();
        let _ = shared.event_sender.send(AdapterEvent::StatusChange {
            scope: None,
            status: "stopped".into(),
        });
    });
}

fn spawn_stderr_drain(stderr: ChildStderr) {
    std::thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines().map_while(Result::ok) {
            if !line.trim().is_empty() {
                eprintln!("[joi:acp] {}", line);
            }
        }
    });
}

fn handle_incoming_message(shared: &Arc<AcpShared>, message: Value) {
    let method = message
        .get("method")
        .and_then(|v| v.as_str())
        .map(|v| v.to_string());
    let id = message.get("id").cloned();
    eprintln!(
        "[joi:acp] dispatch method={:?} id={:?}",
        method.as_deref(),
        id.as_ref().and_then(request_id_key)
    );
    match (method.as_deref(), id) {
        (Some(method), Some(id_value)) => handle_agent_request(shared, method, id_value, message),
        (Some(method), None) => handle_agent_notification(shared, method, message),
        (None, Some(_)) => handle_agent_response(shared, message),
        _ => {}
    }
}

fn handle_agent_request(shared: &Arc<AcpShared>, method: &str, id: Value, message: Value) {
    match method {
        "session/request_permission" => {
            let params = message.get("params").cloned().unwrap_or(Value::Null);
            let session_id = params.get("sessionId").and_then(|v| v.as_str());
            let scope =
                match session_id.and_then(|sid| shared.sessions_by_id.lock().get(sid).cloned()) {
                    Some(s) => s,
                    None => {
                        // Permission request for a session we don't recognize. Reject
                        // so the agent doesn't hang waiting for a response we'll
                        // never produce.
                        let _ = shared.write_message(&json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "error": {
                                "code": -32602,
                                "message": "permission request for unknown session",
                            }
                        }));
                        return;
                    }
                };
            let tool_call = params.get("toolCall").cloned().unwrap_or(Value::Null);
            let action_id = compose_action_id(&shared.action_namespace, &id);
            let (title, description) = describe_permission_request(&tool_call);
            let choices = extract_permission_choices(params.get("options"));
            let allowed_ids = choices.iter().map(|c| c.id.clone()).collect();
            shared.pending_permissions.lock().insert(
                action_id.clone(),
                PendingPermission {
                    request_id: id,
                    option_ids: allowed_ids,
                    scope: scope.clone(),
                },
            );
            let _ = shared.event_sender.send(AdapterEvent::ActionRequest {
                scope: Some(scope),
                id: action_id,
                request_type: "permission".into(),
                title,
                description,
                choices,
            });
        }
        unsupported => {
            let _ = shared.write_message(&json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32601,
                    "message": format!("joi-server does not implement `{}`", unsupported),
                }
            }));
        }
    }
}

fn handle_agent_notification(shared: &Arc<AcpShared>, method: &str, message: Value) {
    if method != "session/update" {
        return;
    }
    let session_id = message
        .get("params")
        .and_then(|p| p.get("sessionId"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let scope = session_id
        .as_ref()
        .and_then(|sid| shared.sessions_by_id.lock().get(sid).cloned());

    let update = message
        .get("params")
        .and_then(|p| p.get("update"))
        .cloned()
        .unwrap_or(Value::Null);
    match update.get("sessionUpdate").and_then(|v| v.as_str()) {
        Some("agent_message_chunk") => {
            if let Some(text) = extract_text_chunk(&update) {
                let _ = shared.event_sender.send(AdapterEvent::Text {
                    scope: scope.clone(),
                    content: text,
                    is_partial: true,
                });
            }
        }
        Some("tool_call") | Some("tool_call_update") => {
            let tool_name = update
                .get("title")
                .and_then(|v| v.as_str())
                .or_else(|| update.get("kind").and_then(|v| v.as_str()))
                .or_else(|| update.get("toolCallId").and_then(|v| v.as_str()))
                .unwrap_or("tool_call")
                .to_string();
            let _ = shared.event_sender.send(AdapterEvent::ToolUse {
                scope,
                tool_name,
                input: update,
            });
        }
        _ => {}
    }
}

fn handle_agent_response(shared: &Arc<AcpShared>, message: Value) {
    let id_key = match message.get("id").and_then(request_id_key) {
        Some(k) => k,
        None => return,
    };
    if let Some(waiter) = shared.response_waiters.lock().remove(&id_key) {
        let response = if let Some(error) = message.get("error") {
            Err(json_value_to_string(error))
        } else {
            Ok(message.get("result").cloned().unwrap_or(Value::Null))
        };
        let _ = waiter.send(response);
        return;
    }
    let popped = {
        let mut guard = shared.in_flight_prompts.lock();
        let popped = guard.remove(&id_key);
        let remaining = guard.len();
        popped.map(|scope| (scope, remaining))
    };
    if let Some((scope, remaining)) = popped {
        eprintln!(
            "[joi:acp] session/prompt response id={} scope={} (in_flight remaining={})",
            id_key, scope.id, remaining
        );
        if let Some(error) = message.get("error") {
            let _ = shared.event_sender.send(AdapterEvent::Error {
                scope: Some(scope.clone()),
                message: json_value_to_string(error),
            });
            let _ = shared.event_sender.send(AdapterEvent::Finished {
                scope: Some(scope),
                success: false,
                summary: json_value_to_string(error),
            });
            return;
        }
        let stop_reason = message
            .get("result")
            .and_then(|r| r.get("stopReason"))
            .and_then(|v| v.as_str())
            .unwrap_or("completed")
            .to_string();
        let _ = shared.event_sender.send(AdapterEvent::Finished {
            scope: Some(scope),
            success: stop_reason != "cancelled",
            summary: stop_reason,
        });
        return;
    }
    eprintln!(
        "[joi:acp] response id={} matched no waiter and no in-flight prompt (orphan)",
        id_key
    );
}

fn fail_pending_waiters(shared: &Arc<AcpShared>, message: String) {
    let mut waiters = shared.response_waiters.lock();
    for (_, w) in waiters.drain() {
        let _ = w.send(Err(message.clone()));
    }
}

fn extract_text_chunk(update: &Value) -> Option<String> {
    match update.get("content") {
        Some(Value::Array(blocks)) => blocks.iter().find_map(extract_text_block),
        Some(block) => extract_text_block(block),
        None => None,
    }
}

fn extract_text_block(block: &Value) -> Option<String> {
    if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
        return Some(t.into());
    }
    block.get("content").and_then(extract_text_block)
}

fn describe_permission_request(tool_call: &Value) -> (String, String) {
    let name = tool_call
        .get("title")
        .and_then(|v| v.as_str())
        .or_else(|| tool_call.get("kind").and_then(|v| v.as_str()))
        .or_else(|| tool_call.get("toolCallId").and_then(|v| v.as_str()))
        .unwrap_or("tool");
    let description =
        serde_json::to_string_pretty(tool_call).unwrap_or_else(|_| json_value_to_string(tool_call));
    (format!("Permission required: {}", name), description)
}

fn extract_permission_choices(raw: Option<&Value>) -> Vec<ActionChoice> {
    raw.and_then(|v| v.as_array())
        .map(|opts| {
            opts.iter()
                .filter_map(|option| {
                    Some(ActionChoice {
                        id: option.get("optionId")?.as_str()?.to_string(),
                        label: option
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or_else(|| {
                                option
                                    .get("optionId")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("Option")
                            })
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn request_id_key(id: &Value) -> Option<String> {
    match id {
        Value::String(v) => Some(v.clone()),
        Value::Number(v) => Some(v.to_string()),
        _ => None,
    }
}

fn compose_action_id(namespace: &str, id: &Value) -> String {
    match request_id_key(id) {
        Some(req) => format!("{}:{}", namespace, req),
        None => format!("{}:unknown", namespace),
    }
}

fn truncate_for_log(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut cut = max_bytes;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}…(+{}b)", &s[..cut], s.len() - cut)
}

fn json_value_to_string(value: &Value) -> String {
    value
        .as_str()
        .map(ToString::to_string)
        .unwrap_or_else(|| serde_json::to_string(value).unwrap_or_else(|_| "<invalid json>".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_env_output_ignores_shell_noise_and_reads_nul_records() {
        let stdout =
            b"hello from shell\n__START__\nHOME=/Users/joi\0PATH=/opt/bin:/usr/bin\0\n__END__\n";

        let env = parse_env_output(stdout, "__START__", "__END__").expect("parse env");

        assert_eq!(env.get("HOME").map(String::as_str), Some("/Users/joi"));
        assert_eq!(
            env.get("PATH").map(String::as_str),
            Some("/opt/bin:/usr/bin")
        );
    }

    #[test]
    fn parse_env_output_rejects_missing_markers() {
        let err = parse_env_output(b"HOME=/Users/joi\0", "__START__", "__END__")
            .expect_err("must reject missing markers");

        assert!(err.contains("start marker"));
    }

    #[test]
    fn shell_quote_handles_single_quotes() {
        assert_eq!(shell_quote("/tmp/it's fine"), "'/tmp/it'\\''s fine'");
    }

    #[test]
    fn select_initialize_auth_method_reads_only_unambiguous_method() {
        let initialize = json!({
            "authMethods": [
                { "id": "qodercli-login", "name": "Login" }
            ]
        });

        assert_eq!(
            select_initialize_auth_method(&initialize, None).map(|method| method.id),
            Some("qodercli-login".to_string())
        );

        let ambiguous = json!({
            "authMethods": [
                { "id": "one" },
                { "id": "two" }
            ]
        });
        assert_eq!(select_initialize_auth_method(&ambiguous, None), None);
    }

    #[test]
    fn select_initialize_auth_method_parses_terminal_auth_meta() {
        let initialize = json!({
            "authMethods": [
                {
                    "id": "qodercli-login",
                    "name": "Log in",
                    "_meta": {
                        "terminal-auth": {
                            "label": "Qoder CLI Login",
                            "command": "/tmp/qodercli",
                            "args": ["--login"],
                            "env": { "QODER_HOME": "/tmp/qoder" }
                        }
                    }
                }
            ]
        });

        let method =
            select_initialize_auth_method(&initialize, None).expect("select single auth method");

        assert_eq!(method.id, "qodercli-login");
        let terminal = method.terminal_auth.expect("terminal auth");
        assert_eq!(terminal.command, "/tmp/qodercli");
        assert_eq!(terminal.args, vec!["--login"]);
        assert_eq!(
            terminal.env.get("QODER_HOME").map(String::as_str),
            Some("/tmp/qoder")
        );
        assert_eq!(terminal.label.as_deref(), Some("Qoder CLI Login"));
    }

    #[test]
    fn select_initialize_auth_method_keeps_configured_id_when_not_advertised() {
        let initialize = json!({ "authMethods": [] });

        let method = select_initialize_auth_method(&initialize, Some("manual-login"))
            .expect("configured auth method");

        assert_eq!(method.id, "manual-login");
        assert_eq!(method.terminal_auth, None);
    }

    #[test]
    fn auth_required_error_matches_qoder_shape() {
        assert!(is_auth_required_error(
            r#"{"code":-32000,"message":"Authentication required"}"#
        ));
        assert!(is_auth_required_error("Authentication required"));
        assert!(!is_auth_required_error(
            r#"{"code":-32000,"message":"different"}"#
        ));
    }
}
