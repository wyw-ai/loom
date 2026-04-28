//! `joi agent serve` — external agent runtime client (v1 phase E3c).
//!
//! Loads `AgentSpec` JSON files from `~/.config/joi/agents/` (override with
//! `--specs`), and for each spec opens a dedicated WebSocket to the joi server
//! and supervises that one agent through the same `agent-runtime` adapter trait
//! used by ACP/command transports.
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
    method, stream_kind, AgentSpec, BundleInstallMode, EventAppendResult, TurnOpenResult,
};
use proto::types::trace::TraceKind;
use proto::types::{Event, Ref, RefKind, Relation, RelationKind, ScopeKind, ScopeRef, TurnStatus};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TryRecvError;

use agent_runtime::acp::{AcpAdapter, AcpConfig};
use agent_runtime::command::{CommandAdapter, CommandConfig};
use agent_runtime::{
    agent_child_server_url, prepare_bundle_install, resolved_bundle_version,
    validate_bundle_current, Adapter, AdapterEvent, AdapterPrompt,
};

use crate::client::Client;

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
        let (kept, skipped): (Vec<_>, Vec<_>) = specs
            .into_iter()
            .partition(|s| allow.contains(s.actor.id.as_str()));
        specs = kept;
        let skipped_ids: Vec<&str> = skipped.iter().map(|s| s.actor.id.as_str()).collect();
        let unknown: Vec<&str> = allow_actors
            .iter()
            .map(String::as_str)
            .filter(|id| !specs.iter().any(|s| s.actor.id == *id))
            .collect();
        eprintln!(
            "joi agent serve: --allow-actors filter active; loaded {} of {} agent(s){}{}",
            specs.len(),
            total_loaded,
            if skipped_ids.is_empty() {
                String::new()
            } else {
                format!("; skipped: {}", skipped_ids.join(","))
            },
            if unknown.is_empty() {
                String::new()
            } else {
                format!(
                    "; warning: requested ids with no matching spec: {}",
                    unknown.join(",")
                )
            },
        );
    }
    if specs.is_empty() {
        return Err(anyhow!(
            "no agent specs to serve under {} (loaded {}, after --allow-actors filter: 0)",
            specs_dir.display(),
            total_loaded,
        ));
    }

    eprintln!(
        "joi agent serve: loaded {} agent(s) from {}",
        specs.len(),
        specs_dir.display()
    );
    let mut handles = Vec::new();
    for spec in specs {
        let server = server_url.clone();
        let root = data_root.clone();
        let actor = spec.actor.id.clone();
        handles.push(tokio::spawn(async move {
            if let Err(e) = run_agent_worker(spec, server, root).await {
                eprintln!("[{actor}] worker exited with error: {e}");
            }
        }));
    }
    eprintln!("joi agent serve: ready (ctrl-c to stop)");
    let _ = tokio::signal::ctrl_c().await;
    eprintln!("\njoi agent serve: shutting down");
    for h in handles {
        h.abort();
    }
    Ok(())
}

/// The path to *this* joi binary. Used as the `command` for the
/// auto-injected `joi-memory` MCP server entry; falls back to the bare
/// name `"joi"` (hoping it's on PATH) if we can't resolve our own exe.
fn current_joi_binary() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .or_else(|| Some(PathBuf::from("joi")))
}

fn default_specs_dir() -> PathBuf {
    dirs::config_dir()
        .map(|d| d.join("joi").join("agents"))
        .unwrap_or_else(|| PathBuf::from(".joi").join("agents"))
}

fn default_data_root() -> PathBuf {
    for key in ["JOI_AGENT_DATA_ROOT", "AGENTHUB_HOME", "AGENTX_HOME"] {
        if let Some(value) = std::env::var_os(key) {
            if !value.is_empty() {
                return PathBuf::from(value);
            }
        }
    }
    dirs::home_dir()
        .map(|d| d.join(".agentx"))
        .unwrap_or_else(|| PathBuf::from(".agentx"))
}

fn load_specs(dir: &Path) -> Result<Vec<AgentSpec>> {
    let mut out = Vec::new();
    for entry in
        std::fs::read_dir(dir).with_context(|| format!("read specs dir {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let text =
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        warn_deprecated_transport_fields(&text, &path);
        match serde_json::from_str::<AgentSpec>(&text) {
            Ok(spec) => out.push(spec),
            Err(e) => eprintln!("[warn] skipping {}: {}", path.display(), e),
        }
    }
    Ok(out)
}

fn warn_deprecated_transport_fields(text: &str, path: &Path) {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return;
    };
    if value
        .get("transport")
        .and_then(|transport| transport.get("cwd"))
        .is_some()
    {
        eprintln!(
            "[warn] {}: transport.cwd is ignored; Joi computes ACP session cwd \
             from the target channel/thread workspace",
            path.display()
        );
    }

    let transport = value
        .get("transport")
        .and_then(|transport| transport.as_object());
    let command = transport
        .and_then(|transport| transport.get("command"))
        .and_then(|command| command.as_str());
    const ZED_QODERCLI_ACP_VERSION: &str = "0.1.48";
    let qodercli_pin = transport
        .and_then(|transport| transport.get("args"))
        .and_then(|args| args.as_array())
        .into_iter()
        .flatten()
        .filter_map(|arg| arg.as_str())
        .find_map(|arg| arg.strip_prefix("@qoder-ai/qodercli@"));
    if command == Some("npx") && qodercli_pin.is_some_and(|pin| pin != ZED_QODERCLI_ACP_VERSION) {
        eprintln!(
            "[warn] {}: pinned @qoder-ai/qodercli version differs from Zed \
             registry ({ZED_QODERCLI_ACP_VERSION}); Qoder ACP auth may fail \
             even when Zed works",
            path.display(),
        );
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
    data_root: PathBuf,
}

#[derive(Clone)]
struct ScopePaths {
    channel_root: PathBuf,
    channel_shared: PathBuf,
    channel_artifacts: PathBuf,
    agent_root: PathBuf,
    workspace: PathBuf,
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
        Self {
            profile: agent_root.join("profile"),
            bundle_root: agent_root.join("bundles"),
            bundle_current: agent_root.join("bundles").join("current"),
            root: agent_root,
            sessions: data_root.join("sessions"),
            data_root: data_root.to_path_buf(),
        }
    }

    fn scope(&self, actor_id: &str, channel_id: &str) -> ScopePaths {
        let channel_root = self.data_root.join("channels").join(channel_id);
        let channel_shared = channel_root.join("shared");
        let channel_artifacts = channel_shared.join("artifacts");
        let agent_root = channel_root.join("agents").join(actor_id);
        ScopePaths {
            channel_root,
            channel_shared,
            channel_artifacts,
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
            let identity_file = spec
                .identity
                .as_ref()
                .map(|s| s.files.identity.as_str())
                .unwrap_or("identity.md");
            let soul_file = spec
                .identity
                .as_ref()
                .map(|s| s.files.soul.as_str())
                .unwrap_or("soul.md");
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
                    description: "",
                    identity_file,
                    soul_file,
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

    fn ensure_scope(&self, actor_id: &str, channel_id: &str) -> std::io::Result<ScopePaths> {
        let scope = self.scope(actor_id, channel_id);
        std::fs::create_dir_all(&scope.workspace)?;
        std::fs::create_dir_all(&scope.logs)?;
        std::fs::create_dir_all(&scope.channel_artifacts)?;
        agent_runtime::ensure_agents_md(&scope.workspace, actor_id)?;
        Ok(scope)
    }

    fn template_vars(&self, actor_id: &str, channel_id: &str) -> BTreeMap<String, String> {
        let scope = self.scope(actor_id, channel_id);
        let mut vars = BTreeMap::new();
        vars.insert(
            "agent.workspace".into(),
            scope.workspace.display().to_string(),
        );
        vars.insert("agent.root".into(), scope.agent_root.display().to_string());
        vars.insert("agent.profile".into(), self.profile.display().to_string());
        vars.insert("agent.logs".into(), scope.logs.display().to_string());
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
        server_url: &str,
    ) -> BTreeMap<String, String> {
        let scope = self.scope(actor_id, channel_id);
        let mut env = BTreeMap::new();
        env.insert("JOI_SERVER".into(), server_url.to_string());
        env.insert("JOI_ACTOR".into(), actor_id.to_string());
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
        env
    }
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
    /// `Finished` (and on any non-partial `Text` chunk).
    text_buffer: Mutex<HashMap<String, String>>,
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
}

#[derive(Clone)]
struct ActiveTurn {
    id: String,
    scope: ScopeRef,
    trigger_event_id: String,
    /// Actor that triggered the current turn — needed when emitting a
    /// `action.request` so we can hand the choice back to them.
    trigger_actor: String,
    /// Set after a human cancels the turn. The server has already closed the
    /// turn, but we keep this slot occupied until the adapter's eventual
    /// Finished(cancelled) arrives so that stale completion cannot close the
    /// next turn in the same scope.
    cancel_requested: bool,
}

impl WorkerState {
    fn new(
        actor_id: String,
        spec: AgentSpec,
        profile_dir: PathBuf,
        paths: AgentPaths,
        agent_server_url: String,
    ) -> Self {
        Self {
            actor_id,
            spec,
            profile_dir,
            paths,
            agent_server_url,
            active_turns: Mutex::new(HashMap::new()),
            pending_triggers: Mutex::new(HashMap::new()),
            text_buffer: Mutex::new(HashMap::new()),
            seeded: Mutex::new(HashSet::new()),
            scope_channel_cache: Mutex::new(HashMap::new()),
            seen_events: Mutex::new(HashSet::new()),
            action_map: Mutex::new(HashMap::new()),
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

    fn remember_event(&self, event_id: &str) -> bool {
        self.seen_events
            .lock()
            .expect("seen_events poisoned")
            .insert(event_id.to_string())
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
    client
        .open_connection_as(&actor_id, "agent", Some(&display_name))
        .await?;
    eprintln!("[{actor_id}] connected to {server_url} as agent");

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

    notification_loop(client, state, adapter, event_tx, &actor_id).await
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
            if let Err(e) = handle_action_response(&state, &adapter, &event).await {
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
        let request_id = match state.lookup_action_request(request_event_id) {
            Some(id) => id,
            None => match action_request_id_from_response(event) {
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
                         (agent serve may have restarted after the action.request)",
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
        let active = ActiveTurn {
            id: turn_res.turn.id.clone(),
            scope: trigger.scope.clone(),
            trigger_event_id: trigger.id.clone(),
            trigger_actor: trigger.actor_id.clone(),
            cancel_requested: false,
        };
        state.set_turn(active.clone());

        let user_text = render_prompt(&trigger);
        let prompt = compose_envelope_prompt(client, state, &trigger.scope, &user_text).await;
        let channel_id = resolve_channel_for_scope(client, state, &trigger.scope)
            .await
            .ok_or_else(|| anyhow!("cannot resolve channel for scope {}", trigger.scope.id))?;
        let scope_paths = state.paths.ensure_scope(&state.actor_id, &channel_id)?;
        let adapter_prompt = AdapterPrompt {
            scope: trigger.scope.clone(),
            content: prompt,
            cwd: scope_paths.workspace,
            env: state
                .paths
                .scope_env(&state.actor_id, &channel_id, &state.agent_server_url),
            template_vars: state.paths.template_vars(&state.actor_id, &channel_id),
        };

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
) -> String {
    let scope_bootstrap = if state.take_seed_slot(&scope.id) {
        seed_manifest(&state.actor_id, scope)
    } else {
        String::new()
    };

    let identity_spec = state.spec.identity.as_ref();
    let memory_spec = state.spec.memory.as_ref();

    if identity_spec.is_none() && memory_spec.is_none() {
        return if scope_bootstrap.is_empty() {
            user_text.to_string()
        } else {
            format!("{scope_bootstrap}\n\n=== User message ===\n{user_text}")
        };
    }

    let channel_id = resolve_channel_for_scope(client, state, scope).await;

    let (prompt, _sections) =
        agent_runtime::envelope::build_envelope(&agent_runtime::envelope::BuildContext {
            profile_dir: &state.profile_dir,
            identity_spec,
            memory_spec,
            channel_id: channel_id.as_deref(),
            thread_context: "",
            user_message: user_text,
            scope_bootstrap: &scope_bootstrap,
        });
    prompt
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
         You are an agent driven by `joi agent serve`.\n\
         Identity:\n\
           actor id      = {actor_id}\n\
           current scope = {scope_kind}:{scope_id}\n\
         \n\
         You can shell out to the `joi` CLI for read-only access. JOI_SERVER\n\
         and JOI_ACTOR are already injected into your env, so commands like:\n\
           joi --json event list --in {scope_id}{scope_flag}\n\
           joi --json artifact get <art_id|artifact://...>\n\
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
            is_partial,
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
            if !is_partial {
                if let Some(text) = state.take_text(&active.id) {
                    flush_text(
                        client,
                        actor_id,
                        &active.scope,
                        &active.id,
                        &active.trigger_event_id,
                        text,
                    )
                    .await?;
                }
            }
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
                flush_text(
                    client,
                    actor_id,
                    &active.scope,
                    &active.id,
                    &active.trigger_event_id,
                    text,
                )
                .await?;
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
    use proto::methods::{AgentBundleSpec, AgentTransport};
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
                session: None,
                output_format: None,
                prompt_via: proto::methods::PromptVia::default(),
            },
            autostart: false,
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
}
