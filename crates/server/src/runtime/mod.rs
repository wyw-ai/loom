// Adapter trait + ACP / command transports moved to the standalone
// `agent-runtime` crate so the v1 `joi agent serve` external client can reuse
// them. Re-exported here so existing in-server call sites
// (`runtime::acp`, `runtime::command`, `runtime::adapter`) keep compiling.
pub use agent_runtime::acp;
pub use agent_runtime::adapter;
pub use agent_runtime::command;

pub mod wakeup;

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use proto::methods::{AgentInfo, AgentSpec};
use proto::types::*;
use thiserror::Error;
use tokio::sync::mpsc;

use crate::store::{Store, StoreError};

use self::acp::AcpAdapter;
use self::adapter::{Adapter, AdapterEvent};
use self::command::{CommandAdapter, CommandConfig};

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("agent not found: {0}")]
    NotFound(String),
    #[error("not running")]
    NotRunning,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("store: {0}")]
    Store(#[from] StoreError),
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("acp: {0}")]
    Acp(String),
}

pub type RuntimeResult<T> = Result<T, RuntimeError>;

pub struct RegisteredAgent {
    pub spec: AgentSpec,
    pub adapter: Option<Arc<dyn Adapter>>,
    pub status: String,
    pub pid: Option<u32>,
    /// Adapter-reported session id from `start`. ACP returns `None` (sessions are
    /// allocated lazily per scope inside the adapter); command transport returns
    /// `Some("cmd:<actor>")`. Surfaced via `AgentInfo` for ops only.
    pub session_id: Option<String>,
    pub log: VecDeque<String>,
    /// Pending action.request -> adapter request id mapping (key = event id).
    /// The adapter handle is held so the action response can be routed even if
    /// the agent is unregistered/re-registered between request and response.
    pub action_map: HashMap<String, (Arc<dyn Adapter>, String)>,
    /// Currently active turn per scope. The same agent can be @-mentioned in
    /// multiple channels concurrently; each scope is its own conversation, so
    /// each gets its own turn slot keyed by `scope.id`.
    pub active_turns: HashMap<String, String>,
    /// Per-scope FIFO of triggers (chat events that wake this agent) that
    /// arrived while the scope was already in flight. Drained one at a time
    /// when the scope's current turn closes — same-scope back-to-back prompts
    /// are strictly serialized.
    pub pending_triggers: HashMap<String, VecDeque<Event>>,
    /// Per-scope first-prompt-seeded flag. Each scope's first prompt to a
    /// freshly-started agent gets the bootstrap manifest prepended; subsequent
    /// prompts in the same scope (or any prompt in any other scope after the
    /// agent has been seeded once) skip it.
    pub seeded: HashSet<String>,
    /// Per-turn streaming text buffer. Partial text chunks accumulate here and
    /// are flushed as a single `content.add` event when the turn closes; the
    /// raw chunks themselves are exposed as turn-private `text.delta` trace
    /// frames so the owner can see the cursor moving. Keyed by `turn_id` (which
    /// is itself unique per scope), so no scope axis needed here.
    pub text_buffer: HashMap<String, String>,
    /// Per-turn monotonic counter for `turn/stream.update` notifications.
    /// Lets clients detect dropped frames; ordering is undefined across turns.
    /// Cleared (along with `text_buffer`) when the agent stops.
    pub stream_seq: HashMap<String, u64>,
}

impl RegisteredAgent {
    fn new(spec: AgentSpec) -> Self {
        Self {
            spec,
            adapter: None,
            status: "stopped".into(),
            pid: None,
            session_id: None,
            log: VecDeque::new(),
            action_map: HashMap::new(),
            active_turns: HashMap::new(),
            pending_triggers: HashMap::new(),
            seeded: HashSet::new(),
            text_buffer: HashMap::new(),
            stream_seq: HashMap::new(),
        }
    }

    pub fn info(&self) -> AgentInfo {
        AgentInfo {
            spec: self.spec.clone(),
            status: self.status.clone(),
            pid: self.pid,
            session_id: self.session_id.clone(),
        }
    }
}

pub struct RuntimeManager {
    pub data_dir: PathBuf,
    pub agents_dir: PathBuf,
    /// WebSocket URL agents should hit when they shell out to `joi` (JOI_SERVER).
    pub server_url: String,
    /// Directory containing the `joi` CLI binary (typically the same dir as the
    /// running `joi-server` binary). Prepended to ACP child PATH on spawn so the
    /// agent can run `joi --json ...` without prior installation. `None` if not
    /// detected — the server logs a warning at boot in that case.
    pub cli_dir: Option<PathBuf>,
    agents: Mutex<HashMap<String, RegisteredAgent>>,
    store: Arc<Store>,
}

impl RuntimeManager {
    pub fn new(
        data_dir: PathBuf,
        agents_dir: PathBuf,
        store: Arc<Store>,
        server_url: String,
    ) -> RuntimeResult<Arc<Self>> {
        std::fs::create_dir_all(&data_dir)?;
        std::fs::create_dir_all(&agents_dir)?;
        let cli_dir = detect_cli_dir();
        if cli_dir.is_none() {
            tracing::warn!(
                "joi CLI not found next to joi-server; ACP children will not have `joi` on \
                 PATH unless the operator installs it (e.g. `cargo install --path crates/cli`).",
            );
        }
        let mgr = Arc::new(Self {
            data_dir,
            agents_dir,
            server_url,
            cli_dir,
            agents: Mutex::new(HashMap::new()),
            store,
        });
        mgr.load_disk_specs()?;
        Ok(mgr)
    }

    fn load_disk_specs(&self) -> RuntimeResult<()> {
        for entry in std::fs::read_dir(&self.agents_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            match self.load_spec_file(&path) {
                Ok(spec) => {
                    if let Err(e) = self.register_loaded(spec, false) {
                        tracing::warn!(?path, %e, "failed to register agent");
                    }
                }
                Err(e) => tracing::warn!(?path, %e, "failed to load agent spec"),
            }
        }
        Ok(())
    }

    fn load_spec_file(&self, path: &Path) -> RuntimeResult<AgentSpec> {
        let text = std::fs::read_to_string(path)?;
        let spec: AgentSpec = serde_json::from_str(&text)?;
        Ok(spec)
    }

    pub fn register(&self, spec: AgentSpec) -> RuntimeResult<AgentInfo> {
        let path = self.agents_dir.join(format!("{}.json", spec.actor.id));
        std::fs::write(&path, serde_json::to_string_pretty(&spec)?)?;
        self.register_loaded(spec, false)
    }

    fn register_loaded(&self, spec: AgentSpec, _was_running: bool) -> RuntimeResult<AgentInfo> {
        let actor = Actor {
            id: spec.actor.id.clone(),
            kind: ActorKind::Agent,
            display_name: spec.actor.display_name.clone(),
            capabilities: spec.actor.capabilities.clone(),
            _meta: spec.actor._meta.clone(),
        };
        let _ = self.store.upsert_actor(actor);
        let mut agents = self.agents.lock();
        let existing = agents.remove(&spec.actor.id);
        let mut entry = RegisteredAgent::new(spec.clone());
        if let Some(prev) = existing {
            entry.adapter = prev.adapter;
            entry.status = prev.status;
            entry.pid = prev.pid;
            entry.session_id = prev.session_id;
            entry.log = prev.log;
            entry.action_map = prev.action_map;
            entry.active_turns = prev.active_turns;
            entry.pending_triggers = prev.pending_triggers;
            entry.seeded = prev.seeded;
            entry.text_buffer = prev.text_buffer;
            entry.stream_seq = prev.stream_seq;
        }
        let info = entry.info();
        agents.insert(spec.actor.id.clone(), entry);
        Ok(info)
    }

    pub fn unregister(&self, actor_id: &str) -> RuntimeResult<()> {
        let path = self.agents_dir.join(format!("{}.json", actor_id));
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        let mut agents = self.agents.lock();
        agents.remove(actor_id);
        Ok(())
    }

    pub fn list(&self) -> Vec<AgentInfo> {
        self.agents.lock().values().map(|a| a.info()).collect()
    }

    pub fn get_info(&self, actor_id: &str) -> Option<AgentInfo> {
        self.agents.lock().get(actor_id).map(|a| a.info())
    }

    pub fn log(&self, actor_id: &str, tail: u32) -> Option<Vec<String>> {
        let agents = self.agents.lock();
        agents.get(actor_id).map(|a| {
            let n = tail.max(1) as usize;
            let len = a.log.len();
            let start = len.saturating_sub(n);
            a.log.iter().skip(start).cloned().collect()
        })
    }

    pub fn workspace_for(&self, actor_id: &str) -> PathBuf {
        self.data_dir
            .join("agents")
            .join(actor_id)
            .join("workspace")
    }

    pub fn profile_for(&self, actor_id: &str) -> PathBuf {
        self.data_dir.join("agents").join(actor_id).join("profile")
    }

    pub fn logs_for(&self, actor_id: &str) -> PathBuf {
        self.data_dir.join("agents").join(actor_id).join("logs")
    }

    pub fn root_for(&self, actor_id: &str) -> PathBuf {
        self.data_dir.join("agents").join(actor_id)
    }

    /// Where the command transport keeps its `(actor, scope) -> session_id`
    /// bookkeeping. Sits inside the runtime data dir so a wipe of one agent's
    /// state also clears its resume tokens.
    pub fn sessions_dir(&self) -> PathBuf {
        self.data_dir.join("agent-client").join("sessions")
    }

    pub fn set_status(&self, actor_id: &str, status: &str) {
        let mut agents = self.agents.lock();
        if let Some(a) = agents.get_mut(actor_id) {
            a.status = status.into();
        }
    }

    pub fn set_runtime_handle(
        &self,
        actor_id: &str,
        adapter: Arc<dyn Adapter>,
        pid: Option<u32>,
        session_id: Option<String>,
    ) {
        let mut agents = self.agents.lock();
        if let Some(a) = agents.get_mut(actor_id) {
            a.adapter = Some(adapter);
            a.pid = pid;
            a.session_id = session_id;
            a.status = "running".into();
        }
    }

    pub fn clear_runtime_handle(&self, actor_id: &str) {
        let mut agents = self.agents.lock();
        if let Some(a) = agents.get_mut(actor_id) {
            a.adapter = None;
            a.pid = None;
            a.session_id = None;
            a.status = "stopped".into();
            a.active_turns.clear();
            a.pending_triggers.clear();
            a.seeded.clear();
            a.text_buffer.clear();
            a.stream_seq.clear();
        }
    }

    /// Append a chunk of streaming agent text to the per-turn buffer. Returns
    /// the chunk back so callers can also emit it as a `text.delta` trace
    /// frame without re-borrowing.
    pub fn push_text_chunk(&self, actor_id: &str, turn_id: &str, chunk: &str) {
        if chunk.is_empty() {
            return;
        }
        let mut agents = self.agents.lock();
        if let Some(a) = agents.get_mut(actor_id) {
            a.text_buffer
                .entry(turn_id.to_string())
                .or_default()
                .push_str(chunk);
        }
    }

    /// Drain the per-turn streaming text buffer. Returns `None` when nothing
    /// has been buffered for the turn (e.g. the agent never produced text
    /// before closing).
    pub fn take_text_buffer(&self, actor_id: &str, turn_id: &str) -> Option<String> {
        let mut agents = self.agents.lock();
        let entry = agents.get_mut(actor_id)?;
        let text = entry.text_buffer.remove(turn_id)?;
        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    }

    /// Allocate the next monotonic seq for `turn/stream.update` notifications
    /// on `turn_id`. Counter starts at 1 per turn; returns 0 when the agent
    /// is unknown (caller should drop the delta in that case).
    pub fn next_stream_seq(&self, actor_id: &str, turn_id: &str) -> u64 {
        let mut agents = self.agents.lock();
        let Some(entry) = agents.get_mut(actor_id) else {
            return 0;
        };
        let n = entry.stream_seq.entry(turn_id.to_string()).or_insert(0);
        *n += 1;
        *n
    }

    /// Returns `true` exactly once per (actor, scope) pair — flips the seeded
    /// bit so callers can prepend the bootstrap manifest to the first prompt
    /// each scope sends to a freshly-started agent. Subsequent calls for the
    /// same scope (or a re-register that preserved `seeded`) return `false`.
    pub fn take_seed_slot(&self, actor_id: &str, scope_id: &str) -> bool {
        let mut agents = self.agents.lock();
        match agents.get_mut(actor_id) {
            Some(a) => a.seeded.insert(scope_id.to_string()),
            None => false,
        }
    }

    pub fn adapter_for(&self, actor_id: &str) -> Option<Arc<dyn Adapter>> {
        self.agents
            .lock()
            .get(actor_id)
            .and_then(|a| a.adapter.clone())
    }

    pub fn record_action_request(
        &self,
        actor_id: &str,
        event_id: String,
        adapter: Arc<dyn Adapter>,
        request_id: String,
    ) {
        let mut agents = self.agents.lock();
        if let Some(a) = agents.get_mut(actor_id) {
            a.action_map.insert(event_id, (adapter, request_id));
        }
    }

    pub fn take_action_mapping(
        &self,
        actor_id: &str,
        event_id: &str,
    ) -> Option<(Arc<dyn Adapter>, String)> {
        let mut agents = self.agents.lock();
        agents.get_mut(actor_id)?.action_map.remove(event_id)
    }

    pub fn set_active_turn(&self, actor_id: &str, scope_id: &str, turn_id: String) {
        let mut agents = self.agents.lock();
        if let Some(a) = agents.get_mut(actor_id) {
            a.active_turns.insert(scope_id.to_string(), turn_id);
        }
    }

    /// Drop the active turn for `(actor, scope)` and atomically pop the next
    /// queued trigger for that scope (if any). Returning the trigger inside the
    /// same lock prevents a racing `enqueue_trigger` from getting wedged behind
    /// the now-empty active slot.
    pub fn clear_active_turn(&self, actor_id: &str, scope_id: &str) -> Option<Event> {
        let mut agents = self.agents.lock();
        let a = agents.get_mut(actor_id)?;
        a.active_turns.remove(scope_id);
        let queue = a.pending_triggers.get_mut(scope_id)?;
        let next = queue.pop_front();
        if queue.is_empty() {
            a.pending_triggers.remove(scope_id);
        }
        next
    }

    pub fn active_turn(&self, actor_id: &str, scope_id: &str) -> Option<String> {
        self.agents
            .lock()
            .get(actor_id)
            .and_then(|a| a.active_turns.get(scope_id).cloned())
    }

    /// Push a trigger event onto the per-scope queue. Caller must already have
    /// determined the scope is busy (i.e. `active_turn` returned `Some`); if it
    /// isn't, it'd be simpler to dispatch directly than to enqueue.
    pub fn enqueue_trigger(&self, actor_id: &str, scope_id: &str, event: Event) {
        let mut agents = self.agents.lock();
        if let Some(a) = agents.get_mut(actor_id) {
            a.pending_triggers
                .entry(scope_id.to_string())
                .or_default()
                .push_back(event);
        }
    }

    pub fn spec_for(&self, actor_id: &str) -> Option<AgentSpec> {
        self.agents.lock().get(actor_id).map(|a| a.spec.clone())
    }

    /// Start the agent process if not already running, returning the adapter.
    pub async fn ensure_started(
        self: &Arc<Self>,
        actor_id: &str,
    ) -> RuntimeResult<Arc<dyn Adapter>> {
        if let Some(adapter) = self.adapter_for(actor_id) {
            return Ok(adapter);
        }
        let spec = self
            .spec_for(actor_id)
            .ok_or_else(|| RuntimeError::NotFound(actor_id.into()))?;
        std::fs::create_dir_all(self.workspace_for(actor_id))?;
        std::fs::create_dir_all(self.profile_for(actor_id))?;
        std::fs::create_dir_all(self.logs_for(actor_id))?;
        if let Err(e) = agent_runtime::ensure_agents_md(&self.workspace_for(actor_id), actor_id) {
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
            let profile_dir = self.profile_for(actor_id);
            if let Err(e) =
                agent_runtime::ensure_profile_scaffold(&agent_runtime::ProfileScaffold {
                    profile_dir: &profile_dir,
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

        let workdir = if spec.transport.cwd.is_empty() {
            self.workspace_for(actor_id)
        } else {
            PathBuf::from(self.expand_path_vars(&spec.transport.cwd, actor_id))
        };

        let mut env: BTreeMap<String, String> = spec
            .transport
            .env
            .iter()
            .map(|(k, v)| (k.clone(), self.expand_path_vars(v, actor_id)))
            .collect();
        env.entry("JOI_SERVER".into())
            .or_insert_with(|| self.server_url.clone());
        env.entry("JOI_ACTOR".into())
            .or_insert_with(|| actor_id.to_string());
        if let Some(dir) = self.cli_dir.as_ref() {
            env.entry("PATH".into())
                .or_insert_with(|| prepend_path(dir));
        }
        let args = spec
            .transport
            .args
            .iter()
            .map(|a| self.expand_path_vars(a, actor_id))
            .collect::<Vec<_>>();

        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let adapter: Arc<dyn Adapter> = match spec.transport.kind.as_str() {
            // "acp_stdio" is the default and the only kind v0 understood. Empty
            // string is also tolerated for legacy specs that predate the field.
            "acp_stdio" | "" => {
                let joi_binary = self.cli_dir.as_ref().map(|d| d.join(cli_binary_name()));
                let mcp_servers = agent_runtime::build_mcp_servers(
                    joi_binary.as_deref(),
                    actor_id,
                    &self.profile_for(actor_id),
                    spec.memory.as_ref(),
                );
                let cfg = acp::AcpConfig {
                    command: spec.transport.command.clone(),
                    args,
                    env,
                    cwd: workdir,
                    auth_method: spec.transport.auth_method.clone(),
                    mcp_servers,
                };
                Arc::new(AcpAdapter::new(cfg))
            }
            "command" => {
                let sessions_dir = self.sessions_dir();
                let cfg = CommandConfig::from_transport(
                    actor_id.to_string(),
                    spec.transport.command.clone(),
                    args,
                    env,
                    workdir,
                    &spec.transport,
                    sessions_dir,
                );
                Arc::new(CommandAdapter::new(cfg))
            }
            other => {
                return Err(RuntimeError::Acp(format!(
                    "unknown transport kind `{other}` for agent {actor_id}"
                )))
            }
        };
        let info = adapter
            .start(event_tx)
            .await
            .map_err(|e| RuntimeError::Acp(e.to_string()))?;
        self.set_runtime_handle(actor_id, adapter.clone(), info.pid, info.session_id);
        wakeup::spawn_event_consumer(
            self.clone(),
            self.store.clone(),
            actor_id.to_string(),
            event_rx,
        );
        Ok(adapter)
    }

    pub async fn stop(&self, actor_id: &str) -> RuntimeResult<()> {
        let adapter = self.adapter_for(actor_id).ok_or(RuntimeError::NotRunning)?;
        let _ = adapter.stop().await;
        self.clear_runtime_handle(actor_id);
        Ok(())
    }

    pub fn store(&self) -> Arc<Store> {
        self.store.clone()
    }

    fn expand_path_vars(&self, input: &str, actor_id: &str) -> String {
        let workspace = self.workspace_for(actor_id).display().to_string();
        let profile = self.profile_for(actor_id).display().to_string();
        let logs = self.logs_for(actor_id).display().to_string();
        let root = self.root_for(actor_id).display().to_string();
        input
            .replace("{agent.workspace}", &workspace)
            .replace("{agent.profile}", &profile)
            .replace("{agent.logs}", &logs)
            .replace("{agent.root}", &root)
    }
}

#[allow(dead_code)]
pub(crate) fn _silence_event_unused(_e: &AdapterEvent) {}

fn cli_binary_name() -> &'static str {
    if cfg!(windows) {
        "joi.exe"
    } else {
        "joi"
    }
}

fn detect_cli_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    if dir.join(cli_binary_name()).is_file() {
        Some(dir.to_path_buf())
    } else {
        None
    }
}

fn prepend_path(dir: &Path) -> String {
    let mut paths: Vec<PathBuf> = vec![dir.to_path_buf()];
    if let Some(existing) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    std::env::join_paths(paths)
        .map(|os| os.to_string_lossy().to_string())
        .unwrap_or_else(|_| dir.display().to_string())
}
