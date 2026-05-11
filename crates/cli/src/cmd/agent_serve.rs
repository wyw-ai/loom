//! Daemon agent worker internals.
//!
//! `joi daemon` synthesizes `AgentSpec`s from the desktop machine config and
//! uses this module to open a dedicated WebSocket per agent, then supervise
//! that one agent through the shared `agent-runtime` adapter trait.
//!
//! Architecture (per docs/architecture-v1-agent-client.md §6):
//!   * one tokio task per agent ⇒ one `Client` ⇒ one WS frame to the server
//!   * `connection/open` with `actorKind = "agent"` binds the connection to the
//!     agent's actor id; the server's actor-inbox delivery (see
//!     `crates/server/src/ws.rs::fanout`) then pushes every `HandsOffTo`-targeted
//!     `event.created` straight to this connection
//!   * active scopes are also subscribed while prompts run, so older servers
//!     that only scope-broadcast `action.response` events still unblock ACP
//!     permission prompts
//!   * a notification loop turns those events into `Adapter::send_prompt` calls,
//!     opening / tracking a turn through `turn/open` + `turn/close`
//!   * a translator task drains `AdapterEvent`s and re-emits them as
//!     `event/append` (public content) + `turn/trace.append` (owner-only trace)
//!     RPCs.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Result};
use proto::methods::{
    method, stream_kind, AgentModelChoice, AgentSpec, BundleInstallMode, EventAppendResult,
    TurnOpenResult,
};
use proto::types::trace::TraceKind;
use proto::types::{
    ActorKind, Event, Meta, Ref, RefKind, Relation, RelationKind, ScopeKind, ScopeRef, TurnStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::time::{sleep, Duration};

use agent_runtime::acp::{AcpAdapter, AcpConfig};
use agent_runtime::command::{CommandAdapter, CommandConfig};
use agent_runtime::interactive::{InteractiveCommandAdapter, InteractiveCommandConfig};
use agent_runtime::usage;
use agent_runtime::{
    agent_child_server_url, prepare_bundle_install, resolved_bundle_version,
    validate_bundle_current, Adapter, AdapterEvent, AdapterModelOptions, AdapterPrompt, TokenUsage,
};

use crate::client::Client;
use crate::daemon_ipc;

const RECONNECT_BASE_DELAY_SECS: u64 = 2;
const RECONNECT_MAX_DELAY_SECS: u64 = 30;

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

/// The path to *this* joi binary. Used as the `command` for the
/// auto-injected `joi-memory` MCP server entry; falls back to the bare
/// name `"joi"` (hoping it's on PATH) if we can't resolve our own exe.
fn current_joi_binary() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .or_else(|| Some(PathBuf::from("joi")))
}

#[derive(Debug, Clone)]
pub struct MachineHostSpec {
    pub machine_id: String,
    pub actor_id: String,
    pub display_name: String,
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
    let _: Value = client
        .call(
            method::ACTOR_UPSERT,
            json!({
                "actor": {
                    "id": &host.actor_id,
                    "kind": "service",
                    "displayName": &host.display_name,
                    "_meta": {
                        "role": "machine",
                        "machineId": &host.machine_id,
                    },
                },
            }),
        )
        .await
        .with_context(|| format!("actor/upsert for machine {}", host.machine_id))?;
    client
        .open_connection_as(&host.actor_id, "service", Some(&host.display_name))
        .await?;
    eprintln!(
        "[{}] machine host connected to {} as {}",
        host.machine_id, server_url, host.actor_id
    );

    loop {
        sleep(Duration::from_secs(15)).await;
        let _: Value = client.call_raw(method::ACTOR_LIST, None).await?;
    }
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
        let scope_workspaces_root = std::env::var_os("JOI_SCOPE_WORKSPACES_ROOT")
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
        if spec.identity.is_some() || spec.memory.is_some() {
            let identity = spec.identity.as_ref();
            let identity_file = identity
                .map(|s| s.files.identity.as_str())
                .unwrap_or("identity.md");
            let soul_file = identity.map(|s| s.files.soul.as_str()).unwrap_or("soul.md");
            let description = identity
                .and_then(|s| s.description.as_deref())
                .unwrap_or("");
            let identity_seed = identity
                .and_then(|s| s.scaffold.as_ref())
                .and_then(|s| s.identity.as_deref());
            let soul_seed = identity
                .and_then(|s| s.scaffold.as_ref())
                .and_then(|s| s.soul.as_deref());
            let memory_root = spec
                .memory
                .as_ref()
                .map(|m| m.store.root.as_str())
                .unwrap_or("./memory/records");
            if let Err(e) =
                agent_runtime::ensure_profile_scaffold(&agent_runtime::ProfileScaffold {
                    profile_dir: &self.profile,
                    actor_id,
                    display_name: &spec.actor.display_name,
                    description,
                    identity_file,
                    soul_file,
                    memory_root,
                    identity_seed,
                    soul_seed,
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
        let mut vars = BTreeMap::new();
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
        env.insert("JOI_SERVER".into(), server_url.to_string());
        if let Some(socket) = daemon_ipc::env_socket_path() {
            env.insert(
                daemon_ipc::ENV_DAEMON_SOCKET.into(),
                socket.display().to_string(),
            );
        }
        env.insert("JOI_ACTOR".into(), actor_id.to_string());
        env.insert("JOI_SCOPE_ID".into(), scope_ref.id.clone());
        env.insert(
            "JOI_SCOPE_KIND".into(),
            scope_kind_name(scope_ref.kind).to_string(),
        );
        if let Some(active) = active {
            env.insert("JOI_TURN_ID".into(), active.id.clone());
            env.insert("JOI_TRIGGER_ACTOR".into(), active.trigger_actor.clone());
        }
        env.insert(
            "JOI_AGENT_PROFILE".into(),
            self.profile.display().to_string(),
        );
        env.insert(
            "JOI_AGENT_BUNDLE_DIR".into(),
            self.bundle_current.display().to_string(),
        );
        env.insert("AGENTX_CHANNEL_ID".into(), channel_id.to_string());
        env.insert(
            "AGENTX_CHANNEL_ROOT".into(),
            scope.channel_root.display().to_string(),
        );
        env.insert(
            "AGENTX_CHANNEL_SHARED".into(),
            scope.channel_shared.display().to_string(),
        );
        env.insert(
            "AGENTX_CHANNEL_SHARED_ARTIFACTS".into(),
            scope.channel_artifacts.display().to_string(),
        );
        env.insert(
            "AGENTX_AGENT_ROOT".into(),
            scope.agent_root.display().to_string(),
        );
        env.insert(
            "AGENTX_AGENT_WORKSPACE".into(),
            scope.workspace.display().to_string(),
        );
        env.insert("AGENTX_AGENT_LOGS".into(), scope.logs.display().to_string());
        env.insert(
            "AGENTX_SCOPE_SKILLS".into(),
            scope.skills.display().to_string(),
        );
        env.insert(
            "JOI_SCOPE_SKILLS_DIR".into(),
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

struct WorkerState {
    actor_id: String,
    /// Cached copy of the on-disk spec. Reads only; specs are load-once in v1.
    spec: AgentSpec,
    /// Resolved profile dir — same one `AgentPaths.profile` points at. Copied
    /// here so prompt-envelope code can read identity/soul/memory without
    /// threading `paths` through every call.
    profile_dir: PathBuf,
    paths: AgentPaths,
    agent_server_url: String,
    /// In-flight turn per scope. Same agent in multiple channels => multiple
    /// concurrent turns, one per scope.id; same scope back-to-back is enforced
    /// to be FIFO via `pending_triggers` below.
    active_turns: Mutex<HashMap<String, ActiveTurn>>,
    /// Per-scope queue of triggers received while the scope was busy. Drained
    /// one-at-a-time when the scope's current turn closes.
    pending_triggers: Mutex<HashMap<String, VecDeque<Event>>>,
    /// Per-turn streaming text buffer; flushed as a single `content.add` on
    /// `Finished`, once final usage metadata is available.
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
    /// Event ids already received from the server notification stream. The
    /// same event can arrive through both scope broadcast and actor-inbox
    /// routing when we subscribe to an active scope for legacy server
    /// compatibility.
    seen_events: Mutex<HashSet<String>>,
    /// action.request event id → underlying ACP request id.
    action_map: Mutex<HashMap<String, String>>,
    /// action.request event id → metadata for Joi-owned model selection prompts.
    model_action_map: Mutex<HashMap<String, ModelActionRequest>>,
    /// Currently selected model id for this actor. Loaded from profile state
    /// first, then from `spec.models.default`.
    selected_model: Mutex<Option<String>>,
}

#[derive(Clone)]
struct ActiveTurn {
    id: String,
    scope: ScopeRef,
    trigger_event_id: String,
    prompt_stats: PromptStats,
    prompt_breakdown: PromptBreakdown,
    /// Actor that triggered the current turn — needed when emitting a
    /// `action.request` so we can hand the choice back to them.
    trigger_actor: String,
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

impl WorkerState {
    fn new(
        actor_id: String,
        spec: AgentSpec,
        profile_dir: PathBuf,
        paths: AgentPaths,
        agent_server_url: String,
    ) -> Self {
        let selected_model = load_model_state(&profile_dir)
            .filter(|model| persisted_model_is_allowed(&spec, model))
            .or_else(|| default_model_for_spec(&spec));
        Self {
            actor_id,
            spec,
            profile_dir,
            paths,
            agent_server_url,
            active_turns: Mutex::new(HashMap::new()),
            pending_triggers: Mutex::new(HashMap::new()),
            text_buffer: Mutex::new(HashMap::new()),
            usage_totals: Mutex::new(HashMap::new()),
            seeded: Mutex::new(HashSet::new()),
            scope_channel_cache: Mutex::new(HashMap::new()),
            seen_events: Mutex::new(HashSet::new()),
            action_map: Mutex::new(HashMap::new()),
            model_action_map: Mutex::new(HashMap::new()),
            selected_model: Mutex::new(selected_model),
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

    /// Drop the active turn for `scope_id` and atomically pop the next queued
    /// trigger (if any). Returning the trigger inside the same lock keeps a
    /// racing `enqueue` from getting wedged behind the now-empty slot.
    fn clear_turn(&self, scope_id: &str) -> Option<Event> {
        let mut active = self.active_turns.lock().expect("active_turns poisoned");
        active.remove(scope_id);
        drop(active);
        let mut queues = self.pending_triggers.lock().expect("pending poisoned");
        let q = queues.get_mut(scope_id)?;
        let next = q.pop_front();
        if q.is_empty() {
            queues.remove(scope_id);
        }
        next
    }

    fn enqueue(&self, scope_id: &str, event: Event) {
        self.pending_triggers
            .lock()
            .expect("pending poisoned")
            .entry(scope_id.to_string())
            .or_default()
            .push_back(event);
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

    fn record_action_request(&self, event_id: String, request_id: String) {
        self.action_map
            .lock()
            .expect("action_map poisoned")
            .insert(event_id, request_id);
    }

    fn lookup_action_request(&self, event_id: &str) -> Option<String> {
        self.action_map
            .lock()
            .expect("action_map poisoned")
            .get(event_id)
            .cloned()
    }

    fn forget_action_request(&self, event_id: &str) {
        let _ = self
            .action_map
            .lock()
            .expect("action_map poisoned")
            .remove(event_id);
    }

    fn current_model(&self) -> Option<String> {
        self.selected_model
            .lock()
            .expect("selected_model poisoned")
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

    fn record_model_action_request(&self, event_id: String, request: ModelActionRequest) {
        self.model_action_map
            .lock()
            .expect("model_action_map poisoned")
            .insert(event_id, request);
    }

    fn lookup_model_action_request(&self, event_id: &str) -> Option<ModelActionRequest> {
        self.model_action_map
            .lock()
            .expect("model_action_map poisoned")
            .get(event_id)
            .cloned()
    }

    fn is_model_action_request(&self, event_id: &str) -> bool {
        self.model_action_map
            .lock()
            .expect("model_action_map poisoned")
            .contains_key(event_id)
    }

    fn forget_model_action_request(&self, event_id: &str) {
        let _ = self
            .model_action_map
            .lock()
            .expect("model_action_map poisoned")
            .remove(event_id);
    }

    fn remember_event(&self, event_id: &str) -> bool {
        self.seen_events
            .lock()
            .expect("seen_events poisoned")
            .insert(event_id.to_string())
    }
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

fn persisted_model_is_allowed(spec: &AgentSpec, model: &str) -> bool {
    let model = model.trim();
    if model.is_empty() {
        return false;
    }
    if transport_supports_runtime_model_options(spec) {
        return true;
    }
    let choices = model_choices_for_spec(spec);
    if choices.is_empty() {
        return true;
    }
    choices.iter().any(|choice| choice.id == model)
}

fn transport_supports_runtime_model_options(spec: &AgentSpec) -> bool {
    spec.transport.kind == "acp_stdio"
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
    eprintln!(
        "[{actor_id}] connected to {server_url} as {:?}",
        spec.actor.kind
    );

    let agent_server_url = agent_child_server_url(&server_url);
    if agent_server_url != server_url {
        eprintln!(
            "[{actor_id}] injecting JOI_SERVER={} for child agents (agent-client connected via {})",
            agent_server_url, server_url
        );
    }
    let state = Arc::new(WorkerState::new(
        actor_id.clone(),
        spec.clone(),
        paths.profile.clone(),
        paths.clone(),
        agent_server_url.clone(),
    ));
    let (event_tx, event_rx) = mpsc::unbounded_channel::<AdapterEvent>();
    let adapter = build_adapter(&spec, &paths, &bundle_paths, &agent_server_url)?;

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

fn build_adapter(
    spec: &AgentSpec,
    paths: &AgentPaths,
    bundle_paths: &BundlePaths,
    server_url: &str,
) -> Result<Arc<dyn Adapter>> {
    let mut process_env: BTreeMap<String, String> = spec
        .transport
        .env
        .iter()
        .map(|(k, v)| (k.clone(), paths.expand(v, Some(bundle_paths))))
        .collect();
    let mut command_env = spec.transport.env.clone();
    process_env
        .entry("JOI_SERVER".into())
        .or_insert_with(|| server_url.to_string());
    command_env
        .entry("JOI_SERVER".into())
        .or_insert_with(|| server_url.to_string());
    if let Some(socket) = daemon_ipc::env_socket_path() {
        let socket = socket.display().to_string();
        process_env
            .entry(daemon_ipc::ENV_DAEMON_SOCKET.into())
            .or_insert_with(|| socket.clone());
        command_env
            .entry(daemon_ipc::ENV_DAEMON_SOCKET.into())
            .or_insert(socket);
    }
    process_env
        .entry("JOI_ACTOR".into())
        .or_insert_with(|| spec.actor.id.clone());
    command_env
        .entry("JOI_ACTOR".into())
        .or_insert_with(|| spec.actor.id.clone());
    process_env
        .entry("JOI_AGENT_PROFILE".into())
        .or_insert_with(|| paths.profile.display().to_string());
    command_env
        .entry("JOI_AGENT_PROFILE".into())
        .or_insert_with(|| paths.profile.display().to_string());
    process_env
        .entry("JOI_AGENT_BUNDLE_ROOT".into())
        .or_insert_with(|| bundle_paths.root.display().to_string());
    command_env
        .entry("JOI_AGENT_BUNDLE_ROOT".into())
        .or_insert_with(|| bundle_paths.root.display().to_string());
    process_env
        .entry("JOI_AGENT_BUNDLE_DIR".into())
        .or_insert_with(|| bundle_paths.current.display().to_string());
    command_env
        .entry("JOI_AGENT_BUNDLE_DIR".into())
        .or_insert_with(|| bundle_paths.current.display().to_string());
    if !bundle_paths.version.is_empty() {
        process_env
            .entry("JOI_AGENT_BUNDLE_VERSION".into())
            .or_insert_with(|| bundle_paths.version.clone());
        command_env
            .entry("JOI_AGENT_BUNDLE_VERSION".into())
            .or_insert_with(|| bundle_paths.version.clone());
    }
    let process_args: Vec<String> = spec
        .transport
        .args
        .iter()
        .map(|a| paths.expand(a, Some(bundle_paths)))
        .collect();

    match spec.transport.kind.as_str() {
        "acp_stdio" => {
            let joi_binary = current_joi_binary();
            let mcp_servers = agent_runtime::build_mcp_servers(
                joi_binary.as_deref(),
                &spec.actor.id,
                &paths.profile,
                spec.memory.as_ref(),
                spec.announcement.as_ref(),
                Some(server_url),
            );
            let cfg = AcpConfig {
                command: spec.transport.command.clone(),
                args: process_args,
                env: process_env,
                process_cwd: paths.root.clone(),
                auth_method: spec.transport.auth_method.clone(),
                mcp_servers,
            };
            Ok(Arc::new(AcpAdapter::new(cfg)))
        }
        "command" => {
            let cfg = CommandConfig::from_transport(
                spec.actor.id.clone(),
                spec.transport.command.clone(),
                spec.transport.args.clone(),
                command_env,
                &spec.transport,
                paths.sessions.clone(),
            );
            Ok(Arc::new(CommandAdapter::new(cfg)))
        }
        "interactive_command" => {
            let interactive = spec.transport.interactive.clone().unwrap_or_default();
            let model = spec
                .transport
                .model
                .clone()
                .or_else(|| spec.models.as_ref().and_then(|m| m.default.clone()));
            let cfg = InteractiveCommandConfig::new(
                spec.actor.id.clone(),
                spec.transport.command.clone(),
                &spec.transport.args,
                command_env,
                model,
                interactive,
                spec.transport.provider.clone(),
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
    loop {
        // Drain pending notifications. We pop them one by one and dispatch
        // each on its own; the borrow on `notifications` is released between
        // iterations so nested RPC calls (turn/open, event/append) can use the
        // same Client without deadlock.
        let next = {
            let mut rx = client.notifications.lock().await;
            rx.recv().await
        };
        let Some(n) = next else {
            eprintln!("[{actor_id}] server disconnected, worker exiting");
            return Ok(());
        };

        if n.method != method::STREAM_UPDATE {
            continue;
        }
        let Some(params) = n.params else { continue };
        if params.get("kind").and_then(|v| v.as_str()) != Some(stream_kind::EVENT_CREATED) {
            continue;
        }
        let Some(event_value) = params.get("data").and_then(|d| d.get("event")).cloned() else {
            continue;
        };
        let Ok(event) = serde_json::from_value::<Event>(event_value) else {
            continue;
        };
        if !state.remember_event(&event.id) {
            continue;
        }
        if event.kind == "action.response" {
            if let Err(e) = handle_action_response(&client, &state, &adapter, &event).await {
                eprintln!("[{actor_id}] failed to handle action.response: {e}");
            }
            continue;
        }
        if event.kind == "turn.close" && is_for_us(&event, actor_id) {
            if let Err(e) = handle_turn_close(&client, &state, &adapter, &event).await {
                eprintln!("[{actor_id}] failed to handle turn.close: {e}");
            }
            continue;
        }
        if !is_for_us(&event, actor_id) {
            continue;
        }

        match handle_control_command(&client, &state, &adapter, &event_tx, &mut started, &event)
            .await
        {
            Ok(true) => continue,
            Ok(false) => {}
            Err(e) => {
                eprintln!("[{actor_id}] failed to handle control command: {e}");
                continue;
            }
        }

        // Lazy start the adapter on first hands_off_to event.
        if !started {
            eprintln!(
                "[{actor_id}] starting adapter (first hand-off; ACP cold-start \
                 can take 30-60s while the agent refreshes its model registry)…"
            );
            if let Err(e) = adapter.start(event_tx.clone()).await {
                eprintln!("[{actor_id}] adapter start failed: {e}");
                continue;
            }
            started = true;
            eprintln!("[{actor_id}] adapter ready");
        }

        if let Err(e) = handle_handoff(&client, &state, &adapter, &event).await {
            eprintln!("[{actor_id}] failed to handle handoff: {e}");
        }
    }
}

async fn handle_action_response(
    client: &Arc<Client>,
    state: &Arc<WorkerState>,
    adapter: &Arc<dyn Adapter>,
    event: &Event,
) -> Result<()> {
    let mut saw_response_relation = false;
    for relation in &event.relations {
        if !matches!(relation.kind, RelationKind::RespondsTo)
            || relation.target.kind != RefKind::Event
        {
            continue;
        }
        saw_response_relation = true;
        let request_event_id = relation.target.id.as_str();
        let echoed_request_id = action_request_id_from_response(event);
        if echoed_request_id
            .as_deref()
            .is_some_and(is_joi_tool_request_id)
        {
            eprintln!(
                "[{}] action.response {} is for a joi human-interaction tool {}; leaving it for the waiting tool process",
                state.actor_id, event.id, request_event_id
            );
            return Ok(());
        }
        if state.is_model_action_request(request_event_id)
            || echoed_request_id
                .as_deref()
                .is_some_and(|id| id.starts_with("joi:model:"))
        {
            handle_model_action_response(client, state, adapter, event, request_event_id).await?;
            return Ok(());
        }
        let request_id = match state.lookup_action_request(request_event_id) {
            Some(id) => id,
            None => match echoed_request_id {
                Some(id) => {
                    eprintln!(
                        "[{}] action.response {} used echoed ACP request id for {}",
                        state.actor_id, event.id, request_event_id
                    );
                    id
                }
                None => {
                    eprintln!(
                        "[{}] action.response {} ignored: no pending ACP request for {} \
                         (joi daemon may have restarted after the action.request)",
                        state.actor_id, event.id, request_event_id
                    );
                    continue;
                }
            },
        };
        let option_id = event
            .payload
            .get("optionId")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if option_id.is_empty() {
            eprintln!(
                "[{}] action.response {} ignored: missing payload.optionId",
                state.actor_id, event.id
            );
            return Ok(());
        }
        eprintln!(
            "[{}] action.response {} -> ACP request {} option {}",
            state.actor_id, event.id, request_id, option_id
        );
        adapter
            .respond_action(request_id.clone(), option_id)
            .await
            .map_err(|e| anyhow!("adapter respond_action failed: {e}"))?;
        state.forget_action_request(request_event_id);
        return Ok(());
    }
    if !saw_response_relation {
        eprintln!(
            "[{}] action.response {} ignored: missing responds_to relation",
            state.actor_id, event.id
        );
    }
    Ok(())
}

fn action_request_id_from_response(event: &Event) -> Option<String> {
    event
        .payload
        .get("requestId")
        .or_else(|| event.payload.get("actionId"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
}

fn is_joi_tool_request_id(id: &str) -> bool {
    id.starts_with("joi:question:") || id.starts_with("joi:approval:")
}

async fn handle_model_action_response(
    client: &Arc<Client>,
    state: &Arc<WorkerState>,
    adapter: &Arc<dyn Adapter>,
    event: &Event,
    request_event_id: &str,
) -> Result<()> {
    let option_id = event
        .payload
        .get("optionId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if option_id.is_empty() {
        eprintln!(
            "[{}] model action.response {} ignored: missing payload.optionId",
            state.actor_id, event.id
        );
        return Ok(());
    }

    let request = state
        .lookup_model_action_request(request_event_id)
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
            "[{}] model action.response {} ignored: unknown model `{}`",
            state.actor_id, event.id, option_id
        );
        state.forget_model_action_request(request_event_id);
        return Ok(());
    };

    if let Err(err) =
        apply_model_selection(state, adapter, event, &request.source, &option_id).await
    {
        state.forget_model_action_request(request_event_id);
        append_model_selection_failure(client, state, event, &choice, &option_id, &err).await?;
        eprintln!(
            "[{}] failed to select model `{}` via {}: {}",
            state.actor_id, option_id, event.id, err
        );
        return Ok(());
    }
    state.forget_model_action_request(request_event_id);

    let label = model_choice_label(&choice);
    let text = model_selection_success_text(&request.source, label, &option_id);
    append_event(
        client,
        "content.add",
        &state.actor_id,
        &event.scope,
        None,
        json!({
            "contentType": "text/markdown",
            "text": text
        }),
        vec![responds_to(&event.id)],
        None,
    )
    .await?;
    eprintln!(
        "[{}] selected model `{}` via {}",
        state.actor_id, option_id, event.id
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
            "Model set to `{label}` (`{option_id}`). It is saved for this agent and will be used when Joi creates a new ACP session."
        ),
    }
}

async fn apply_model_selection(
    state: &WorkerState,
    adapter: &Arc<dyn Adapter>,
    event: &Event,
    source: &ModelActionSource,
    option_id: &str,
) -> Result<()> {
    match source {
        ModelActionSource::Spec => {
            state.set_current_model(option_id.to_string())?;
        }
        ModelActionSource::Adapter { config_id } => {
            adapter
                .set_model_option(
                    event.scope.clone(),
                    config_id.clone(),
                    option_id.to_string(),
                )
                .await
                .map_err(|err| anyhow!("ACP session/set_config_option failed: {err}"))?;
            state.set_current_model_unchecked(option_id.to_string())?;
        }
    }
    Ok(())
}

async fn append_model_selection_failure(
    client: &Arc<Client>,
    state: &WorkerState,
    event: &Event,
    choice: &AgentModelChoice,
    option_id: &str,
    err: &anyhow::Error,
) -> Result<()> {
    let label = model_choice_label(choice);
    append_event(
        client,
        "content.add",
        &state.actor_id,
        &event.scope,
        None,
        json!({
            "contentType": "text/markdown",
            "text": format!("Failed to set model `{label}` (`{option_id}`): `{err}`.")
        }),
        vec![responds_to(&event.id)],
        None,
    )
    .await?;
    Ok(())
}

async fn handle_turn_close(
    _client: &Arc<Client>,
    state: &Arc<WorkerState>,
    adapter: &Arc<dyn Adapter>,
    event: &Event,
) -> Result<()> {
    if event.payload.get("status").and_then(|v| v.as_str()) != Some("cancelled") {
        return Ok(());
    }
    let Some(turn_id) = event.turn_id.as_deref() else {
        return Ok(());
    };
    let Some(active) = state.current_turn(&event.scope.id) else {
        return Ok(());
    };
    if active.id != turn_id {
        return Ok(());
    }
    let active = state
        .mark_cancel_requested(&event.scope.id, turn_id)
        .unwrap_or(active);

    if let Err(e) = adapter.cancel(active.scope.clone()).await {
        tracing::warn!(
            actor = %state.actor_id,
            turn = %active.id,
            scope = %active.scope.id,
            %e,
            "adapter cancel failed",
        );
    }
    if state.take_text(&active.id).is_some() {
        tracing::debug!(
            actor = %state.actor_id,
            turn = %active.id,
            scope = %active.scope.id,
            "discarding buffered text from cancelled turn"
        );
    }
    Ok(())
}

fn is_for_us(event: &Event, actor_id: &str) -> bool {
    if event.actor_id == actor_id {
        return false; // self-loop
    }
    event.relations.iter().any(|r| {
        matches!(r.kind, RelationKind::HandsOffTo)
            && r.target.kind == RefKind::Actor
            && r.target.id == actor_id
    })
}

async fn handle_control_command(
    client: &Arc<Client>,
    state: &Arc<WorkerState>,
    adapter: &Arc<dyn Adapter>,
    event_tx: &mpsc::UnboundedSender<AdapterEvent>,
    started: &mut bool,
    trigger: &Event,
) -> Result<bool> {
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
    trigger: &Event,
    adapter_start_error: Option<String>,
) -> Result<()> {
    let mut adapter_error = adapter_start_error;
    let adapter_options = if adapter_error.is_none() {
        match build_adapter_prompt(client, state, &trigger.scope, String::new(), None).await {
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
        append_event(
            client,
            "content.add",
            &state.actor_id,
            &trigger.scope,
            None,
            json!({
                "contentType": "text/markdown",
                "text": text
            }),
            vec![responds_to(&trigger.id)],
            None,
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
        "requestId": format!("joi:model:{}", trigger.id),
        "requestType": "joi.model.select",
        "title": format!("Choose model for @{}", state.actor_id),
        "description": format!(
            "Current model: {current_label}\n\n{source_description}"
        ),
        "choices": payload_choices,
    });
    let appended = append_event(
        client,
        "action.request",
        &state.actor_id,
        &trigger.scope,
        None,
        payload,
        vec![
            responds_to(&trigger.id),
            Relation {
                kind: RelationKind::HandsOffTo,
                target: Ref {
                    kind: RefKind::Actor,
                    id: trigger.actor_id.clone(),
                    _meta: None,
                },
                _meta: None,
            },
        ],
        None,
    )
    .await?;
    state.record_model_action_request(
        appended.event.id.clone(),
        ModelActionRequest { source, choices },
    );
    eprintln!(
        "[{}] opened model picker {} for {}",
        state.actor_id, appended.event.id, trigger.actor_id
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
        "These choices came from the runtime definition. The selected model is saved for this actor and used when Joi creates ACP sessions.",
    )
}

fn responds_to(event_id: &str) -> Relation {
    Relation {
        kind: RelationKind::RespondsTo,
        target: Ref {
            kind: RefKind::Event,
            id: event_id.to_string(),
            _meta: None,
        },
        _meta: None,
    }
}

async fn handle_handoff(
    client: &Arc<Client>,
    state: &Arc<WorkerState>,
    adapter: &Arc<dyn Adapter>,
    trigger: &Event,
) -> Result<()> {
    // Same-scope FIFO: if the scope already has a turn in flight, queue this
    // trigger and let translate_one pick it up after `Finished`.
    if state.current_turn(&trigger.scope.id).is_some() {
        state.enqueue(&trigger.scope.id, trigger.clone());
        return Ok(());
    }
    dispatch_handoff(client, state, adapter, trigger.clone()).await
}

/// Open a turn, mark the scope busy, send the prompt to the adapter. Used by
/// both the initial hand-off and the Finished handler when it pops the next
/// queued trigger. On `send_prompt` failure we iteratively drain the queue
/// (rather than spawn-recursing) so a single bad prompt can't strand the rest
/// and the future stays Send for `tokio::spawn`.
async fn dispatch_handoff(
    client: &Arc<Client>,
    state: &Arc<WorkerState>,
    adapter: &Arc<dyn Adapter>,
    mut trigger: Event,
) -> Result<()> {
    loop {
        subscribe_scope(client, state, &trigger.scope).await;
        let turn_res: TurnOpenResult = client
            .call(
                method::TURN_OPEN,
                json!({
                    "actorId": state.actor_id,
                    "scope": trigger.scope,
                    "triggerEventId": trigger.id,
                }),
            )
            .await?;
        let user_text = render_prompt(&trigger);
        let prompt = compose_envelope_prompt(client, state, &trigger.scope, &user_text).await;
        let active = ActiveTurn {
            id: turn_res.turn.id.clone(),
            scope: trigger.scope.clone(),
            trigger_event_id: trigger.id.clone(),
            prompt_stats: prompt.stats.clone(),
            prompt_breakdown: prompt.breakdown.clone(),
            trigger_actor: trigger.actor_id.clone(),
            cancel_requested: false,
        };
        state.set_turn(active.clone());

        let adapter_prompt =
            build_adapter_prompt(client, state, &trigger.scope, prompt.content, Some(&active))
                .await?;

        match adapter.send_prompt(adapter_prompt).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                let _ = close_turn(client, &active.id, TurnStatus::Failed).await;
                let scope_id = trigger.scope.id.clone();
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
    content: String,
    active: Option<&ActiveTurn>,
) -> Result<AdapterPrompt> {
    let channel_id = resolve_channel_for_scope(client, state, scope)
        .await
        .ok_or_else(|| anyhow!("cannot resolve channel for scope {}", scope.id))?;
    let scope_paths = state
        .paths
        .ensure_scope(&state.actor_id, &channel_id, scope)?;
    Ok(AdapterPrompt {
        scope: scope.clone(),
        content,
        model: state.current_model(),
        cwd: scope_paths.workspace,
        env: state.paths.scope_env(
            &state.actor_id,
            &channel_id,
            scope,
            &state.agent_server_url,
            active,
        ),
        template_vars: state
            .paths
            .template_vars(&state.actor_id, &channel_id, scope),
    })
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

fn render_prompt(trigger: &Event) -> String {
    if let Some(text) = trigger.payload.get("text").and_then(|v| v.as_str()) {
        return text.to_string();
    }
    if let Some(text) = trigger.payload.get("message").and_then(|v| v.as_str()) {
        return text.to_string();
    }
    serde_json::to_string(&trigger.payload).unwrap_or_default()
}

/// Per-turn prompt composition for v1. Mirrors
/// `server::runtime::wakeup::compose_envelope_prompt` — agents that don't
/// configure identity / memory fall back to the pre-envelope shape.
async fn compose_envelope_prompt(
    client: &Arc<Client>,
    state: &Arc<WorkerState>,
    scope: &ScopeRef,
    user_text: &str,
) -> PromptTelemetry {
    let scope_bootstrap =
        if state.take_seed_slot(&scope.id) || command_transport_without_resume(&state.spec) {
            seed_manifest(&state.actor_id, scope)
        } else {
            String::new()
        };

    let identity_spec = state.spec.identity.as_ref();
    let memory_spec = state.spec.memory.as_ref();

    if identity_spec.is_none() && memory_spec.is_none() {
        let sections = if scope_bootstrap.is_empty() {
            vec![agent_runtime::PromptSection {
                name: "user_message",
                content: user_text.to_string(),
            }]
        } else {
            vec![
                agent_runtime::PromptSection {
                    name: "scope_bootstrap",
                    content: scope_bootstrap.clone(),
                },
                agent_runtime::PromptSection {
                    name: "user_message",
                    content: format!("=== User message ===\n{user_text}"),
                },
            ]
        };
        let content = sections
            .iter()
            .map(|section| section.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        return prompt_telemetry(content, &sections);
    }

    let channel_id = resolve_channel_for_scope(client, state, scope).await;

    let (prompt, sections) =
        agent_runtime::envelope::build_envelope(&agent_runtime::envelope::BuildContext {
            profile_dir: &state.profile_dir,
            identity_spec,
            memory_spec,
            channel_id: channel_id.as_deref(),
            thread_context: "",
            user_message: user_text,
            scope_bootstrap: &scope_bootstrap,
        });
    prompt_telemetry(prompt, &sections)
}

fn prompt_telemetry(content: String, sections: &[agent_runtime::PromptSection]) -> PromptTelemetry {
    let stats = prompt_stats(&content);
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
    let total_tokens = breakdown_sections
        .iter()
        .map(|section| section.approx_token_count)
        .sum::<u64>()
        .max(1) as f64;
    for section in &mut breakdown_sections {
        section.percentage = (section.approx_token_count as f64 / total_tokens) * 100.0;
    }
    PromptTelemetry {
        content,
        stats,
        breakdown: PromptBreakdown {
            sections: breakdown_sections,
        },
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
        "identity" => "Identity",
        "soul" => "Soul",
        "bootstrap_memory" => "Bootstrap Memory",
        "turn_memory" => "Turn Memory",
        "scope_bootstrap" => "Scope Bootstrap",
        "user_message" => "Latest Message",
        other => other,
    }
}

fn command_transport_without_resume(spec: &AgentSpec) -> bool {
    if spec.transport.kind != "command" {
        return false;
    }
    match spec.transport.session.as_ref() {
        Some(session) => session.first_run_capture.is_none() || session.resume_args.is_none(),
        None => true,
    }
}

/// Resolve a scope → channel_id. Channel scopes are identity — they are the
/// channel. Thread scopes need a one-time `thread/list` sweep; the result is
/// cached on `WorkerState` so we don't hit the server per turn. A lookup
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
            let res: proto::methods::ThreadListResult =
                client.call(method::THREAD_LIST, json!({})).await.ok()?;
            let mut cache = state.scope_channel_cache.lock().ok()?;
            let mut found: Option<String> = None;
            for t in res.threads {
                if t.id == scope.id {
                    found = Some(t.channel_id.clone());
                }
                cache.insert(t.id, t.channel_id);
            }
            found
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
    let scope_flag = if matches!(scope.kind, ScopeKind::Channel) {
        " --channel"
    } else {
        ""
    };
    format!(
        "=== System: Joi multi-actor context (auto-injected on session start) ===\n\
         You are an agent driven by `joi daemon`.\n\
         Identity:\n\
           actor id      = {actor_id}\n\
           current scope = {scope_kind}:{scope_id}\n\
         \n\
         You can shell out to the `joi` CLI for server access. JOI_SERVER,\n\
         JOI_DAEMON_SOCKET, JOI_ACTOR, JOI_SCOPE_ID, JOI_SCOPE_KIND, JOI_TURN_ID, and JOI_TRIGGER_ACTOR are already injected into your env,\n\
         so commands like:\n\
           joi --json event list --in {scope_id}{scope_flag}\n\
           joi --json artifact get <art_id|artifact://...>\n\
           joi --json ask-user-question --title \"Choose option\" --question \"Which option?\" --choice a=A --choice b=B\n\
           joi --json request-approval --title \"Approval required\" --reason \"Run the deploy command\"\n\
         `joi ask-user-question` is for choices or missing input; its JSON\n\
         output is the human's answer to your question, not an approval.\n\
         `joi request-approval` is for approve/reject gates before risky work.\n\
         continue the current task using `answer.optionId`, `answer.label`, or\n\
         `answer.text`, and phrase follow-up messages as the user's answer.\n\
         Message targets use `#<channel_id>` for channels and\n\
         `#<channel_id>:<root_event_id>` for threads; use\n\
         `joi --json thread list` to map a thread scope id to that target.\n\
         Use `--json` for machine-readable output and `joi <subcommand> --help`\n\
         for the full surface. Only the message after the marker line is the new\n\
         user input.\n\
         ",
        actor_id = actor_id,
        scope_kind = scope_kind,
        scope_id = scope.id,
        scope_flag = scope_flag,
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
            is_partial: _,
        } => {
            let Some(active) = active else {
                tracing::warn!(actor = %actor_id, "Text event without matching active turn; dropping");
                return Ok(());
            };
            if active.cancel_requested {
                return Ok(());
            }
            state.push_text(&active.id, &content);
            append_trace(
                client,
                &active.id,
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
                &active.id,
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
            // `joi action accept/decline`) and remember the ACP request id.
            // The server reverse-delivers the eventual action.response back
            // to this agent connection through actor-inbox fanout.
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
            let relations = vec![Relation {
                kind: RelationKind::HandsOffTo,
                target: Ref {
                    kind: RefKind::Actor,
                    id: active.trigger_actor.clone(),
                    _meta: None,
                },
                _meta: None,
            }];
            let appended = append_event(
                client,
                "action.request",
                actor_id,
                &active.scope,
                Some(&active.id),
                payload,
                relations,
                None,
            )
            .await?;
            state.record_action_request(appended.event.id.clone(), id.clone());
            eprintln!(
                "[{actor_id}] action.request {} -> trigger {} (ACP request {})",
                appended.event.id, active.trigger_actor, id
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
                    &active.id,
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
            if active.cancel_requested {
                let _ = state.take_text(&active.id);
            } else if let Some(text) = state.take_text(&active.id) {
                let meta = build_turn_meta(state, &active, usage.as_ref(), &text);
                flush_text(
                    client,
                    actor_id,
                    &active.scope,
                    &active.id,
                    &active.trigger_event_id,
                    text,
                    Some(meta),
                )
                .await?;
            } else if !success {
                if let Some(text) = failed_turn_text(&summary) {
                    let meta = build_turn_meta(state, &active, usage.as_ref(), &text);
                    flush_text(
                        client,
                        actor_id,
                        &active.scope,
                        &active.id,
                        &active.trigger_event_id,
                        text,
                        Some(meta),
                    )
                    .await?;
                }
            }
            if !active.cancel_requested {
                let status = if success {
                    TurnStatus::Closed
                } else {
                    TurnStatus::Failed
                };
                let _ = append_event(
                    client,
                    "turn.close",
                    actor_id,
                    &active.scope,
                    Some(&active.id),
                    json!({
                        "status": format!("{:?}", status).to_lowercase(),
                        "stopReason": summary,
                    }),
                    vec![],
                    None,
                )
                .await;
                if let Err(e) = close_turn(client, &active.id, status).await {
                    tracing::warn!(
                        actor = %actor_id,
                        turn = %active.id,
                        scope = %active.scope.id,
                        %e,
                        "close_turn RPC failed; clearing slot anyway so the queue can drain"
                    );
                }
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
                if let Err(e) = dispatch_handoff(client, state, adapter, next).await {
                    eprintln!("[{actor_id}] failed to dispatch queued trigger: {e}");
                }
            }
        }
        AdapterEvent::Error { scope: _, message } => {
            if let Some(active) = active {
                append_trace(
                    client,
                    &active.id,
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

    let mut meta = Meta::new();
    meta.insert(
        "prompt_stats".into(),
        serde_json::to_value(&active.prompt_stats).unwrap_or(Value::Null),
    );
    meta.insert(
        "prompt_breakdown".into(),
        serde_json::to_value(&active.prompt_breakdown).unwrap_or(Value::Null),
    );
    meta.insert(
        "token_usage".into(),
        serde_json::to_value(&usage_meta).unwrap_or(Value::Null),
    );
    meta
}

async fn append_trace(
    client: &Arc<Client>,
    turn_id: &str,
    kind: TraceKind,
    payload: Value,
) -> Result<()> {
    let _: Value = client
        .call(
            method::TURN_TRACE_APPEND,
            json!({ "turnId": turn_id, "kind": kind, "payload": payload }),
        )
        .await
        .with_context(|| format!("turn/trace.append turn={turn_id}"))?;
    Ok(())
}

async fn append_event(
    client: &Arc<Client>,
    kind: &str,
    actor_id: &str,
    scope: &ScopeRef,
    turn_id: Option<&str>,
    payload: Value,
    relations: Vec<Relation>,
    meta: Option<Meta>,
) -> Result<EventAppendResult> {
    let mut input = json!({
        "type": kind,
        "actorId": actor_id,
        "scope": scope,
        "payload": payload,
        "relations": relations,
    });
    if let Some(t) = turn_id {
        input["turnId"] = json!(t);
    }
    if let Some(meta) = meta {
        input["_meta"] = serde_json::to_value(meta)?;
    }
    let res: EventAppendResult = client
        .call(method::EVENT_APPEND, json!({ "event": input }))
        .await
        .with_context(|| format!("event/append kind={kind}"))?;
    Ok(res)
}

async fn flush_text(
    client: &Arc<Client>,
    actor_id: &str,
    scope: &ScopeRef,
    turn_id: &str,
    trigger_event_id: &str,
    text: String,
    meta: Option<Meta>,
) -> Result<()> {
    let relations = if trigger_event_id.is_empty() {
        vec![]
    } else {
        vec![Relation {
            kind: RelationKind::RespondsTo,
            target: Ref {
                kind: RefKind::Event,
                id: trigger_event_id.to_string(),
                _meta: None,
            },
            _meta: None,
        }]
    };
    append_event(
        client,
        "content.add",
        actor_id,
        scope,
        Some(turn_id),
        json!({ "contentType": "text/markdown", "text": text }),
        relations,
        meta,
    )
    .await
    .map(|_| ())
    .map_err(|e| {
        tracing::warn!(
            actor = %actor_id,
            turn = %turn_id,
            scope = %scope.id,
            %e,
            "content.add failed"
        );
        e
    })
}

async fn close_turn(client: &Arc<Client>, turn_id: &str, status: TurnStatus) -> Result<()> {
    let _: Value = client
        .call(
            method::TURN_CLOSE,
            json!({ "turnId": turn_id, "status": status }),
        )
        .await
        .with_context(|| format!("turn/close turn={turn_id}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proto::methods::{AgentBundleSpec, AgentModelChoice, AgentModelSpec, AgentTransport};
    use proto::types::{Actor, ActorKind};

    fn sample_spec(bundle: Option<AgentBundleSpec>) -> AgentSpec {
        AgentSpec {
            actor: Actor {
                id: "actor_demo".into(),
                kind: ActorKind::Agent,
                display_name: "Demo".into(),
                capabilities: None,
                _meta: None,
            },
            transport: AgentTransport {
                kind: "command".into(),
                command: "echo".into(),
                args: Vec::new(),
                env: std::collections::BTreeMap::new(),
                auth_method: None,
                model: None,
                session: None,
                output_format: None,
                prompt_via: proto::methods::PromptVia::default(),
                interactive: None,
                provider: None,
            },
            autostart: false,
            models: None,
            bundle,
            identity: None,
            memory: None,
            announcement: None,
        }
    }

    fn temp_path(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "joi-agent-serve-tests-{name}-{}",
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
            scope: scope.clone(),
            trigger_event_id: "evt_trigger".into(),
            prompt_stats: empty_prompt_stats(),
            prompt_breakdown: empty_prompt_breakdown(),
            trigger_actor: "human_alice".into(),
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
            env.get("JOI_SCOPE_ID").map(String::as_str),
            Some("chan_demo")
        );
        assert_eq!(
            env.get("JOI_SCOPE_KIND").map(String::as_str),
            Some("channel")
        );
        assert_eq!(
            env.get("AGENTX_CHANNEL_ID").map(String::as_str),
            Some("chan_demo")
        );
        assert_eq!(
            env.get("JOI_TURN_ID").map(String::as_str),
            Some("turn_demo")
        );
        assert_eq!(
            env.get("JOI_TRIGGER_ACTOR").map(String::as_str),
            Some("human_alice")
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn command_transport_without_resume_tracks_session_capability() {
        let no_session = sample_spec(None);
        assert!(command_transport_without_resume(&no_session));

        let mut resumable = sample_spec(None);
        resumable.transport.session = Some(proto::methods::CommandSession {
            first_run_capture: Some("stdout_json:.session_id".into()),
            resume_args: Some(vec!["--resume".into(), "{session_id}".into(), "-p".into()]),
        });
        assert!(!command_transport_without_resume(&resumable));

        let mut acp = sample_spec(None);
        acp.transport.kind = "acp_stdio".into();
        assert!(!command_transport_without_resume(&acp));
    }

    #[test]
    fn joi_human_interaction_request_ids_are_tool_local() {
        assert!(is_joi_tool_request_id("joi:question:abc"));
        assert!(is_joi_tool_request_id("joi:approval:abc"));
        assert!(!is_joi_tool_request_id("joi:model:abc"));
        assert!(!is_joi_tool_request_id("acp:permission:abc"));
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
            scope: scope.clone(),
            trigger_event_id: "evt_1".into(),
            prompt_stats: empty_prompt_stats(),
            prompt_breakdown: empty_prompt_breakdown(),
            trigger_actor: "actor_human".into(),
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
        spec.transport.kind = "acp_stdio".into();
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
}
