//! `joi agent serve` — external agent runtime client (v1 phase E3c).
//!
//! Loads `AgentSpec` JSON files from `~/.config/joi/agents/` (override with
//! `--specs`), and for each spec opens a dedicated WebSocket to the joi server
//! and supervises that one agent through the same `agent-runtime` adapter trait
//! the embedded supervisor uses.
//!
//! Architecture (per docs/architecture-v1-agent-client.md §6):
//!   * one tokio task per agent ⇒ one `Client` ⇒ one WS frame to the server
//!   * `connection/open` with `actorKind = "agent"` binds the connection to the
//!     agent's actor id; the server's actor-inbox delivery (see
//!     `crates/server/src/ws.rs::fanout`) then pushes every `HandsOffTo`-targeted
//!     `event.created` straight to this connection — no scope/subscribe needed
//!   * a notification loop turns those events into `Adapter::send_prompt` calls,
//!     opening / tracking a turn through `turn/open` + `turn/close`
//!   * a translator task drains `AdapterEvent`s and re-emits them as
//!     `event/append` (public content) + `turn/trace.append` (owner-only trace)
//!     RPCs — mirroring v0's `runtime::wakeup::translate_event` translation
//!     table 1:1, just with RPC instead of in-process store calls
//!
//! Pair this with the server flag `JOI_DISABLE_EMBEDDED_RUNTIME=1` to opt the
//! server out of embedded supervision so v1 is the only path. Without that flag
//! both the embedded supervisor AND `joi agent serve` will try to drive the same
//! agent — racing on `turn/open` and double-flushing text.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Result};
use proto::methods::{
    method, stream_kind, AgentBundleSpec, AgentSpec, BundleInstallMode, EventAppendResult,
    TurnOpenResult,
};
use proto::types::trace::TraceKind;
use proto::types::{Event, Ref, RefKind, Relation, RelationKind, ScopeKind, ScopeRef, TurnStatus};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TryRecvError;

use agent_runtime::acp::{AcpAdapter, AcpConfig};
use agent_runtime::command::{CommandAdapter, CommandConfig};
use agent_runtime::{Adapter, AdapterEvent};

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
    dirs::data_local_dir()
        .map(|d| d.join("joi").join("agent-client"))
        .unwrap_or_else(|| PathBuf::from(".joi").join("agent-client"))
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
        match serde_json::from_str::<AgentSpec>(&text) {
            Ok(spec) => out.push(spec),
            Err(e) => eprintln!("[warn] skipping {}: {}", path.display(), e),
        }
    }
    Ok(out)
}

/// Per-agent paths under the agent-client data root. Keeps templating
/// (`{agent.workspace}` etc.) consistent between embedded and external mode.
struct AgentPaths {
    workspace: PathBuf,
    profile: PathBuf,
    logs: PathBuf,
    root: PathBuf,
    bundle_root: PathBuf,
    bundle_current: PathBuf,
    sessions: PathBuf,
}

impl AgentPaths {
    fn new(root: &Path, actor_id: &str) -> Self {
        let agent_root = root.join("agents").join(actor_id);
        Self {
            workspace: agent_root.join("workspace"),
            profile: agent_root.join("profile"),
            logs: agent_root.join("logs"),
            bundle_root: agent_root.join("bundles"),
            bundle_current: agent_root.join("bundles").join("current"),
            root: agent_root,
            sessions: root.join("sessions"),
        }
    }

    fn ensure(&self, actor_id: &str, spec: &AgentSpec) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.workspace)?;
        std::fs::create_dir_all(&self.profile)?;
        std::fs::create_dir_all(&self.logs)?;
        std::fs::create_dir_all(&self.sessions)?;
        ensure_bundle(actor_id, spec, self)?;
        if let Err(e) = agent_runtime::ensure_agents_md(&self.workspace, actor_id) {
            tracing::warn!(actor = %actor_id, %e, "failed to write AGENTS.md");
        }
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

    fn expand(&self, input: &str) -> String {
        input
            .replace("{agent.workspace}", &self.workspace.display().to_string())
            .replace("{agent.profile}", &self.profile.display().to_string())
            .replace("{agent.logs}", &self.logs.display().to_string())
            .replace("{agent.root}", &self.root.display().to_string())
            .replace("{agent.home}", &self.root.display().to_string())
            .replace(
                "{agent.bundle_root}",
                &self.bundle_root.display().to_string(),
            )
            .replace("{agent.bundle}", &self.bundle_current.display().to_string())
    }
}

fn ensure_bundle(_actor_id: &str, spec: &AgentSpec, paths: &AgentPaths) -> std::io::Result<()> {
    std::fs::create_dir_all(&paths.bundle_root)?;
    let Some(bundle) = spec.bundle.as_ref() else {
        std::fs::create_dir_all(&paths.bundle_current)?;
        return Ok(());
    };
    if bundle.source.trim().is_empty() {
        std::fs::create_dir_all(&paths.bundle_current)?;
        return Ok(());
    }
    let source = PathBuf::from(paths.expand(&bundle.source));
    let version = normalized_bundle_version(bundle, &source);
    let install_dir = paths.bundle_root.join(&version);
    install_bundle_dir(&source, &install_dir, bundle.install_mode)?;
    link_current_bundle(&install_dir, &paths.bundle_current)?;
    Ok(())
}

fn normalized_bundle_version(bundle: &AgentBundleSpec, source: &Path) -> String {
    let raw = if !bundle.version.trim().is_empty() {
        bundle.version.trim().to_string()
    } else {
        source
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("bundle")
            .to_string()
    };
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
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
}

#[derive(Clone)]
struct ActiveTurn {
    id: String,
    scope: ScopeRef,
    /// Actor that triggered the current turn — needed when emitting a
    /// `action.request` so we can hand the choice back to them.
    trigger_actor: String,
}

impl WorkerState {
    fn new(actor_id: String, spec: AgentSpec, profile_dir: PathBuf) -> Self {
        Self {
            actor_id,
            spec,
            profile_dir,
            active_turns: Mutex::new(HashMap::new()),
            pending_triggers: Mutex::new(HashMap::new()),
            text_buffer: Mutex::new(HashMap::new()),
            seeded: Mutex::new(HashSet::new()),
            scope_channel_cache: Mutex::new(HashMap::new()),
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
}

async fn run_agent_worker(spec: AgentSpec, server_url: String, data_root: PathBuf) -> Result<()> {
    let actor_id = spec.actor.id.clone();
    let display_name = if spec.actor.display_name.is_empty() {
        actor_id.clone()
    } else {
        spec.actor.display_name.clone()
    };
    let paths = AgentPaths::new(&data_root, &actor_id);
    paths.ensure(&actor_id, &spec)?;

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

    let state = Arc::new(WorkerState::new(
        actor_id.clone(),
        spec.clone(),
        paths.profile.clone(),
    ));
    let (event_tx, event_rx) = mpsc::unbounded_channel::<AdapterEvent>();
    let adapter = build_adapter(&spec, &paths, &server_url)?;

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
    server_url: &str,
) -> Result<Arc<dyn Adapter>> {
    let workdir = if spec.transport.cwd.is_empty() {
        paths.workspace.clone()
    } else {
        PathBuf::from(paths.expand(&spec.transport.cwd))
    };
    let mut env: std::collections::BTreeMap<String, String> = spec
        .transport
        .env
        .iter()
        .map(|(k, v)| (k.clone(), paths.expand(v)))
        .collect();
    // Mirror the embedded runtime's auto-injection so an agent that shells
    // out to `joi --json ...` from inside a tool call always knows where the
    // server lives and which actor it speaks as.
    env.entry("JOI_SERVER".into())
        .or_insert_with(|| server_url.to_string());
    env.entry("JOI_ACTOR".into())
        .or_insert_with(|| spec.actor.id.clone());
    env.entry("JOI_AGENT_ROOT".into())
        .or_insert_with(|| paths.root.display().to_string());
    env.entry("JOI_ACTOR_HOME".into())
        .or_insert_with(|| paths.root.display().to_string());
    env.entry("JOI_AGENT_WORKSPACE".into())
        .or_insert_with(|| paths.workspace.display().to_string());
    env.entry("JOI_AGENT_PROFILE".into())
        .or_insert_with(|| paths.profile.display().to_string());
    env.entry("JOI_AGENT_LOGS".into())
        .or_insert_with(|| paths.logs.display().to_string());
    env.entry("JOI_AGENT_BUNDLE_ROOT".into())
        .or_insert_with(|| paths.bundle_root.display().to_string());
    env.entry("JOI_AGENT_BUNDLE_DIR".into())
        .or_insert_with(|| paths.bundle_current.display().to_string());
    if let Some(bundle) = spec.bundle.as_ref() {
        if !bundle.version.trim().is_empty() {
            let version =
                normalized_bundle_version(bundle, &PathBuf::from(paths.expand(&bundle.source)));
            env.entry("JOI_AGENT_BUNDLE_VERSION".into())
                .or_insert(version);
        }
    }
    let args: Vec<String> = spec
        .transport
        .args
        .iter()
        .map(|a| paths.expand(a))
        .collect();

    match spec.transport.kind.as_str() {
        "acp_stdio" | "" => {
            let joi_binary = current_joi_binary();
            let mcp_servers = agent_runtime::build_mcp_servers(
                joi_binary.as_deref(),
                &spec.actor.id,
                &paths.profile,
                spec.memory.as_ref(),
            );
            let cfg = AcpConfig {
                command: spec.transport.command.clone(),
                args,
                env,
                cwd: workdir,
                auth_method: spec.transport.auth_method.clone(),
                mcp_servers,
            };
            Ok(Arc::new(AcpAdapter::new(cfg)))
        }
        "command" => {
            let cfg = CommandConfig::from_transport(
                spec.actor.id.clone(),
                spec.transport.command.clone(),
                args,
                env,
                workdir,
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
        if !is_for_us(&event, actor_id) {
            continue;
        }

        // Lazy start the adapter on first hands_off_to event. Same shape as the
        // embedded `RuntimeManager::ensure_started` lifecycle.
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
            trigger_actor: trigger.actor_id.clone(),
        };
        state.set_turn(active.clone());

        let user_text = render_prompt(&trigger);
        let prompt = compose_envelope_prompt(client, state, &trigger.scope, &user_text).await;

        match adapter.send_prompt(trigger.scope.clone(), prompt).await {
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
                    flush_text(client, actor_id, &active.scope, &active.id, text).await?;
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
            id: _,
            request_type,
            title,
            description,
            choices,
        } => {
            let Some(active) = active else {
                tracing::warn!(actor = %actor_id, "ActionRequest event without matching active turn; dropping");
                return Ok(());
            };
            // For v1 MVP we surface the request to the trigger actor (so they
            // can `joi action accept/decline`), but the response routing back
            // into the adapter is not yet wired — `respond_action` plumbing
            // requires a separate channel from the server's action.response
            // handler back to this worker. Track as a known gap.
            let payload = json!({
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
            append_event(
                client,
                "action.request",
                actor_id,
                &active.scope,
                Some(&active.id),
                payload,
                relations,
            )
            .await?;
        }
        AdapterEvent::StatusChange { scope: _, status } => {
            // If the event is scope-tagged AND that scope has a live turn,
            // surface as a Status trace; otherwise just log it.
            if let Some(active) = active {
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
            if let Some(text) = state.take_text(&active.id) {
                flush_text(client, actor_id, &active.scope, &active.id, text).await?;
            }
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
    text: String,
) -> Result<()> {
    append_event(
        client,
        "content.add",
        actor_id,
        scope,
        Some(turn_id),
        json!({ "contentType": "text/markdown", "text": text }),
        vec![],
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
