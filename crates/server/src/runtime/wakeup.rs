//! Glue between `RuntimeManager` and an in-process `Store`:
//!   - watches the store's broadcast for newly appended events,
//!   - when an event targets/handsoff to a registered agent, ensure the agent is
//!     started, open a Turn for it, and forward the trigger text via session/prompt,
//!   - translates incoming `AgentEvent`s back into store events.

use std::sync::Arc;

use proto::types::*;
use serde_json::json;
use tokio::sync::mpsc;

use super::acp::AgentEvent;
use super::RuntimeManager;
use crate::store::{Store, StoreEvent};

/// Spawn the supervisor that watches the store for events directed at agents
/// and routes them into ACP children. This must be called once at process boot
/// after both `Store` and `RuntimeManager` exist.
pub fn spawn_supervisor(manager: Arc<RuntimeManager>, store: Arc<Store>) {
    let mut rx = store.subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(ev) => handle_store_event(&manager, &store, ev).await,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "store broadcast lagged");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

async fn handle_store_event(manager: &Arc<RuntimeManager>, _store: &Arc<Store>, ev: StoreEvent) {
    let event = match ev {
        StoreEvent::EventCreated(e) => e,
        _ => return,
    };
    // The event's own actor may be an agent; ignore self-loops.
    let mut targets: Vec<String> = Vec::new();
    for r in &event.relations {
        if matches!(r.kind, RelationKind::Targets | RelationKind::HandsOffTo)
            && r.target.kind == RefKind::Actor
            && manager.spec_for(&r.target.id).is_some()
            && r.target.id != event.actor_id
        {
            targets.push(r.target.id.clone());
        }
    }
    if targets.is_empty() {
        return;
    }
    for actor_id in targets {
        if let Err(e) = wake_agent(manager.clone(), actor_id.clone(), event.clone()).await {
            tracing::warn!(actor = %actor_id, %e, "failed to wake agent");
        }
    }
}

async fn wake_agent(
    manager: Arc<RuntimeManager>,
    actor_id: String,
    trigger: Event,
) -> Result<(), String> {
    let adapter = manager
        .ensure_started(&actor_id)
        .await
        .map_err(|e| e.to_string())?;
    // Open a turn for this agent in the trigger's scope.
    let store = manager.store();
    let turn = store
        .open_turn(
            actor_id.clone(),
            trigger.scope.clone(),
            Some(trigger.id.clone()),
        )
        .map_err(|e| e.to_string())?;
    manager.set_active_turn(&actor_id, Some(turn.id.clone()));

    let user_text = render_prompt(&trigger);
    let prompt_text = if manager.take_seed_slot(&actor_id) {
        format!(
            "{}\n\n=== User message ===\n{}",
            seed_manifest(&actor_id, &trigger.scope),
            user_text
        )
    } else {
        user_text
    };
    if let Err(e) = adapter.send_prompt(prompt_text).await {
        let _ = store.close_turn(&turn.id, TurnStatus::Failed);
        manager.set_active_turn(&actor_id, None);
        return Err(e);
    }
    Ok(())
}

fn render_prompt(trigger: &Event) -> String {
    // Prefer explicit content; otherwise fall back to handoff.offer.message.
    if let Some(text) = trigger.payload.get("text").and_then(|v| v.as_str()) {
        return text.to_string();
    }
    if let Some(text) = trigger.payload.get("message").and_then(|v| v.as_str()) {
        return text.to_string();
    }
    serde_json::to_string(&trigger.payload).unwrap_or_default()
}

/// One-time bootstrap shown to the agent on its very first prompt of a session.
/// It tells the model who it is, what scope it is in, and which `joi` commands
/// are available for reading server state. Environment variables JOI_SERVER and
/// JOI_ACTOR are already injected into the child process.
fn seed_manifest(actor_id: &str, scope: &ScopeRef) -> String {
    let scope_kind = match scope.kind {
        ScopeKind::Conversation => "conversation",
        ScopeKind::Space => "space",
    };
    let scope_flag = if matches!(scope.kind, ScopeKind::Space) {
        " --space"
    } else {
        ""
    };
    format!(
        "=== System: Joi multi-actor context (auto-injected on session start) ===\n\
         You are an ACP agent running inside the Joi multi-actor server.\n\
         Identity:\n\
           actor id      = {actor_id}\n\
           current scope = {scope_kind}:{scope_id}\n\
         \n\
         You can shell out to the `joi` CLI to read server state. The env vars\n\
         JOI_SERVER and JOI_ACTOR are already set, so commands like:\n\
           joi --json event list --in {scope_id}{scope_flag}\n\
           joi --json event list --in {scope_id}{scope_flag} --before <event_id>\n\
           joi --json conv list\n\
           joi --json space list\n\
           joi --json actor list\n\
           joi --json agent list\n\
         will work without flags. Use `--json` for machine-readable output.\n\
         Use `joi --help` and `joi <subcommand> --help` for the full surface.\n\
         Only the message after the marker line is the new user input.\n\
         ",
        actor_id = actor_id,
        scope_kind = scope_kind,
        scope_id = scope.id,
        scope_flag = scope_flag,
    )
}

/// Spawned per agent process when it starts; consumes `AgentEvent`s and turns
/// them into store events under the currently-active turn for that agent.
pub fn spawn_event_consumer(
    manager: Arc<RuntimeManager>,
    store: Arc<Store>,
    actor_id: String,
    mut rx: mpsc::UnboundedReceiver<AgentEvent>,
) {
    tokio::spawn(async move {
        while let Some(ev) = rx.recv().await {
            translate_event(&manager, &store, &actor_id, ev).await;
        }
        manager.clear_runtime_handle(&actor_id);
    });
}

async fn translate_event(
    manager: &Arc<RuntimeManager>,
    store: &Arc<Store>,
    actor_id: &str,
    ev: AgentEvent,
) {
    let turn_id = manager.active_turn(actor_id);
    let scope = match turn_id
        .as_deref()
        .and_then(|id| store.get_turn(id))
        .map(|t| t.scope)
    {
        Some(s) => s,
        None => return,
    };
    let actor = actor_id.to_string();
    match ev {
        AgentEvent::Text { content, .. } => {
            let payload = json!({
                "contentType": "text/markdown",
                "text": content,
            });
            let _ = store.append_event(
                "content.add".into(),
                actor,
                scope,
                turn_id,
                payload,
                vec![],
                None,
            );
        }
        AgentEvent::ToolUse { tool_name, input } => {
            let payload = json!({
                "toolName": tool_name,
                "input": input,
                "status": "running",
            });
            let _ = store.append_event(
                "tool.report".into(),
                actor,
                scope,
                turn_id,
                payload,
                vec![],
                None,
            );
        }
        AgentEvent::ActionRequest {
            id,
            request_type,
            title,
            description,
            choices,
        } => {
            let payload = json!({
                "requestType": request_type,
                "title": title,
                "description": description,
                "choices": choices.iter().map(|c| json!({
                    "id": c.id,
                    "label": c.label,
                })).collect::<Vec<_>>(),
            });
            // We need to know the human (or other) actor that triggered this turn
            // so they can respond. Use the trigger event's actor.
            let trigger_actor = turn_id
                .as_deref()
                .and_then(|tid| store.get_turn(tid))
                .and_then(|t| t.trigger_event_id.clone())
                .and_then(|eid| store.get_event(&eid))
                .map(|e| e.actor_id);
            let mut relations = vec![];
            if let Some(human) = trigger_actor {
                relations.push(Relation {
                    kind: RelationKind::Targets,
                    target: Ref {
                        kind: RefKind::Actor,
                        id: human,
                        _meta: None,
                    },
                    _meta: None,
                });
            }
            if let Ok(event) = store.append_event(
                "action.request".into(),
                actor.clone(),
                scope,
                turn_id.clone(),
                payload,
                relations,
                None,
            ) {
                if let Some(adapter) = manager.adapter_for(&actor) {
                    manager.record_action_request(&actor, event.id, adapter, id);
                }
            }
        }
        AgentEvent::StatusChange { status } => {
            manager.set_status(&actor, &status);
        }
        AgentEvent::Finished { success, summary } => {
            if let Some(tid) = turn_id {
                let status = if success {
                    TurnStatus::Closed
                } else {
                    TurnStatus::Failed
                };
                let _ = store.append_event(
                    "turn.close".into(),
                    actor.clone(),
                    scope,
                    Some(tid.clone()),
                    json!({ "status": format!("{:?}", status).to_lowercase(), "stopReason": summary }),
                    vec![],
                    None,
                );
                let _ = store.close_turn(&tid, status);
            }
            manager.set_active_turn(&actor, None);
        }
        AgentEvent::Error { message } => {
            let payload = json!({
                "contentType": "text/plain",
                "text": format!("[error] {}", message),
            });
            let _ = store.append_event(
                "content.add".into(),
                actor,
                scope,
                turn_id,
                payload,
                vec![],
                None,
            );
        }
    }
}

/// Hand off an action.response event from a human into the ACP child.
pub async fn forward_action_response(
    manager: &Arc<RuntimeManager>,
    store: &Arc<Store>,
    response_event: &Event,
) {
    let mut target_request: Option<(String, String)> = None;
    for r in &response_event.relations {
        if matches!(r.kind, RelationKind::RespondsTo) && r.target.kind == RefKind::Event {
            // Find the action.request event and the agent that issued it.
            if let Some(req_event) = store.get_event(&r.target.id) {
                target_request = Some((req_event.actor_id.clone(), req_event.id.clone()));
                break;
            }
        }
    }
    let Some((agent_actor, request_event_id)) = target_request else {
        return;
    };
    let Some((adapter, request_id)) = manager.take_action_mapping(&agent_actor, &request_event_id)
    else {
        return;
    };
    let option_id = response_event
        .payload
        .get("optionId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if let Err(e) = adapter.respond_permission(request_id, option_id).await {
        tracing::warn!(%e, "failed to forward action response");
    }
}
