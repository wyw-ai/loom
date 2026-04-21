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

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Result};
use proto::methods::{method, stream_kind, AgentSpec, EventAppendResult, TurnOpenResult};
use proto::types::trace::TraceKind;
use proto::types::{Event, Relation, Ref, RefKind, RelationKind, ScopeKind, ScopeRef, TurnStatus};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TryRecvError;

use agent_runtime::acp::{AcpAdapter, AcpConfig};
use agent_runtime::command::{CommandAdapter, CommandConfig};
use agent_runtime::{Adapter, AdapterEvent};

use crate::client::Client;

pub async fn run(specs_dir_opt: Option<PathBuf>, server_url: String) -> Result<()> {
    let specs_dir = specs_dir_opt.unwrap_or_else(default_specs_dir);
    std::fs::create_dir_all(&specs_dir)
        .with_context(|| format!("create specs dir {}", specs_dir.display()))?;
    let data_root = default_data_root();
    std::fs::create_dir_all(&data_root)
        .with_context(|| format!("create data dir {}", data_root.display()))?;

    let specs = load_specs(&specs_dir)?;
    if specs.is_empty() {
        return Err(anyhow!(
            "no agent specs found in {}; drop AgentSpec JSON files there or pass --specs <dir>",
            specs_dir.display()
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
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("read specs dir {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("read {}", path.display()))?;
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
    cache: PathBuf,
    logs: PathBuf,
    root: PathBuf,
    sessions: PathBuf,
}

impl AgentPaths {
    fn new(root: &Path, actor_id: &str) -> Self {
        let agent_root = root.join("agents").join(actor_id);
        Self {
            workspace: agent_root.join("workspace"),
            cache: agent_root.join("cache"),
            logs: agent_root.join("logs"),
            root: agent_root,
            sessions: root.join("sessions"),
        }
    }

    fn ensure(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.workspace)?;
        std::fs::create_dir_all(&self.cache)?;
        std::fs::create_dir_all(&self.logs)?;
        std::fs::create_dir_all(&self.sessions)?;
        Ok(())
    }

    fn expand(&self, input: &str) -> String {
        input
            .replace("{agent.workspace}", &self.workspace.display().to_string())
            .replace("{agent.cache}", &self.cache.display().to_string())
            .replace("{agent.logs}", &self.logs.display().to_string())
            .replace("{agent.root}", &self.root.display().to_string())
    }
}

struct WorkerState {
    actor_id: String,
    /// Currently in-flight turn id (v0 also serializes one prompt at a time).
    active_turn: Mutex<Option<ActiveTurn>>,
    /// Per-turn streaming text buffer; flushed as a single `content.add` on
    /// `Finished` (and on any non-partial `Text` chunk).
    text_buffer: Mutex<HashMap<String, String>>,
    /// Set once on the first prompt; used to inject the bootstrap manifest just
    /// like the embedded runtime does.
    seeded: Mutex<bool>,
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
    fn new(actor_id: String) -> Self {
        Self {
            actor_id,
            active_turn: Mutex::new(None),
            text_buffer: Mutex::new(HashMap::new()),
            seeded: Mutex::new(false),
        }
    }

    fn current_turn(&self) -> Option<ActiveTurn> {
        self.active_turn.lock().expect("active_turn poisoned").clone()
    }

    fn set_turn(&self, t: Option<ActiveTurn>) {
        *self.active_turn.lock().expect("active_turn poisoned") = t;
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

    fn take_seed_slot(&self) -> bool {
        let mut s = self.seeded.lock().expect("seeded poisoned");
        if !*s {
            *s = true;
            true
        } else {
            false
        }
    }
}

async fn run_agent_worker(
    spec: AgentSpec,
    server_url: String,
    data_root: PathBuf,
) -> Result<()> {
    let actor_id = spec.actor.id.clone();
    let display_name = if spec.actor.display_name.is_empty() {
        actor_id.clone()
    } else {
        spec.actor.display_name.clone()
    };
    let paths = AgentPaths::new(&data_root, &actor_id);
    paths.ensure()?;

    let client = Client::connect(&server_url).await?;
    client.initialize().await?;
    client
        .open_connection_as(&actor_id, "agent", Some(&display_name))
        .await?;
    eprintln!("[{actor_id}] connected to {server_url} as agent");

    let state = Arc::new(WorkerState::new(actor_id.clone()));
    let (event_tx, event_rx) = mpsc::unbounded_channel::<AdapterEvent>();
    let adapter = build_adapter(&spec, &paths, &server_url)?;

    // Translator: AdapterEvent → server RPC. Drains until adapter drops the
    // sender (worker exit) — at which point the loop falls out and the task
    // ends.
    {
        let client = client.clone();
        let state = state.clone();
        let actor = actor_id.clone();
        tokio::spawn(async move {
            translate_events(client, state, actor, event_rx).await;
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
    let args: Vec<String> = spec
        .transport
        .args
        .iter()
        .map(|a| paths.expand(a))
        .collect();

    match spec.transport.kind.as_str() {
        "acp_stdio" | "" => {
            let cfg = AcpConfig {
                command: spec.transport.command.clone(),
                args,
                env,
                cwd: workdir,
                auth_method: spec.transport.auth_method.clone(),
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
        let Some(event_value) = params.get("data").and_then(|d| d.get("event")).cloned()
        else {
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
            if let Err(e) = adapter.start(event_tx.clone()).await {
                eprintln!("[{actor_id}] adapter start failed: {e}");
                continue;
            }
            started = true;
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
    state.set_turn(Some(active.clone()));

    let user_text = render_prompt(trigger);
    let prompt = if state.take_seed_slot() {
        format!(
            "{}\n\n=== User message ===\n{}",
            seed_manifest(&state.actor_id, &trigger.scope),
            user_text
        )
    } else {
        user_text
    };

    if let Err(e) = adapter.send_prompt(trigger.scope.clone(), prompt).await {
        let _ = close_turn(client, &active.id, TurnStatus::Failed).await;
        state.set_turn(None);
        return Err(anyhow!("adapter send_prompt failed: {e}"));
    }
    Ok(())
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
    actor_id: String,
    mut rx: mpsc::UnboundedReceiver<AdapterEvent>,
) {
    loop {
        // Async first, then drain everything that's queued behind it. This
        // keeps the order strict (mpsc is FIFO) without ever holding the lock
        // across awaits.
        let Some(ev) = rx.recv().await else { return };
        if let Err(e) = translate_one(&client, &state, &actor_id, ev).await {
            eprintln!("[{actor_id}] translate failed: {e}");
        }
        loop {
            match rx.try_recv() {
                Ok(ev) => {
                    if let Err(e) = translate_one(&client, &state, &actor_id, ev).await {
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
    actor_id: &str,
    ev: AdapterEvent,
) -> Result<()> {
    // Without an active turn there's nowhere to attach the output. This
    // matches the embedded runtime's behavior of dropping out-of-turn events.
    let Some(active) = state.current_turn() else {
        return Ok(());
    };
    match ev {
        AdapterEvent::Text { content, is_partial } => {
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
        AdapterEvent::ToolUse { tool_name, input } => {
            append_trace(
                client,
                &active.id,
                TraceKind::ToolStart,
                json!({ "toolName": tool_name, "input": input }),
            )
            .await?;
        }
        AdapterEvent::ActionRequest {
            id: _,
            request_type,
            title,
            description,
            choices,
        } => {
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
            let mut relations = Vec::new();
            relations.push(Relation {
                kind: RelationKind::HandsOffTo,
                target: Ref {
                    kind: RefKind::Actor,
                    id: active.trigger_actor.clone(),
                    _meta: None,
                },
                _meta: None,
            });
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
        AdapterEvent::StatusChange { status } => {
            append_trace(
                client,
                &active.id,
                TraceKind::Status,
                json!({ "status": status }),
            )
            .await?;
        }
        AdapterEvent::Finished { success, summary } => {
            if let Some(text) = state.take_text(&active.id) {
                flush_text(client, actor_id, &active.scope, &active.id, text).await?;
            }
            let status = if success {
                TurnStatus::Closed
            } else {
                TurnStatus::Failed
            };
            // turn.close event mirrors v0; close_turn is the state-machine call.
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
            close_turn(client, &active.id, status).await?;
            state.set_turn(None);
        }
        AdapterEvent::Error { message } => {
            append_trace(
                client,
                &active.id,
                TraceKind::Error,
                json!({ "message": message }),
            )
            .await?;
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
    let _ = append_event(
        client,
        "content.add",
        actor_id,
        scope,
        Some(turn_id),
        json!({ "contentType": "text/markdown", "text": text }),
        vec![],
    )
    .await?;
    Ok(())
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
