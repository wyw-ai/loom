pub mod acp;
pub mod registry;
pub mod wakeup;

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use proto::methods::{AgentInfo, AgentSpec};
use proto::types::*;
use thiserror::Error;
use tokio::sync::mpsc;

use crate::store::{Store, StoreError};

use self::acp::{AcpAdapter, AgentEvent};

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("agent not found: {0}")]
    NotFound(String),
    #[error("already started")]
    AlreadyStarted,
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
    pub adapter: Option<Arc<AcpAdapter>>,
    pub status: String,
    pub pid: Option<u32>,
    pub session_id: Option<String>,
    pub log: VecDeque<String>,
    /// Pending action.request -> ACP request id mapping (key = event id)
    pub action_map: HashMap<String, (Arc<AcpAdapter>, String)>,
    /// Currently active turn for this agent (only one in-flight prompt at a time in v0)
    pub active_turn_id: Option<String>,
    /// True after the agent's session has received its first manifest-bearing prompt.
    pub seeded: bool,
    /// Per-turn streaming text buffer. Partial text chunks accumulate here and
    /// are flushed as a single `content.add` event when the turn closes; the
    /// raw chunks themselves are exposed as turn-private `text.delta` trace
    /// frames so the owner can see the cursor moving.
    pub text_buffer: HashMap<String, String>,
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
            active_turn_id: None,
            seeded: false,
            text_buffer: HashMap::new(),
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
        let existing = agents.remove(&spec.actor.id).map(|a| {
            (
                a.adapter,
                a.status,
                a.pid,
                a.session_id,
                a.log,
                a.action_map,
                a.active_turn_id,
                a.text_buffer,
            )
        });
        let mut entry = RegisteredAgent::new(spec.clone());
        if let Some((adapter, status, pid, session_id, log, action_map, turn, text_buffer)) =
            existing
        {
            entry.adapter = adapter;
            entry.status = status;
            entry.pid = pid;
            entry.session_id = session_id;
            entry.log = log;
            entry.action_map = action_map;
            entry.active_turn_id = turn;
            entry.text_buffer = text_buffer;
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

    pub fn cache_for(&self, actor_id: &str) -> PathBuf {
        self.data_dir.join("agents").join(actor_id).join("cache")
    }

    pub fn logs_for(&self, actor_id: &str) -> PathBuf {
        self.data_dir.join("agents").join(actor_id).join("logs")
    }

    pub fn root_for(&self, actor_id: &str) -> PathBuf {
        self.data_dir.join("agents").join(actor_id)
    }

    pub fn append_log(&self, actor_id: &str, line: String) {
        let mut agents = self.agents.lock();
        if let Some(a) = agents.get_mut(actor_id) {
            if a.log.len() >= 500 {
                a.log.pop_front();
            }
            a.log.push_back(line);
        }
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
        adapter: Arc<AcpAdapter>,
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
            a.active_turn_id = None;
            a.seeded = false;
            a.text_buffer.clear();
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

    /// Returns `true` exactly once per session — flips the agent's `seeded` flag
    /// from false to true so callers can prepend a one-time bootstrap manifest
    /// to the very first prompt of a freshly-started ACP child.
    pub fn take_seed_slot(&self, actor_id: &str) -> bool {
        let mut agents = self.agents.lock();
        match agents.get_mut(actor_id) {
            Some(a) if !a.seeded => {
                a.seeded = true;
                true
            }
            _ => false,
        }
    }

    pub fn adapter_for(&self, actor_id: &str) -> Option<Arc<AcpAdapter>> {
        self.agents
            .lock()
            .get(actor_id)
            .and_then(|a| a.adapter.clone())
    }

    pub fn record_action_request(
        &self,
        actor_id: &str,
        event_id: String,
        adapter: Arc<AcpAdapter>,
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
    ) -> Option<(Arc<AcpAdapter>, String)> {
        let mut agents = self.agents.lock();
        agents.get_mut(actor_id)?.action_map.remove(event_id)
    }

    pub fn set_active_turn(&self, actor_id: &str, turn_id: Option<String>) {
        let mut agents = self.agents.lock();
        if let Some(a) = agents.get_mut(actor_id) {
            a.active_turn_id = turn_id;
        }
    }

    pub fn active_turn(&self, actor_id: &str) -> Option<String> {
        self.agents
            .lock()
            .get(actor_id)
            .and_then(|a| a.active_turn_id.clone())
    }

    pub fn spec_for(&self, actor_id: &str) -> Option<AgentSpec> {
        self.agents.lock().get(actor_id).map(|a| a.spec.clone())
    }

    /// Start the agent process if not already running, returning the adapter.
    pub async fn ensure_started(
        self: &Arc<Self>,
        actor_id: &str,
    ) -> RuntimeResult<Arc<AcpAdapter>> {
        if let Some(adapter) = self.adapter_for(actor_id) {
            return Ok(adapter);
        }
        let spec = self
            .spec_for(actor_id)
            .ok_or_else(|| RuntimeError::NotFound(actor_id.into()))?;
        std::fs::create_dir_all(self.workspace_for(actor_id))?;
        std::fs::create_dir_all(self.cache_for(actor_id))?;
        std::fs::create_dir_all(self.logs_for(actor_id))?;

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
        let cfg = acp::AcpConfig {
            command: spec.transport.command.clone(),
            args,
            env,
            cwd: workdir,
            auth_method: spec.transport.auth_method.clone(),
        };
        let adapter = Arc::new(AcpAdapter::new(cfg));
        let info = adapter
            .start(event_tx)
            .await
            .map_err(|e| RuntimeError::Acp(e.to_string()))?;
        self.set_runtime_handle(actor_id, adapter.clone(), info.pid, Some(info.session_id));
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
        let cache = self.cache_for(actor_id).display().to_string();
        let logs = self.logs_for(actor_id).display().to_string();
        let root = self.root_for(actor_id).display().to_string();
        input
            .replace("{agent.workspace}", &workspace)
            .replace("{agent.cache}", &cache)
            .replace("{agent.logs}", &logs)
            .replace("{agent.root}", &root)
    }
}

#[allow(dead_code)]
pub(crate) fn _silence_event_unused(_e: &AgentEvent) {}

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
