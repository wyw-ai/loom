//! Daemon agent worker internals.
//!
//! `loom-daemon` loads daemon-local `AgentSpec`s and uses this module to open a
//! dedicated WebSocket per agent, then supervise that one agent through the
//! shared `agent-runtime` adapter trait.
//!
//! Architecture:
//!   * one tokio task per agent ⇒ one `Client` ⇒ one WS frame to the server
//!   * `connection/open` with `actorKind = "agent"` binds the connection to the
//!     agent's actor id; the server's actor-inbox delivery pushes directed
//!     messages and control records straight to this connection
//!   * active scopes are subscribed while prompts run so run updates and action
//!     responses unblock the adapter without polling delay
//!   * a notification loop turns directed messages into `Adapter::send_prompt`
//!     calls and opens/closes a `run.*` lifecycle record
//!   * a translator task drains `AdapterEvent`s and writes final messages plus
//!     private `run.append` trace frames back to the server.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Result};
use chrono::{Local, SecondsFormat, Utc};
use proto::methods::{
    method, stream_kind, ActorListResult, AgentConfigActivateResult, AgentConfigPublishResult,
    AgentModelChoice, AgentSpec, AgentTransport, BundleInstallMode, InboxListResult,
    MessageListResult, MessageSendResult, PromptTemplateSpec, RunAppendResult, RunCloseResult,
    RunOpenResult, TaskAssignmentContextResult, TaskAssignmentUpdateResult, ThreadListResult,
    TriggerPrefixApplyOn,
};
use proto::types::trace::TraceKind;
use proto::types::{
    ActorKind, AudienceKind, DeliveryPolicy, Message, MessageIntent, Meta, Run, RunStatus,
    ScopeKind, ScopeRef, TaskAssignmentStatus,
};
#[cfg(test)]
use proto::types::{Event, RefKind, RelationKind};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{interval, sleep, Duration};

use agent_runtime::acp::{AcpAdapter, AcpConfig};
use agent_runtime::command::{CommandAdapter, CommandConfig};
use agent_runtime::interactive::{InteractiveCommandAdapter, InteractiveCommandConfig};
use agent_runtime::usage;
use agent_runtime::{
    agent_child_server_url, prepare_bundle_install, resolved_bundle_version,
    validate_bundle_current, Adapter, AdapterEvent, AdapterModelOptions, AdapterPrompt, PromptPart,
    PromptRoleHint, TokenUsage,
};

use crate::client::Client;
use crate::config;
use crate::daemon_ipc;

const RECONNECT_BASE_DELAY_SECS: u64 = 2;
const RECONNECT_MAX_DELAY_SECS: u64 = 30;
const LOOM_CLI_ENV: &str = "LOOM_CLI";
const LOOM_NO_REPLY_FILE_ENV: &str = "LOOM_NO_REPLY_FILE";
const LOCAL_ACTOR_INBOX_DELIVERY_META: &str = "__loom_local_actor_inbox_delivery";

pub async fn run(
    specs_dir_opt: Option<PathBuf>,
    server_url: String,
    allow_actors: Vec<String>,
) -> Result<()> {
    let specs_dir = specs_dir_opt.unwrap_or_else(default_specs_dir);
    std::fs::create_dir_all(&specs_dir)
        .with_context(|| format!("create specs dir {}", specs_dir.display()))?;
    let data_root = default_data_root();
    std::fs::create_dir_all(&data_root)
        .with_context(|| format!("create data dir {}", data_root.display()))?;

    let mut specs = load_specs(&specs_dir)?;
    let total_loaded = specs.len();
    if !allow_actors.is_empty() {
        let allow: HashSet<&str> = allow_actors.iter().map(String::as_str).collect();
        specs.retain(|spec| allow.contains(spec.actor.id.as_str()));
    }
    if specs.is_empty() {
        return Err(anyhow!(
            "no agent specs to serve under {} (loaded {}, after --allow-actors filter: {})",
            specs_dir.display(),
            total_loaded,
            specs.len(),
        ));
    }

    eprintln!(
        "loom agent serve: loaded {} agent(s) from {}",
        specs.len(),
        specs_dir.display()
    );
    let mut handles = Vec::new();
    for spec in specs {
        let server = server_url.clone();
        let root = data_root.clone();
        let actor = spec.actor.id.clone();
        let specs_dir_for_task = specs_dir.clone();
        handles.push(tokio::spawn(async move {
            let mut attempt = 0u32;
            let marker = crate::cmd::reload::agent_marker_path(&root, &actor);
            let mut last_spec = spec;
            loop {
                attempt = attempt.saturating_add(1);
                let delay = reconnect_delay(attempt);
                let spec_for_run = match reload_spec(&specs_dir_for_task, &actor) {
                    Ok(Some(next)) => {
                        last_spec = next;
                        last_spec.clone()
                    }
                    Ok(None) => last_spec.clone(),
                    Err(e) => {
                        eprintln!(
                            "[{actor}] failed to re-read spec ({e}); continuing with last-known spec"
                        );
                        last_spec.clone()
                    }
                };
                let baseline_epoch = crate::cmd::reload::read_epoch(&marker);
                let worker = tokio::spawn(run_agent_worker(
                    spec_for_run,
                    server.clone(),
                    root.clone(),
                ));
                let watcher_marker = marker.clone();
                let worker_abort = worker.abort_handle();
                let actor_for_watch = actor.clone();
                let watcher = tokio::spawn(async move {
                    loop {
                        sleep(Duration::from_millis(1000)).await;
                        let cur = crate::cmd::reload::read_epoch(&watcher_marker);
                        if cur > baseline_epoch {
                            eprintln!(
                                "[{actor_for_watch}] reload requested (epoch_ms={cur}); restarting worker"
                            );
                            worker_abort.abort();
                            return;
                        }
                    }
                });

                let join_result = worker.await;
                watcher.abort();
                let _ = watcher.await;
                match join_result {
                    Ok(Ok(())) => {
                        attempt = 0;
                        eprintln!(
                            "[{actor}] worker disconnected; reconnecting in {}s",
                            delay.as_secs()
                        );
                    }
                    Ok(Err(e)) => {
                        eprintln!(
                            "[{actor}] worker exited with error: {e}; reconnecting in {}s",
                            delay.as_secs()
                        );
                    }
                    Err(join_err) if join_err.is_cancelled() => {
                        attempt = 0;
                        eprintln!("[{actor}] worker aborted for reload; respawning");
                        continue;
                    }
                    Err(join_err) => {
                        eprintln!(
                            "[{actor}] worker task panicked: {join_err}; reconnecting in {}s",
                            delay.as_secs()
                        );
                    }
                }
                sleep(delay).await;
            }
        }));
    }

    futures_util::future::pending::<()>().await;
    #[allow(unreachable_code)]
    {
        for handle in handles {
            handle.abort();
        }
        Ok(())
    }
}

pub fn spawn_agent_worker_loop(
    spec: AgentSpec,
    server_url: String,
    data_root: PathBuf,
) -> tokio::task::JoinHandle<()> {
    let actor = spec.actor.id.clone();
    tokio::spawn(async move {
        let mut attempt = 0u32;
        loop {
            attempt = attempt.saturating_add(1);
            let delay = reconnect_delay(attempt);
            match run_agent_worker(spec.clone(), server_url.clone(), data_root.clone()).await {
                Ok(()) => {
                    attempt = 0;
                    eprintln!(
                        "[{actor}] worker disconnected; reconnecting in {}s",
                        delay.as_secs()
                    );
                }
                Err(e) => {
                    eprintln!(
                        "[{actor}] worker exited with error: {e}; reconnecting in {}s",
                        delay.as_secs()
                    );
                }
            }
            sleep(delay).await;
        }
    })
}

pub fn spawn_machine_host_loop(
    host: MachineHostSpec,
    server_url: String,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        run_machine_host_loop(host, server_url).await;
    })
}

fn reconnect_delay(attempt: u32) -> Duration {
    let shift = attempt.saturating_sub(1).min(4);
    Duration::from_secs((RECONNECT_BASE_DELAY_SECS << shift).min(RECONNECT_MAX_DELAY_SECS))
}

pub(crate) fn default_data_root_pub() -> PathBuf {
    default_data_root()
}

fn default_data_root() -> PathBuf {
    if let Ok(s) = std::env::var("LOOM_AGENT_DATA_ROOT") {
        if !s.is_empty() {
            return PathBuf::from(s);
        }
    }
    dirs::data_dir()
        .map(|d| d.join("loom").join("agents"))
        .unwrap_or_else(|| PathBuf::from(".loom").join("agents-data"))
}

fn default_specs_dir() -> PathBuf {
    if let Ok(s) = std::env::var("LOOM_AGENT_SPECS") {
        if !s.is_empty() {
            return PathBuf::from(s);
        }
    }
    dirs::config_dir()
        .map(|d| d.join("loom").join("agents"))
        .unwrap_or_else(|| PathBuf::from(".loom").join("agents"))
}

pub(crate) fn load_specs(dir: &Path) -> Result<Vec<AgentSpec>> {
    let mut out = Vec::new();
    for entry in
        std::fs::read_dir(dir).with_context(|| format!("read specs dir {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        let target = if file_type.is_dir() {
            let nested = path.join("spec.json");
            if !nested.exists() {
                continue;
            }
            nested
        } else if path.extension().and_then(|s| s.to_str()) == Some("json") {
            path
        } else {
            continue;
        };
        let text = std::fs::read_to_string(&target)
            .with_context(|| format!("read {}", target.display()))?;
        match serde_json::from_str::<AgentSpec>(&text) {
            Ok(spec) => out.push(spec),
            Err(e) => eprintln!("[warn] skipping {}: {}", target.display(), e),
        }
    }
    out.sort_by(|a, b| a.actor.id.cmp(&b.actor.id));
    Ok(out)
}

fn reload_spec(specs_dir: &Path, actor_id: &str) -> Result<Option<AgentSpec>> {
    let nested = specs_dir.join(actor_id).join("spec.json");
    let flat = specs_dir.join(format!("{actor_id}.json"));
    let path = if nested.exists() {
        nested
    } else if flat.exists() {
        flat
    } else {
        return Ok(None);
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(anyhow!("read {}: {e}", path.display())),
    };
    let spec: AgentSpec =
        serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
    Ok(Some(spec))
}

/// The path to the Loom CLI. `loom-daemon` is often launched by absolute path,
/// so `current_exe()` points at `loom-daemon`; agents need the sibling `loom`
/// binary instead.
fn current_loom_binary() -> Option<PathBuf> {
    resolve_loom_cli_binary().or_else(|| Some(PathBuf::from("loom")))
}

fn resolve_loom_cli_binary() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(LOOM_CLI_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| is_executable_file(path))
    {
        return Some(path);
    }

    if let Ok(current) = std::env::current_exe() {
        if executable_stem_is(&current, "loom") && is_executable_file(&current) {
            return Some(current);
        }
        if let Some(parent) = current.parent() {
            let sibling = parent.join(executable_name("loom"));
            if is_executable_file(&sibling) {
                return Some(sibling);
            }
        }
    }

    find_executable_on_path("loom")
}

fn inject_loom_cli_env(env: &mut BTreeMap<String, String>, loom_binary: Option<&Path>) {
    let Some(loom_binary) = loom_binary else {
        return;
    };
    env.entry(LOOM_CLI_ENV.into())
        .or_insert_with(|| loom_binary.display().to_string());
    if let Some(parent) = loom_binary.parent() {
        prepend_path_dir(env, parent);
    }
}

fn prepend_path_dir(env: &mut BTreeMap<String, String>, dir: &Path) {
    if dir.as_os_str().is_empty() {
        return;
    }
    let existing = env
        .get("PATH")
        .cloned()
        .or_else(|| std::env::var("PATH").ok())
        .unwrap_or_default();
    let mut dirs = vec![dir.to_path_buf()];
    dirs.extend(std::env::split_paths(&existing).filter(|path| path != dir));
    if let Ok(joined) = std::env::join_paths(dirs) {
        env.insert("PATH".into(), joined.to_string_lossy().into_owned());
    }
}

fn find_executable_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(executable_name(name));
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn executable_name(stem: &str) -> String {
    #[cfg(windows)]
    {
        format!("{stem}.exe")
    }
    #[cfg(not(windows))]
    {
        stem.to_string()
    }
}

fn executable_stem_is(path: &Path, expected: &str) -> bool {
    path.file_stem()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == expected)
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

pub struct MachineCommandTask {
    pub payload: Value,
    pub reply: oneshot::Sender<Value>,
}

#[derive(Clone)]
pub struct MachineHostSpec {
    pub machine_id: String,
    pub actor_id: String,
    pub display_name: String,
    pub metadata: Arc<Mutex<Value>>,
    pub command_tx: Option<mpsc::UnboundedSender<MachineCommandTask>>,
}

async fn run_machine_host_loop(host: MachineHostSpec, server_url: String) {
    let mut attempt = 0u32;
    loop {
        attempt = attempt.saturating_add(1);
        let delay = reconnect_delay(attempt);
        match run_machine_host_once(&host, &server_url).await {
            Ok(()) => {
                attempt = 0;
                eprintln!(
                    "[{}] machine host disconnected; reconnecting in {}s",
                    host.machine_id,
                    delay.as_secs()
                );
            }
            Err(e) => {
                eprintln!(
                    "[{}] machine host exited with error: {e:#}; reconnecting in {}s",
                    host.machine_id,
                    delay.as_secs()
                );
            }
        }
        sleep(delay).await;
    }
}

async fn run_machine_host_once(host: &MachineHostSpec, server_url: &str) -> Result<()> {
    let client = Client::connect(server_url).await?;
    client.initialize().await?;
    upsert_machine_actor(&client, host).await?;
    client
        .open_connection_as(&host.actor_id, "service", Some(&host.display_name))
        .await?;
    eprintln!(
        "[{}] machine host connected to {} as {}",
        host.machine_id, server_url, host.actor_id
    );

    let mut notifications = client.notifications.lock().await;
    let mut heartbeat = interval(Duration::from_secs(15));
    let mut in_progress = HashSet::new();
    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                upsert_machine_actor(&client, host).await?;
                drain_machine_commands(&client, host, &mut in_progress).await?;
                let _: Value = client.call_raw(method::ACTOR_LIST, None).await?;
            }
            maybe_notification = notifications.recv() => {
                let Some(notification) = maybe_notification else {
                    return Err(anyhow!("machine host notification stream closed"));
                };
                if notification.method == method::MACHINE_COMMAND_NOTIFY
                    || notification.method == method::MACHINE_COMMAND
                {
                    handle_machine_command_notification(&client, host, notification.params, &mut in_progress).await?;
                }
            }
        }
    }
}

async fn drain_machine_commands(
    client: &Client,
    host: &MachineHostSpec,
    in_progress: &mut HashSet<String>,
) -> Result<()> {
    let response: Value = client
        .call_raw(
            method::MACHINE_COMMAND_LIST,
            Some(json!({
                "machineId": host.machine_id,
                "machineActorId": host.actor_id,
                "statuses": ["queued", "delivered", "running"],
                "limit": 50,
            })),
        )
        .await?;
    let Some(commands) = response.get("commands").and_then(Value::as_array) else {
        return Ok(());
    };
    for command in commands {
        handle_machine_command_notification(
            client,
            host,
            Some(machine_command_payload_from_record(command)),
            in_progress,
        )
        .await?;
    }
    Ok(())
}

fn machine_command_payload_from_record(command: &Value) -> Value {
    json!({
        "commandId": command.get("commandId").cloned().unwrap_or(Value::Null),
        "machineId": command.get("machineId").cloned().unwrap_or(Value::Null),
        "machineActorId": command.get("machineActorId").cloned().unwrap_or(Value::Null),
        "requestedBy": command.get("requestedBy").cloned().unwrap_or(Value::Null),
        "workspaceId": command.get("workspaceId").cloned().unwrap_or(Value::Null),
        "operation": command.get("operation").cloned().unwrap_or(Value::Null),
        "payload": command.get("payload").cloned().unwrap_or(Value::Null),
        "command": command.get("payload").cloned().unwrap_or(Value::Null),
        "ifInventoryRevision": command.get("ifInventoryRevision").cloned().unwrap_or(Value::Null),
    })
}

async fn handle_machine_command_notification(
    client: &Client,
    host: &MachineHostSpec,
    params: Option<Value>,
    in_progress: &mut HashSet<String>,
) -> Result<()> {
    let Some(payload) = params else {
        return Ok(());
    };
    let command_id = payload
        .get("commandId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let machine_id = payload
        .get("machineId")
        .and_then(Value::as_str)
        .unwrap_or(&host.machine_id)
        .to_string();
    let machine_actor_id = payload
        .get("machineActorId")
        .and_then(Value::as_str)
        .unwrap_or(&host.actor_id)
        .to_string();
    if command_id.trim().is_empty() {
        tracing::warn!("machine command notification without commandId");
        return Ok(());
    }
    if !in_progress.insert(command_id.clone()) {
        return Ok(());
    }

    let result = if machine_id != host.machine_id || machine_actor_id != host.actor_id {
        json!({
            "commandId": command_id,
            "machineId": machine_id,
            "machineActorId": machine_actor_id,
            "ok": false,
            "error": "machine command target mismatch",
        })
    } else if let Some(command_tx) = &host.command_tx {
        let _: Value = client
            .call_raw(
                method::MACHINE_COMMAND_ACK,
                Some(json!({
                    "commandId": command_id,
                    "machineId": machine_id,
                    "machineActorId": machine_actor_id,
                })),
            )
            .await?;
        let (reply_tx, reply_rx) = oneshot::channel();
        if command_tx
            .send(MachineCommandTask {
                payload,
                reply: reply_tx,
            })
            .is_err()
        {
            json!({
                "commandId": command_id,
                "machineId": machine_id,
                "machineActorId": machine_actor_id,
                "ok": false,
                "error": "machine command executor is unavailable",
            })
        } else {
            match tokio::time::timeout(Duration::from_secs(30), reply_rx).await {
                Ok(Ok(value)) => value,
                Ok(Err(_)) => json!({
                    "commandId": command_id,
                    "machineId": machine_id,
                    "machineActorId": machine_actor_id,
                    "ok": false,
                    "error": "machine command executor dropped the reply channel",
                }),
                Err(_) => json!({
                    "commandId": command_id,
                    "machineId": machine_id,
                    "machineActorId": machine_actor_id,
                    "ok": false,
                    "error": "machine command executor timed out",
                }),
            }
        }
    } else {
        json!({
            "commandId": command_id,
            "machineId": machine_id,
            "machineActorId": machine_actor_id,
            "ok": false,
            "error": "machine command executor is not configured",
        })
    };

    upsert_machine_actor(client, host).await?;
    let _: Value = client
        .call_raw(method::MACHINE_COMMAND_RESULT, Some(result))
        .await?;
    in_progress.remove(&command_id);
    Ok(())
}

async fn upsert_machine_actor(client: &Client, host: &MachineHostSpec) -> Result<()> {
    let metadata = host.metadata.lock().unwrap().clone();
    let _: Value = client
        .call(
            method::ACTOR_UPSERT,
            json!({
                "actor": {
                    "id": &host.actor_id,
                    "kind": "service",
                    "displayName": &host.display_name,
                    "_meta": metadata,
                },
            }),
        )
        .await
        .with_context(|| format!("actor/upsert for machine {}", host.machine_id))?;
    Ok(())
}

/// Actor-level state plus channel-scoped workspaces under the AgentX root.
#[derive(Clone)]
struct AgentPaths {
    profile: PathBuf,
    root: PathBuf,
    bundle_root: PathBuf,
    bundle_current: PathBuf,
    sessions: PathBuf,
    scope_workspaces_root: PathBuf,
    data_root: PathBuf,
}

#[derive(Clone)]
struct ScopePaths {
    channel_root: PathBuf,
    channel_shared: PathBuf,
    channel_artifacts: PathBuf,
    agent_root: PathBuf,
    workspace: PathBuf,
    skills: PathBuf,
    logs: PathBuf,
}

#[derive(Debug, Clone)]
struct BundlePaths {
    root: PathBuf,
    current: PathBuf,
    version: String,
}

impl AgentPaths {
    fn new(data_root: &Path, actor_id: &str) -> Self {
        let agent_root = data_root.join("agents").join(actor_id);
        let scope_workspaces_root = std::env::var_os("LOOM_SCOPE_WORKSPACES_ROOT")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| data_root.join("workspaces"));
        Self {
            profile: agent_root.join("profile"),
            bundle_root: agent_root.join("bundles"),
            bundle_current: agent_root.join("bundles").join("current"),
            root: agent_root,
            sessions: data_root.join("sessions"),
            scope_workspaces_root,
            data_root: data_root.to_path_buf(),
        }
    }

    fn scope(&self, actor_id: &str, channel_id: &str, scope_ref: &ScopeRef) -> ScopePaths {
        let channel_root = self.data_root.join("channels").join(channel_id);
        let channel_shared = channel_root.join("shared");
        let channel_artifacts = channel_shared.join("artifacts");
        let agent_root = channel_root.join("agents").join(actor_id);
        ScopePaths {
            channel_root,
            channel_shared,
            channel_artifacts,
            skills: self.scope_skills_dir(scope_ref),
            workspace: agent_root.join("workspace"),
            logs: agent_root.join("logs"),
            agent_root,
        }
    }

    fn ensure(
        &self,
        actor_id: &str,
        spec: &AgentSpec,
        bundle_paths: &BundlePaths,
    ) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.profile)?;
        std::fs::create_dir_all(&self.sessions)?;
        ensure_bundle(actor_id, spec, bundle_paths, self)?;
        if spec.memory.is_some() {
            let memory_root = spec
                .memory
                .as_ref()
                .map(|m| m.store.root.as_str())
                .unwrap_or("./memory/records");
            if let Err(e) =
                agent_runtime::ensure_profile_scaffold(&agent_runtime::ProfileScaffold {
                    profile_dir: &self.profile,
                    memory_root,
                })
            {
                tracing::warn!(actor = %actor_id, %e, "failed to scaffold profile");
            }
        }
        Ok(())
    }

    fn expand_base(&self, input: &str) -> String {
        input
            .replace("{agent.profile}", &self.profile.display().to_string())
            .replace("{agent.root}", &self.root.display().to_string())
    }

    fn bundle_paths(&self, spec: &AgentSpec) -> BundlePaths {
        let bundle = spec.bundle.as_ref().cloned().unwrap_or_default();
        let source_value = self.expand_base(&bundle.source);
        let root = if bundle.root.trim().is_empty() {
            self.bundle_root.clone()
        } else {
            PathBuf::from(self.expand_base(&bundle.root))
        };
        let current = if bundle.current.trim().is_empty() {
            self.bundle_current.clone()
        } else {
            PathBuf::from(self.expand_base(&bundle.current))
        };
        BundlePaths {
            root,
            current,
            version: resolved_bundle_version(&bundle, Path::new(&source_value)),
        }
    }

    fn expand(&self, input: &str, bundle_paths: Option<&BundlePaths>) -> String {
        let base = self.expand_base(input);
        let bundle_root = bundle_paths
            .map(|paths| paths.root.display().to_string())
            .unwrap_or_else(|| self.bundle_root.display().to_string());
        let bundle_current = bundle_paths
            .map(|paths| paths.current.display().to_string())
            .unwrap_or_else(|| self.bundle_current.display().to_string());
        base.replace("{agent.bundle_root}", &bundle_root)
            .replace("{agent.bundle}", &bundle_current)
    }

    fn ensure_scope(
        &self,
        actor_id: &str,
        channel_id: &str,
        scope_ref: &ScopeRef,
    ) -> std::io::Result<ScopePaths> {
        let scope = self.scope(actor_id, channel_id, scope_ref);
        std::fs::create_dir_all(&scope.workspace)?;
        std::fs::create_dir_all(&scope.logs)?;
        std::fs::create_dir_all(&scope.channel_artifacts)?;
        ensure_scope_skills_link(&scope.workspace, &scope.skills)?;
        agent_runtime::ensure_agents_md(&scope.workspace, actor_id)?;
        Ok(scope)
    }

    fn template_vars(
        &self,
        actor_id: &str,
        channel_id: &str,
        scope_ref: &ScopeRef,
    ) -> BTreeMap<String, String> {
        let scope = self.scope(actor_id, channel_id, scope_ref);
        let scope_kind = scope_kind_name(scope_ref.kind).to_string();
        let mut vars = BTreeMap::new();
        vars.insert("actor.id".into(), actor_id.to_string());
        vars.insert("scope.id".into(), scope_ref.id.clone());
        vars.insert("scope.kind".into(), scope_kind);
        vars.insert("channel.id".into(), channel_id.to_string());
        vars.insert(
            "thread.id".into(),
            if matches!(scope_ref.kind, ScopeKind::Thread) {
                scope_ref.id.clone()
            } else {
                String::new()
            },
        );
        vars.insert(
            "workspace.dir".into(),
            scope.workspace.display().to_string(),
        );
        vars.insert(
            "agent.workspace".into(),
            scope.workspace.display().to_string(),
        );
        vars.insert("agent.root".into(), scope.agent_root.display().to_string());
        vars.insert("agent.profile".into(), self.profile.display().to_string());
        vars.insert("agent.logs".into(), scope.logs.display().to_string());
        vars.insert("agent.skills".into(), scope.skills.display().to_string());
        vars.insert("scope.skills".into(), scope.skills.display().to_string());
        vars.insert(
            "agent.bundle_root".into(),
            self.bundle_root.display().to_string(),
        );
        vars.insert(
            "agent.bundle".into(),
            self.bundle_current.display().to_string(),
        );
        let skill_body =
            std::fs::read_to_string(self.bundle_current.join("SKILL.md")).unwrap_or_default();
        vars.insert("agent.skillBody".into(), skill_body);
        vars.insert(
            "channel.root".into(),
            scope.channel_root.display().to_string(),
        );
        vars.insert(
            "channel.shared".into(),
            scope.channel_shared.display().to_string(),
        );
        vars.insert(
            "channel.sharedArtifacts".into(),
            scope.channel_artifacts.display().to_string(),
        );
        vars
    }

    fn scope_env(
        &self,
        actor_id: &str,
        channel_id: &str,
        scope_ref: &ScopeRef,
        server_url: &str,
        active: Option<&ActiveTurn>,
    ) -> BTreeMap<String, String> {
        let scope = self.scope(actor_id, channel_id, scope_ref);
        let mut env = BTreeMap::new();
        env.insert("LOOM_SERVER".into(), server_url.to_string());
        inject_loom_cli_env(&mut env, resolve_loom_cli_binary().as_deref());
        if let Some(socket) = daemon_ipc::env_socket_path() {
            env.insert("LOOM_DAEMON_SOCKET".into(), socket.display().to_string());
            env.insert(
                daemon_ipc::ENV_DAEMON_SOCKET.into(),
                socket.display().to_string(),
            );
        }
        env.insert("LOOM_ACTOR".into(), actor_id.to_string());
        env.insert("LOOM_SCOPE_ID".into(), scope_ref.id.clone());
        env.insert(
            "LOOM_SCOPE_KIND".into(),
            scope_kind_name(scope_ref.kind).to_string(),
        );
        insert_current_time_env(&mut env);
        if let Some(active) = active {
            env.insert("LOOM_RUN_ID".into(), active.run_id.clone());
            env.insert(
                "LOOM_TRIGGER_MESSAGE_ID".into(),
                active.trigger_source_id.clone(),
            );
            env.insert("LOOM_TRIGGER_ACTOR".into(), active.trigger_actor.clone());
            if let Some(target) = active.reply_target.as_deref() {
                env.insert("LOOM_REPLY_TARGET".into(), target.to_string());
            }
            if let Some(path) = active.no_reply_file.as_ref() {
                env.insert(LOOM_NO_REPLY_FILE_ENV.into(), path.display().to_string());
            }
        }
        env.insert(
            "LOOM_AGENT_PROFILE".into(),
            self.profile.display().to_string(),
        );
        env.insert(
            "LOOM_AGENT_BUNDLE_DIR".into(),
            self.bundle_current.display().to_string(),
        );
        env.insert("LOOM_CHANNEL_ID".into(), channel_id.to_string());
        env.insert("AGENTX_CHANNEL_ID".into(), channel_id.to_string());
        env.insert(
            "LOOM_CHANNEL_ROOT".into(),
            scope.channel_root.display().to_string(),
        );
        env.insert(
            "AGENTX_CHANNEL_ROOT".into(),
            scope.channel_root.display().to_string(),
        );
        env.insert(
            "LOOM_CHANNEL_SHARED".into(),
            scope.channel_shared.display().to_string(),
        );
        env.insert(
            "AGENTX_CHANNEL_SHARED".into(),
            scope.channel_shared.display().to_string(),
        );
        env.insert(
            "LOOM_CHANNEL_SHARED_ARTIFACTS".into(),
            scope.channel_artifacts.display().to_string(),
        );
        env.insert(
            "AGENTX_CHANNEL_SHARED_ARTIFACTS".into(),
            scope.channel_artifacts.display().to_string(),
        );
        env.insert(
            "LOOM_AGENT_ROOT".into(),
            scope.agent_root.display().to_string(),
        );
        env.insert(
            "AGENTX_AGENT_ROOT".into(),
            scope.agent_root.display().to_string(),
        );
        env.insert(
            "LOOM_AGENT_WORKSPACE".into(),
            scope.workspace.display().to_string(),
        );
        env.insert(
            "AGENTX_AGENT_WORKSPACE".into(),
            scope.workspace.display().to_string(),
        );
        env.insert("LOOM_AGENT_LOGS".into(), scope.logs.display().to_string());
        env.insert("AGENTX_AGENT_LOGS".into(), scope.logs.display().to_string());
        env.insert(
            "LOOM_SCOPE_SKILLS".into(),
            scope.skills.display().to_string(),
        );
        env.insert(
            "AGENTX_SCOPE_SKILLS".into(),
            scope.skills.display().to_string(),
        );
        env.insert(
            "LOOM_SCOPE_SKILLS_DIR".into(),
            scope.skills.display().to_string(),
        );
        env
    }

    fn scope_skills_dir(&self, scope_ref: &ScopeRef) -> PathBuf {
        self.scope_workspaces_root
            .join(scope_kind_name(scope_ref.kind))
            .join(&scope_ref.id)
            .join("skills")
    }
}

fn scope_kind_name(kind: ScopeKind) -> &'static str {
    match kind {
        ScopeKind::Channel => "channel",
        ScopeKind::Thread => "thread",
    }
}

fn run_no_reply_file(logs_dir: &Path, run_id: &str) -> PathBuf {
    let safe_run_id = run_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    logs_dir.join(format!("{safe_run_id}.no-reply.json"))
}

fn ensure_scope_skills_link(workspace: &Path, skills_target: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(skills_target)?;
    let link_path = workspace.join("skills");
    match std::fs::read_link(&link_path) {
        Ok(existing) if existing == skills_target => return Ok(()),
        Ok(_) => remove_path_if_exists(&link_path)?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => remove_path_if_exists(&link_path)?,
    }
    symlink_path(skills_target, &link_path)
}

fn ensure_bundle(
    _actor_id: &str,
    spec: &AgentSpec,
    paths: &BundlePaths,
    agent_paths: &AgentPaths,
) -> std::io::Result<()> {
    std::fs::create_dir_all(&paths.root)?;
    let current = validate_bundle_current(
        &agent_paths.root,
        &agent_paths.profile,
        &agent_paths.root.join("logs"),
        &paths.root,
        &paths.current,
    )?;
    let Some(bundle) = spec.bundle.as_ref() else {
        reset_bundle_current_dir(&current)?;
        return Ok(());
    };
    if bundle.source.trim().is_empty() {
        reset_bundle_current_dir(&current)?;
        return Ok(());
    }
    let source = PathBuf::from(agent_paths.expand_base(&bundle.source));
    let install_dir = prepare_bundle_install(&paths.root, &source, bundle)?.install_dir;
    install_bundle_dir(&source, &install_dir, bundle.install_mode)?;
    link_current_bundle(&install_dir, &current)?;
    Ok(())
}
fn install_bundle_dir(
    source: &Path,
    target: &Path,
    mode: BundleInstallMode,
) -> std::io::Result<()> {
    if !source.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("bundle source `{}` does not exist", source.display()),
        ));
    }
    remove_path_if_exists(target)?;
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match mode {
        BundleInstallMode::Copy => copy_recursively(source, target),
        BundleInstallMode::Symlink => symlink_path(source, target),
    }
}

fn link_current_bundle(installed: &Path, current: &Path) -> std::io::Result<()> {
    remove_path_if_exists(current)?;
    if let Some(parent) = current.parent() {
        std::fs::create_dir_all(parent)?;
    }
    symlink_path(installed, current)
}

fn reset_bundle_current_dir(current: &Path) -> std::io::Result<()> {
    remove_path_if_exists(current)?;
    std::fs::create_dir_all(current)
}

fn remove_path_if_exists(path: &Path) -> std::io::Result<()> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    if meta.file_type().is_symlink() || meta.is_file() {
        std::fs::remove_file(path)
    } else {
        std::fs::remove_dir_all(path)
    }
}

fn copy_recursively(source: &Path, target: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(source)?;
    if meta.file_type().is_symlink() {
        let real = std::fs::canonicalize(source)?;
        return copy_recursively(&real, target);
    }
    if meta.is_file() {
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(source, target)?;
        return Ok(());
    }
    std::fs::create_dir_all(target)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        copy_recursively(&entry.path(), &target.join(entry.file_name()))?;
    }
    Ok(())
}

#[cfg(unix)]
fn symlink_path(source: &Path, target: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(source, target)
}

#[cfg(windows)]
fn symlink_path(source: &Path, target: &Path) -> std::io::Result<()> {
    if source.is_dir() {
        std::os::windows::fs::symlink_dir(source, target)
    } else {
        std::os::windows::fs::symlink_file(source, target)
    }
}

#[derive(Clone)]
enum AgentTrigger {
    Message(Message),
    #[cfg(test)]
    Event(Event),
}

impl AgentTrigger {
    fn id(&self) -> &str {
        match self {
            AgentTrigger::Message(message) => &message.id,
            #[cfg(test)]
            AgentTrigger::Event(event) => &event.id,
        }
    }

    fn scope(&self) -> &ScopeRef {
        match self {
            AgentTrigger::Message(message) => &message.scope,
            #[cfg(test)]
            AgentTrigger::Event(event) => &event.scope,
        }
    }

    fn actor_id(&self) -> &str {
        match self {
            AgentTrigger::Message(message) => &message.author_actor_id,
            #[cfg(test)]
            AgentTrigger::Event(event) => &event.actor_id,
        }
    }

    fn meta_value(&self, key: &str) -> Option<&Value> {
        match self {
            AgentTrigger::Message(message) => message.metadata.get(key),
            #[cfg(test)]
            AgentTrigger::Event(event) => event
                .payload
                .get("_meta")
                .and_then(Value::as_object)
                .and_then(|meta| meta.get(key))
                .or_else(|| event._meta.as_ref().and_then(|meta| meta.get(key))),
        }
    }

    fn reply_target(&self) -> Option<String> {
        match self {
            AgentTrigger::Message(message) => Some(reply_target_for_message(message)),
            #[cfg(test)]
            AgentTrigger::Event(_) => None,
        }
    }

    fn is_message(&self) -> bool {
        matches!(self, AgentTrigger::Message(_))
    }
}

fn reply_target_for_message(message: &Message) -> String {
    match message.scope.kind {
        ScopeKind::Thread => message.target.clone(),
        ScopeKind::Channel
            if message.parent_message_id.is_none() && message.thread_root_message_id.is_none() =>
        {
            format!("#{}:{}", message.scope.id, message.id)
        }
        ScopeKind::Channel => message.target.clone(),
    }
}

struct WorkerState {
    actor_id: String,
    /// Cached copy of the on-disk spec. Reads only; specs are load-once in v1.
    spec: AgentSpec,
    /// Runtime-only provider resolution. This is intentionally separate from
    /// AgentSpec so providerRef resolution does not rewrite on-disk spec data.
    transport: AgentTransport,
    /// Resolved profile dir — same one `AgentPaths.profile` points at. Copied
    /// here so prompt-envelope code can read legacy profile fields and memory without
    /// threading `paths` through every call.
    profile_dir: PathBuf,
    paths: AgentPaths,
    agent_server_url: String,
    agent_config_version_id: String,
    /// In-flight turn per scope. A worker may own one adapter instance, but
    /// scope/session state is isolated below the adapter boundary, so only
    /// prompts in the same scope block each other.
    active_turns: Mutex<HashMap<String, ActiveTurn>>,
    /// Per-scope queues of triggers received while that scope is busy. Human
    /// triggers are kept ahead of service callbacks within the same scope so
    /// stale automation cannot starve an explicit user request.
    pending_triggers: Mutex<HashMap<String, VecDeque<AgentTrigger>>>,
    /// Per-turn streaming text buffer. Token/chunk streams are buffered until
    /// the adapter reports a message boundary; complete assistant messages are
    /// emitted to chat immediately.
    text_buffer: Mutex<HashMap<String, String>>,
    /// Provider session usage accumulated per scope. ACP, command, and
    /// interactive transports all use scope as the session boundary here.
    usage_totals: Mutex<HashMap<String, TokenUsage>>,
    /// Per-scope first-prompt-seeded set; first prompt for a given scope on a
    /// freshly-started agent gets the bootstrap manifest prepended.
    seeded: Mutex<HashSet<String>>,
    /// thread_id → channel_id cache. Populated on miss by a single
    /// `thread/list` RPC and reused from then on. Channel scopes don't need
    /// resolution (scope.id IS the channel id) so those don't populate it.
    scope_channel_cache: Mutex<HashMap<String, String>>,
    /// Trigger source ids already received from the server notification stream.
    /// The same source can arrive through both scope broadcast and actor-inbox
    /// routing when we subscribe to an active scope for legacy server
    /// compatibility.
    seen_sources: Mutex<HashSet<String>>,
    /// action.request message id -> underlying ACP request id.
    action_map: Mutex<HashMap<String, String>>,
    /// action.request message id -> metadata for Loom-owned model selection prompts.
    model_action_map: Mutex<HashMap<String, ModelActionRequest>>,
    /// Currently selected model id for this actor. Loaded from profile state
    /// first, then from `spec.models.default`.
    selected_model: Mutex<Option<String>>,
    /// Actor id → display name cache used when rendering trigger prompts.
    /// The server keeps actor rows authoritative; this cache is a fallback
    /// when actor/list is temporarily unavailable.
    actor_display_cache: Mutex<HashMap<String, String>>,
}

#[derive(Clone)]
struct ActiveTurn {
    /// Temporary legacy turn id kept only for existing cancellation UI and
    /// compatibility with old action.response events while execution state
    /// moves to Run.
    id: String,
    run_id: String,
    scope: ScopeRef,
    trigger_source_id: String,
    trigger_is_message: bool,
    reply_target: Option<String>,
    prompt_stats: PromptStats,
    prompt_breakdown: PromptBreakdown,
    /// Actor that triggered the current turn — needed when emitting a
    /// `action.request` so we can hand the choice back to them.
    trigger_actor: String,
    /// Local side-channel used by `loom run ignore` so the model can end a
    /// turn without posting a visible final answer.
    no_reply_file: Option<PathBuf>,
    no_reply_requested: bool,
    /// Set after a human cancels the turn. The server has already closed the
    /// turn, but we keep this slot occupied until the adapter's eventual
    /// Finished(cancelled) arrives so that stale completion cannot close the
    /// next turn in the same scope.
    cancel_requested: bool,
}

#[derive(Debug, Clone, Serialize)]
struct PromptStats {
    char_count: usize,
    byte_count: usize,
    approx_token_count: u64,
}

#[derive(Debug, Clone, Serialize)]
struct PromptBreakdown {
    sections: Vec<PromptBreakdownSection>,
}

#[derive(Debug, Clone, Serialize)]
struct PromptBreakdownSection {
    key: String,
    label: String,
    char_count: usize,
    byte_count: usize,
    approx_token_count: u64,
    percentage: f64,
}

#[derive(Debug, Clone)]
struct PromptTelemetry {
    content: String,
    parts: Vec<PromptPart>,
    stats: PromptStats,
    breakdown: PromptBreakdown,
}

#[derive(Debug, Clone, Serialize)]
struct TokenUsageMeta {
    increment: TokenUsage,
    cumulative: TokenUsage,
}

#[derive(Debug, Clone)]
struct ModelActionRequest {
    source: ModelActionSource,
    choices: Vec<AgentModelChoice>,
}

#[derive(Debug, Clone)]
enum ModelActionSource {
    Spec,
    Adapter { config_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelStateFile {
    model: String,
}

#[cfg(test)]
fn test_command_transport() -> AgentTransport {
    AgentTransport {
        kind: "command".into(),
        command: "echo".into(),
        args: Vec::new(),
        env: std::collections::BTreeMap::new(),
        auth_method: None,
        model: None,
        model_args: Vec::new(),
        session: None,
        output_format: None,
        prompt_via: proto::methods::PromptVia::default(),
        prompt: None,
        stdin: None,
        timeout_ms: None,
        idle_timeout_ms: None,
        interactive: None,
        provider: None,
    }
}

impl WorkerState {
    #[cfg(test)]
    fn new(
        actor_id: String,
        spec: AgentSpec,
        profile_dir: PathBuf,
        paths: AgentPaths,
        agent_server_url: String,
    ) -> Self {
        Self::new_with_transport(
            actor_id,
            spec,
            test_command_transport(),
            profile_dir,
            paths,
            agent_server_url,
        )
    }

    #[cfg(test)]
    fn new_with_transport(
        actor_id: String,
        spec: AgentSpec,
        transport: AgentTransport,
        profile_dir: PathBuf,
        paths: AgentPaths,
        agent_server_url: String,
    ) -> Self {
        Self::new_with_agent_config_version(
            actor_id,
            spec,
            transport,
            profile_dir,
            paths,
            agent_server_url,
            "agent_config_test".into(),
        )
    }

    fn new_with_agent_config_version(
        actor_id: String,
        spec: AgentSpec,
        transport: AgentTransport,
        profile_dir: PathBuf,
        paths: AgentPaths,
        agent_server_url: String,
        agent_config_version_id: String,
    ) -> Self {
        let selected_model = load_model_state(&profile_dir)
            .filter(|model| persisted_model_is_allowed(&spec, &transport, model))
            .or_else(|| default_model_for_spec(&spec));
        let mut actor_display_cache = HashMap::new();
        if !spec.actor.display_name.trim().is_empty() {
            actor_display_cache.insert(spec.actor.id.clone(), spec.actor.display_name.clone());
        }
        Self {
            actor_id,
            spec,
            transport,
            profile_dir,
            paths,
            agent_server_url,
            agent_config_version_id,
            active_turns: Mutex::new(HashMap::new()),
            pending_triggers: Mutex::new(HashMap::new()),
            text_buffer: Mutex::new(HashMap::new()),
            usage_totals: Mutex::new(HashMap::new()),
            seeded: Mutex::new(HashSet::new()),
            scope_channel_cache: Mutex::new(HashMap::new()),
            seen_sources: Mutex::new(HashSet::new()),
            action_map: Mutex::new(HashMap::new()),
            model_action_map: Mutex::new(HashMap::new()),
            selected_model: Mutex::new(selected_model),
            actor_display_cache: Mutex::new(actor_display_cache),
        }
    }

    fn current_turn(&self, scope_id: &str) -> Option<ActiveTurn> {
        self.active_turns
            .lock()
            .expect("active_turns poisoned")
            .get(scope_id)
            .cloned()
    }

    fn set_turn(&self, turn: ActiveTurn) {
        self.active_turns
            .lock()
            .expect("active_turns poisoned")
            .insert(turn.scope.id.clone(), turn);
    }

    fn mark_cancel_requested(&self, scope_id: &str, turn_id: &str) -> Option<ActiveTurn> {
        let mut active = self.active_turns.lock().expect("active_turns poisoned");
        let turn = active.get_mut(scope_id)?;
        if turn.id != turn_id {
            return None;
        }
        turn.cancel_requested = true;
        Some(turn.clone())
    }

    fn mark_no_reply_requested(&self, run_id: &str) -> Option<ActiveTurn> {
        let mut active = self.active_turns.lock().expect("active_turns poisoned");
        let turn = active.values_mut().find(|turn| turn.run_id == run_id)?;
        turn.no_reply_requested = true;
        Some(turn.clone())
    }

    /// Drop the active turn for `scope_id` and pop the next queued trigger for
    /// that same scope (if any).
    fn clear_turn(&self, scope_id: &str) -> Option<AgentTrigger> {
        let mut active = self.active_turns.lock().expect("active_turns poisoned");
        active.remove(scope_id);
        drop(active);
        let mut pending = self.pending_triggers.lock().expect("pending poisoned");
        let next = match pending.get_mut(scope_id) {
            Some(queue) => queue.pop_front(),
            None => None,
        };
        if pending
            .get(scope_id)
            .map(|queue| queue.is_empty())
            .unwrap_or(false)
        {
            pending.remove(scope_id);
        }
        next
    }

    fn enqueue(&self, scope_id: &str, trigger: AgentTrigger) {
        let mut pending = self.pending_triggers.lock().expect("pending poisoned");
        if pending
            .values()
            .any(|queue| queue.iter().any(|queued| queued.id() == trigger.id()))
        {
            return;
        }
        let queue = pending.entry(scope_id.to_string()).or_default();
        if is_priority_trigger(&trigger) {
            let insert_at = queue
                .iter()
                .rposition(is_priority_trigger)
                .map(|idx| idx + 1)
                .unwrap_or(0);
            queue.insert(insert_at, trigger);
        } else {
            queue.push_back(trigger);
        }
    }

    fn has_pending_source(&self, source_id: &str) -> bool {
        self.pending_triggers
            .lock()
            .expect("pending poisoned")
            .values()
            .any(|queue| queue.iter().any(|trigger| trigger.id() == source_id))
    }

    fn has_active_trigger(&self, source_id: &str) -> bool {
        self.active_turns
            .lock()
            .expect("active_turns poisoned")
            .values()
            .any(|turn| turn.trigger_source_id == source_id)
    }

    fn push_text(&self, turn_id: &str, chunk: &str) {
        if chunk.is_empty() {
            return;
        }
        self.text_buffer
            .lock()
            .expect("text_buffer poisoned")
            .entry(turn_id.to_string())
            .or_default()
            .push_str(chunk);
    }

    fn take_text(&self, turn_id: &str) -> Option<String> {
        let mut buf = self.text_buffer.lock().expect("text_buffer poisoned");
        buf.remove(turn_id).filter(|s| !s.is_empty())
    }

    fn accumulate_usage(&self, scope_id: &str, increment: &TokenUsage) -> TokenUsage {
        let mut totals = self.usage_totals.lock().expect("usage_totals poisoned");
        let total = totals.entry(scope_id.to_string()).or_default();
        usage::add_usage(total, increment);
        usage::normalized_usage(total.clone())
    }

    fn take_seed_slot(&self, scope_id: &str) -> bool {
        self.seeded
            .lock()
            .expect("seeded poisoned")
            .insert(scope_id.to_string())
    }

    fn record_action_request(&self, message_id: String, request_id: String) {
        self.action_map
            .lock()
            .expect("action_map poisoned")
            .insert(message_id, request_id);
    }

    fn lookup_action_request(&self, message_id: &str) -> Option<String> {
        self.action_map
            .lock()
            .expect("action_map poisoned")
            .get(message_id)
            .cloned()
    }

    fn forget_action_request(&self, message_id: &str) {
        let _ = self
            .action_map
            .lock()
            .expect("action_map poisoned")
            .remove(message_id);
    }

    fn current_model(&self) -> Option<String> {
        self.selected_model
            .lock()
            .expect("selected_model poisoned")
            .clone()
    }

    fn cache_actor_displays(&self, actors: impl IntoIterator<Item = (String, String)>) {
        let mut cache = self
            .actor_display_cache
            .lock()
            .expect("actor_display_cache poisoned");
        for (id, display) in actors {
            if !id.trim().is_empty() && !display.trim().is_empty() {
                cache.insert(id, display);
            }
        }
    }

    fn actor_display_snapshot(&self) -> HashMap<String, String> {
        self.actor_display_cache
            .lock()
            .expect("actor_display_cache poisoned")
            .clone()
    }

    fn model_choices(&self) -> Vec<AgentModelChoice> {
        model_choices_for_spec(&self.spec)
    }

    fn model_choice(&self, id: &str) -> Option<AgentModelChoice> {
        self.model_choices()
            .into_iter()
            .find(|choice| choice.id == id)
    }

    fn set_current_model(&self, model: String) -> Result<()> {
        if !model_is_allowed(&self.spec, &model) {
            return Err(anyhow!(
                "model `{model}` is not configured for {}",
                self.actor_id
            ));
        }
        self.set_current_model_unchecked(model)
    }

    fn set_current_model_unchecked(&self, model: String) -> Result<()> {
        let model = model.trim().to_string();
        if model.is_empty() {
            return Err(anyhow!("model cannot be empty for {}", self.actor_id));
        }
        persist_model_state(&self.profile_dir, &model)?;
        *self.selected_model.lock().expect("selected_model poisoned") = Some(model);
        Ok(())
    }

    fn record_model_action_request(&self, message_id: String, request: ModelActionRequest) {
        self.model_action_map
            .lock()
            .expect("model_action_map poisoned")
            .insert(message_id, request);
    }

    fn lookup_model_action_request(&self, message_id: &str) -> Option<ModelActionRequest> {
        self.model_action_map
            .lock()
            .expect("model_action_map poisoned")
            .get(message_id)
            .cloned()
    }

    fn is_model_action_request(&self, message_id: &str) -> bool {
        self.model_action_map
            .lock()
            .expect("model_action_map poisoned")
            .contains_key(message_id)
    }

    fn forget_model_action_request(&self, message_id: &str) {
        let _ = self
            .model_action_map
            .lock()
            .expect("model_action_map poisoned")
            .remove(message_id);
    }

    fn remember_source(&self, source_id: &str) -> bool {
        self.seen_sources
            .lock()
            .expect("seen_sources poisoned")
            .insert(source_id.to_string())
    }
}

fn is_priority_trigger(trigger: &AgentTrigger) -> bool {
    trigger.actor_id().starts_with("actor_human_")
        || matches!(trigger.scope().kind, ScopeKind::Channel)
}

fn model_state_path(profile_dir: &Path) -> PathBuf {
    profile_dir.join("model.json")
}

fn load_model_state(profile_dir: &Path) -> Option<String> {
    let path = model_state_path(profile_dir);
    let text = std::fs::read_to_string(path).ok()?;
    let state: ModelStateFile = serde_json::from_str(&text).ok()?;
    let model = state.model.trim();
    if model.is_empty() {
        None
    } else {
        Some(model.to_string())
    }
}

fn persist_model_state(profile_dir: &Path, model: &str) -> Result<()> {
    std::fs::create_dir_all(profile_dir)
        .with_context(|| format!("create profile dir {}", profile_dir.display()))?;
    let path = model_state_path(profile_dir);
    let text = serde_json::to_string_pretty(&ModelStateFile {
        model: model.to_string(),
    })?;
    std::fs::write(&path, format!("{text}\n"))
        .with_context(|| format!("write model state {}", path.display()))?;
    Ok(())
}

fn default_model_for_spec(spec: &AgentSpec) -> Option<String> {
    spec.models
        .as_ref()
        .and_then(|models| models.default.as_deref())
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(ToOwned::to_owned)
        .filter(|model| model_is_allowed(spec, model))
        .or_else(|| {
            model_choices_for_spec(spec)
                .into_iter()
                .next()
                .map(|c| c.id)
        })
}

fn model_is_allowed(spec: &AgentSpec, model: &str) -> bool {
    let model = model.trim();
    if model.is_empty() {
        return false;
    }
    model_choices_for_spec(spec)
        .iter()
        .any(|choice| choice.id == model)
}

fn persisted_model_is_allowed(spec: &AgentSpec, transport: &AgentTransport, model: &str) -> bool {
    let model = model.trim();
    if model.is_empty() {
        return false;
    }
    if transport_supports_runtime_model_options(transport) {
        return true;
    }
    let choices = model_choices_for_spec(spec);
    if choices.is_empty() {
        return true;
    }
    choices.iter().any(|choice| choice.id == model)
}

fn transport_supports_runtime_model_options(transport: &AgentTransport) -> bool {
    transport.kind == "acp_stdio"
}

fn model_choices_for_spec(spec: &AgentSpec) -> Vec<AgentModelChoice> {
    let Some(models) = spec.models.as_ref() else {
        return Vec::new();
    };
    let mut choices = Vec::new();
    let mut seen = HashSet::new();
    for choice in &models.choices {
        let id = choice.id.trim();
        if id.is_empty() || !seen.insert(id.to_string()) {
            continue;
        }
        let mut choice = choice.clone();
        choice.id = id.to_string();
        choices.push(choice);
    }
    if let Some(default) = models
        .default
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty())
    {
        if seen.insert(default.to_string()) {
            choices.insert(
                0,
                AgentModelChoice {
                    id: default.to_string(),
                    label: default.to_string(),
                    description: None,
                },
            );
        }
    }
    choices
}

fn model_choice_label(choice: &AgentModelChoice) -> &str {
    if choice.label.trim().is_empty() {
        choice.id.as_str()
    } else {
        choice.label.as_str()
    }
}

async fn run_agent_worker(spec: AgentSpec, server_url: String, data_root: PathBuf) -> Result<()> {
    let transport = resolve_transport_for_spec(&spec)
        .map_err(|e| anyhow!("resolve transport for {}: {e}", spec.actor.id))?;
    let actor_id = spec.actor.id.clone();
    let display_name = if spec.actor.display_name.is_empty() {
        actor_id.clone()
    } else {
        spec.actor.display_name.clone()
    };
    let paths = AgentPaths::new(&data_root, &actor_id);
    let bundle_paths = paths.bundle_paths(&spec);
    paths.ensure(&actor_id, &spec, &bundle_paths)?;

    let client = Client::connect(&server_url).await?;
    client.initialize().await?;
    // Pre-register the actor row before opening our agent-bound connection.
    // `connection/open` would auto-upsert under the hood, but it can't carry
    // the spec's full `Actor` (display name, kind, capabilities) — explicitly
    // upserting first guarantees the invite UI on humans' machines lists the
    // agent with its proper metadata even before we own the inbox.
    let _: Value = client
        .call(method::ACTOR_UPSERT, json!({ "actor": spec.actor }))
        .await
        .with_context(|| format!("actor/upsert for {}", actor_id))?;
    let actor_kind = match spec.actor.kind {
        ActorKind::Human => "human",
        ActorKind::Agent => "agent",
        ActorKind::Service => "service",
    };
    client
        .open_connection_as(&actor_id, actor_kind, Some(&display_name))
        .await?;
    let agent_config_version_id =
        publish_runtime_agent_config(&client, &actor_id, &spec, &transport).await?;
    eprintln!(
        "[{actor_id}] connected to {server_url} as {:?}",
        spec.actor.kind
    );

    let agent_server_url = agent_child_server_url(&server_url);
    if agent_server_url != server_url {
        eprintln!(
            "[{actor_id}] injecting LOOM_SERVER={} for child agents (agent-client connected via {})",
            agent_server_url, server_url
        );
    }
    let state = Arc::new(WorkerState::new_with_agent_config_version(
        actor_id.clone(),
        spec.clone(),
        transport.clone(),
        paths.profile.clone(),
        paths.clone(),
        agent_server_url.clone(),
        agent_config_version_id,
    ));
    let (event_tx, event_rx) = mpsc::unbounded_channel::<AdapterEvent>();
    let adapter = build_adapter(&spec, &transport, &paths, &bundle_paths, &agent_server_url)?;

    // Translator: AdapterEvent → server RPC. Drains until adapter drops the
    // sender (worker exit) — at which point the loop falls out and the task
    // ends. The adapter handle is passed in so the Finished branch can pop the
    // next queued trigger for the same scope and dispatch it.
    {
        let client = client.clone();
        let state = state.clone();
        let actor = actor_id.clone();
        let adapter = adapter.clone();
        tokio::spawn(async move {
            translate_events(client, state, adapter, actor, event_rx).await;
        });
    }

    let result = notification_loop(client, state, adapter.clone(), event_tx, &actor_id).await;
    if let Err(e) = adapter.stop().await {
        tracing::warn!(actor = %actor_id, %e, "adapter stop failed before reconnect");
    }
    result
}

fn resolve_transport_for_spec(spec: &AgentSpec) -> Result<AgentTransport> {
    agent_runtime::provider::default_registry()
        .and_then(|registry| registry.resolve_transport(&spec.provider_ref))
        .map_err(|e| anyhow!(e))
}

async fn publish_runtime_agent_config(
    client: &Client,
    actor_id: &str,
    spec: &AgentSpec,
    transport: &AgentTransport,
) -> Result<String> {
    let spec_json = serde_json::to_value(spec).context("serialize agent spec")?;
    let spec_bytes = serde_json::to_vec(spec).context("serialize agent spec for hash")?;
    let mut hasher = Sha256::new();
    hasher.update(&spec_bytes);
    let spec_hash = format!("{:x}", hasher.finalize());
    let version = format!("runtime-{}", &spec_hash[..12]);
    let prompt = spec
        .prompt_template
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .context("serialize prompt template")?
        .unwrap_or_default();
    let model = default_model_for_spec(spec)
        .or_else(|| transport.model.clone())
        .unwrap_or_default();
    let capabilities = spec
        .actor
        .capabilities
        .as_ref()
        .and_then(|value| value.get("capabilities"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let published: AgentConfigPublishResult = client
        .call(
            method::AGENT_CONFIG_PUBLISH,
            json!({
                "actorId": actor_id,
                "version": version,
                "prompt": prompt,
                "model": model,
                "adapter": transport.kind,
                "tools": spec_json,
                "capabilityTags": capabilities,
                "metadata": {
                    "source": "loom-daemon",
                    "specHash": format!("sha256:{spec_hash}"),
                }
            }),
        )
        .await
        .with_context(|| format!("agent_config.publish for {actor_id}"))?;
    let activated: AgentConfigActivateResult = client
        .call(
            method::AGENT_CONFIG_ACTIVATE,
            json!({
                "actorId": actor_id,
                "versionId": published.version.id,
            }),
        )
        .await
        .with_context(|| format!("agent_config.activate for {actor_id}"))?;
    Ok(activated.version.id)
}

fn build_adapter(
    spec: &AgentSpec,
    transport: &AgentTransport,
    paths: &AgentPaths,
    bundle_paths: &BundlePaths,
    server_url: &str,
) -> Result<Arc<dyn Adapter>> {
    let mut process_env: BTreeMap<String, String> = transport
        .env
        .iter()
        .map(|(k, v)| (k.clone(), paths.expand(v, Some(bundle_paths))))
        .collect();
    let mut command_env = transport.env.clone();
    let loom_binary = current_loom_binary();
    inject_loom_cli_env(&mut process_env, loom_binary.as_deref());
    inject_loom_cli_env(&mut command_env, loom_binary.as_deref());
    process_env
        .entry("LOOM_SERVER".into())
        .or_insert_with(|| server_url.to_string());
    command_env
        .entry("LOOM_SERVER".into())
        .or_insert_with(|| server_url.to_string());
    if let Some(socket) = daemon_ipc::env_socket_path() {
        let socket = socket.display().to_string();
        process_env
            .entry("LOOM_DAEMON_SOCKET".into())
            .or_insert_with(|| socket.clone());
        command_env
            .entry("LOOM_DAEMON_SOCKET".into())
            .or_insert_with(|| socket.clone());
        process_env
            .entry(daemon_ipc::ENV_DAEMON_SOCKET.into())
            .or_insert_with(|| socket.clone());
        command_env
            .entry(daemon_ipc::ENV_DAEMON_SOCKET.into())
            .or_insert(socket);
    }
    process_env
        .entry("LOOM_ACTOR".into())
        .or_insert_with(|| spec.actor.id.clone());
    command_env
        .entry("LOOM_ACTOR".into())
        .or_insert_with(|| spec.actor.id.clone());
    process_env
        .entry("LOOM_AGENT_PROFILE".into())
        .or_insert_with(|| paths.profile.display().to_string());
    command_env
        .entry("LOOM_AGENT_PROFILE".into())
        .or_insert_with(|| paths.profile.display().to_string());
    process_env
        .entry("LOOM_AGENT_BUNDLE_ROOT".into())
        .or_insert_with(|| bundle_paths.root.display().to_string());
    command_env
        .entry("LOOM_AGENT_BUNDLE_ROOT".into())
        .or_insert_with(|| bundle_paths.root.display().to_string());
    process_env
        .entry("LOOM_AGENT_BUNDLE_DIR".into())
        .or_insert_with(|| bundle_paths.current.display().to_string());
    command_env
        .entry("LOOM_AGENT_BUNDLE_DIR".into())
        .or_insert_with(|| bundle_paths.current.display().to_string());
    if !bundle_paths.version.is_empty() {
        process_env
            .entry("LOOM_AGENT_BUNDLE_VERSION".into())
            .or_insert_with(|| bundle_paths.version.clone());
        command_env
            .entry("LOOM_AGENT_BUNDLE_VERSION".into())
            .or_insert_with(|| bundle_paths.version.clone());
    }
    insert_static_local_time_env(&mut process_env);
    insert_static_local_time_env(&mut command_env);
    let process_args: Vec<String> = transport
        .args
        .iter()
        .map(|a| paths.expand(a, Some(bundle_paths)))
        .collect();

    match transport.kind.as_str() {
        "acp_stdio" => {
            let mcp_servers = agent_runtime::build_mcp_servers(
                loom_binary.as_deref(),
                &spec.actor.id,
                &paths.profile,
                spec.memory.as_ref(),
                spec.announcement.as_ref(),
                Some(server_url),
            );
            let cfg = AcpConfig {
                command: transport.command.clone(),
                args: process_args,
                env: process_env,
                process_cwd: paths.root.clone(),
                auth_method: transport.auth_method.clone(),
                mcp_servers,
            };
            Ok(Arc::new(AcpAdapter::new(cfg)))
        }
        "command" => {
            let cfg = CommandConfig::from_transport(
                spec.actor.id.clone(),
                transport.command.clone(),
                transport.args.clone(),
                command_env,
                transport,
                paths.sessions.clone(),
            );
            Ok(Arc::new(CommandAdapter::new(cfg)))
        }
        "interactive_command" => {
            let interactive = transport.interactive.clone().unwrap_or_default();
            let model = spec
                .models
                .as_ref()
                .and_then(|m| m.default.clone())
                .or_else(|| transport.model.clone());
            let cfg = InteractiveCommandConfig::new(
                spec.actor.id.clone(),
                transport.command.clone(),
                &transport.args,
                command_env,
                model,
                transport.model_args.clone(),
                interactive,
                transport.provider.clone(),
                paths.sessions.clone(),
                paths.profile.clone(),
            );
            Ok(Arc::new(InteractiveCommandAdapter::new(cfg)))
        }
        other => Err(anyhow!(
            "unknown transport kind `{other}` for agent {}",
            spec.actor.id
        )),
    }
}

async fn notification_loop(
    client: Arc<Client>,
    state: Arc<WorkerState>,
    adapter: Arc<dyn Adapter>,
    event_tx: mpsc::UnboundedSender<AdapterEvent>,
    actor_id: &str,
) -> Result<()> {
    let mut started = false;
    let mut inbox_poll = interval(Duration::from_secs(15));
    loop {
        // Drain pending notifications. We pop them one by one and dispatch
        // each on its own; the borrow on `notifications` is released between
        // iterations so nested RPC calls (run.open, message.send) can use the
        // same Client without deadlock.
        let next = tokio::select! {
            _ = inbox_poll.tick() => {
                if let Err(e) = drain_pending_inbox(
                    &client,
                    &state,
                    &adapter,
                    &event_tx,
                    &mut started,
                    actor_id,
                ).await {
                    eprintln!("[{actor_id}] failed to drain pending inbox: {e}");
                }
                continue;
            }
            next = async {
                let mut rx = client.notifications.lock().await;
                rx.recv().await
            } => next,
        };
        let Some(n) = next else {
            eprintln!("[{actor_id}] server disconnected, worker exiting");
            return Ok(());
        };

        if n.method != method::STREAM_UPDATE {
            continue;
        }
        let Some(params) = n.params else { continue };
        let Some(kind) = params.get("kind").and_then(|v| v.as_str()) else {
            continue;
        };
        if kind == stream_kind::MESSAGE_CREATED {
            let Some(message_value) = params.get("data").and_then(|d| d.get("message")).cloned()
            else {
                continue;
            };
            let Ok(mut message) = serde_json::from_value::<Message>(message_value) else {
                continue;
            };
            let inbox_delivery = is_actor_inbox_delivery_for(&params, actor_id);
            if !is_message_for_us_with_delivery(&message, actor_id, inbox_delivery) {
                continue;
            }
            if inbox_delivery {
                mark_actor_inbox_delivery(&mut message, actor_id);
            }
            if !state.remember_source(&message.id) {
                continue;
            }
            if message.metadata.get("kind").and_then(Value::as_str) == Some("action.response") {
                if let Err(e) =
                    handle_action_response_message(&client, &state, &adapter, &message).await
                {
                    eprintln!("[{actor_id}] failed to handle action.response message: {e}");
                } else if let Err(e) =
                    record_delivery_seen_by_id(&client, actor_id, &message.id).await
                {
                    tracing::warn!(
                        actor = %actor_id,
                        message = %message.id,
                        %e,
                        "failed to record delivery ack for action.response message"
                    );
                }
                continue;
            }
            match handle_message_trigger(
                &client,
                &state,
                &adapter,
                &event_tx,
                &mut started,
                actor_id,
                message,
            )
            .await
            {
                Ok(TriggerOutcome::Dispatched) => {}
                Ok(TriggerOutcome::Queued) => {}
                Err(e) => eprintln!("[{actor_id}] failed to handle message trigger: {e}"),
            }
            continue;
        }
        if kind == stream_kind::RUN_UPDATED {
            let Some(run_value) = params.get("data").and_then(|d| d.get("run")).cloned() else {
                continue;
            };
            let Ok(run) = serde_json::from_value::<Run>(run_value) else {
                continue;
            };
            if run.actor_id == actor_id && run_requests_no_reply(&run) {
                let _ = state.mark_no_reply_requested(&run.id);
            }
            continue;
        }
    }
}

fn action_request_id_from_message(message: &Message) -> Option<String> {
    message
        .metadata
        .get("requestId")
        .or_else(|| message.metadata.get("actionId"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
}

fn is_loom_tool_request_id(id: &str) -> bool {
    id.starts_with("loom:question:") || id.starts_with("loom:approval:")
}

async fn handle_model_action_response_message(
    client: &Arc<Client>,
    state: &Arc<WorkerState>,
    adapter: &Arc<dyn Adapter>,
    message: &Message,
    request_message_id: &str,
) -> Result<()> {
    let option_id = message
        .metadata
        .get("optionId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if option_id.is_empty() {
        eprintln!(
            "[{}] model action.response message {} ignored: missing metadata.optionId",
            state.actor_id, message.id
        );
        return Ok(());
    }

    let request = state
        .lookup_model_action_request(request_message_id)
        .unwrap_or_else(|| ModelActionRequest {
            source: ModelActionSource::Spec,
            choices: state.model_choices(),
        });

    let choice = request
        .choices
        .iter()
        .find(|choice| choice.id == option_id)
        .cloned()
        .or_else(|| state.model_choice(&option_id));

    let Some(choice) = choice else {
        eprintln!(
            "[{}] model action.response message {} ignored: unknown model `{}`",
            state.actor_id, message.id, option_id
        );
        state.forget_model_action_request(request_message_id);
        return Ok(());
    };

    if let Err(err) =
        apply_model_selection_for_scope(state, adapter, &message.scope, &request.source, &option_id)
            .await
    {
        state.forget_model_action_request(request_message_id);
        append_model_selection_failure_message(client, state, message, &choice, &option_id, &err)
            .await?;
        eprintln!(
            "[{}] failed to select model `{}` via {}: {}",
            state.actor_id, option_id, message.id, err
        );
        return Ok(());
    }
    state.forget_model_action_request(request_message_id);

    let label = model_choice_label(&choice);
    let text = model_selection_success_text(&request.source, label, &option_id);
    send_scope_message(
        client,
        &message.scope,
        text,
        Some(message.id.clone()),
        Some(message.author_actor_id.clone()),
        MessageIntent::StatusUpdate,
        DeliveryPolicy::NotifyOnly,
        Meta::default(),
    )
    .await?;
    eprintln!(
        "[{}] selected model `{}` via {}",
        state.actor_id, option_id, message.id
    );
    Ok(())
}

fn model_selection_success_text(
    source: &ModelActionSource,
    label: &str,
    option_id: &str,
) -> String {
    match source {
        ModelActionSource::Adapter { .. } => format!(
            "Model set to `{label}` (`{option_id}`). It has been applied to the current ACP session and saved for future sessions."
        ),
        ModelActionSource::Spec => format!(
            "Model set to `{label}` (`{option_id}`). It is saved for this agent and will be used when Loom creates a new ACP session."
        ),
    }
}

async fn apply_model_selection_for_scope(
    state: &WorkerState,
    adapter: &Arc<dyn Adapter>,
    scope: &ScopeRef,
    source: &ModelActionSource,
    option_id: &str,
) -> Result<()> {
    match source {
        ModelActionSource::Spec => {
            state.set_current_model(option_id.to_string())?;
        }
        ModelActionSource::Adapter { config_id } => {
            adapter
                .set_model_option(scope.clone(), config_id.clone(), option_id.to_string())
                .await
                .map_err(|err| anyhow!("ACP session/set_config_option failed: {err}"))?;
            state.set_current_model_unchecked(option_id.to_string())?;
        }
    }
    Ok(())
}

async fn append_model_selection_failure_message(
    client: &Arc<Client>,
    _state: &WorkerState,
    message: &Message,
    choice: &AgentModelChoice,
    option_id: &str,
    err: &anyhow::Error,
) -> Result<()> {
    let label = model_choice_label(choice);
    send_scope_message(
        client,
        &message.scope,
        format!("Failed to set model `{label}` (`{option_id}`): `{err}`."),
        Some(message.id.clone()),
        Some(message.author_actor_id.clone()),
        MessageIntent::StatusUpdate,
        DeliveryPolicy::NotifyOnly,
        Meta::default(),
    )
    .await
    .map(|_| ())
}

#[cfg(test)]
fn is_for_us(event: &Event, actor_id: &str) -> bool {
    // Self-authored directed events are still explicit routing signals. They are not
    // the normal way to enter a thread, but generated callbacks or deliberate
    // follow-up turns must not be filtered out just because author == target.
    event.relations.iter().any(|r| {
        matches!(r.kind, RelationKind::DirectedTo)
            && r.target.kind == RefKind::Actor
            && r.target.id == actor_id
    })
}

#[cfg(test)]
fn is_message_for_us(message: &Message, actor_id: &str) -> bool {
    is_message_for_us_with_delivery(message, actor_id, false)
}

fn is_inbox_message_for_us(message: &Message, actor_id: &str) -> bool {
    is_message_for_us_with_delivery(message, actor_id, true)
}

fn is_message_for_us_with_delivery(
    message: &Message,
    actor_id: &str,
    actor_inbox_delivery: bool,
) -> bool {
    if message.author_actor_id == actor_id {
        return false;
    }
    if message.target == format!("dm:@{actor_id}") {
        return true;
    }
    if message.audience.iter().any(|audience| match audience.kind {
        AudienceKind::Actor => {
            audience.id == actor_id
                && (message.delivery_policy == DeliveryPolicy::WakeAgent
                    || message.metadata.get("kind").and_then(Value::as_str)
                        == Some("action.request"))
        }
        AudienceKind::All | AudienceKind::Agents => {
            message.delivery_policy == DeliveryPolicy::WakeAgent
        }
        AudienceKind::Humans | AudienceKind::Group => false,
    }) {
        return true;
    }
    actor_inbox_delivery && message_counts_as_actor_inbox_attention(message, actor_id)
}

fn is_actor_inbox_delivery_for(params: &Value, actor_id: &str) -> bool {
    params
        .get("delivery")
        .and_then(|delivery| delivery.get("actorId"))
        .and_then(Value::as_str)
        == Some(actor_id)
}

fn mark_actor_inbox_delivery(message: &mut Message, actor_id: &str) {
    message.metadata.insert(
        LOCAL_ACTOR_INBOX_DELIVERY_META.into(),
        json!({
            "actorId": actor_id,
            "source": "actor_inbox",
        }),
    );
}

fn is_local_actor_inbox_delivery(trigger: &AgentTrigger, actor_id: &str) -> bool {
    match trigger {
        AgentTrigger::Message(message) => {
            message
                .metadata
                .get(LOCAL_ACTOR_INBOX_DELIVERY_META)
                .and_then(|delivery| delivery.get("actorId"))
                .and_then(Value::as_str)
                == Some(actor_id)
        }
        #[cfg(test)]
        AgentTrigger::Event(_) => false,
    }
}

fn run_requests_no_reply(run: &Run) -> bool {
    run.metadata
        .get("noReply")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || run
            .metadata
            .get("replyMode")
            .and_then(Value::as_str)
            .is_some_and(|value| value == "none" || value == "ignore")
}

fn message_counts_as_actor_inbox_attention(message: &Message, actor_id: &str) -> bool {
    if message.delivery_policy == DeliveryPolicy::Silent {
        return false;
    }
    if message
        .audience
        .iter()
        .any(|audience| audience.kind == AudienceKind::Actor && audience.id == actor_id)
    {
        return false;
    }
    if message.scope.kind == ScopeKind::Thread {
        return true;
    }
    message.delivery_policy == DeliveryPolicy::WakeAgent
        && message
            .audience
            .iter()
            .any(|audience| audience.kind == AudienceKind::Group)
}

fn trigger_message(trigger: &AgentTrigger) -> Option<&Message> {
    match trigger {
        AgentTrigger::Message(message) => Some(message),
        #[cfg(test)]
        AgentTrigger::Event(_) => None,
    }
}

async fn handle_message_trigger(
    client: &Arc<Client>,
    state: &Arc<WorkerState>,
    adapter: &Arc<dyn Adapter>,
    event_tx: &mpsc::UnboundedSender<AdapterEvent>,
    started: &mut bool,
    actor_id: &str,
    message: Message,
) -> Result<TriggerOutcome> {
    let trigger = AgentTrigger::Message(message);
    match handle_control_command(client, state, adapter, event_tx, started, &trigger).await {
        Ok(true) => {
            record_delivery_seen_by_id(client, actor_id, trigger.id()).await?;
            return Ok(TriggerOutcome::Dispatched);
        }
        Ok(false) => {}
        Err(e) => {
            record_delivery_seen_by_id(client, actor_id, trigger.id()).await?;
            return Err(e);
        }
    }
    if let Some(err) = try_ensure_adapter_started(state, adapter, event_tx, started).await {
        return Err(anyhow!(
            "adapter start failed while handling message trigger: {err}"
        ));
    }
    handle_trigger(client, state, adapter, trigger).await
}

async fn handle_action_response_message(
    client: &Arc<Client>,
    state: &Arc<WorkerState>,
    adapter: &Arc<dyn Adapter>,
    message: &Message,
) -> Result<()> {
    let request_message_id = message
        .parent_message_id
        .as_deref()
        .or_else(|| {
            message
                .metadata
                .get("requestMessageId")
                .and_then(Value::as_str)
        })
        .filter(|value| !value.trim().is_empty());
    let Some(request_message_id) = request_message_id else {
        eprintln!(
            "[{}] action.response message {} ignored: missing parentMessageId",
            state.actor_id, message.id
        );
        return Ok(());
    };
    let echoed_request_id = action_request_id_from_message(message);
    if echoed_request_id
        .as_deref()
        .is_some_and(is_loom_tool_request_id)
    {
        eprintln!(
            "[{}] action.response message {} is for a loom human-interaction tool {}; leaving it for the waiting tool process",
            state.actor_id, message.id, request_message_id
        );
        return Ok(());
    }
    if state.is_model_action_request(request_message_id)
        || echoed_request_id
            .as_deref()
            .is_some_and(|id| id.starts_with("loom:model:"))
    {
        handle_model_action_response_message(client, state, adapter, message, request_message_id)
            .await?;
        return Ok(());
    }
    let request_id = match state.lookup_action_request(request_message_id) {
        Some(id) => id,
        None => match echoed_request_id {
            Some(id) => {
                eprintln!(
                    "[{}] action.response message {} used echoed ACP request id for {}",
                    state.actor_id, message.id, request_message_id
                );
                id
            }
            None => {
                eprintln!(
                    "[{}] action.response message {} ignored: no pending ACP request for {} \
                     (loom-daemon may have restarted after the action.request)",
                    state.actor_id, message.id, request_message_id
                );
                return Ok(());
            }
        },
    };
    let option_id = message
        .metadata
        .get("optionId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if option_id.is_empty() {
        eprintln!(
            "[{}] action.response message {} ignored: missing metadata.optionId",
            state.actor_id, message.id
        );
        return Ok(());
    }
    eprintln!(
        "[{}] action.response message {} -> ACP request {} option {}",
        state.actor_id, message.id, request_id, option_id
    );
    adapter
        .respond_action(request_id.clone(), option_id)
        .await
        .map_err(|e| anyhow!("adapter respond_action failed: {e}"))?;
    state.forget_action_request(request_message_id);
    Ok(())
}

async fn drain_pending_inbox(
    client: &Arc<Client>,
    state: &Arc<WorkerState>,
    adapter: &Arc<dyn Adapter>,
    event_tx: &mpsc::UnboundedSender<AdapterEvent>,
    started: &mut bool,
    actor_id: &str,
) -> Result<()> {
    let res: InboxListResult = client
        .call(
            method::INBOX_LIST,
            json!({
                "actorId": actor_id,
                "state": "pending",
                "limit": 200,
            }),
        )
        .await?;
    if res.deliveries.is_empty() {
        return Ok(());
    }
    let max_age = pending_inbox_max_age();
    let now = Utc::now();
    for entry in res.deliveries {
        let source_id = entry.delivery.source_id.clone();
        if let Some(mut message) = entry.message {
            if !state.remember_source(&message.id) {
                if state.has_pending_source(&message.id) || state.has_active_trigger(&message.id) {
                    continue;
                }
                tracing::warn!(
                    actor = %actor_id,
                    message = %message.id,
                    "retrying pending message delivery that was seen but is no longer active or queued"
                );
            }
            let too_old = now.signed_duration_since(message.created_at) > max_age;
            if too_old {
                tracing::info!(
                    actor = %actor_id,
                    message = %message.id,
                    created_at = %message.created_at,
                    "dropping stale pending message delivery from durable inbox"
                );
                record_delivery_seen_by_id(client, actor_id, &message.id).await?;
                continue;
            }
            if !is_inbox_message_for_us(&message, actor_id) {
                record_delivery_seen_by_id(client, actor_id, &message.id).await?;
                continue;
            }
            mark_actor_inbox_delivery(&mut message, actor_id);
            if message.metadata.get("kind").and_then(Value::as_str) == Some("action.response") {
                handle_action_response_message(client, state, adapter, &message).await?;
                record_delivery_seen_by_id(client, actor_id, &message.id).await?;
                continue;
            }
            let _ = handle_message_trigger(
                client, state, adapter, event_tx, started, actor_id, message,
            )
            .await?;
            continue;
        }
        tracing::warn!(
            actor = %actor_id,
            source = %source_id,
            "acknowledging inbox delivery whose source is no longer a message"
        );
        record_delivery_seen_by_id(client, actor_id, &source_id).await?;
    }
    Ok(())
}

fn pending_inbox_max_age() -> chrono::Duration {
    let secs = std::env::var("LOOM_AGENT_PENDING_MAX_AGE_SECS")
        .ok()
        .and_then(|raw| raw.trim().parse::<i64>().ok())
        .filter(|secs| *secs > 0)
        .unwrap_or(6 * 60 * 60);
    chrono::Duration::seconds(secs)
}

async fn record_delivery_seen(
    client: &Arc<Client>,
    state: &Arc<WorkerState>,
    trigger: &AgentTrigger,
) -> Result<()> {
    record_delivery_seen_by_id(client, &state.actor_id, trigger.id()).await
}

async fn record_delivery_seen_by_id(
    client: &Arc<Client>,
    actor_id: &str,
    source_id: &str,
) -> Result<()> {
    let _: Value = client
        .call(
            method::DELIVERY_ACK,
            json!({
                "actorId": actor_id,
                "sourceId": source_id,
            }),
        )
        .await?;
    Ok(())
}

async fn handle_control_command(
    client: &Arc<Client>,
    state: &Arc<WorkerState>,
    adapter: &Arc<dyn Adapter>,
    event_tx: &mpsc::UnboundedSender<AdapterEvent>,
    started: &mut bool,
    trigger: &AgentTrigger,
) -> Result<bool> {
    match trigger {
        AgentTrigger::Message(message) => {
            if handle_run_cancel_message(state, adapter, message).await? {
                return Ok(true);
            }
        }
        #[cfg(test)]
        AgentTrigger::Event(_) => {}
    }
    match render_prompt(trigger).trim() {
        "/model" | "/models" => {
            let adapter_start_error =
                try_ensure_adapter_started(state, adapter, event_tx, started).await;
            open_model_picker(client, state, adapter, trigger, adapter_start_error).await?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

async fn handle_run_cancel_message(
    state: &Arc<WorkerState>,
    adapter: &Arc<dyn Adapter>,
    message: &Message,
) -> Result<bool> {
    if message.metadata.get("kind").and_then(Value::as_str) != Some("run.cancel") {
        return Ok(false);
    }
    let Some(run_id) = message.metadata.get("runId").and_then(Value::as_str) else {
        tracing::warn!(
            actor = %state.actor_id,
            message = %message.id,
            "run.cancel message missing runId"
        );
        return Ok(true);
    };
    let Some(active) = state.current_turn(&message.scope.id) else {
        return Ok(true);
    };
    if active.run_id != run_id {
        return Ok(true);
    }
    let active = state
        .mark_cancel_requested(&message.scope.id, run_id)
        .unwrap_or(active);

    if let Err(e) = adapter.cancel(active.scope.clone()).await {
        tracing::warn!(
            actor = %state.actor_id,
            run = %active.run_id,
            scope = %active.scope.id,
            %e,
            "adapter cancel failed",
        );
    }
    if state.take_text(&active.id).is_some() {
        tracing::debug!(
            actor = %state.actor_id,
            run = %active.run_id,
            scope = %active.scope.id,
            "discarding buffered text from canceled run"
        );
    }
    Ok(true)
}

async fn try_ensure_adapter_started(
    state: &WorkerState,
    adapter: &Arc<dyn Adapter>,
    event_tx: &mpsc::UnboundedSender<AdapterEvent>,
    started: &mut bool,
) -> Option<String> {
    if *started {
        return None;
    }
    eprintln!(
        "[{}] starting adapter for control command (ACP cold-start can take 30-60s)…",
        state.actor_id
    );
    match adapter.start(event_tx.clone()).await {
        Ok(_) => {
            *started = true;
            eprintln!("[{}] adapter ready", state.actor_id);
            None
        }
        Err(err) => {
            eprintln!(
                "[{}] adapter start failed for control command: {}",
                state.actor_id, err
            );
            Some(err)
        }
    }
}

async fn open_model_picker(
    client: &Arc<Client>,
    state: &Arc<WorkerState>,
    adapter: &Arc<dyn Adapter>,
    trigger: &AgentTrigger,
    adapter_start_error: Option<String>,
) -> Result<()> {
    let mut adapter_error = adapter_start_error;
    let empty_prompt = prompt_telemetry(String::new(), &[]);
    let adapter_options = if adapter_error.is_none() {
        match build_adapter_prompt(client, state, trigger.scope(), &empty_prompt, None, None).await
        {
            Ok(prompt) => match adapter.list_model_options(prompt).await {
                Ok(options) => options,
                Err(err) => {
                    eprintln!(
                        "[{}] failed to load ACP model options: {}",
                        state.actor_id, err
                    );
                    adapter_error = Some(err);
                    None
                }
            },
            Err(err) => {
                adapter_error = Some(err.to_string());
                None
            }
        }
    } else {
        None
    };
    let (choices, current, source, source_description) =
        model_picker_choices(state, adapter_options);
    if choices.is_empty() {
        let mut text = "No model choices are available. The ACP runtime did not return model config options, and this runtime definition does not define model choices.".to_string();
        if let Some(err) = adapter_error {
            text.push_str(&format!("\n\nACP model lookup failed: `{err}`"));
        }
        send_scope_message(
            client,
            trigger.scope(),
            text,
            trigger.is_message().then(|| trigger.id().to_string()),
            Some(trigger.actor_id().to_string()),
            MessageIntent::StatusUpdate,
            DeliveryPolicy::NotifyOnly,
            Meta::default(),
        )
        .await?;
        return Ok(());
    }

    let payload_choices = choices
        .iter()
        .map(|choice| {
            let mut label = model_choice_label(choice).to_string();
            if current.as_deref() == Some(choice.id.as_str()) {
                label.push_str(" (current)");
            }
            json!({ "id": choice.id.clone(), "label": label })
        })
        .collect::<Vec<_>>();
    let current_label = current.as_deref().unwrap_or("(none)");
    let payload = json!({
        "requestId": format!("loom:model:{}", trigger.id()),
        "requestType": "loom.model.select",
        "title": format!("Choose model for @{}", state.actor_id),
        "description": format!(
            "Current model: {current_label}\n\n{source_description}"
        ),
        "choices": payload_choices,
    });
    let sent = send_action_request_message(
        client,
        trigger.scope(),
        trigger.actor_id().to_string(),
        payload,
        trigger.is_message().then(|| trigger.id().to_string()),
        None,
    )
    .await?;
    state.record_model_action_request(
        sent.message.id.clone(),
        ModelActionRequest { source, choices },
    );
    eprintln!(
        "[{}] opened model picker {} for {}",
        state.actor_id,
        sent.message.id,
        trigger.actor_id()
    );
    Ok(())
}

fn model_picker_choices(
    state: &WorkerState,
    adapter_options: Option<AdapterModelOptions>,
) -> (
    Vec<AgentModelChoice>,
    Option<String>,
    ModelActionSource,
    &'static str,
) {
    if let Some(options) = adapter_options.filter(|options| !options.choices.is_empty()) {
        let choices = options
            .choices
            .into_iter()
            .map(|choice| AgentModelChoice {
                id: choice.id,
                label: choice.label,
                description: choice.description,
            })
            .collect();
        return (
            choices,
            options.current_value.or_else(|| state.current_model()),
            ModelActionSource::Adapter {
                config_id: options.config_id,
            },
            "These choices came from the ACP runtime for this session. The selected model is saved locally and applied to the current ACP session.",
        );
    }

    (
        state.model_choices(),
        state.current_model(),
        ModelActionSource::Spec,
        "These choices came from the runtime definition. The selected model is saved for this actor and used when Loom creates ACP sessions.",
    )
}

enum TriggerOutcome {
    Dispatched,
    Queued,
}

async fn handle_trigger(
    client: &Arc<Client>,
    state: &Arc<WorkerState>,
    adapter: &Arc<dyn Adapter>,
    trigger: AgentTrigger,
) -> Result<TriggerOutcome> {
    let trigger = trigger.clone();
    // Scope FIFO: the same actor can handle independent scopes concurrently,
    // but prompts in one thread/channel remain ordered.
    if state.current_turn(&trigger.scope().id).is_some() {
        let scope_id = trigger.scope().id.clone();
        state.enqueue(&scope_id, trigger);
        return Ok(TriggerOutcome::Queued);
    }
    dispatch_trigger(client, state, adapter, trigger)
        .await
        .map(|_| TriggerOutcome::Dispatched)
}

/// Open a turn, mark the scope busy, send the prompt to the adapter. Used by
/// both the initial trigger and the Finished handler when it pops the next
/// queued trigger. On `send_prompt` failure we iteratively drain the queue
/// (rather than spawn-recursing) so a single bad prompt can't strand the rest
/// and the future stays Send for `tokio::spawn`.
async fn dispatch_trigger(
    client: &Arc<Client>,
    state: &Arc<WorkerState>,
    adapter: &Arc<dyn Adapter>,
    mut trigger: AgentTrigger,
) -> Result<AgentTrigger> {
    loop {
        subscribe_scope(client, state, trigger.scope()).await;
        let run_res: RunOpenResult = client
            .call(
                method::RUN_OPEN,
                json!({
                    "actorId": state.actor_id,
                    "scope": trigger.scope(),
                    "startReason": trigger.id(),
                    "agentConfigVersionId": state.agent_config_version_id.clone(),
                    "metadata": {
                        "triggerSourceId": trigger.id(),
                        "triggerIsMessage": trigger.is_message(),
                    },
                }),
            )
            .await?;
        let user_text = render_trigger_prompt(client, state, &trigger).await;
        let prompt = compose_envelope_prompt(client, state, &trigger, &user_text).await;
        let no_reply_file =
            no_reply_file_for_turn(client, state, trigger.scope(), &run_res.run.id).await;
        let active = ActiveTurn {
            id: run_res.run.id.clone(),
            run_id: run_res.run.id.clone(),
            scope: trigger.scope().clone(),
            trigger_source_id: trigger.id().to_string(),
            trigger_is_message: trigger.is_message(),
            reply_target: trigger.reply_target(),
            prompt_stats: prompt.stats.clone(),
            prompt_breakdown: prompt.breakdown.clone(),
            trigger_actor: trigger.actor_id().to_string(),
            no_reply_file,
            no_reply_requested: false,
            cancel_requested: false,
        };
        state.set_turn(active.clone());
        mark_assignment_running_if_needed(client, state, &trigger).await;
        if run_started_ack_enabled() {
            append_run_started_ack(client, state, &active, &trigger).await;
        }

        let adapter_prompt = build_adapter_prompt(
            client,
            state,
            trigger.scope(),
            &prompt,
            Some(&active),
            Some(&trigger),
        )
        .await?;

        match adapter.send_prompt(adapter_prompt).await {
            Ok(()) => return Ok(trigger),
            Err(e) => {
                let _ = close_run(client, &active.run_id, RunStatus::Failed).await;
                if let Err(ack_err) = record_delivery_seen(client, state, &trigger).await {
                    tracing::warn!(
                        actor = %state.actor_id,
                        event = %trigger.id(),
                        %ack_err,
                        "failed to record delivery ack for failed trigger dispatch"
                    );
                }
                let scope_id = trigger.scope().id.clone();
                match state.clear_turn(&scope_id) {
                    Some(next) => {
                        tracing::warn!(
                            actor = %state.actor_id,
                            %e,
                            "send_prompt failed; trying next queued trigger"
                        );
                        trigger = next;
                        continue;
                    }
                    None => return Err(anyhow!("adapter send_prompt failed: {e}")),
                }
            }
        }
    }
}

async fn build_adapter_prompt(
    client: &Arc<Client>,
    state: &Arc<WorkerState>,
    scope: &ScopeRef,
    prompt: &PromptTelemetry,
    active: Option<&ActiveTurn>,
    trigger: Option<&AgentTrigger>,
) -> Result<AdapterPrompt> {
    let channel_id = resolve_channel_for_scope(client, state, scope)
        .await
        .ok_or_else(|| anyhow!("cannot resolve channel for scope {}", scope.id))?;
    let scope_paths = state
        .paths
        .ensure_scope(&state.actor_id, &channel_id, scope)?;
    let mut template_vars = state
        .paths
        .template_vars(&state.actor_id, &channel_id, scope);
    extend_prompt_template_vars(&mut template_vars, state, trigger);
    template_vars.insert(
        "loom.configDir".into(),
        config::config_dir().display().to_string(),
    );
    let outputs = agent_runtime::provider::render_prompt_outputs(
        state.transport.prompt.as_ref(),
        &prompt.parts,
        &prompt.content,
    )
    .map_err(|e| anyhow!("render provider prompt outputs: {e}"))?;
    Ok(AdapterPrompt {
        scope: scope.clone(),
        content: prompt.content.clone(),
        parts: prompt.parts.clone(),
        outputs,
        model: state.current_model(),
        cwd: scope_paths.workspace,
        env: state.paths.scope_env(
            &state.actor_id,
            &channel_id,
            scope,
            &state.agent_server_url,
            active,
        ),
        template_vars,
    })
}

async fn no_reply_file_for_turn(
    client: &Arc<Client>,
    state: &Arc<WorkerState>,
    scope: &ScopeRef,
    run_id: &str,
) -> Option<PathBuf> {
    let channel_id = resolve_channel_for_scope(client, state, scope).await?;
    let paths = state
        .paths
        .ensure_scope(&state.actor_id, &channel_id, scope)
        .ok()?;
    Some(run_no_reply_file(&paths.logs, run_id))
}

async fn subscribe_scope(client: &Arc<Client>, state: &WorkerState, scope: &ScopeRef) {
    if let Err(e) = client
        .call::<_, Value>(method::SCOPE_SUBSCRIBE, json!({ "scope": scope }))
        .await
    {
        eprintln!(
            "[{}] scope/subscribe {}:{} failed: {e}",
            state.actor_id,
            match scope.kind {
                ScopeKind::Channel => "channel",
                ScopeKind::Thread => "thread",
            },
            scope.id
        );
    }
}

fn render_prompt(trigger: &AgentTrigger) -> String {
    match trigger {
        AgentTrigger::Message(message) => {
            if !message.body.is_empty() {
                return message.body.clone();
            }
            serde_json::to_string(&message.metadata).unwrap_or_default()
        }
        #[cfg(test)]
        AgentTrigger::Event(event) => {
            if let Some(text) = event.payload.get("text").and_then(|v| v.as_str()) {
                return text.to_string();
            }
            if let Some(text) = event.payload.get("message").and_then(|v| v.as_str()) {
                return text.to_string();
            }
            serde_json::to_string(&event.payload).unwrap_or_default()
        }
    }
}

async fn render_trigger_prompt(
    client: &Arc<Client>,
    state: &Arc<WorkerState>,
    trigger: &AgentTrigger,
) -> String {
    let actor_names = actor_display_map_for_prompt(client, state).await;
    let mut prompt = render_trigger_prompt_with_names(
        &state.actor_id,
        &state.spec.actor.display_name,
        trigger,
        &actor_names,
    );
    if let Some(context) = assignment_context_for_prompt(client, trigger).await {
        prompt.push_str("\n\n");
        prompt.push_str(&context);
    }
    prompt
}

async fn recent_conversation_context(
    client: &Arc<Client>,
    state: &Arc<WorkerState>,
    trigger: &AgentTrigger,
) -> String {
    let Some(message) = trigger_message(trigger) else {
        return String::new();
    };
    let target = reply_target_for_message(message);
    let result: Result<MessageListResult> = client
        .call(
            method::MESSAGE_LIST,
            json!({
                "target": target,
                "limit": 20,
            }),
        )
        .await
        .with_context(|| format!("message.list target={target}"));
    let messages = match result {
        Ok(result) => result.messages,
        Err(err) => {
            tracing::debug!(
                actor = %state.actor_id,
                message = %message.id,
                %err,
                "recent conversation context unavailable"
            );
            return String::new();
        }
    };
    if messages.is_empty() {
        return String::new();
    }
    let actor_names = actor_display_map_for_prompt(client, state).await;
    format_recent_conversation_context(&messages, message, &actor_names)
}

fn format_recent_conversation_context(
    messages: &[Message],
    trigger: &Message,
    actor_names: &HashMap<String, String>,
) -> String {
    let lines = messages
        .iter()
        .filter(|message| message.id != trigger.id)
        .filter(|message| message.created_at <= Utc::now())
        .filter(|message| {
            message.metadata.get("kind").and_then(Value::as_str) != Some("run.started_ack")
        })
        .filter_map(|message| {
            let body = compact_message_body(&message.body);
            if body.is_empty() {
                return None;
            }
            Some(format!(
                "- {}: {}",
                actor_label(&message.author_actor_id, actor_names),
                body
            ))
        })
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return String::new();
    }
    format!(
        "=== Recent Loom conversation ===\n\
         These are prior messages in the same thread/channel. Continue from them; do not repeat a number or answer another actor already supplied.\n{}",
        lines.join("\n")
    )
}

fn compact_message_body(body: &str) -> String {
    let compact = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= 800 {
        compact
    } else {
        format!("{}...", compact.chars().take(800).collect::<String>())
    }
}

fn join_prompt_sections(sections: impl IntoIterator<Item = String>) -> String {
    sections
        .into_iter()
        .filter(|section| !section.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

async fn actor_display_map_for_prompt(
    client: &Arc<Client>,
    state: &Arc<WorkerState>,
) -> HashMap<String, String> {
    match client
        .call::<_, ActorListResult>(method::ACTOR_LIST, json!({}))
        .await
    {
        Ok(result) => {
            state.cache_actor_displays(
                result
                    .actors
                    .into_iter()
                    .map(|actor| (actor.id, actor.display_name)),
            );
        }
        Err(err) => {
            tracing::debug!(
                actor = %state.actor_id,
                %err,
                "actor/list failed while rendering prompt; using cached actor display names",
            );
        }
    }
    state.actor_display_snapshot()
}

fn render_trigger_prompt_with_names(
    local_actor_id: &str,
    local_display_name: &str,
    trigger: &AgentTrigger,
    actor_names: &HashMap<String, String>,
) -> String {
    let text = annotate_actor_mentions(&render_prompt(trigger), actor_names);
    let from = actor_label(trigger.actor_id(), actor_names);
    let me = actor_label_with_fallback(local_actor_id, local_display_name, actor_names);
    let delivery_targets = trigger_target_ids(trigger);
    let target_labels = delivery_targets
        .iter()
        .map(|id| actor_label(id, actor_names))
        .collect::<Vec<_>>();
    let local_inbox_delivery = is_local_actor_inbox_delivery(trigger, local_actor_id);
    let delivery = if delivery_targets.iter().any(|id| id == local_actor_id) {
        "explicit route to you"
    } else if local_inbox_delivery {
        "thread/task attention to you"
    } else if delivery_targets.is_empty() {
        "scope message"
    } else {
        "explicit route to another actor"
    };
    let visible = if target_labels.is_empty() {
        format!("{from}: {text}")
    } else if local_inbox_delivery && !delivery_targets.iter().any(|id| id == local_actor_id) {
        format!(
            "{from} (visible route -> {}): {text}",
            target_labels.join(", ")
        )
    } else {
        format!("route -> {}: {text}", target_labels.join(", "))
    };
    let scope = trigger.scope();
    let scope_kind = scope_kind_name(scope.kind);
    let source_label = if trigger.is_message() {
        "Message id"
    } else {
        "Source id"
    };

    let mut out = format!(
        "=== Latest Loom message ===\n\
         Your actor: {me}\n\
         From: {from}\n\
         Scope: {scope_kind}:{scope_id}\n\
         {source_label}: {source_id}\n\
         Delivery: {delivery}\n",
        scope_id = scope.id,
        source_id = trigger.id(),
    );
    if !target_labels.is_empty() {
        out.push_str("Route target(s): ");
        out.push_str(&target_labels.join(", "));
        out.push('\n');
    }
    if let Some(task_context) = trigger_task_context(trigger) {
        out.push_str(&task_context);
    }
    out.push_str("Visible message:\n");
    out.push_str(&visible);
    out
}

fn trigger_task_context(trigger: &AgentTrigger) -> Option<String> {
    let task_id = trigger
        .meta_value("taskId")
        .and_then(|value| value.as_str())?;
    let mut out = format!("Task id: {task_id}\n");
    if let Some(number) = trigger
        .meta_value("taskNumber")
        .and_then(|value| value.as_u64())
    {
        out.push_str(&format!("Task number: #{number}\n"));
    }
    if let Some(status) = trigger
        .meta_value("taskStatus")
        .and_then(|value| value.as_str())
    {
        out.push_str(&format!("Task status: {status}\n"));
    }
    if let Some(owner) = trigger
        .meta_value("taskOwnerActorId")
        .and_then(|value| value.as_str())
    {
        out.push_str(&format!("Task owner: {owner}\n"));
    }
    if let Some(assignment_id) = trigger
        .meta_value("assignmentId")
        .and_then(|value| value.as_str())
    {
        out.push_str(&format!("Assignment id: {assignment_id}\n"));
    }
    if let Some(expected) = trigger
        .meta_value("expectedOutput")
        .and_then(|value| value.as_str())
    {
        out.push_str("Expected output: ");
        out.push_str(expected);
        out.push('\n');
    }
    Some(out)
}

async fn assignment_context_for_prompt(
    client: &Arc<Client>,
    trigger: &AgentTrigger,
) -> Option<String> {
    let assignment_id = trigger
        .meta_value("assignmentId")
        .and_then(|value| value.as_str())?;
    match client
        .call::<_, TaskAssignmentContextResult>(
            method::TASK_ASSIGNMENT_CONTEXT,
            json!({ "assignmentId": assignment_id }),
        )
        .await
    {
        Ok(context) => {
            let body = serde_json::to_string_pretty(&context)
                .unwrap_or_else(|_| "{\"error\":\"failed to render assignment context\"}".into());
            Some(format!(
                "=== Loom assignment context ===\n\
                 This JSON is the authoritative task input. Read it before acting; use preflight before external side effects.\n\
                 Assignment lifecycle rules:\n\
                 - Publish durable outputs with `loom artifact publish`, then make them typed task outputs with `loom task artifact attach <task_id> --artifact-id <art_id> --schema <schema> --role <role> --status active`.\n\
                 - Record durable evidence with `loom task fact append`; do not use plain messages as gate evidence.\n\
                 - Finish this assignment with `loom task assignment update <assignment_id> --status completed --result <summary> --result-artifact-id <art_id> ... --result-fact-id <fact_id> ...`.\n\
                 - Do not route to another actor directly to finish an assignment; the assignment update returns the task to the assigning actor.\n\
                 ```json\n{body}\n```"
            ))
        }
        Err(err) => Some(format!(
            "=== Loom assignment context ===\n\
             Failed to load assignment-context for {assignment_id}: {err}. Return a blocked/stale result instead of continuing from natural-language instruction only."
        )),
    }
}

fn trigger_target_ids(trigger: &AgentTrigger) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut targets = Vec::new();
    match trigger {
        AgentTrigger::Message(message) => {
            for audience in &message.audience {
                if audience.kind == AudienceKind::Actor && seen.insert(audience.id.clone()) {
                    targets.push(audience.id.clone());
                }
            }
        }
        #[cfg(test)]
        AgentTrigger::Event(event) => {
            for relation in &event.relations {
                if matches!(relation.kind, RelationKind::DirectedTo)
                    && relation.target.kind == RefKind::Actor
                    && seen.insert(relation.target.id.clone())
                {
                    targets.push(relation.target.id.clone());
                }
            }
        }
    }
    targets
}

fn actor_label(actor_id: &str, actor_names: &HashMap<String, String>) -> String {
    actor_label_with_fallback(actor_id, actor_id, actor_names)
}

fn actor_label_with_fallback(
    actor_id: &str,
    fallback_display: &str,
    actor_names: &HashMap<String, String>,
) -> String {
    let display = actor_names
        .get(actor_id)
        .map(String::as_str)
        .unwrap_or(fallback_display)
        .trim();
    if display.is_empty() || display == actor_id {
        format!("@{actor_id}")
    } else {
        format!("{display} (@{actor_id})")
    }
}

fn annotate_actor_mentions(text: &str, actor_names: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut idx = 0;
    while idx < text.len() {
        let ch = text[idx..].chars().next().expect("idx is char boundary");
        if ch == '@' {
            let token_start = idx + ch.len_utf8();
            let mut token_end = token_start;
            for (offset, candidate) in text[token_start..].char_indices() {
                if is_actor_ref_char(candidate) {
                    token_end = token_start + offset + candidate.len_utf8();
                } else {
                    break;
                }
            }
            if token_end > token_start {
                let actor_id = &text[token_start..token_end];
                if actor_names.contains_key(actor_id) {
                    out.push_str(&actor_label(actor_id, actor_names));
                    idx = token_end;
                    continue;
                }
            }
        }
        out.push(ch);
        idx += ch.len_utf8();
    }
    out
}

fn is_actor_ref_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':')
}

fn actor_context_manifest(actor_id: &str, display_name: &str) -> String {
    let mut actor_names = HashMap::new();
    if !display_name.trim().is_empty() {
        actor_names.insert(actor_id.to_string(), display_name.to_string());
    }
    let label = actor_label_with_fallback(actor_id, display_name, &actor_names);
    format!(
        "=== System: Loom actor context ===\n\
         You are {label}.\n\
         Treat this as your stable runtime actor context. Other @actors in the\n\
         latest message are routing targets or people being discussed; they\n\
         are not this actor."
    )
}

#[derive(Debug, Clone)]
struct LocalTimeInfo {
    local_rfc3339: String,
    utc_rfc3339: String,
    utc_offset: String,
    zone_abbrev: String,
    timezone_name: Option<String>,
}

fn local_time_info() -> LocalTimeInfo {
    let local = Local::now();
    let utc = local.with_timezone(&Utc);
    LocalTimeInfo {
        local_rfc3339: local.to_rfc3339_opts(SecondsFormat::Secs, false),
        utc_rfc3339: utc.to_rfc3339_opts(SecondsFormat::Secs, true),
        utc_offset: local.format("%:z").to_string(),
        zone_abbrev: local.format("%Z").to_string(),
        timezone_name: local_timezone_name(),
    }
}

fn local_time_manifest() -> String {
    let info = local_time_info();
    let timezone = timezone_label(&info);
    format!(
        "=== System: Local time context ===\n\
         Current local time: {local}\n\
         Current UTC time: {utc}\n\
         Local timezone: {timezone}\n\
         UTC offset: {offset}\n\
         Loom protocol timestamps are RFC3339 UTC, often ending in `Z`.\n\
         Convert those timestamps to the local timezone above before comparing\n\
         them with GUI/chat timestamps or describing times to the user.",
        local = info.local_rfc3339,
        utc = info.utc_rfc3339,
        timezone = timezone,
        offset = info.utc_offset,
    )
}

fn insert_static_local_time_env(env: &mut BTreeMap<String, String>) {
    let info = local_time_info();
    env.entry("LOOM_LOCAL_TIMEZONE".into())
        .or_insert_with(|| timezone_env_value(&info));
    env.entry("LOOM_LOCAL_TIMEZONE_LABEL".into())
        .or_insert_with(|| timezone_label(&info));
    env.entry("LOOM_LOCAL_UTC_OFFSET".into())
        .or_insert_with(|| info.utc_offset.clone());
    if !info.zone_abbrev.is_empty() {
        env.entry("LOOM_LOCAL_TIMEZONE_ABBR".into())
            .or_insert_with(|| info.zone_abbrev.clone());
    }
    if let Some(name) = info.timezone_name {
        env.entry("TZ".into()).or_insert(name);
    }
}

fn insert_current_time_env(env: &mut BTreeMap<String, String>) {
    let info = local_time_info();
    env.insert("LOOM_CURRENT_TIME".into(), info.local_rfc3339.clone());
    env.insert("LOOM_CURRENT_TIME_UTC".into(), info.utc_rfc3339.clone());
    env.insert("LOOM_LOCAL_TIMEZONE".into(), timezone_env_value(&info));
    env.insert("LOOM_LOCAL_TIMEZONE_LABEL".into(), timezone_label(&info));
    env.insert("LOOM_LOCAL_UTC_OFFSET".into(), info.utc_offset.clone());
    if !info.zone_abbrev.is_empty() {
        env.insert("LOOM_LOCAL_TIMEZONE_ABBR".into(), info.zone_abbrev.clone());
    }
    if let Some(name) = info.timezone_name {
        env.insert("TZ".into(), name);
    }
}

fn timezone_env_value(info: &LocalTimeInfo) -> String {
    info.timezone_name
        .clone()
        .unwrap_or_else(|| info.utc_offset.clone())
}

fn timezone_label(info: &LocalTimeInfo) -> String {
    match (
        info.timezone_name.as_deref(),
        info.zone_abbrev.trim().is_empty(),
    ) {
        (Some(name), false) => format!("{name} ({}, UTC{})", info.zone_abbrev, info.utc_offset),
        (Some(name), true) => format!("{name} (UTC{})", info.utc_offset),
        (None, false) => format!("{} (UTC{})", info.zone_abbrev, info.utc_offset),
        (None, true) => format!("UTC{}", info.utc_offset),
    }
}

fn local_timezone_name() -> Option<String> {
    std::env::var("TZ")
        .ok()
        .and_then(|tz| normalize_timezone_value(&tz))
        .or_else(timezone_from_localtime_link)
}

fn timezone_from_localtime_link() -> Option<String> {
    let link = std::fs::read_link("/etc/localtime").ok()?;
    let raw = link.to_string_lossy();
    for marker in ["/zoneinfo/", "/usr/share/zoneinfo/"] {
        if let Some((_, suffix)) = raw.split_once(marker) {
            return normalize_timezone_value(suffix);
        }
    }
    None
}

fn normalize_timezone_value(value: &str) -> Option<String> {
    let mut value = value.trim().trim_start_matches(':').trim();
    if let Some(stripped) = value.strip_prefix("posix/") {
        value = stripped;
    }
    if let Some(stripped) = value.strip_prefix("right/") {
        value = stripped;
    }
    if value.is_empty() || value == "localtime" || value.starts_with('/') || value.contains('\0') {
        return None;
    }
    Some(value.to_string())
}

/// Per-turn prompt composition for v1. Mirrors
/// `server::runtime::wakeup::compose_envelope_prompt` — agents that don't
/// configure legacy profile fields / memory fall back to the pre-envelope shape.
async fn compose_envelope_prompt(
    client: &Arc<Client>,
    state: &Arc<WorkerState>,
    trigger: &AgentTrigger,
    user_text: &str,
) -> PromptTelemetry {
    let scope = trigger.scope();
    let first_turn = state.take_seed_slot(&scope.id);
    let actor_context = actor_context_manifest(&state.actor_id, &state.spec.actor.display_name);
    let conversation_context = recent_conversation_context(client, state, trigger).await;
    let runtime_context =
        join_prompt_sections([local_time_manifest(), conversation_context.clone()]);
    let scope_bootstrap = if first_turn || command_transport_without_resume(&state.transport) {
        seed_manifest(&state.actor_id, scope)
    } else {
        String::new()
    };
    let channel_id = resolve_channel_for_scope(client, state, scope).await;
    let template_vars = channel_id
        .as_deref()
        .map(|channel_id| prompt_template_vars(state, trigger, channel_id))
        .unwrap_or_else(|| minimal_prompt_template_vars(state, trigger));
    let user_text = apply_prompt_template(
        state.spec.prompt_template.as_ref(),
        &template_vars,
        first_turn,
        &user_text,
    );

    let memory_spec = state.spec.memory.as_ref();

    if memory_spec.is_none() {
        let mut sections = vec![agent_runtime::PromptSection {
            name: "actor_context",
            content: actor_context.clone(),
        }];
        if !scope_bootstrap.is_empty() {
            sections.push(agent_runtime::PromptSection {
                name: "scope_bootstrap",
                content: scope_bootstrap.clone(),
            });
        }
        sections.push(agent_runtime::PromptSection {
            name: "runtime_context",
            content: runtime_context.clone(),
        });
        sections.push(agent_runtime::PromptSection {
            name: "user_message",
            content: format!("=== User message ===\n{user_text}"),
        });
        let content = sections
            .iter()
            .map(|section| section.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        return apply_trigger_prefix_to_prompt(
            &state.spec,
            prompt_telemetry(content, &sections),
            first_turn,
            trigger_prompt_prefix_from_trigger(trigger),
        );
    }

    let (prompt, sections) =
        agent_runtime::envelope::build_envelope(&agent_runtime::envelope::BuildContext {
            actor_context: &actor_context,
            profile_dir: &state.profile_dir,
            memory_spec,
            channel_id: channel_id.as_deref(),
            thread_context: &conversation_context,
            runtime_context: &runtime_context,
            user_message: &user_text,
            scope_bootstrap: &scope_bootstrap,
        });
    apply_trigger_prefix_to_prompt(
        &state.spec,
        prompt_telemetry(prompt, &sections),
        first_turn,
        trigger_prompt_prefix_from_trigger(trigger),
    )
}

fn apply_trigger_prefix_to_prompt(
    spec: &AgentSpec,
    mut prompt: PromptTelemetry,
    first_turn: bool,
    trigger_prefix: Option<&str>,
) -> PromptTelemetry {
    let Some(prefix) = trigger_prefix.or_else(|| trigger_prefix_for_turn(spec, first_turn)) else {
        return prompt;
    };
    if prompt.content.starts_with(prefix)
        || prompt
            .parts
            .iter()
            .any(|part| part.key == "user_message" && part.content.starts_with(prefix))
    {
        return prompt;
    }

    if let Some(part) = prompt
        .parts
        .iter_mut()
        .find(|part| part.key == "user_message")
    {
        part.content = format!("{prefix}{}", part.content);
        prompt.content = prompt
            .parts
            .iter()
            .map(|part| part.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
    } else {
        prompt.content = format!("{prefix}{}", prompt.content);
    }
    prompt.stats = prompt_stats(&prompt.content);

    let stats = prompt_stats(prefix);
    prompt.breakdown.sections.insert(
        0,
        PromptBreakdownSection {
            key: "trigger_prefix".to_string(),
            label: "Trigger Prefix".to_string(),
            char_count: stats.char_count,
            byte_count: stats.byte_count,
            approx_token_count: stats.approx_token_count,
            percentage: 0.0,
        },
    );
    recalculate_prompt_breakdown_percentages(&mut prompt.breakdown.sections);
    prompt
}

fn trigger_prompt_prefix_from_trigger(trigger: &AgentTrigger) -> Option<&str> {
    trigger
        .meta_value("triggerPromptPrefix")
        .and_then(|value| value.as_str())
        .filter(|prefix| !prefix.is_empty())
}

fn trigger_prefix_for_turn(spec: &AgentSpec, first_turn: bool) -> Option<&str> {
    let trigger = spec.trigger.as_ref()?;
    let prefix = trigger.trigger_prompt_prefix.as_str();
    if prefix.is_empty() {
        return None;
    }
    let applies = match trigger.apply_on {
        TriggerPrefixApplyOn::EveryTurn => true,
        TriggerPrefixApplyOn::FirstTurn => first_turn,
    };
    applies.then_some(prefix)
}

fn apply_prompt_template(
    template: Option<&PromptTemplateSpec>,
    vars: &BTreeMap<String, String>,
    first_turn: bool,
    user_text: &str,
) -> String {
    let Some(template) = template else {
        return user_text.to_string();
    };
    let mut sections = Vec::new();
    sections.extend(
        template
            .every_turn_prefix
            .iter()
            .map(|line| expand_prompt_vars(line, vars)),
    );
    if first_turn {
        sections.extend(
            template
                .first_turn_prefix
                .iter()
                .map(|line| expand_prompt_vars(line, vars)),
        );
    }
    sections.push(user_text.to_string());
    sections.extend(
        template
            .every_turn_suffix
            .iter()
            .map(|line| expand_prompt_vars(line, vars)),
    );
    sections
        .into_iter()
        .filter(|section| !section.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn prompt_template_vars(
    state: &WorkerState,
    trigger: &AgentTrigger,
    channel_id: &str,
) -> BTreeMap<String, String> {
    let mut vars = state
        .paths
        .template_vars(&state.actor_id, channel_id, trigger.scope());
    extend_prompt_template_vars(&mut vars, state, Some(trigger));
    vars
}

fn minimal_prompt_template_vars(
    state: &WorkerState,
    trigger: &AgentTrigger,
) -> BTreeMap<String, String> {
    let mut vars = BTreeMap::new();
    vars.insert("actor.id".into(), state.actor_id.clone());
    vars.insert("scope.id".into(), trigger.scope().id.clone());
    vars.insert(
        "scope.kind".into(),
        scope_kind_name(trigger.scope().kind).to_string(),
    );
    extend_prompt_template_vars(&mut vars, state, Some(trigger));
    vars
}

fn extend_prompt_template_vars(
    vars: &mut BTreeMap<String, String>,
    state: &WorkerState,
    trigger: Option<&AgentTrigger>,
) {
    if let Some(trigger) = trigger {
        vars.insert("trigger.id".into(), trigger.id().to_string());
        vars.insert("trigger.actor_id".into(), trigger.actor_id().to_string());
        if let Some(target) = trigger.reply_target() {
            vars.insert("reply.target".into(), target);
        }
    }
    if let Some(template) = state.spec.prompt_template.as_ref() {
        if let Some(active_skill) = template.active_skill.as_deref() {
            vars.insert("prompt.activeSkill".into(), active_skill.to_string());
        }
        for (key, value) in &template.vars {
            vars.insert(format!("vars.{key}"), value.clone());
        }
    }
}

fn expand_prompt_vars(input: &str, vars: &BTreeMap<String, String>) -> String {
    let mut out = input.to_string();
    for (key, value) in vars {
        out = out.replace(&format!("{{{key}}}"), value);
    }
    out
}

fn prompt_telemetry(content: String, sections: &[agent_runtime::PromptSection]) -> PromptTelemetry {
    let stats = prompt_stats(&content);
    let parts = sections
        .iter()
        .filter(|section| !section.content.trim().is_empty())
        .map(prompt_part_from_section)
        .collect::<Vec<_>>();
    let mut breakdown_sections: Vec<PromptBreakdownSection> = sections
        .iter()
        .filter(|section| !section.content.trim().is_empty())
        .map(|section| {
            let stats = prompt_stats(&section.content);
            PromptBreakdownSection {
                key: section.name.to_string(),
                label: prompt_section_label(section.name).to_string(),
                char_count: stats.char_count,
                byte_count: stats.byte_count,
                approx_token_count: stats.approx_token_count,
                percentage: 0.0,
            }
        })
        .collect();
    recalculate_prompt_breakdown_percentages(&mut breakdown_sections);
    PromptTelemetry {
        content,
        parts,
        stats,
        breakdown: PromptBreakdown {
            sections: breakdown_sections,
        },
    }
}

fn prompt_part_from_section(section: &agent_runtime::PromptSection) -> PromptPart {
    PromptPart {
        key: section.name.to_string(),
        title: prompt_section_title(section.name).to_string(),
        content: section.content.clone(),
        role_hint: match section.name {
            "actor_context" | "bootstrap_memory" | "scope_bootstrap" => PromptRoleHint::System,
            _ => PromptRoleHint::User,
        },
    }
}

fn prompt_section_title(name: &str) -> &str {
    match name {
        "actor_context" => "System: Loom actor context",
        "bootstrap_memory" => "System: Bootstrap memory",
        "turn_memory" => "Context: Turn memory",
        "runtime_context" => "Context: Runtime context",
        "scope_bootstrap" => "System: Loom multi-actor context",
        "user_message" => "User message",
        other => other,
    }
}

fn recalculate_prompt_breakdown_percentages(sections: &mut [PromptBreakdownSection]) {
    let total_tokens = sections
        .iter()
        .map(|section| section.approx_token_count)
        .sum::<u64>()
        .max(1) as f64;
    for section in sections {
        section.percentage = (section.approx_token_count as f64 / total_tokens) * 100.0;
    }
}

fn prompt_stats(text: &str) -> PromptStats {
    PromptStats {
        char_count: text.chars().count(),
        byte_count: text.len(),
        approx_token_count: usage::estimate_tokens(text),
    }
}

fn prompt_section_label(name: &str) -> &str {
    match name {
        "actor_context" => "Actor Context",
        "bootstrap_memory" => "Bootstrap Memory",
        "turn_memory" => "Turn Memory",
        "runtime_context" => "Runtime Context",
        "scope_bootstrap" => "Scope Bootstrap",
        "user_message" => "Latest Message",
        other => other,
    }
}

fn command_transport_without_resume(transport: &AgentTransport) -> bool {
    if transport.kind != "command" {
        return false;
    }
    match transport.session.as_ref() {
        Some(session) => session.first_run_capture.is_none() || session.resume_args.is_none(),
        None => true,
    }
}

/// Resolve a scope → channel_id. Channel scopes are identity — they are the
/// channel. Thread scopes need a one-time `thread/list` sweep; the result is
/// cached on `WorkerState` so we don't hit the server per turn. Archived
/// threads are queried as a fallback because explicit routed messages can arrive from
/// historical threads that are no longer in the active list. A lookup
/// failure (network error, thread not visible, etc.) returns `None`, which
/// the memory selector interprets as "no channel scope available" and falls
/// open — slightly leakier but never-wedging.
async fn resolve_channel_for_scope(
    client: &Arc<Client>,
    state: &Arc<WorkerState>,
    scope: &ScopeRef,
) -> Option<String> {
    match scope.kind {
        ScopeKind::Channel => Some(scope.id.clone()),
        ScopeKind::Thread => {
            if let Some(cached) = state
                .scope_channel_cache
                .lock()
                .ok()
                .and_then(|c| c.get(&scope.id).cloned())
            {
                return Some(cached);
            }
            for params in [json!({}), json!({ "archived": true })] {
                let res: proto::methods::ThreadListResult =
                    client.call(method::THREAD_LIST, params).await.ok()?;
                let mut cache = state.scope_channel_cache.lock().ok()?;
                let mut found: Option<String> = None;
                for t in res.threads {
                    if t.id == scope.id {
                        found = Some(t.channel_id.clone());
                    }
                    cache.insert(t.id, t.channel_id);
                }
                if found.is_some() {
                    return found;
                }
            }
            None
        }
    }
}

/// Mirror of `runtime::wakeup::seed_manifest`. Duplicated rather than extracted
/// for the v1 MVP — the manifest format is small and embedded/external paths
/// will diverge anyway (e.g. agent client may wire a richer set of CLI hints).
fn seed_manifest(actor_id: &str, scope: &ScopeRef) -> String {
    let scope_kind = match scope.kind {
        ScopeKind::Thread => "thread",
        ScopeKind::Channel => "channel",
    };
    format!(
        "=== System: Loom multi-actor context (auto-injected on session start) ===\n\
         You are an agent driven by `loom-daemon`.\n\
         Identity:\n\
           actor id      = {actor_id}\n\
           current scope = {scope_kind}:{scope_id}\n\
         \n\
         You can shell out to the `loom` CLI for server access. The daemon prepends the CLI directory to PATH and also sets LOOM_CLI to the absolute CLI path when it can resolve one. LOOM_SERVER,\n\
         LOOM_CLI, LOOM_DAEMON_SOCKET, LOOM_ACTOR, LOOM_SCOPE_ID, LOOM_SCOPE_KIND, LOOM_RUN_ID, LOOM_TRIGGER_MESSAGE_ID, LOOM_TRIGGER_ACTOR, LOOM_REPLY_TARGET, and LOOM_NO_REPLY_FILE are already injected into your env,\n\
         so commands like:\n\
           loom --json inbox list --no-ack\n\
           \"$LOOM_CLI\" --json inbox list --no-ack\n\
           loom --json message read --target '#<channel_id>:<root_message_id>'\n\
           loom --json message send --target '#<channel_id>:<root_message_id>' --if-latest <message_id> --text \"rebased delta\"\n\
           loom --json run ignore --reason \"not directed at me\"\n\
           loom --json task claim --source-message \"$LOOM_TRIGGER_MESSAGE_ID\"\n\
           loom --json task complete <task_id> --result \"short outcome summary\"\n\
           loom --json artifact get <art_id|artifact://...>\n\
           loom --json task assign <task_id> --to <actor_id> --type <type> --instruction <text> --contract-file <path>\n\
           loom --json ask-user-question --title \"Choose option\" --question \"Which option?\" --choice a=A --choice b=B\n\
           loom --json request-approval --title \"Approval required\" --reason \"Run the deploy command\"\n\
        Assistant text is internal run transcript only. It is not published to\n\
        the channel or thread. For any visible reply, call\n\
        `loom --json message send --target \"$LOOM_REPLY_TARGET\" --text ...`\n\
        when LOOM_REPLY_TARGET is set; after that, final\n\
        assistant text may be empty or a private note. When no visible reply is\n\
        needed, call `loom --json run ignore --reason \"...\"`.\n\
        If the user or another actor asks you to hand off, wake, route, or\n\
        notify a specific actor, that routed visible message is required work.\n\
        A plain notify_only thread message does not wake the target actor. Send\n\
        a message whose text includes `@actor_id` and whose flags include\n\
        `--intent request_action --delivery-policy wake_agent`; only call\n\
        `run ignore` after that message was successfully sent or when no\n\
        routed visible message is needed.\n\
        The same rule applies to turn-taking: when your visible message expects\n\
        a specific actor's next answer, guess, review, or decision, route it to\n\
        that actor with `@actor_id`, `--intent request_action`, and\n\
        `--delivery-policy wake_agent`.\n\
        Only send messages when you have actionable content: a requested\n\
        answer, a claimed work unit and result, a material state change, a\n\
        needed question, or a real blocker. Do not send visibility-only\n\
        updates, acknowledgements, or \"nothing to do\" summaries.\n\
        Hard collaboration rule: claim before work, rebase before send. If a\n\
        top-level message is a work item, try to claim it by source message\n\
        before doing substantive work. A successful task claim makes you the\n\
        lifecycle owner/coordinator; it is not a lock over every internal work\n\
        unit. If claim fails because another owner exists, stop for ordinary\n\
        single-owner work. For shared/multi-agent work (`@all`, explicit slots,\n\
        roles, or \"each agent\" instructions), do not steal the task owner;\n\
        read the latest canonical thread and only participate in an unclaimed\n\
        internal slot/work unit if one is still needed. Before sending any\n\
        visible messages with `loom message send`, read latest, adjust your\n\
        content to the still-needed delta, and send with `--if-latest <message_id>`.\n\
        `@all` and multi-actor routed work is concurrent by default. Do not\n\
        assume the daemon serialized other agents ahead of you; use the latest\n\
        thread state as the source of truth and rebase visible output against it.\n\
        When you contribute to shared work owned by another actor, post only\n\
        the still-needed delta in the canonical thread: the internal unit you\n\
        claimed, the result you produced, what remains, and whether the task\n\
        owner needs to close the outer task. Do not complete the outer task\n\
        unless you are its owner/coordinator.\n\
        If you are the task owner/coordinator and the task reaches its\n\
        acceptance criteria, you must call\n\
        `loom --json task complete <task_id> --result ...` exactly once before\n\
        or with the final visible summary. A message saying \"complete\", \"done\",\n\
        or a final answer without that tool call does not complete the task. If\n\
        the task id is not in context, query Loom for the task anchored to the\n\
        source message or thread root, then complete that task id.\n\
        No acknowledgement ping-pong: if the latest routed message is only a\n\
        confirmation, receipt, already-final result, or \"no further action\",\n\
        do not reply. For terminal tasks, stay silent unless the message asks\n\
        for new work. If your decision is \"no action needed\" or \"not for me\",\n\
        call `loom --json run ignore --reason \"not directed at me\"` and then\n\
        end the turn without visible answer text. The runtime will not infer\n\
        no-reply from message text or keyword heuristics. Do not send a\n\
        confirmation or explain the silence.\n\
        `loom task assign` requires a machine-readable contract. Do not fall back\n\
        to direct actor routing when assignment creation fails; report the\n\
        blocker or fix the contract and retry the assignment.\n\
        A direct handoff without an explicit `@actor_id` audience and\n\
        `wake_agent` delivery is only a visible note; it will not start the\n\
        receiving agent.\n\
        For games, Q&A, reviews, or any other back-and-forth, each turn that\n\
        needs the other actor to respond must be a routed wake message to that\n\
        actor.\n\
         `loom ask-user-question` is for choices or missing input; its JSON\n\
         output is the human's answer to your question, not an approval.\n\
         `loom request-approval` is for approve/reject gates before risky work.\n\
         continue the current task using `answer.optionId`, `answer.label`, or\n\
         `answer.text`, and phrase follow-up messages as the user's answer.\n\
         Message targets use `#<channel_id>` for channels and\n\
         `#<channel_id>:<root_message_id>` for threads; sending to a thread\n\
         target creates or reuses the thread automatically. Use\n\
         `loom --json thread list` only when you need to map a thread scope id\n\
         back to that target.\n\
         Use `--json` for machine-readable output and `loom <subcommand> --help`\n\
         for the full surface. Only the message after the marker line is the new\n\
         user input.\n\
         ",
        actor_id = actor_id,
        scope_kind = scope_kind,
        scope_id = scope.id,
    )
}

async fn translate_events(
    client: Arc<Client>,
    state: Arc<WorkerState>,
    adapter: Arc<dyn Adapter>,
    actor_id: String,
    mut rx: mpsc::UnboundedReceiver<AdapterEvent>,
) {
    loop {
        // Async first, then drain everything that's queued behind it. This
        // keeps the order strict (mpsc is FIFO) without ever holding the lock
        // across awaits.
        let Some(ev) = rx.recv().await else { return };
        if let Err(e) = translate_one(&client, &state, &adapter, &actor_id, ev).await {
            eprintln!("[{actor_id}] translate failed: {e}");
        }
        loop {
            match rx.try_recv() {
                Ok(ev) => {
                    if let Err(e) = translate_one(&client, &state, &adapter, &actor_id, ev).await {
                        eprintln!("[{actor_id}] translate failed: {e}");
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
    }
}

async fn translate_one(
    client: &Arc<Client>,
    state: &Arc<WorkerState>,
    adapter: &Arc<dyn Adapter>,
    actor_id: &str,
    ev: AdapterEvent,
) -> Result<()> {
    // Resolve the scope this event belongs to and look up the active turn for
    // it. Per-scope variants (Text/ToolUse/ActionRequest/Finished) require a
    // scope tag; agent-wide ones (StatusChange/Error with `scope: None`) are
    // handled in their own arms below.
    let scope_for_event = ev.scope().cloned();
    let active = scope_for_event
        .as_ref()
        .and_then(|s| state.current_turn(&s.id));

    match ev {
        AdapterEvent::Text {
            scope: _,
            content,
            is_partial,
        } => {
            let Some(active) = active else {
                tracing::warn!(actor = %actor_id, "Text event without matching active turn; dropping");
                return Ok(());
            };
            if active.cancel_requested {
                return Ok(());
            }
            if turn_no_reply_requested(&active) {
                if !is_partial {
                    let _ = state.take_text(&active.id);
                }
            } else if is_partial {
                state.push_text(&active.id, &content);
            } else if agent_text_auto_publish_enabled() {
                if let Some(text) = state.take_text(&active.id) {
                    if let Some(text) = visible_agent_text_for_turn(&active, &text) {
                        let meta = build_turn_base_meta(&active);
                        flush_text(client, actor_id, &active, text, Some(meta)).await?;
                    }
                }
                if let Some(text) = visible_agent_text_for_turn(&active, &content) {
                    let meta = build_turn_base_meta(&active);
                    flush_text(client, actor_id, &active, text, Some(meta)).await?;
                }
            } else {
                let _ = state.take_text(&active.id);
            }
            append_trace(
                client,
                &active.run_id,
                TraceKind::TextDelta,
                json!({ "text": content }),
            )
            .await?;
        }
        AdapterEvent::ToolUse {
            scope: _,
            tool_name,
            input,
        } => {
            let Some(active) = active else {
                tracing::warn!(actor = %actor_id, "ToolUse event without matching active turn; dropping");
                return Ok(());
            };
            if active.cancel_requested {
                return Ok(());
            }
            append_trace(
                client,
                &active.run_id,
                TraceKind::ToolStart,
                json!({ "toolName": tool_name, "input": input }),
            )
            .await?;
        }
        AdapterEvent::ActionRequest {
            scope: _,
            id,
            request_type,
            title,
            description,
            choices,
        } => {
            let Some(active) = active else {
                tracing::warn!(actor = %actor_id, "ActionRequest event without matching active turn; dropping");
                return Ok(());
            };
            if active.cancel_requested {
                return Ok(());
            }
            // Surface the request to the trigger actor (so they can
            // `loom action accept/decline`) and remember the ACP request id.
            // The response message is parented to this request and delivered
            // back to the agent through the durable actor inbox.
            let payload = json!({
                "requestId": id,
                "requestType": request_type,
                "title": title,
                "description": description,
                "choices": choices.iter().map(|c| json!({
                    "id": c.id,
                    "label": c.label,
                })).collect::<Vec<_>>(),
            });
            let sent = send_action_request_message(
                client,
                &active.scope,
                active.trigger_actor.clone(),
                payload,
                active
                    .trigger_is_message
                    .then(|| active.trigger_source_id.clone()),
                Some(active.run_id.clone()),
            )
            .await?;
            state.record_action_request(sent.message.id.clone(), id.clone());
            eprintln!(
                "[{actor_id}] action.request {} -> trigger {} (ACP request {})",
                sent.message.id, active.trigger_actor, id
            );
        }
        AdapterEvent::StatusChange { scope: _, status } => {
            // If the event is scope-tagged AND that scope has a live turn,
            // surface as a Status trace; otherwise just log it.
            if let Some(active) = active {
                if active.cancel_requested {
                    return Ok(());
                }
                append_trace(
                    client,
                    &active.run_id,
                    TraceKind::Status,
                    json!({ "status": status }),
                )
                .await?;
            } else {
                tracing::debug!(actor = %actor_id, %status, "adapter status (no active turn)");
            }
        }
        AdapterEvent::Finished {
            scope,
            success,
            summary,
            usage,
        } => {
            let Some(active) = active else {
                tracing::warn!(
                    actor = %actor_id,
                    ?scope,
                    "Finished event without matching active turn; dropping"
                );
                return Ok(());
            };
            if active.cancel_requested || turn_no_reply_requested(&active) {
                let _ = state.take_text(&active.id);
            } else if agent_text_auto_publish_enabled() {
                if let Some(text) = state.take_text(&active.id) {
                    if let Some(text) = visible_agent_text_for_turn(&active, &text) {
                        let meta = build_turn_meta(state, &active, usage.as_ref(), &text);
                        flush_text(client, actor_id, &active, text, Some(meta)).await?;
                    }
                }
            } else {
                let _ = state.take_text(&active.id);
            }
            if !success && agent_text_auto_publish_enabled() {
                if let Some(text) = failed_turn_text(&summary) {
                    let meta = build_turn_meta(state, &active, usage.as_ref(), &text);
                    flush_text(client, actor_id, &active, text, Some(meta)).await?;
                }
            }
            let run_status = if active.cancel_requested {
                RunStatus::Canceled
            } else if success {
                RunStatus::Completed
            } else {
                RunStatus::Failed
            };
            if let Err(e) = close_run(client, &active.run_id, run_status).await {
                tracing::warn!(
                    actor = %actor_id,
                    run = %active.run_id,
                    scope = %active.scope.id,
                    %e,
                    "run.close RPC failed; clearing slot anyway so the queue can drain"
                );
            }
            if let Err(e) =
                record_delivery_seen_by_id(client, actor_id, &active.trigger_source_id).await
            {
                tracing::warn!(
                    actor = %actor_id,
                    event = %active.trigger_source_id,
                    %e,
                    "failed to record delivery ack after adapter finished"
                );
            }
            // Drop the active slot for this scope and pick up the next queued
            // trigger (if any). Clear unconditionally — if close_turn failed
            // server-side we still need to free the slot, otherwise the queue
            // is stranded forever.
            let scope_id = scope
                .map(|s| s.id)
                .unwrap_or_else(|| active.scope.id.clone());
            let next_trigger = state.clear_turn(&scope_id);
            if let Some(next) = next_trigger {
                match dispatch_trigger(client, state, adapter, next).await {
                    Ok(_) => {}
                    Err(e) => eprintln!("[{actor_id}] failed to dispatch queued trigger: {e}"),
                }
            }
        }
        AdapterEvent::Error { scope: _, message } => {
            if let Some(active) = active {
                append_trace(
                    client,
                    &active.run_id,
                    TraceKind::Error,
                    json!({ "message": message }),
                )
                .await?;
            } else {
                eprintln!("[{actor_id}] adapter error (agent-wide): {message}");
            }
        }
    }
    Ok(())
}

fn failed_turn_text(summary: &str) -> Option<String> {
    let summary = summary.trim();
    if summary.is_empty() {
        None
    } else {
        Some(format!("Agent run failed:\n\n{summary}"))
    }
}

fn parent_message_id_for_reply_target(trigger_source_id: &str, target: &str) -> Option<String> {
    let root_message_id = target.strip_prefix('#').and_then(|raw| raw.split_once(':'));
    if root_message_id.is_some_and(|(_, root_message_id)| root_message_id == trigger_source_id) {
        None
    } else {
        Some(trigger_source_id.to_string())
    }
}

fn build_turn_meta(
    state: &WorkerState,
    active: &ActiveTurn,
    provider_usage: Option<&TokenUsage>,
    output_text: &str,
) -> Meta {
    let increment = provider_usage
        .cloned()
        .map(usage::normalized_usage)
        .unwrap_or_else(|| {
            usage::estimated_usage(active.prompt_stats.approx_token_count, output_text)
        });
    let cumulative = state.accumulate_usage(&active.scope.id, &increment);
    let usage_meta = TokenUsageMeta {
        increment,
        cumulative,
    };

    let mut meta = build_turn_base_meta(active);
    meta.insert(
        "token_usage".into(),
        serde_json::to_value(&usage_meta).unwrap_or(Value::Null),
    );
    meta
}

fn build_turn_base_meta(active: &ActiveTurn) -> Meta {
    let mut meta = Meta::new();
    meta.insert(
        "trigger_source_id".into(),
        json!(active.trigger_source_id.clone()),
    );
    meta.insert("run_id".into(), json!(active.run_id.clone()));
    meta.insert(
        "prompt_stats".into(),
        serde_json::to_value(&active.prompt_stats).unwrap_or(Value::Null),
    );
    meta.insert(
        "prompt_breakdown".into(),
        serde_json::to_value(&active.prompt_breakdown).unwrap_or(Value::Null),
    );
    meta
}

fn non_empty_agent_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn agent_text_auto_publish_enabled() -> bool {
    agent_text_auto_publish_enabled_from_env(std::env::var("LOOM_AGENT_AUTO_PUBLISH_FINAL").ok())
}

fn agent_text_auto_publish_enabled_from_env(value: Option<String>) -> bool {
    value
        .as_deref()
        .map(|value| matches!(value, "1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON"))
        .unwrap_or(false)
}

fn visible_agent_text_for_turn(active: &ActiveTurn, text: &str) -> Option<String> {
    let text = non_empty_agent_text(text)?;
    if turn_no_reply_requested(active) {
        None
    } else {
        Some(text)
    }
}

fn turn_no_reply_requested(active: &ActiveTurn) -> bool {
    active.no_reply_requested
        || active
            .no_reply_file
            .as_ref()
            .is_some_and(|path| path.is_file())
}

async fn append_trace(
    client: &Arc<Client>,
    run_id: &str,
    kind: TraceKind,
    payload: Value,
) -> Result<()> {
    let _: RunAppendResult = client
        .call(
            method::RUN_APPEND,
            json!({
                "runId": run_id,
                "status": "running",
                "frameKind": trace_frame_kind(kind),
                "payload": payload,
            }),
        )
        .await
        .with_context(|| format!("run.append run={run_id}"))?;
    Ok(())
}

fn trace_frame_kind(kind: TraceKind) -> &'static str {
    match kind {
        TraceKind::ToolStart => "trace.tool.start",
        TraceKind::ToolUpdate => "trace.tool.update",
        TraceKind::ToolEnd => "trace.tool.end",
        TraceKind::TextDelta => "trace.text.delta",
        TraceKind::Status => "trace.status",
        TraceKind::Error => "trace.error",
    }
}

async fn mark_assignment_running_if_needed(
    client: &Arc<Client>,
    state: &WorkerState,
    trigger: &AgentTrigger,
) {
    let Some(assignment_id) = assignment_id_for_start(trigger) else {
        return;
    };
    let result: Result<TaskAssignmentUpdateResult> = client
        .call(
            method::TASK_ASSIGNMENT_UPDATE,
            json!({
                "assignmentId": assignment_id,
                "status": TaskAssignmentStatus::Running,
            }),
        )
        .await
        .with_context(|| format!("task/assignment.update assignment={assignment_id}"));
    if let Err(e) = result {
        tracing::warn!(
            actor = %state.actor_id,
            assignment = %assignment_id,
            %e,
            "failed to mark assignment running"
        );
    }
}

async fn append_run_started_ack(
    client: &Arc<Client>,
    state: &WorkerState,
    active: &ActiveTurn,
    trigger: &AgentTrigger,
) {
    let metadata = {
        let mut metadata = Meta::default();
        metadata.insert("kind".into(), json!("run.started_ack"));
        metadata.insert("triggerSourceId".into(), json!(trigger.id()));
        metadata
    };
    let result = if let Some(target) = active.reply_target.as_deref() {
        let parent_message_id = parent_message_id_for_reply_target(trigger.id(), target);
        send_agent_message(
            client,
            target,
            "已收到，正在处理。".into(),
            parent_message_id,
            Some(trigger.actor_id().to_string()),
            MessageIntent::StatusUpdate,
            DeliveryPolicy::NotifyOnly,
            metadata,
        )
        .await
        .map(|_| ())
    } else {
        send_scope_message(
            client,
            &active.scope,
            "已收到，正在处理。".into(),
            trigger.is_message().then(|| trigger.id().to_string()),
            Some(trigger.actor_id().to_string()),
            MessageIntent::StatusUpdate,
            DeliveryPolicy::NotifyOnly,
            metadata,
        )
        .await
        .map(|_| ())
    };
    if let Err(e) = result {
        tracing::warn!(
            actor = %state.actor_id,
            run = %active.run_id,
            scope = %active.scope.id,
            %e,
            "failed to append run started acknowledgement"
        );
    }
}

fn run_started_ack_enabled() -> bool {
    std::env::var("LOOM_AGENT_RUN_STARTED_ACK")
        .ok()
        .map(|value| {
            matches!(
                value.as_str(),
                "1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON"
            )
        })
        .unwrap_or(false)
}

fn assignment_id_for_start(trigger: &AgentTrigger) -> Option<&str> {
    if trigger.meta_value("assignmentStatus").is_some() {
        return None;
    }
    trigger
        .meta_value("assignmentId")
        .and_then(|value| value.as_str())
        .filter(|id| !id.trim().is_empty())
}

async fn send_agent_message(
    client: &Arc<Client>,
    target: &str,
    body: String,
    parent_message_id: Option<String>,
    audience_actor_id: Option<String>,
    intent: MessageIntent,
    delivery_policy: DeliveryPolicy,
    metadata: Meta,
) -> Result<MessageSendResult> {
    let audience = audience_actor_id
        .map(|actor_id| {
            vec![json!({
                "kind": "actor",
                "id": actor_id,
            })]
        })
        .unwrap_or_default();
    let mut input = json!({
        "target": target,
        "body": body,
        "audience": audience,
        "intent": intent,
        "deliveryPolicy": delivery_policy,
        "metadata": metadata,
    });
    if let Some(parent_message_id) = parent_message_id {
        input["parentMessageId"] = json!(parent_message_id);
    }
    client
        .call(method::MESSAGE_SEND, input)
        .await
        .with_context(|| format!("message.send target={target}"))
}

async fn send_scope_message(
    client: &Arc<Client>,
    scope: &ScopeRef,
    body: String,
    parent_message_id: Option<String>,
    audience_actor_id: Option<String>,
    intent: MessageIntent,
    delivery_policy: DeliveryPolicy,
    metadata: Meta,
) -> Result<MessageSendResult> {
    let target = message_target_for_scope(client, scope).await?;
    send_agent_message(
        client,
        &target,
        body,
        parent_message_id,
        audience_actor_id,
        intent,
        delivery_policy,
        metadata,
    )
    .await
    .with_context(|| format!("message.send scope={}", scope.id))
}

async fn message_target_for_scope(client: &Arc<Client>, scope: &ScopeRef) -> Result<String> {
    match scope.kind {
        ScopeKind::Channel => Ok(format!("#{}", scope.id)),
        ScopeKind::Thread => {
            let res: ThreadListResult = client
                .call(method::THREAD_LIST, json!({ "archived": false }))
                .await
                .context("thread/list")?;
            let thread = res
                .threads
                .into_iter()
                .find(|thread| thread.id == scope.id)
                .ok_or_else(|| anyhow!("thread {} not found", scope.id))?;
            Ok(format!("#{}:{}", thread.channel_id, thread.root_message_id))
        }
    }
}

async fn send_action_request_message(
    client: &Arc<Client>,
    scope: &ScopeRef,
    target_actor: String,
    payload: Value,
    parent_message_id: Option<String>,
    run_id: Option<String>,
) -> Result<MessageSendResult> {
    let mut metadata = action_request_metadata(payload)?;
    if let Some(run_id) = run_id.filter(|value| !value.trim().is_empty()) {
        metadata.insert("runId".into(), json!(run_id));
    }
    let body = format_action_request_body(&metadata);
    send_scope_message(
        client,
        scope,
        body,
        parent_message_id,
        Some(target_actor),
        MessageIntent::RequestAction,
        DeliveryPolicy::WakeAgent,
        metadata,
    )
    .await
}

fn action_request_metadata(payload: Value) -> Result<Meta> {
    let mut metadata = Meta::default();
    metadata.insert("kind".into(), json!("action.request"));
    match payload {
        Value::Object(map) => {
            for (key, value) in map {
                metadata.insert(key, value);
            }
        }
        other => {
            metadata.insert("payload".into(), other);
        }
    }
    Ok(metadata)
}

fn format_action_request_body(metadata: &Meta) -> String {
    let title = metadata
        .get("title")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("Action requested");
    let description = metadata
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let mut body = format!("Action requested: {title}");
    if !description.is_empty() {
        body.push_str("\n\n");
        body.push_str(description);
    }
    if let Some(choices) = metadata.get("choices").and_then(Value::as_array) {
        for choice in choices {
            let id = choice.get("id").and_then(Value::as_str).unwrap_or("");
            let label = choice.get("label").and_then(Value::as_str).unwrap_or("");
            if !id.is_empty() || !label.is_empty() {
                body.push_str(&format!("\n- {id}: {label}"));
            }
        }
    }
    body
}

async fn flush_text(
    client: &Arc<Client>,
    actor_id: &str,
    active: &ActiveTurn,
    text: String,
    meta: Option<Meta>,
) -> Result<()> {
    if active.trigger_is_message {
        if let Some(target) = active.reply_target.as_deref() {
            let parent_message_id =
                parent_message_id_for_reply_target(&active.trigger_source_id, target);
            return send_agent_message(
                client,
                target,
                text,
                parent_message_id,
                Some(active.trigger_actor.clone()),
                MessageIntent::Chat,
                DeliveryPolicy::NotifyOnly,
                meta.unwrap_or_default(),
            )
            .await
            .map(|_| ())
            .map_err(|e| {
                tracing::warn!(
                    actor = %actor_id,
                    turn = %active.id,
                    scope = %active.scope.id,
                    %e,
                    "message.send failed"
                );
                e
            });
        }
    }
    send_scope_message(
        client,
        &active.scope,
        text,
        active
            .trigger_is_message
            .then(|| active.trigger_source_id.clone()),
        Some(active.trigger_actor.clone()),
        MessageIntent::Chat,
        DeliveryPolicy::NotifyOnly,
        meta.unwrap_or_default(),
    )
    .await
    .map(|_| ())
    .map_err(|e| {
        tracing::warn!(
            actor = %actor_id,
            turn = %active.id,
            scope = %active.scope.id,
            %e,
            "content.add failed"
        );
        e
    })
}

async fn close_run(client: &Arc<Client>, run_id: &str, status: RunStatus) -> Result<()> {
    let _: RunCloseResult = client
        .call(
            method::RUN_CLOSE,
            json!({ "runId": run_id, "status": status }),
        )
        .await
        .with_context(|| format!("run.close run={run_id}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proto::methods::{
        AgentBundleSpec, AgentModelChoice, AgentModelSpec, AgentProviderRef, TriggerSpec,
    };
    use proto::types::{Actor, ActorKind, MessageKind, Ref, Relation};

    fn sample_spec(bundle: Option<AgentBundleSpec>) -> AgentSpec {
        AgentSpec {
            actor: Actor {
                id: "actor_demo".into(),
                kind: ActorKind::Agent,
                display_name: "Demo".into(),
                capabilities: None,
                _meta: None,
            },
            provider_ref: AgentProviderRef {
                id: "test".into(),
                mode: Some("print".into()),
                model: None,
                reasoning_effort: None,
            },
            autostart: false,
            models: None,
            bundle,
            memory: None,
            announcement: None,
            trigger: None,
            prompt_template: None,
        }
    }

    fn sample_message(
        id: &str,
        scope: ScopeRef,
        target: &str,
        parent_message_id: Option<&str>,
        thread_root_message_id: Option<&str>,
    ) -> Message {
        Message {
            id: id.into(),
            scope,
            target: target.into(),
            author_actor_id: "actor_human".into(),
            created_at: Utc::now(),
            kind: MessageKind::Human,
            body: "ping".into(),
            mentions: Vec::new(),
            audience: Vec::new(),
            intent: MessageIntent::Chat,
            delivery_policy: DeliveryPolicy::WakeAgent,
            parent_message_id: parent_message_id.map(ToString::to_string),
            thread_root_message_id: thread_root_message_id.map(ToString::to_string),
            task_id: None,
            attachments: Vec::new(),
            reactions: Vec::new(),
            metadata: Meta::default(),
        }
    }

    fn temp_path(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "loom-agent-serve-tests-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock drift")
                .as_nanos()
        ));
        path
    }

    fn empty_prompt_stats() -> PromptStats {
        PromptStats {
            char_count: 0,
            byte_count: 0,
            approx_token_count: 0,
        }
    }

    fn empty_prompt_breakdown() -> PromptBreakdown {
        PromptBreakdown {
            sections: Vec::new(),
        }
    }

    #[test]
    fn bundle_paths_resolve_custom_root_and_current() {
        let root = temp_path("bundle-paths");
        let paths = AgentPaths::new(&root, "actor_demo");
        let spec = sample_spec(Some(AgentBundleSpec {
            root: "{agent.root}/runtime/bundles".into(),
            current: "{agent.root}/runtime/live".into(),
            ..Default::default()
        }));

        let bundle_paths = paths.bundle_paths(&spec);

        assert_eq!(
            bundle_paths.root,
            paths.root.join("runtime").join("bundles")
        );
        assert_eq!(
            bundle_paths.current,
            paths.root.join("runtime").join("live")
        );

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn ensure_bundle_without_bundle_replaces_stale_current_symlink() {
        let root = temp_path("bundle-cleanup");
        let paths = AgentPaths::new(&root, "actor_demo");
        std::fs::create_dir_all(&paths.bundle_root).expect("create bundle root");
        std::fs::create_dir_all(paths.root.join("workspace")).expect("create workspace");
        std::fs::create_dir_all(&paths.profile).expect("create profile");
        std::fs::create_dir_all(paths.root.join("logs")).expect("create logs");
        let old_target = paths.root.join("old-bundle");
        std::fs::create_dir_all(&old_target).expect("create old target");
        symlink_path(&old_target, &paths.bundle_current).expect("seed stale symlink");

        let spec = sample_spec(None);
        let bundle_paths = paths.bundle_paths(&spec);
        ensure_bundle("actor_demo", &spec, &bundle_paths, &paths).expect("ensure bundle");

        let meta =
            std::fs::symlink_metadata(&paths.bundle_current).expect("bundle current metadata");
        assert!(meta.is_dir());
        assert!(!meta.file_type().is_symlink());

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn ensure_bundle_rejects_current_inside_profile() {
        let root = temp_path("bundle-current-profile");
        let paths = AgentPaths::new(&root, "actor_demo");
        std::fs::create_dir_all(paths.root.join("workspace")).expect("create workspace");
        std::fs::create_dir_all(&paths.profile).expect("create profile");
        std::fs::create_dir_all(paths.root.join("logs")).expect("create logs");
        let spec = sample_spec(Some(AgentBundleSpec {
            current: "{agent.profile}/live".into(),
            ..Default::default()
        }));

        let bundle_paths = paths.bundle_paths(&spec);
        let err =
            ensure_bundle("actor_demo", &spec, &bundle_paths, &paths).expect_err("must reject");

        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn bundle_paths_derive_version_from_source_basename() {
        let root = temp_path("bundle-version");
        let paths = AgentPaths::new(&root, "actor_demo");
        let spec = sample_spec(Some(AgentBundleSpec {
            source: "{agent.root}/bundles/demo-bundle".into(),
            ..Default::default()
        }));

        let bundle_paths = paths.bundle_paths(&spec);

        assert_eq!(bundle_paths.version, "demo-bundle");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn ensure_scope_links_scope_specific_skills_into_workspace() {
        let root = temp_path("scope-skills-link");
        let paths = AgentPaths::new(&root, "actor_demo");
        let scope = ScopeRef {
            kind: ScopeKind::Thread,
            id: "thread_demo".into(),
        };

        let scope_paths = paths
            .ensure_scope("actor_demo", "chan_demo", &scope)
            .expect("ensure scope");

        assert_eq!(
            std::fs::read_link(scope_paths.workspace.join("skills")).expect("skills link"),
            root.join("workspaces")
                .join("thread")
                .join("thread_demo")
                .join("skills")
        );
        assert!(scope_paths.skills.exists());

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn scope_env_includes_current_scope_identity() {
        let root = temp_path("scope-env");
        let paths = AgentPaths::new(&root, "actor_demo");
        let scope = ScopeRef {
            kind: ScopeKind::Channel,
            id: "chan_demo".into(),
        };

        let active = ActiveTurn {
            id: "turn_demo".into(),
            run_id: "run_demo".into(),
            scope: scope.clone(),
            trigger_source_id: "msg_trigger".into(),
            trigger_is_message: true,
            reply_target: Some("#chan_demo".into()),
            prompt_stats: empty_prompt_stats(),
            prompt_breakdown: empty_prompt_breakdown(),
            trigger_actor: "human_alice".into(),
            no_reply_file: Some(root.join("no-reply.json")),
            no_reply_requested: false,
            cancel_requested: false,
        };
        let env = paths.scope_env(
            "actor_demo",
            "chan_demo",
            &scope,
            "ws://127.0.0.1:7891/rpc",
            Some(&active),
        );

        assert_eq!(
            env.get("LOOM_SCOPE_ID").map(String::as_str),
            Some("chan_demo")
        );
        assert_eq!(
            env.get("LOOM_SCOPE_ID").map(String::as_str),
            Some("chan_demo")
        );
        assert_eq!(
            env.get("LOOM_SCOPE_KIND").map(String::as_str),
            Some("channel")
        );
        assert_eq!(
            env.get("AGENTX_CHANNEL_ID").map(String::as_str),
            Some("chan_demo")
        );
        assert_eq!(
            env.get("LOOM_CHANNEL_ID").map(String::as_str),
            Some("chan_demo")
        );
        assert_eq!(env.get("LOOM_RUN_ID").map(String::as_str), Some("run_demo"));
        assert_eq!(
            env.get("LOOM_REPLY_TARGET").map(String::as_str),
            Some("#chan_demo")
        );
        let expected_no_reply_file = root.join("no-reply.json").display().to_string();
        assert_eq!(
            env.get(LOOM_NO_REPLY_FILE_ENV).map(String::as_str),
            Some(expected_no_reply_file.as_str())
        );
        assert_eq!(
            env.get("LOOM_TRIGGER_MESSAGE_ID").map(String::as_str),
            Some("msg_trigger")
        );
        assert_eq!(
            env.get("LOOM_TRIGGER_MESSAGE_ID").map(String::as_str),
            Some("msg_trigger")
        );
        assert_eq!(
            env.get("LOOM_TRIGGER_ACTOR").map(String::as_str),
            Some("human_alice")
        );
        assert!(env.get("LOOM_CURRENT_TIME").is_some());
        assert!(env.get("LOOM_CURRENT_TIME_UTC").is_some());
        assert!(env.get("LOOM_LOCAL_UTC_OFFSET").is_some());
        assert!(env.get("LOOM_LOCAL_TIMEZONE").is_some());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn inject_loom_cli_env_sets_absolute_cli_and_prepends_path() {
        let mut env = BTreeMap::new();
        env.insert("PATH".into(), "/usr/bin:/bin".into());
        let loom = Path::new("/opt/loom/bin").join("loom");

        inject_loom_cli_env(&mut env, Some(&loom));

        assert_eq!(
            env.get(LOOM_CLI_ENV).map(String::as_str),
            Some("/opt/loom/bin/loom")
        );
        let paths = std::env::split_paths(env.get("PATH").expect("PATH")).collect::<Vec<_>>();
        assert_eq!(paths.first(), Some(&PathBuf::from("/opt/loom/bin")));
        assert!(paths.contains(&PathBuf::from("/usr/bin")));
        assert!(paths.contains(&PathBuf::from("/bin")));
    }

    #[test]
    fn top_level_channel_message_replies_target_thread() {
        let message = sample_message(
            "msg_root",
            ScopeRef {
                kind: ScopeKind::Channel,
                id: "chan_demo".into(),
            },
            "#chan_demo",
            None,
            None,
        );

        assert_eq!(reply_target_for_message(&message), "#chan_demo:msg_root");
    }

    #[test]
    fn thread_message_preserves_thread_target() {
        let message = sample_message(
            "msg_reply",
            ScopeRef {
                kind: ScopeKind::Thread,
                id: "thread_demo".into(),
            },
            "#chan_demo:msg_root",
            Some("msg_root"),
            Some("msg_root"),
        );

        assert_eq!(reply_target_for_message(&message), "#chan_demo:msg_root");
    }

    #[test]
    fn thread_root_reply_target_omits_invalid_channel_parent() {
        assert_eq!(
            parent_message_id_for_reply_target("msg_root", "#chan_demo:msg_root"),
            None
        );
        assert_eq!(
            parent_message_id_for_reply_target("msg_reply", "#chan_demo:msg_root"),
            Some("msg_reply".into())
        );
        assert_eq!(
            parent_message_id_for_reply_target("msg_channel", "#chan_demo"),
            Some("msg_channel".into())
        );
    }

    #[test]
    fn all_wake_message_is_deliverable_to_agent_worker() {
        let mut message = sample_message(
            "msg_all",
            ScopeRef {
                kind: ScopeKind::Channel,
                id: "chan_demo".into(),
            },
            "#chan_demo",
            None,
            None,
        );
        message.audience = vec![proto::types::AudienceRef {
            kind: AudienceKind::All,
            id: "all".into(),
            display: Some("@all".into()),
        }];
        message.delivery_policy = DeliveryPolicy::WakeAgent;

        assert!(is_message_for_us(&message, "actor_agent_echo"));

        message.delivery_policy = DeliveryPolicy::NotifyOnly;
        assert!(!is_message_for_us(&message, "actor_agent_echo"));
    }

    #[test]
    fn actor_notify_only_message_does_not_wake_agent_worker() {
        let mut message = sample_message(
            "msg_notify",
            ScopeRef {
                kind: ScopeKind::Thread,
                id: "thread_demo".into(),
            },
            "#chan_demo:msg_root",
            Some("msg_parent"),
            Some("msg_root"),
        );
        message.author_actor_id = "actor_agent_sender".into();
        message.audience = vec![proto::types::AudienceRef {
            kind: AudienceKind::Actor,
            id: "actor_agent_echo".into(),
            display: None,
        }];
        message.delivery_policy = DeliveryPolicy::NotifyOnly;

        assert!(!is_message_for_us(&message, "actor_agent_echo"));

        message.delivery_policy = DeliveryPolicy::WakeAgent;
        assert!(is_message_for_us(&message, "actor_agent_echo"));
    }

    #[test]
    fn thread_attention_inbox_delivery_wakes_agent_worker() {
        let mut message = sample_message(
            "msg_thread_attention",
            ScopeRef {
                kind: ScopeKind::Thread,
                id: "thread_demo".into(),
            },
            "#chan_demo:msg_root",
            Some("msg_parent"),
            Some("msg_root"),
        );
        message.author_actor_id = "actor_agent_g".into();
        message.kind = MessageKind::Agent;
        message.body = "CLAIM C FILL C=7 BOARD=4127".into();
        message.audience = vec![proto::types::AudienceRef {
            kind: AudienceKind::Actor,
            id: "actor_human_requester".into(),
            display: None,
        }];
        message.delivery_policy = DeliveryPolicy::NotifyOnly;

        assert!(!is_message_for_us(&message, "actor_agent_echo"));
        assert!(is_inbox_message_for_us(&message, "actor_agent_echo"));
    }

    #[test]
    fn explicit_notify_only_actor_inbox_delivery_still_does_not_wake_agent_worker() {
        let mut message = sample_message(
            "msg_notify",
            ScopeRef {
                kind: ScopeKind::Thread,
                id: "thread_demo".into(),
            },
            "#chan_demo:msg_root",
            Some("msg_parent"),
            Some("msg_root"),
        );
        message.author_actor_id = "actor_agent_sender".into();
        message.audience = vec![proto::types::AudienceRef {
            kind: AudienceKind::Actor,
            id: "actor_agent_echo".into(),
            display: None,
        }];
        message.delivery_policy = DeliveryPolicy::NotifyOnly;

        assert!(!is_inbox_message_for_us(&message, "actor_agent_echo"));
    }

    #[test]
    fn explicit_noop_text_does_not_override_delivery_routing() {
        let mut message = sample_message(
            "msg_noop",
            ScopeRef {
                kind: ScopeKind::Thread,
                id: "thread_demo".into(),
            },
            "#chan_demo:msg_root",
            Some("msg_parent"),
            Some("msg_root"),
        );
        message.author_actor_id = "actor_agent_sender".into();
        message.kind = MessageKind::Agent;
        message.body = "No action needed - message is routed to boyd, not me.".into();
        message.audience = vec![proto::types::AudienceRef {
            kind: AudienceKind::Actor,
            id: "actor_agent_echo".into(),
            display: None,
        }];
        message.delivery_policy = DeliveryPolicy::WakeAgent;

        assert!(is_message_for_us(&message, "actor_agent_echo"));
    }

    #[test]
    fn explicit_terminal_text_does_not_override_delivery_routing() {
        let mut message = sample_message(
            "msg_terminal",
            ScopeRef {
                kind: ScopeKind::Thread,
                id: "thread_demo".into(),
            },
            "#chan_demo:msg_root",
            Some("msg_parent"),
            Some("msg_root"),
        );
        message.author_actor_id = "actor_agent_sender".into();
        message.kind = MessageKind::Agent;
        message.body =
            "Acknowledged. Task complete: A=4, B=12, C=7 -> BOARD=4127. No further action needed."
                .into();
        message.audience = vec![proto::types::AudienceRef {
            kind: AudienceKind::Actor,
            id: "actor_agent_echo".into(),
            display: None,
        }];
        message.delivery_policy = DeliveryPolicy::WakeAgent;

        assert!(is_message_for_us(&message, "actor_agent_echo"));
    }

    #[test]
    fn terminal_agent_message_with_work_request_can_wake_agent_worker() {
        let mut message = sample_message(
            "msg_closeout",
            ScopeRef {
                kind: ScopeKind::Thread,
                id: "thread_demo".into(),
            },
            "#chan_demo:msg_root",
            Some("msg_parent"),
            Some("msg_root"),
        );
        message.author_actor_id = "actor_agent_sender".into();
        message.kind = MessageKind::Agent;
        message.body =
            "Task complete: A=4, B=12, C=7. Please mark the task complete with this result.".into();
        message.audience = vec![proto::types::AudienceRef {
            kind: AudienceKind::Actor,
            id: "actor_agent_echo".into(),
            display: None,
        }];
        message.delivery_policy = DeliveryPolicy::WakeAgent;

        assert!(is_message_for_us(&message, "actor_agent_echo"));
    }

    #[test]
    fn dm_notify_only_message_still_wakes_target_agent() {
        let mut message = sample_message(
            "msg_dm",
            ScopeRef {
                kind: ScopeKind::Channel,
                id: "chan_demo".into(),
            },
            "dm:@actor_agent_echo",
            None,
            None,
        );
        message.author_actor_id = "actor_agent_sender".into();
        message.delivery_policy = DeliveryPolicy::NotifyOnly;

        assert!(is_message_for_us(&message, "actor_agent_echo"));
    }

    #[test]
    fn visible_agent_text_does_not_apply_content_heuristics() {
        let active = ActiveTurn {
            id: "turn_demo".into(),
            run_id: "run_demo".into(),
            scope: ScopeRef {
                kind: ScopeKind::Thread,
                id: "thread_demo".into(),
            },
            trigger_source_id: "msg_trigger".into(),
            trigger_is_message: true,
            reply_target: Some("#chan_demo:msg_root".into()),
            prompt_stats: empty_prompt_stats(),
            prompt_breakdown: empty_prompt_breakdown(),
            trigger_actor: "actor_agent_qzz".into(),
            no_reply_file: None,
            no_reply_requested: false,
            cancel_requested: false,
        };

        assert_eq!(
            visible_agent_text_for_turn(
                &active,
                "No action needed - message is routed to boyd, not me."
            )
            .as_deref(),
            Some("No action needed - message is routed to boyd, not me.")
        );
        assert_eq!(
            visible_agent_text_for_turn(&active, "CLAIM C\nFILL C=7\nBOARD=4127").as_deref(),
            Some("CLAIM C\nFILL C=7\nBOARD=4127")
        );
    }

    #[test]
    fn agent_text_auto_publish_is_legacy_opt_in() {
        assert!(!agent_text_auto_publish_enabled_from_env(None));
        assert!(!agent_text_auto_publish_enabled_from_env(Some(
            "false".into()
        )));
        assert!(agent_text_auto_publish_enabled_from_env(Some("1".into())));
        assert!(agent_text_auto_publish_enabled_from_env(Some(
            "true".into()
        )));
    }

    #[test]
    fn run_ignore_marker_suppresses_visible_output() {
        let root = temp_path("no-reply-marker");
        std::fs::create_dir_all(&root).expect("create marker dir");
        let marker = root.join("run_1.no-reply.json");
        std::fs::write(&marker, "{}").expect("write marker");
        let active = ActiveTurn {
            id: "turn_demo".into(),
            run_id: "run_demo".into(),
            scope: ScopeRef {
                kind: ScopeKind::Thread,
                id: "thread_demo".into(),
            },
            trigger_source_id: "msg_trigger".into(),
            trigger_is_message: true,
            reply_target: Some("#chan_demo:msg_root".into()),
            prompt_stats: empty_prompt_stats(),
            prompt_breakdown: empty_prompt_breakdown(),
            trigger_actor: "actor_human_boyd".into(),
            no_reply_file: Some(marker),
            no_reply_requested: false,
            cancel_requested: false,
        };

        assert_eq!(
            visible_agent_text_for_turn(&active, "Standing by quietly."),
            None
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn recent_conversation_context_names_prior_agent_replies() {
        let trigger = sample_message(
            "msg_root",
            ScopeRef {
                kind: ScopeKind::Channel,
                id: "chan_demo".into(),
            },
            "#chan_demo",
            None,
            None,
        );
        let mut reply = sample_message(
            "msg_echo",
            ScopeRef {
                kind: ScopeKind::Thread,
                id: "thread_demo".into(),
            },
            "#chan_demo:msg_root",
            None,
            Some("msg_root"),
        );
        reply.author_actor_id = "actor_agent_echo".into();
        reply.kind = MessageKind::Agent;
        reply.body = "1".into();
        let mut names = HashMap::new();
        names.insert("actor_agent_echo".into(), "Echo".into());

        let context =
            format_recent_conversation_context(&[trigger.clone(), reply], &trigger, &names);

        assert!(context.contains("Recent Loom conversation"));
        assert!(context.contains("Echo (@actor_agent_echo): 1"));
        assert!(!context.contains("@actor_human"));
    }

    #[test]
    fn trigger_task_context_includes_status_and_owner() {
        let mut message = sample_message(
            "msg_task",
            ScopeRef {
                kind: ScopeKind::Thread,
                id: "thread_demo".into(),
            },
            "#chan_demo:msg_root",
            Some("msg_root"),
            Some("msg_root"),
        );
        message.metadata.insert("taskId".into(), json!("task_123"));
        message.metadata.insert("taskNumber".into(), json!(7));
        message
            .metadata
            .insert("taskStatus".into(), json!("claimed"));
        message
            .metadata
            .insert("taskOwnerActorId".into(), json!("actor_agent_echo"));

        let context = trigger_task_context(&AgentTrigger::Message(message)).expect("task context");

        assert!(context.contains("Task id: task_123"));
        assert!(context.contains("Task number: #7"));
        assert!(context.contains("Task status: claimed"));
        assert!(context.contains("Task owner: actor_agent_echo"));
    }

    #[test]
    fn local_time_manifest_tells_agents_to_convert_utc_protocol_timestamps() {
        let manifest = local_time_manifest();
        assert!(manifest.contains("Current local time:"));
        assert!(manifest.contains("Current UTC time:"));
        assert!(manifest.contains("RFC3339 UTC"));
        assert!(manifest.contains("GUI/chat timestamps"));
    }

    #[test]
    fn actor_context_manifest_names_local_actor_with_display_and_id() {
        let manifest = actor_context_manifest("actor_agent_g_1234", "G仔");

        assert!(manifest.contains("You are G仔 (@actor_agent_g_1234)."));
        assert!(manifest.contains("Other @actors"));
    }

    #[test]
    fn trigger_prompt_restores_routing_semantics_and_display_names() {
        let mut actor_names = HashMap::new();
        actor_names.insert("actor_human_xingchu".into(), "星楚".into());
        actor_names.insert("actor_agent_g_1234".into(), "G仔".into());
        actor_names.insert("actor_agent_emma_142b6f2d".into(), "Emma".into());
        let trigger = Event {
            id: "evt_1".into(),
            kind: "content.add".into(),
            actor_id: "actor_human_xingchu".into(),
            scope: ScopeRef {
                kind: ScopeKind::Thread,
                id: "thread_story".into(),
            },
            turn_id: None,
            seq: 1,
            occurred_at: Utc::now(),
            payload: json!({
                "contentType": "text/markdown",
                "text": "生成一个童话小说，然后发给@actor_agent_emma_142b6f2d读下"
            }),
            relations: vec![Relation {
                kind: RelationKind::DirectedTo,
                target: Ref {
                    kind: RefKind::Actor,
                    id: "actor_agent_g_1234".into(),
                    _meta: None,
                },
                _meta: None,
            }],
            _meta: None,
        };

        let prompt = render_trigger_prompt_with_names(
            "actor_agent_g_1234",
            "G仔",
            &AgentTrigger::Event(trigger),
            &actor_names,
        );

        assert!(prompt.contains("Your actor: G仔 (@actor_agent_g_1234)"));
        assert!(prompt.contains("From: 星楚 (@actor_human_xingchu)"));
        assert!(prompt.contains("Delivery: explicit route to you"));
        assert!(prompt.contains("route -> G仔 (@actor_agent_g_1234):"));
        assert!(prompt.contains("Emma (@actor_agent_emma_142b6f2d)"));
    }

    #[test]
    fn actor_inbox_thread_attention_prompt_is_not_rendered_as_other_actor_route() {
        let mut actor_names = HashMap::new();
        actor_names.insert("actor_human_boyd".into(), "boyd".into());
        actor_names.insert("actor_agent_echo".into(), "Echo".into());
        actor_names.insert("actor_agent_qzz".into(), "Qzz".into());
        let mut message = sample_message(
            "msg_claim_a",
            ScopeRef {
                kind: ScopeKind::Thread,
                id: "thread_demo".into(),
            },
            "#chan_demo:msg_root",
            Some("msg_root"),
            Some("msg_root"),
        );
        message.author_actor_id = "actor_agent_qzz".into();
        message.kind = MessageKind::Agent;
        message.body = "CLAIM A\nFILL A=4".into();
        message.audience = vec![proto::types::AudienceRef {
            kind: AudienceKind::Actor,
            id: "actor_human_boyd".into(),
            display: None,
        }];
        mark_actor_inbox_delivery(&mut message, "actor_agent_echo");

        let prompt = render_trigger_prompt_with_names(
            "actor_agent_echo",
            "Echo",
            &AgentTrigger::Message(message),
            &actor_names,
        );

        assert!(prompt.contains("Delivery: thread/task attention to you"));
        assert!(!prompt.contains("Delivery: explicit route to another actor"));
        assert!(prompt.contains("Qzz (@actor_agent_qzz) (visible route -> boyd"));
    }

    #[test]
    fn self_authored_directed_event_to_local_actor_is_deliverable() {
        let event = Event {
            id: "evt_self_route".into(),
            kind: "content.add".into(),
            actor_id: "actor_agent_emma".into(),
            scope: ScopeRef {
                kind: ScopeKind::Thread,
                id: "thread_task".into(),
            },
            turn_id: None,
            seq: 1,
            occurred_at: Utc::now(),
            payload: json!({ "text": "continue in the task thread" }),
            relations: vec![Relation {
                kind: RelationKind::DirectedTo,
                target: Ref {
                    kind: RefKind::Actor,
                    id: "actor_agent_emma".into(),
                    _meta: None,
                },
                _meta: None,
            }],
            _meta: None,
        };

        assert!(is_for_us(&event, "actor_agent_emma"));
        assert!(!is_for_us(&event, "actor_agent_q"));
    }

    #[test]
    fn self_authored_content_without_local_route_is_not_deliverable() {
        let own_plain_message = Event {
            id: "evt_self_plain".into(),
            kind: "content.add".into(),
            actor_id: "actor_agent_emma".into(),
            scope: ScopeRef {
                kind: ScopeKind::Thread,
                id: "thread_task".into(),
            },
            turn_id: None,
            seq: 1,
            occurred_at: Utc::now(),
            payload: json!({ "text": "status update" }),
            relations: Vec::new(),
            _meta: None,
        };
        let route_to_other = Event {
            id: "evt_route_other".into(),
            relations: vec![Relation {
                kind: RelationKind::DirectedTo,
                target: Ref {
                    kind: RefKind::Actor,
                    id: "actor_agent_q".into(),
                    _meta: None,
                },
                _meta: None,
            }],
            ..own_plain_message.clone()
        };

        assert!(!is_for_us(&own_plain_message, "actor_agent_emma"));
        assert!(!is_for_us(&route_to_other, "actor_agent_emma"));
    }

    #[test]
    fn assignment_start_detection_skips_return_message() {
        let mut trigger = Event {
            id: "evt_assignment".into(),
            kind: "content.add".into(),
            actor_id: "actor_agent_owner".into(),
            scope: ScopeRef {
                kind: ScopeKind::Thread,
                id: "thread_task".into(),
            },
            turn_id: None,
            seq: 1,
            occurred_at: Utc::now(),
            payload: json!({
                "text": "assignment",
                "_meta": {
                    "taskId": "task_1",
                    "assignmentId": "asgn_1",
                    "assignmentType": "review"
                }
            }),
            relations: vec![],
            _meta: None,
        };

        assert_eq!(
            assignment_id_for_start(&AgentTrigger::Event(trigger.clone())),
            Some("asgn_1")
        );

        trigger.payload["_meta"]["assignmentStatus"] = json!("completed");
        assert_eq!(assignment_id_for_start(&AgentTrigger::Event(trigger)), None);
    }

    #[test]
    fn command_transport_without_resume_tracks_session_capability() {
        let no_session = test_command_transport();
        assert!(command_transport_without_resume(&no_session));

        let mut resumable = test_command_transport();
        resumable.session = Some(proto::methods::CommandSession {
            id_source: None,
            first_run_capture: Some("stdout_json:.session_id".into()),
            resume_args: Some(vec!["--resume".into(), "{session_id}".into(), "-p".into()]),
        });
        assert!(!command_transport_without_resume(&resumable));

        let mut acp = test_command_transport();
        acp.kind = "acp_stdio".into();
        assert!(!command_transport_without_resume(&acp));
    }

    #[test]
    fn trigger_prefix_applies_to_every_turn() {
        let mut spec = sample_spec(None);
        spec.trigger = Some(TriggerSpec {
            trigger_prompt_prefix: "/router\n".into(),
            apply_on: TriggerPrefixApplyOn::EveryTurn,
        });
        let sections = vec![agent_runtime::PromptSection {
            name: "user_message",
            content: "hello".into(),
        }];

        assert_eq!(
            apply_trigger_prefix_to_prompt(
                &spec,
                prompt_telemetry("hello".into(), &sections),
                false,
                None,
            )
            .content,
            "/router\nhello"
        );
        assert_eq!(
            apply_trigger_prefix_to_prompt(
                &spec,
                prompt_telemetry("/router\nhello".into(), &sections),
                false,
                None,
            )
            .content,
            "/router\nhello"
        );
    }

    #[test]
    fn prompt_template_expands_runtime_vars_and_first_turn_prefix() {
        let mut vars = BTreeMap::new();
        vars.insert("actor.id".into(), "actor_router".into());
        vars.insert("scope.kind".into(), "channel".into());
        vars.insert("scope.id".into(), "chan_1".into());
        vars.insert("workspace.dir".into(), "/tmp/work".into());
        vars.insert("agent.skillBody".into(), "# Router skill".into());
        vars.insert("prompt.activeSkill".into(), "router".into());
        vars.insert("vars.mode".into(), "fast".into());
        let template = PromptTemplateSpec {
            active_skill: Some("router".into()),
            every_turn_prefix: vec![
                "[loom] {actor.id} {scope.kind}:{scope.id}".into(),
                "workspace_dir: {workspace.dir}".into(),
                "mode: {vars.mode}".into(),
            ],
            first_turn_prefix: vec![
                "skill: {prompt.activeSkill}".into(),
                "{agent.skillBody}".into(),
            ],
            every_turn_suffix: vec!["done".into()],
            vars: BTreeMap::new(),
        };

        let first = apply_prompt_template(Some(&template), &vars, true, "/router\nhi");
        assert!(first.contains("[loom] actor_router channel:chan_1"));
        assert!(first.contains("workspace_dir: /tmp/work"));
        assert!(first.contains("skill: router"));
        assert!(first.contains("# Router skill"));
        assert!(first.contains("/router\nhi"));
        assert!(first.ends_with("done"));

        let later = apply_prompt_template(Some(&template), &vars, false, "hi");
        assert!(!later.contains("# Router skill"));
        assert!(later.contains("hi"));
    }

    #[test]
    fn trigger_prefix_is_first_in_final_prompt() {
        let mut spec = sample_spec(None);
        spec.trigger = Some(TriggerSpec {
            trigger_prompt_prefix: "/router\n".into(),
            apply_on: TriggerPrefixApplyOn::EveryTurn,
        });
        let sections = vec![agent_runtime::PromptSection {
            name: "user_message",
            content: "=== User message ===\n[loom envelope]\nhello".into(),
        }];
        let prompt = prompt_telemetry(sections[0].content.clone(), &sections);

        let prompt = apply_trigger_prefix_to_prompt(&spec, prompt, false, None);

        assert!(prompt.content.starts_with("/router\n=== User message ==="));
        assert_eq!(prompt.breakdown.sections[0].key, "trigger_prefix");
    }

    #[test]
    fn per_trigger_prefix_overrides_actor_default_and_is_first() {
        let mut spec = sample_spec(None);
        spec.trigger = Some(TriggerSpec {
            trigger_prompt_prefix: "/router\n".into(),
            apply_on: TriggerPrefixApplyOn::EveryTurn,
        });
        let sections = vec![agent_runtime::PromptSection {
            name: "user_message",
            content: "=== User message ===\n[loom envelope]\nhello".into(),
        }];
        let prompt = prompt_telemetry(sections[0].content.clone(), &sections);

        let prompt = apply_trigger_prefix_to_prompt(&spec, prompt, false, Some("/review [loom]\n"));

        assert!(prompt
            .content
            .starts_with("/review [loom]\n=== User message ==="));
        assert!(!prompt.content.starts_with("/router\n"));
        assert_eq!(prompt.breakdown.sections[0].key, "trigger_prefix");
    }

    #[test]
    fn prompt_template_vars_include_trigger_and_paths() {
        let root = temp_path("prompt-template-vars");
        let paths = AgentPaths::new(&root, "actor_demo");
        let mut spec = sample_spec(None);
        spec.prompt_template = Some(PromptTemplateSpec {
            active_skill: Some("demo".into()),
            vars: BTreeMap::from([("role".into(), "router".into())]),
            ..Default::default()
        });
        let state = WorkerState::new(
            "actor_demo".into(),
            spec,
            paths.profile.clone(),
            paths,
            "ws://127.0.0.1:0".into(),
        );
        let trigger = Event {
            id: "evt_trigger".into(),
            kind: "content.add".into(),
            actor_id: "actor_human".into(),
            scope: ScopeRef {
                kind: ScopeKind::Channel,
                id: "chan_demo".into(),
            },
            turn_id: None,
            seq: 1,
            occurred_at: chrono::Utc::now(),
            payload: serde_json::json!({}),
            relations: Vec::new(),
            _meta: None,
        };

        let vars = prompt_template_vars(&state, &AgentTrigger::Event(trigger), "chan_demo");

        assert_eq!(vars.get("actor.id").map(String::as_str), Some("actor_demo"));
        assert_eq!(
            vars.get("channel.id").map(String::as_str),
            Some("chan_demo")
        );
        assert_eq!(
            vars.get("trigger.id").map(String::as_str),
            Some("evt_trigger")
        );
        assert_eq!(
            vars.get("trigger.actor_id").map(String::as_str),
            Some("actor_human")
        );
        assert_eq!(
            vars.get("prompt.activeSkill").map(String::as_str),
            Some("demo")
        );
        assert_eq!(vars.get("vars.role").map(String::as_str), Some("router"));
        assert!(vars
            .get("workspace.dir")
            .is_some_and(|value| value.contains("chan_demo")));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn prompt_template_vars_include_reply_target_for_message() {
        let root = temp_path("prompt-template-reply-target");
        let paths = AgentPaths::new(&root, "actor_demo");
        let spec = sample_spec(None);
        let state = WorkerState::new(
            "actor_demo".into(),
            spec,
            paths.profile.clone(),
            paths,
            "ws://127.0.0.1:0".into(),
        );
        let message = sample_message(
            "msg_reply",
            ScopeRef {
                kind: ScopeKind::Thread,
                id: "thread_demo".into(),
            },
            "#chan_demo:msg_root",
            Some("msg_root"),
            Some("msg_root"),
        );

        let vars = prompt_template_vars(&state, &AgentTrigger::Message(message), "chan_demo");

        assert_eq!(
            vars.get("reply.target").map(String::as_str),
            Some("#chan_demo:msg_root")
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn loom_human_interaction_request_ids_are_tool_local() {
        assert!(is_loom_tool_request_id("loom:question:abc"));
        assert!(is_loom_tool_request_id("loom:approval:abc"));
        assert!(!is_loom_tool_request_id("loom:model:abc"));
        assert!(!is_loom_tool_request_id("acp:permission:abc"));
    }

    #[test]
    fn reconnect_delay_grows_and_caps() {
        assert_eq!(reconnect_delay(0), Duration::from_secs(2));
        assert_eq!(reconnect_delay(1), Duration::from_secs(2));
        assert_eq!(reconnect_delay(2), Duration::from_secs(4));
        assert_eq!(reconnect_delay(3), Duration::from_secs(8));
        assert_eq!(reconnect_delay(4), Duration::from_secs(16));
        assert_eq!(reconnect_delay(5), Duration::from_secs(30));
        assert_eq!(reconnect_delay(99), Duration::from_secs(30));
    }

    #[test]
    fn mark_cancel_requested_only_marks_matching_active_turn() {
        let root = temp_path("cancel-mark");
        let paths = AgentPaths::new(&root, "actor_demo");
        let state = WorkerState::new(
            "actor_demo".into(),
            sample_spec(None),
            paths.profile.clone(),
            paths,
            "ws://127.0.0.1:0".into(),
        );
        let scope = ScopeRef {
            kind: ScopeKind::Channel,
            id: "chan_demo".into(),
        };
        state.set_turn(ActiveTurn {
            id: "turn_1".into(),
            run_id: "run_1".into(),
            scope: scope.clone(),
            trigger_source_id: "msg_1".into(),
            trigger_is_message: true,
            reply_target: Some("#chan_demo".into()),
            prompt_stats: empty_prompt_stats(),
            prompt_breakdown: empty_prompt_breakdown(),
            trigger_actor: "actor_human".into(),
            no_reply_file: None,
            no_reply_requested: false,
            cancel_requested: false,
        });

        assert!(state
            .mark_cancel_requested(&scope.id, "turn_other")
            .is_none());
        assert!(!state.current_turn(&scope.id).unwrap().cancel_requested);

        let marked = state
            .mark_cancel_requested(&scope.id, "turn_1")
            .expect("matching turn should be marked");
        assert!(marked.cancel_requested);
        assert!(state.current_turn(&scope.id).unwrap().cancel_requested);

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn model_choices_include_default_and_dedupe() {
        let mut spec = sample_spec(None);
        spec.models = Some(AgentModelSpec {
            default: Some("model_default".into()),
            choices: vec![
                AgentModelChoice {
                    id: "model_fast".into(),
                    label: "Fast".into(),
                    description: None,
                },
                AgentModelChoice {
                    id: "model_fast".into(),
                    label: "Fast duplicate".into(),
                    description: None,
                },
            ],
        });

        let choices = model_choices_for_spec(&spec);

        assert_eq!(
            choices
                .iter()
                .map(|choice| choice.id.as_str())
                .collect::<Vec<_>>(),
            vec!["model_default", "model_fast"]
        );
        assert_eq!(
            default_model_for_spec(&spec).as_deref(),
            Some("model_default")
        );
        assert!(model_is_allowed(&spec, "model_fast"));
        assert!(!model_is_allowed(&spec, "model_missing"));
    }

    #[test]
    fn model_picker_prefers_adapter_choices_over_spec_choices() {
        let mut spec = sample_spec(None);
        spec.models = Some(AgentModelSpec {
            default: Some("spec_default".into()),
            choices: vec![AgentModelChoice {
                id: "spec_fast".into(),
                label: "Spec Fast".into(),
                description: None,
            }],
        });
        let root = temp_path("model-picker-adapter");
        let paths = AgentPaths::new(&root, "actor_demo");
        let state = WorkerState::new(
            "actor_demo".into(),
            spec,
            paths.profile.clone(),
            paths,
            "ws://127.0.0.1:0".into(),
        );
        let (choices, current, source, _) = model_picker_choices(
            &state,
            Some(AdapterModelOptions {
                config_id: "model".into(),
                current_value: Some("runtime_sonnet".into()),
                choices: vec![agent_runtime::AdapterModelChoice {
                    id: "runtime_sonnet".into(),
                    label: "Runtime Sonnet".into(),
                    description: None,
                }],
            }),
        );

        assert_eq!(
            choices.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
            vec!["runtime_sonnet"]
        );
        assert_eq!(current.as_deref(), Some("runtime_sonnet"));
        assert!(matches!(source, ModelActionSource::Adapter { .. }));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn persisted_model_loads_when_static_choices_are_absent() {
        let root = temp_path("dynamic-model-state");
        let paths = AgentPaths::new(&root, "actor_demo");
        std::fs::create_dir_all(&paths.profile).expect("create profile");
        persist_model_state(&paths.profile, "runtime_sonnet").expect("persist model");
        let state = WorkerState::new(
            "actor_demo".into(),
            sample_spec(None),
            paths.profile.clone(),
            paths,
            "ws://127.0.0.1:0".into(),
        );

        assert_eq!(state.current_model().as_deref(), Some("runtime_sonnet"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn acp_persisted_runtime_model_survives_static_choices() {
        let root = temp_path("acp-dynamic-model-state");
        let paths = AgentPaths::new(&root, "actor_demo");
        std::fs::create_dir_all(&paths.profile).expect("create profile");
        persist_model_state(&paths.profile, "runtime_sonnet").expect("persist model");
        let mut spec = sample_spec(None);
        let mut transport = test_command_transport();
        transport.kind = "acp_stdio".into();
        spec.models = Some(AgentModelSpec {
            default: Some("spec_default".into()),
            choices: vec![AgentModelChoice {
                id: "spec_fast".into(),
                label: "Spec Fast".into(),
                description: None,
            }],
        });

        let state = WorkerState::new_with_transport(
            "actor_demo".into(),
            spec,
            transport,
            paths.profile.clone(),
            paths,
            "ws://127.0.0.1:0".into(),
        );

        assert_eq!(state.current_model().as_deref(), Some("runtime_sonnet"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn command_persisted_model_still_respects_static_choices() {
        let root = temp_path("command-static-model-state");
        let paths = AgentPaths::new(&root, "actor_demo");
        std::fs::create_dir_all(&paths.profile).expect("create profile");
        persist_model_state(&paths.profile, "runtime_sonnet").expect("persist model");
        let mut spec = sample_spec(None);
        spec.models = Some(AgentModelSpec {
            default: Some("spec_default".into()),
            choices: vec![AgentModelChoice {
                id: "spec_fast".into(),
                label: "Spec Fast".into(),
                description: None,
            }],
        });

        let state = WorkerState::new(
            "actor_demo".into(),
            spec,
            paths.profile.clone(),
            paths,
            "ws://127.0.0.1:0".into(),
        );

        assert_eq!(state.current_model().as_deref(), Some("spec_default"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn failed_turn_text_uses_non_empty_summary() {
        assert_eq!(failed_turn_text("   "), None);
        assert_eq!(
            failed_turn_text("Not inside a trusted directory").as_deref(),
            Some("Agent run failed:\n\nNot inside a trusted directory")
        );
    }

    #[test]
    fn worker_state_loads_persisted_model_over_default() {
        let root = temp_path("model-state");
        let paths = AgentPaths::new(&root, "actor_demo");
        std::fs::create_dir_all(&paths.profile).expect("create profile");
        persist_model_state(&paths.profile, "model_fast").expect("persist model");
        let mut spec = sample_spec(None);
        spec.models = Some(AgentModelSpec {
            default: Some("model_default".into()),
            choices: vec![AgentModelChoice {
                id: "model_fast".into(),
                label: "Fast".into(),
                description: None,
            }],
        });

        let state = WorkerState::new(
            "actor_demo".into(),
            spec,
            paths.profile.clone(),
            paths,
            "ws://127.0.0.1:0".into(),
        );

        assert_eq!(state.current_model().as_deref(), Some("model_fast"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn worker_state_queues_triggers_per_scope_only() {
        let root = temp_path("scope-queue");
        let paths = AgentPaths::new(&root, "actor_demo");
        let state = WorkerState::new(
            "actor_demo".into(),
            sample_spec(None),
            paths.profile.clone(),
            paths,
            "ws://127.0.0.1:0".into(),
        );
        let active_scope = ScopeRef {
            kind: ScopeKind::Channel,
            id: "chan_triage".into(),
        };
        state.set_turn(ActiveTurn {
            id: "turn_channel".into(),
            run_id: "run_channel".into(),
            scope: active_scope.clone(),
            trigger_source_id: "msg_root".into(),
            trigger_is_message: true,
            reply_target: Some("#chan_triage".into()),
            prompt_stats: empty_prompt_stats(),
            prompt_breakdown: empty_prompt_breakdown(),
            trigger_actor: "actor_human".into(),
            no_reply_file: None,
            no_reply_requested: false,
            cancel_requested: false,
        });
        let queued_channel = Event {
            id: "evt_channel_route".into(),
            kind: "content.add".into(),
            actor_id: "actor_demo".into(),
            scope: active_scope.clone(),
            turn_id: None,
            seq: 1,
            occurred_at: Utc::now(),
            payload: json!({ "text": "continue triage" }),
            relations: Vec::new(),
            _meta: None,
        };
        let queued_thread = Event {
            id: "evt_thread_route".into(),
            kind: "content.add".into(),
            actor_id: "actor_demo".into(),
            scope: ScopeRef {
                kind: ScopeKind::Thread,
                id: "thread_task".into(),
            },
            turn_id: None,
            seq: 1,
            occurred_at: Utc::now(),
            payload: json!({ "text": "continue task" }),
            relations: Vec::new(),
            _meta: None,
        };

        assert!(state.current_turn(&active_scope.id).is_some());
        assert!(state.current_turn(&queued_thread.scope.id).is_none());
        state.enqueue(
            &queued_channel.scope.id,
            AgentTrigger::Event(queued_channel.clone()),
        );
        state.enqueue(
            &queued_thread.scope.id,
            AgentTrigger::Event(queued_thread.clone()),
        );
        assert!(state.has_pending_source(&queued_channel.id));
        assert!(state.has_pending_source(&queued_thread.id));
        assert_eq!(
            state
                .clear_turn(&active_scope.id)
                .map(|trigger| trigger.id().to_string()),
            Some(queued_channel.id)
        );
        assert!(state.current_turn(&active_scope.id).is_none());
        assert!(state.clear_turn(&active_scope.id).is_none());
        assert_eq!(
            state
                .clear_turn(&queued_thread.scope.id)
                .map(|trigger| trigger.id().to_string()),
            Some(queued_thread.id)
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn worker_state_prioritizes_human_triggers_without_reversing_human_fifo() {
        let root = temp_path("scope-priority-queue");
        let paths = AgentPaths::new(&root, "actor_demo");
        let state = WorkerState::new(
            "actor_demo".into(),
            sample_spec(None),
            paths.profile.clone(),
            paths,
            "ws://127.0.0.1:0".into(),
        );
        let active_scope = ScopeRef {
            kind: ScopeKind::Thread,
            id: "thread_busy".into(),
        };
        state.set_turn(ActiveTurn {
            id: "turn_busy".into(),
            run_id: "run_busy".into(),
            scope: active_scope.clone(),
            trigger_source_id: "msg_busy".into(),
            trigger_is_message: true,
            reply_target: Some("#chan_demo:msg_busy".into()),
            prompt_stats: empty_prompt_stats(),
            prompt_breakdown: empty_prompt_breakdown(),
            trigger_actor: "mr-watcher".into(),
            no_reply_file: None,
            no_reply_requested: false,
            cancel_requested: false,
        });

        let service = Event {
            id: "evt_service".into(),
            kind: "content.add".into(),
            actor_id: "mr-watcher".into(),
            scope: active_scope.clone(),
            turn_id: None,
            seq: 1,
            occurred_at: Utc::now(),
            payload: json!({ "text": "service callback" }),
            relations: Vec::new(),
            _meta: None,
        };
        let human_one = Event {
            id: "evt_human_one".into(),
            kind: "content.add".into(),
            actor_id: "actor_human_123".into(),
            scope: active_scope.clone(),
            turn_id: None,
            seq: 1,
            occurred_at: Utc::now(),
            payload: json!({ "text": "first human request" }),
            relations: Vec::new(),
            _meta: None,
        };
        let human_two = Event {
            id: "evt_human_two".into(),
            kind: "content.add".into(),
            actor_id: "actor_human_123".into(),
            scope: active_scope.clone(),
            turn_id: None,
            seq: 1,
            occurred_at: Utc::now(),
            payload: json!({ "text": "second human request" }),
            relations: Vec::new(),
            _meta: None,
        };

        state.enqueue(&service.scope.id, AgentTrigger::Event(service.clone()));
        state.enqueue(&human_one.scope.id, AgentTrigger::Event(human_one.clone()));
        state.enqueue(&human_two.scope.id, AgentTrigger::Event(human_two.clone()));
        state.enqueue(&human_one.scope.id, AgentTrigger::Event(human_one.clone()));

        assert_eq!(
            state
                .clear_turn(&active_scope.id)
                .map(|trigger| trigger.id().to_string()),
            Some(human_one.id)
        );
        assert_eq!(
            state
                .clear_turn(&active_scope.id)
                .map(|trigger| trigger.id().to_string()),
            Some(human_two.id)
        );
        assert_eq!(
            state
                .clear_turn(&active_scope.id)
                .map(|trigger| trigger.id().to_string()),
            Some(service.id)
        );
        assert!(state.clear_turn(&active_scope.id).is_none());
        std::fs::remove_dir_all(root).ok();
    }
}
