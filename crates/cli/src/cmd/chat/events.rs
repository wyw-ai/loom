use std::io::Stdout;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{
    poll, read, Event as CtEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent,
    MouseEventKind,
};
use proto::methods::{method, stream_kind};
use proto::types::{Event, ScopeKind, ScopeRef};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use serde_json::json;
use tokio::sync::mpsc::error::TryRecvError;

use crate::client::Client;

use super::app::{App, Mode, PickerKind};
use super::picker::{PickerItem, PickerOutcome};
use super::ui;

pub async fn run(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    client: Arc<Client>,
    actor_id: String,
    thread_id: String,
) -> Result<()> {
    let scope = ScopeRef {
        kind: ScopeKind::Thread,
        id: thread_id.clone(),
    };

    let display_name = bootstrap_display_name(&client, &actor_id).await;
    let mut app = App::new(
        actor_id.clone(),
        thread_id.clone(),
        display_name.clone(),
    );

    if let Err(e) = subscribe(&client, &actor_id, &scope).await {
        app.history
            .push_system(format!("scope/subscribe failed: {}", e));
    }
    if let Err(e) = backfill(&client, &mut app, &scope).await {
        app.history
            .push_system(format!("scope/read backfill failed: {}", e));
    }
    refresh_actor_directory(&client, &mut app).await;
    app.set_status(format!(
        "connected as {} · {}",
        display_name, thread_id
    ));

    loop {
        let disconnected = drain_notifications(&mut app, &scope, &client).await;
        if disconnected && !app.disconnected {
            app.history.push_system("server connection closed");
            app.set_status("disconnected");
            app.disconnected = true;
        }

        terminal.draw(|f| ui::render(f, &mut app))?;
        if app.should_quit {
            break;
        }

        if poll(Duration::from_millis(100))? {
            match read()? {
                CtEvent::Key(key) => {
                    if key.kind == KeyEventKind::Press {
                        handle_key(&client, &mut app, key, &scope).await;
                    }
                }
                CtEvent::Mouse(mouse) => handle_mouse(&mut app, mouse),
                CtEvent::Resize(_, _) => {}
                _ => {}
            }
        }
    }
    Ok(())
}

async fn subscribe(client: &Client, actor_id: &str, scope: &ScopeRef) -> Result<()> {
    let _: serde_json::Value = client
        .call(
            method::SCOPE_SUBSCRIBE,
            json!({ "actorId": actor_id, "scope": scope }),
        )
        .await?;
    Ok(())
}

async fn backfill(client: &Client, app: &mut App, scope: &ScopeRef) -> Result<()> {
    use proto::methods::ScopeReadResult;
    let res: ScopeReadResult = client
        .call(method::SCOPE_READ, json!({ "scope": scope, "limit": 100 }))
        .await?;
    for ev in res.events {
        app.ingest_event(&ev);
    }
    Ok(())
}

async fn bootstrap_display_name(client: &Client, actor_id: &str) -> String {
    use proto::methods::ActorListResult;
    let actors: Result<ActorListResult, _> = client.call(method::ACTOR_LIST, json!({})).await;
    if let Ok(list) = actors {
        for a in list.actors {
            if a.id == actor_id {
                if !a.display_name.is_empty() {
                    return a.display_name;
                }
            }
        }
    }
    actor_id.to_string()
}

async fn refresh_actor_directory(client: &Client, app: &mut App) {
    use proto::methods::{ActorListResult, AgentListResult};
    app.agent_ids.clear();
    app.agent_statuses.clear();
    if let Ok(list) = client
        .call::<_, ActorListResult>(method::ACTOR_LIST, json!({}))
        .await
    {
        for a in list.actors {
            let name = if a.display_name.is_empty() {
                a.id.clone()
            } else {
                a.display_name.clone()
            };
            let kind = match a.kind {
                proto::types::ActorKind::Human => "human",
                proto::types::ActorKind::Agent => "agent",
                proto::types::ActorKind::Service => "service",
            };
            app.actor_kinds.insert(a.id.clone(), kind.to_string());
            app.display_for.insert(a.id, name);
        }
    }
    if let Ok(list) = client
        .call::<_, AgentListResult>(method::AGENT_LIST, json!({}))
        .await
    {
        for ai in list.agents {
            let actor = ai.spec.actor;
            let name = if actor.display_name.is_empty() {
                actor.id.clone()
            } else {
                actor.display_name.clone()
            };
            app.agent_ids.insert(actor.id.clone());
            app.agent_statuses
                .insert(actor.id.clone(), ai.status.clone());
            app.actor_kinds
                .insert(actor.id.clone(), "agent".to_string());
            app.display_for.insert(actor.id, name);
        }
    }
}

fn handle_notification(app: &mut App, scope: &ScopeRef, n: proto::Notification) {
    if n.method == method::TURN_TRACE_UPDATE {
        if let Some(params) = n.params {
            handle_trace_update(app, &params);
        }
        return;
    }
    if n.method != method::STREAM_UPDATE {
        return;
    }
    let Some(params) = n.params else { return };
    let kind = params
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let n_scope = params.get("scope").cloned();
    if let Some(s) = n_scope {
        if let Ok(parsed) = serde_json::from_value::<ScopeRef>(s) {
            if &parsed != scope {
                return;
            }
        }
    }
    let data = params
        .get("data")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    match kind.as_str() {
        stream_kind::EVENT_CREATED => {
            if let Some(ev_value) = data.get("event").cloned() {
                if let Ok(ev) = serde_json::from_value::<Event>(ev_value) {
                    app.ingest_event(&ev);
                }
            }
        }
        _ => {}
    }
}

/// Turn/trace.update is owner-only; the server has already verified that this
/// connection is bound to the turn's actor before pushing the frame. We surface
/// it on the status line so the operator can see the agent's internal cursor
/// (tool starts, status transitions) without polluting history.
fn handle_trace_update(app: &mut App, params: &serde_json::Value) {
    let frame = match params.get("frame") {
        Some(f) => f,
        None => return,
    };
    let kind = frame.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    let body = frame.get("payload").cloned().unwrap_or(serde_json::Value::Null);
    let summary = match kind {
        "text.delta" => body
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .chars()
            .take(40)
            .collect::<String>(),
        "tool.start" | "tool.update" | "tool.end" => body
            .get("toolName")
            .and_then(|v| v.as_str())
            .unwrap_or("tool")
            .to_string(),
        "status" => body
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        "error" => body
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        _ => String::new(),
    };
    if summary.is_empty() {
        app.set_status(format!("trace · {}", kind));
    } else {
        app.set_status(format!("trace · {}: {}", kind, summary));
    }
}

async fn drain_notifications(app: &mut App, scope: &ScopeRef, client: &Client) -> bool {
    let mut notif_rx = client.notifications.lock().await;
    let mut disconnected = false;
    loop {
        match notif_rx.try_recv() {
            Ok(n) => handle_notification(app, scope, n),
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                disconnected = true;
                break;
            }
        }
    }
    disconnected
}

async fn handle_key(client: &Arc<Client>, app: &mut App, key: KeyEvent, scope: &ScopeRef) {
    if matches!(key.code, KeyCode::Char('c') | KeyCode::Char('d'))
        && key.modifiers.contains(KeyModifiers::CONTROL)
    {
        app.should_quit = true;
        return;
    }
    if key.code == KeyCode::Char('r') && key.modifiers.contains(KeyModifiers::CONTROL) {
        app.open_reply_picker();
        return;
    }
    if let Mode::Picker(_) = app.mode {
        if let Some(p) = app.picker.as_mut() {
            match p.handle_key(key) {
                PickerOutcome::None => {}
                PickerOutcome::Cancelled => app.close_picker(),
                PickerOutcome::Selected(id) => {
                    let kind = match &app.mode {
                        Mode::Picker(k) => match k {
                            PickerKind::HandoffTarget => PickerKind::HandoffTarget,
                            PickerKind::Action => PickerKind::Action,
                            PickerKind::Reply => PickerKind::Reply,
                        },
                        _ => return,
                    };
                    app.close_picker();
                    on_picker_selected(client, app, kind, id, scope).await;
                }
            }
        }
        return;
    }

    // Inline dropdowns (slash + @-mention) are ambient. Navigation keys are
    // routed to whichever is open; Enter populates the input and closes the
    // dropdown rather than sending.
    if app.slash_menu.is_some() || app.at_menu.is_some() {
        match key.code {
            KeyCode::Up | KeyCode::Down | KeyCode::Tab => {
                if let Some(p) = app.slash_menu.as_mut() {
                    let _ = p.handle_key(key);
                } else if let Some(p) = app.at_menu.as_mut() {
                    let _ = p.handle_key(key);
                }
                return;
            }
            KeyCode::Enter | KeyCode::Right => {
                if app.slash_menu.is_some() {
                    let chosen = app.slash_menu.as_mut().and_then(|p| {
                        p.filtered()
                            .get(p.state.selected().unwrap_or(0))
                            .map(|it| it.id.clone())
                    });
                    if let Some(cmd) = chosen {
                        app.input = format!("{} ", cmd);
                        app.slash_menu = None;
                        return;
                    }
                } else if app.at_menu.is_some() {
                    let chosen = app.at_menu.as_mut().and_then(|p| {
                        p.filtered()
                            .get(p.state.selected().unwrap_or(0))
                            .map(|it| it.id.clone())
                    });
                    if let Some(id) = chosen {
                        // Rewrite the leading `@<filter>` token to `@<id> ` so
                        // the user can type the message body before Enter.
                        app.input = format!("@{} ", id);
                        app.at_menu = None;
                        return;
                    }
                }
                // Fall through to normal Enter handling if nothing selected.
            }
            _ => {}
        }
    }

    match key.code {
        KeyCode::Esc => {
            if app.input.is_empty() && app.reply_target.is_some() {
                app.clear_reply_target();
            } else {
                app.input.clear();
                app.slash_menu = None;
                app.at_menu = None;
            }
        }
        KeyCode::Up => app.scroll_up(1),
        KeyCode::Down => app.scroll_down(1),
        KeyCode::PageUp => app.scroll_up(10),
        KeyCode::PageDown => app.scroll_down(10),
        KeyCode::Home => app.jump_to_top(),
        KeyCode::End => app.jump_to_bottom(),
        KeyCode::Backspace => {
            app.input.pop();
            app.update_slash_menu();
            app.update_at_menu();
        }
        KeyCode::Enter => {
            let text = std::mem::take(&mut app.input);
            app.slash_menu = None;
            app.at_menu = None;
            if text.trim().is_empty() {
                return;
            }
            if let Some(rest) = text.strip_prefix('/') {
                handle_slash_input(client, app, rest, scope).await;
            } else if let Some(rest) = text.strip_prefix('@') {
                handle_at_input(client, app, rest, scope).await;
            } else {
                send_message(client, app, &text, scope).await;
            }
        }
        KeyCode::Char(c) => {
            app.input.push(c);
            app.update_slash_menu();
            app.update_at_menu();
        }
        _ => {}
    }
}

fn handle_mouse(app: &mut App, mouse: MouseEvent) {
    match mouse.kind {
        MouseEventKind::ScrollUp => app.scroll_up(3),
        MouseEventKind::ScrollDown => app.scroll_down(3),
        _ => {}
    }
}

/// Parse `@target message body` into a handoff. If no message body is
/// provided we just status-bar a hint instead of sending an empty handoff —
/// the user can keep typing.
async fn handle_at_input(client: &Arc<Client>, app: &mut App, rest: &str, scope: &ScopeRef) {
    let mut split = rest.splitn(2, char::is_whitespace);
    let target = split.next().unwrap_or("").trim().to_string();
    let message = split.next().unwrap_or("").trim().to_string();
    if target.is_empty() {
        app.set_status("usage: @<actor_id> <message>");
        return;
    }
    if !app.agent_ids.contains(&target) {
        app.set_status(format!("unknown agent `{}`", target));
        return;
    }
    do_handoff_with_message(client, app, target, message, scope).await;
}

async fn on_picker_selected(
    client: &Arc<Client>,
    app: &mut App,
    kind: PickerKind,
    id: String,
    scope: &ScopeRef,
) {
    match kind {
        PickerKind::HandoffTarget => {
            do_handoff(client, app, id, scope).await;
        }
        PickerKind::Action => {
            // Default to "allow" / accept; richer choice menu is a follow-up.
            do_action_response(client, app, id, "allow".to_string(), true).await;
        }
        PickerKind::Reply => {
            arm_reply_target(app, id);
        }
    }
}

async fn handle_slash_input(client: &Arc<Client>, app: &mut App, rest: &str, scope: &ScopeRef) {
    let trimmed = rest.trim_start();
    let mut split = trimmed.splitn(2, char::is_whitespace);
    let cmd = split.next().unwrap_or("").to_string();
    let arg = split.next().unwrap_or("").trim().to_string();
    match cmd.as_str() {
        "handoff" => {
            if arg.is_empty() {
                open_handoff_picker(client, app).await;
            } else {
                // `/handoff <target> [message...]` — first token is the target,
                // anything after the next whitespace is the message body.
                let mut a = arg.splitn(2, char::is_whitespace);
                let target = a.next().unwrap_or("").trim().to_string();
                let message = a.next().unwrap_or("").trim().to_string();
                do_handoff_with_message(client, app, target, message, scope).await;
            }
        }
        "reply" => {
            if arg.is_empty() {
                app.open_reply_picker();
            } else {
                arm_reply_target(app, arg);
            }
        }
        "action" => app.open_action_picker(),
        "agents" => list_agents(client, app).await,
        "quit" | "q" | "exit" => app.should_quit = true,
        other => app.set_status(format!("unknown /{}", other)),
    }
}

async fn open_handoff_picker(client: &Arc<Client>, app: &mut App) {
    use proto::methods::{ActorListResult, AgentListResult};
    let mut items: Vec<PickerItem> = Vec::new();
    if let Ok(list) = client
        .call::<_, AgentListResult>(method::AGENT_LIST, json!({}))
        .await
    {
        for ai in list.agents {
            let actor = ai.spec.actor;
            let label = if actor.display_name.is_empty() {
                actor.id.clone()
            } else {
                format!("{} (agent)", actor.display_name)
            };
            items.push(PickerItem::new(actor.id, label).with_status(ai.status));
        }
    }
    if let Ok(list) = client
        .call::<_, ActorListResult>(method::ACTOR_LIST, json!({}))
        .await
    {
        for a in list.actors {
            if a.id == app.actor_id {
                continue;
            }
            if items.iter().any(|i| i.id == a.id) {
                continue;
            }
            let kind = match a.kind {
                proto::types::ActorKind::Human => "human",
                proto::types::ActorKind::Agent => "agent",
                proto::types::ActorKind::Service => "service",
            };
            let label = if a.display_name.is_empty() {
                a.id.clone()
            } else {
                format!("{} ({})", a.display_name, kind)
            };
            items.push(PickerItem::new(a.id, label).with_hint(kind));
        }
    }
    if items.is_empty() {
        app.set_status("no other actors known to the server");
        return;
    }
    app.open_handoff_picker(items);
}

async fn do_handoff(client: &Arc<Client>, app: &mut App, target: String, scope: &ScopeRef) {
    // Modal-picker path: no message body was supplied, send the offer alone.
    do_handoff_with_message(client, app, target, String::new(), scope).await;
}

async fn do_handoff_with_message(
    client: &Arc<Client>,
    app: &mut App,
    target: String,
    message: String,
    scope: &ScopeRef,
) {
    use proto::methods::EventAppendResult;
    let payload = json!({
        "event": {
            "type": "content.add",
            "actorId": app.actor_id,
            "scope": scope,
            "payload": { "contentType": "text/markdown", "text": message },
            "relations": [
                { "kind": "hands_off_to", "target": { "kind": "actor", "id": target.clone() } }
            ],
        }
    });
    let res: Result<EventAppendResult, _> = client.call(method::EVENT_APPEND, payload).await;
    match res {
        Ok(_) => app.set_status(format!("handoff → {}", target)),
        Err(e) => app.set_status(format!("handoff failed: {}", e)),
    }
}

async fn do_action_response(
    client: &Arc<Client>,
    app: &mut App,
    event_id: String,
    option_id: String,
    accepted: bool,
) {
    use proto::methods::EventAppendResult;
    let kind = if accepted { "accepted" } else { "declined" };
    let payload = json!({
        "event": {
            "type": "action.response",
            "actorId": app.actor_id,
            "scope": { "kind": "thread", "id": app.thread_id },
            "payload": { "optionId": option_id, "kind": kind },
            "relations": [
                { "kind": "responds_to", "target": { "kind": "event", "id": event_id.clone() } }
            ],
        }
    });
    let res: Result<EventAppendResult, _> = client.call(method::EVENT_APPEND, payload).await;
    match res {
        Ok(_) => {
            let _ = client
                .call_raw(
                    method::RECEIPT_RECORD,
                    Some(json!({
                        "eventId": event_id,
                        "actorId": app.actor_id,
                        "kind": kind,
                    })),
                )
                .await;
            app.set_status(format!("action {} ({})", kind, option_id));
        }
        Err(e) => app.set_status(format!("action.response failed: {}", e)),
    }
}

async fn list_agents(client: &Arc<Client>, app: &mut App) {
    use proto::methods::AgentListResult;
    match client
        .call::<_, AgentListResult>(method::AGENT_LIST, json!({}))
        .await
    {
        Ok(list) => {
            if list.agents.is_empty() {
                app.history.push_system("(no agents registered)");
            } else {
                let header = "Registered agents:".to_string();
                app.history.push_system(header);
                for ai in list.agents {
                    // Width-padded so columns line up without tab characters.
                    let line = format!(
                        "  • {:<24} {:<20} ({})",
                        ai.spec.actor.id, ai.spec.actor.display_name, ai.status
                    );
                    app.history.push_system(line);
                }
            }
        }
        Err(e) => app.set_status(format!("agent/list failed: {}", e)),
    }
}

fn arm_reply_target(app: &mut App, event_id: String) {
    let preview = app
        .history
        .reply_targets(&|id| app.display_name_for(id))
        .into_iter()
        .find(|(id, _)| id == &event_id)
        .map(|(_, label)| label)
        .unwrap_or_else(|| short_event_id(&event_id));
    if app.input.trim_start().starts_with("/reply") {
        app.input.clear();
        app.slash_menu = None;
        app.at_menu = None;
    }
    app.set_reply_target(event_id, preview);
}

fn message_relations(app: &App) -> (Vec<serde_json::Value>, Option<String>) {
    let mut relations = Vec::new();
    let reply_target = app
        .reply_target
        .as_ref()
        .map(|target| target.event_id.clone());
    if let Some(reply_to) = reply_target.as_ref() {
        relations.push(json!({
            "kind": "replies_to",
            "target": { "kind": "event", "id": reply_to }
        }));
        if let Some(target_actor_id) = app.history.actor_for_event(reply_to) {
            if target_actor_id != app.actor_id && target_actor_id != "system" {
                relations.push(json!({
                    "kind": "hands_off_to",
                    "target": { "kind": "actor", "id": target_actor_id }
                }));
            }
        }
    }
    (relations, reply_target)
}

async fn send_message(client: &Arc<Client>, app: &mut App, text: &str, scope: &ScopeRef) {
    use proto::methods::EventAppendResult;
    let (relations, reply_target) = message_relations(app);
    let payload = json!({
        "event": {
            "type": "content.add",
            "actorId": app.actor_id,
            "scope": scope,
            "payload": { "contentType": "text/markdown", "text": text },
            "relations": relations,
        }
    });
    let res: Result<EventAppendResult, _> = client.call(method::EVENT_APPEND, payload).await;
    match res {
        Ok(r) => {
            let actor = app.actor_id.clone();
            // Render the bubble immediately as Pending. When stream/update
            // echoes the same event id back, History flips it to Delivered.
            app.history.push_outgoing(
                &actor,
                &r.event.id,
                r.event.occurred_at,
                text.to_string(),
                reply_target,
            );
            app.reply_target = None;
            app.jump_to_bottom();
        }
        Err(e) => app.set_status(format!("send failed: {}", e)),
    }
}

fn short_event_id(id: &str) -> String {
    if id.len() > 12 {
        format!("{}…", &id[..12])
    } else {
        id.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{arm_reply_target, message_relations};
    use crate::cmd::chat::app::App;
    use crate::cmd::chat::history::{Bubble, BubbleKind, DeliveryState};
    use chrono::Utc;

    #[test]
    fn arm_reply_target_clears_reply_command_residue() {
        let mut app = App::new(
            "actor_human_current".into(),
            "thread_demo".into(),
            "bojun.cbj".into(),
        );
        app.input = "/reply ".into();

        arm_reply_target(&mut app, "evt_123".into());

        assert!(app.input.is_empty());
        assert_eq!(
            app.reply_target.as_ref().map(|t| t.event_id.as_str()),
            Some("evt_123")
        );
    }

    #[test]
    fn reply_to_other_actor_adds_hands_off_to_relation() {
        let mut app = App::new(
            "actor_human_current".into(),
            "thread_demo".into(),
            "bojun.cbj".into(),
        );
        app.history.bubbles.push(Bubble {
            actor_id: "actor_agent_opencode".into(),
            turn_id: None,
            kind: BubbleKind::Stream,
            text: "hello".into(),
            ts: Utc::now(),
            reply_to_event_id: None,
            trailing_event_id: Some("evt_123".into()),
            delivery: DeliveryState::NotApplicable,
        });
        app.set_reply_target("evt_123".into(), "evt_123".into());

        let (relations, reply_target) = message_relations(&app);

        assert_eq!(reply_target.as_deref(), Some("evt_123"));
        assert_eq!(relations.len(), 2);
        assert_eq!(relations[0]["kind"], "replies_to");
        assert_eq!(relations[0]["target"]["kind"], "event");
        assert_eq!(relations[0]["target"]["id"], "evt_123");
        assert_eq!(relations[1]["kind"], "hands_off_to");
        assert_eq!(relations[1]["target"]["kind"], "actor");
        assert_eq!(relations[1]["target"]["id"], "actor_agent_opencode");
    }

    #[test]
    fn reply_to_self_does_not_add_hands_off_to_relation() {
        let mut app = App::new(
            "actor_human_current".into(),
            "thread_demo".into(),
            "bojun.cbj".into(),
        );
        app.history.bubbles.push(Bubble {
            actor_id: "actor_human_current".into(),
            turn_id: None,
            kind: BubbleKind::Stream,
            text: "hello".into(),
            ts: Utc::now(),
            reply_to_event_id: None,
            trailing_event_id: Some("evt_123".into()),
            delivery: DeliveryState::NotApplicable,
        });
        app.set_reply_target("evt_123".into(), "evt_123".into());

        let (relations, _) = message_relations(&app);

        assert_eq!(relations.len(), 1);
        assert_eq!(relations[0]["kind"], "replies_to");
    }
}
