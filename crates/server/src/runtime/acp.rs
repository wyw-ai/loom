//! Minimal ACP (Agent Client Protocol) stdio adapter.
//!
//! Lifted and slimmed from joi/src-tauri/src/agents/acp.rs — only the wire
//! handling we need for v0:
//!   initialize → optional authenticate → session/new → session/prompt loop,
//!   inbound session/update streams (text + tool_call), session/request_permission,
//!   stop via session/cancel + child kill.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AcpConfig {
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: PathBuf,
    pub auth_method: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AcpStartInfo {
    pub pid: Option<u32>,
    pub session_id: String,
    pub agent_info: Option<Value>,
    pub agent_capabilities: Option<Value>,
}

#[derive(Debug, Clone)]
pub enum AgentEvent {
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

struct PendingPermission {
    request_id: Value,
    option_ids: HashSet<String>,
}

struct AcpShared {
    stdin: Mutex<ChildStdin>,
    next_request_id: AtomicU64,
    response_waiters: Mutex<HashMap<String, std::sync::mpsc::Sender<Result<Value, String>>>>,
    in_flight_prompts: Mutex<HashSet<String>>,
    pending_permissions: Mutex<HashMap<String, PendingPermission>>,
    action_namespace: String,
    event_sender: mpsc::UnboundedSender<AgentEvent>,
}

pub struct AcpAdapter {
    config: AcpConfig,
    inner: Mutex<AcpInner>,
}

struct AcpInner {
    child: Option<Child>,
    shared: Option<Arc<AcpShared>>,
    session_id: Option<String>,
}

impl AcpAdapter {
    pub fn new(config: AcpConfig) -> Self {
        Self {
            config,
            inner: Mutex::new(AcpInner {
                child: None,
                shared: None,
                session_id: None,
            }),
        }
    }

    pub async fn start(
        &self,
        event_sender: mpsc::UnboundedSender<AgentEvent>,
    ) -> Result<AcpStartInfo, String> {
        let cfg = self.config.clone();
        tokio::task::spawn_blocking(move || start_blocking(cfg, event_sender))
            .await
            .map_err(|e| e.to_string())?
            .map(|(info, child, shared, session_id)| {
                let mut inner = self.inner.lock();
                inner.child = Some(child);
                inner.shared = Some(shared);
                inner.session_id = Some(info.session_id.clone());
                drop(inner);
                info
            })
    }

    pub async fn send_prompt(&self, content: String) -> Result<(), String> {
        let (shared, session_id) = {
            let inner = self.inner.lock();
            let shared = inner.shared.clone().ok_or("ACP agent not running")?;
            let session_id = inner.session_id.clone().ok_or("ACP session not ready")?;
            (shared, session_id)
        };
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            let request_id = shared.next_request_id_string();
            shared.in_flight_prompts.lock().insert(request_id.clone());
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
                shared
                    .in_flight_prompts
                    .lock()
                    .remove(&shared.next_request_id_string());
                return Err(err);
            }
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())?
    }

    pub async fn respond_permission(
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
            shared.write_message(&json!({
                "jsonrpc": "2.0",
                "id": pending.request_id,
                "result": {
                    "outcome": {
                        "outcome": "selected",
                        "optionId": option_id,
                    }
                }
            }))
        })
        .await
        .map_err(|e| e.to_string())?
    }

    pub async fn stop(&self) -> Result<(), String> {
        let (shared, session_id, mut child) = {
            let mut inner = self.inner.lock();
            (
                inner.shared.take(),
                inner.session_id.take(),
                inner.child.take(),
            )
        };
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            if let (Some(shared), Some(session_id)) = (shared.as_ref(), session_id.as_deref()) {
                if !shared.in_flight_prompts.lock().is_empty() {
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

fn start_blocking(
    cfg: AcpConfig,
    event_sender: mpsc::UnboundedSender<AgentEvent>,
) -> Result<(AcpStartInfo, Child, Arc<AcpShared>, String), String> {
    let workdir = if cfg.cwd.is_absolute() {
        cfg.cwd.clone()
    } else {
        std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(&cfg.cwd)
    };
    std::fs::create_dir_all(&workdir).map_err(|e| {
        format!(
            "Failed to create ACP workdir `{}`: {}",
            workdir.display(),
            e
        )
    })?;
    let mut cmd = Command::new(&cfg.command);
    cmd.args(&cfg.args)
        .current_dir(&workdir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in &cfg.env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().map_err(|e| {
        format!(
            "Failed to start ACP command `{}` in `{}`: {}",
            cfg.command,
            workdir.display(),
            e
        )
    })?;
    let stdin = child.stdin.take().ok_or("Failed to open agent stdin")?;
    let stdout = child.stdout.take().ok_or("Failed to open agent stdout")?;
    let stderr = child.stderr.take().ok_or("Failed to open agent stderr")?;
    let shared = Arc::new(AcpShared {
        stdin: Mutex::new(stdin),
        next_request_id: AtomicU64::new(1),
        response_waiters: Mutex::new(HashMap::new()),
        in_flight_prompts: Mutex::new(HashSet::new()),
        pending_permissions: Mutex::new(HashMap::new()),
        action_namespace: Uuid::new_v4().to_string(),
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
            },
        }),
        Duration::from_secs(60),
    )?;

    if let Some(method) = cfg.auth_method.as_deref() {
        if !method.trim().is_empty() {
            shared.request_and_wait(
                "authenticate",
                json!({ "methodId": method }),
                Duration::from_secs(30),
            )?;
        }
    }

    let session_result = shared.request_and_wait(
        "session/new",
        json!({
            "cwd": workdir.to_string_lossy(),
            "mcpServers": [],
        }),
        Duration::from_secs(30),
    )?;
    let session_id = session_result
        .get("sessionId")
        .and_then(|v| v.as_str())
        .ok_or("ACP agent did not return sessionId")?
        .to_string();

    let _ = event_sender.send(AgentEvent::StatusChange {
        status: "idle".into(),
    });

    let pid = child.id();
    let info = AcpStartInfo {
        pid: Some(pid),
        session_id: session_id.clone(),
        agent_info: initialize.get("agentInfo").cloned(),
        agent_capabilities: initialize.get("agentCapabilities").cloned(),
    };
    Ok((info, child, shared, session_id))
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
                    let _ = shared.event_sender.send(AgentEvent::Error {
                        message: format!("Failed to read ACP output: {}", err),
                    });
                    break;
                }
            };
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let message: Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(err) => {
                    let _ = shared.event_sender.send(AgentEvent::Error {
                        message: format!("Invalid ACP JSON: {}", err),
                    });
                    continue;
                }
            };
            handle_incoming_message(&shared, message);
        }
        fail_pending_waiters(&shared, "ACP agent disconnected".into());
        shared.in_flight_prompts.lock().clear();
        shared.pending_permissions.lock().clear();
        let _ = shared.event_sender.send(AgentEvent::StatusChange {
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
                },
            );
            let _ = shared.event_sender.send(AgentEvent::ActionRequest {
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
    let update = message
        .get("params")
        .and_then(|p| p.get("update"))
        .cloned()
        .unwrap_or(Value::Null);
    match update.get("sessionUpdate").and_then(|v| v.as_str()) {
        Some("agent_message_chunk") => {
            if let Some(text) = extract_text_chunk(&update) {
                let _ = shared.event_sender.send(AgentEvent::Text {
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
            let _ = shared.event_sender.send(AgentEvent::ToolUse {
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
    if shared.in_flight_prompts.lock().remove(&id_key) {
        if let Some(error) = message.get("error") {
            let _ = shared.event_sender.send(AgentEvent::Error {
                message: json_value_to_string(error),
            });
            let _ = shared.event_sender.send(AgentEvent::Finished {
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
        let _ = shared.event_sender.send(AgentEvent::Finished {
            success: stop_reason != "cancelled",
            summary: stop_reason,
        });
    }
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

fn json_value_to_string(value: &Value) -> String {
    value
        .as_str()
        .map(ToString::to_string)
        .unwrap_or_else(|| serde_json::to_string(value).unwrap_or_else(|_| "<invalid json>".into()))
}
