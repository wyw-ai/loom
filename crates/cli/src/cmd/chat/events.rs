use std::io::Stdout;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{
    poll, read, Event as CtEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent,
    MouseEventKind,
};
use proto::methods::{method, stream_kind};
use proto::types::{Channel, Event, ScopeKind, ScopeRef, Turn};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use serde_json::json;
use tokio::sync::mpsc::error::TryRecvError;

use crate::client::Client;

use super::app::{App, Mode, OpenTurn, PickerKind};
use super::picker::{PickerItem, PickerOutcome};
use super::prompt::{ConfirmKind, PromptKind, PromptModal, PromptOutcome};
use super::sidebar::SidebarFocus;
use super::ui;

pub async fn run(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    client: Arc<Client>,
    actor_id: String,
    thread_id: String,
    scope_kind: ScopeKind,
) -> Result<()> {
    let mut scope = ScopeRef {
        kind: scope_kind.clone(),
        id: thread_id.clone(),
    };

    let display_name = bootstrap_display_name(&client, &actor_id).await;
    let mut app = App::new(
        actor_id.clone(),
        thread_id.clone(),
        scope_kind.clone(),
        display_name.clone(),
    );

    if app.has_scope() {
        if let Err(e) = subscribe(&client, &actor_id, &scope).await {
            app.history
                .push_system(format!("scope/subscribe failed: {}", e));
        }
        if let Err(e) = backfill(&client, &mut app, &scope).await {
            app.history
                .push_system(format!("scope/read backfill failed: {}", e));
        }
    }
    refresh_actor_directory(&client, &mut app).await;
    if app.has_scope() {
        let label = match scope_kind {
            ScopeKind::Thread => format!("thread {}", thread_id),
            ScopeKind::Channel => format!("#channel {}", thread_id),
        };
        app.set_status(format!("connected as {} · {}", display_name, label));
    } else {
        // `joi chat` without `--in`: auto-open the sidebar + populate the
        // channel list so the operator can immediately pick/create a
        // thread instead of staring at an empty chat pane.
        app.history.push_system("Welcome to Joi chat.");
        app.history.push_system(
            "No scope bound. Use the sidebar (Ctrl+B) to pick a channel,",
        );
        app.history.push_system(
            "press Enter to drill into its threads, or press c to enter the channel common area.",
        );
        app.toggle_sidebar();
        initialize_sidebar(&client, &mut app).await;
        app.set_status(format!(
            "connected as {} · pick a channel or thread from the sidebar",
            display_name
        ));
    }

    loop {
        // Sidebar may have queued a scope switch on the previous frame —
        // drain it before the next render so the user immediately sees the
        // new scope's history.
        if let Some(target_scope) = app.pending_scope_switch.take() {
            switch_scope(&client, &mut app, &mut scope, target_scope).await;
        }

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
                CtEvent::Paste(text) => handle_paste(&client, &mut app, text, &scope).await,
                CtEvent::Mouse(mouse) => handle_mouse(&mut app, mouse),
                CtEvent::Resize(_, _) => {}
                _ => {}
            }
        }
    }
    Ok(())
}

/// Re-bind the chat to a different scope (thread or channel). Best-effort:
/// any RPC failure here leaves the previous subscription intact and surfaces
/// the error on the status line so the user can decide to retry or quit.
async fn switch_scope(
    client: &Arc<Client>,
    app: &mut App,
    scope: &mut ScopeRef,
    target_scope: ScopeRef,
) {
    if target_scope == *scope {
        return;
    }
    // Fire and forget unsubscribe — the server tolerates duplicate/missing
    // unsubscribes, and we don't want to block the UI on it. Skip when the
    // previous scope id is empty (launched without `--in`, so we never
    // subscribed in the first place).
    if !scope.id.is_empty() {
        let _ = client
            .call::<_, serde_json::Value>(
                method::SCOPE_UNSUBSCRIBE,
                json!({ "actorId": app.actor_id, "scope": scope }),
            )
            .await;
    }

    if let Err(e) = subscribe(client, &app.actor_id, &target_scope).await {
        app.set_status(format!("scope/subscribe failed: {}", e));
        return;
    }
    app.reset_for_new_scope(&target_scope);
    if let Some(s) = app.sidebar.as_mut() {
        match target_scope.kind {
            ScopeKind::Thread => {
                s.current_thread_id = Some(target_scope.id.clone());
                s.current_channel_id = None;
            }
            ScopeKind::Channel => {
                s.current_channel_id = Some(target_scope.id.clone());
                s.current_thread_id = None;
            }
        }
    }
    *scope = target_scope.clone();
    if let Err(e) = backfill(client, app, &target_scope).await {
        app.history
            .push_system(format!("scope/read backfill failed: {}", e));
    }
    let label = match target_scope.kind {
        ScopeKind::Thread => format!("thread {}", target_scope.id),
        ScopeKind::Channel => format!("#channel {}", target_scope.id),
    };
    app.set_status(format!("switched → {}", label));
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
            // In v1 mode the server's `agent/list` is empty (the registry lives
            // in the external `joi agent serve` process). Promote any
            // kind=Agent actor we see in `actor/list` so handoff/@-mention
            // pickers still find them. v0 mode stays correct because
            // `agent/list` below re-inserts the same ids idempotently.
            if matches!(a.kind, proto::types::ActorKind::Agent) {
                app.agent_ids.insert(a.id.clone());
            }
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
    if n.method == method::TURN_STREAM_UPDATE {
        if let Some(params) = n.params {
            handle_stream_update(app, scope, &params);
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
    let data = params
        .get("data")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    // Channel ACL pushes are actor-inbox direct messages, not scope-bound,
    // so they must be processed BEFORE the scope filter below — otherwise an
    // invite into a different channel would be discarded.
    match kind.as_str() {
        stream_kind::CHANNEL_INVITED => {
            apply_channel_invited(app, &data);
            return;
        }
        stream_kind::CHANNEL_REVOKED => {
            apply_channel_revoked(app, &data);
            return;
        }
        _ => {}
    }
    let n_scope = params.get("scope").cloned();
    if let Some(s) = n_scope {
        if let Ok(parsed) = serde_json::from_value::<ScopeRef>(s) {
            if &parsed != scope {
                return;
            }
        }
    }
    match kind.as_str() {
        stream_kind::EVENT_CREATED => {
            if let Some(ev_value) = data.get("event").cloned() {
                if let Ok(ev) = serde_json::from_value::<Event>(ev_value) {
                    app.ingest_event(&ev);
                }
            }
        }
        stream_kind::TURN_OPENED => {
            if let Some(turn_value) = data.get("turn").cloned() {
                if let Ok(t) = serde_json::from_value::<Turn>(turn_value) {
                    apply_turn_opened(app, t);
                }
            }
        }
        stream_kind::TURN_CLOSED => {
            if let Some(turn_value) = data.get("turn").cloned() {
                if let Ok(t) = serde_json::from_value::<Turn>(turn_value) {
                    app.open_turns.remove(&t.id);
                }
            }
        }
        _ => {}
    }
}

/// Register a freshly opened turn so cancel and the streaming bar know about
/// it even before the agent emits its first `turn/stream.update` delta. Skip
/// turns we ourselves own — humans don't run agent-style turns through this
/// chat, and even if they did, cancel would be a no-op against the same
/// connection.
fn apply_turn_opened(app: &mut App, t: Turn) {
    if t.actor_id == app.actor_id {
        return;
    }
    app.open_turns.insert(
        t.id.clone(),
        OpenTurn {
            turn_id: t.id,
            actor_id: t.actor_id,
            scope: t.scope,
            opened_at: t.opened_at,
        },
    );
}

/// Apply a `channel.invited` actor-inbox push: patch the sidebar's channel +
/// member cache from the embedded `Channel`, and surface a system message
/// when the inviting actor is `app.actor_id` so the operator gets a clear
/// "you were added" line in the chat history.
fn apply_channel_invited(app: &mut App, data: &serde_json::Value) {
    use super::sidebar::MemberRow;
    let actor_id = data
        .get("actorId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let channel_id = data
        .get("channelId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let channel: Option<Channel> = data
        .get("channel")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok());

    if let (Some(s), Some(ch)) = (app.sidebar.as_mut(), channel.as_ref()) {
        // Insert or replace the channel meta so the sidebar reflects the new
        // membership and visibility immediately.
        if let Some(existing) = s.channels.iter_mut().find(|c| c.id == ch.id) {
            existing.members = ch.members.clone();
            existing.visibility = ch.visibility;
            existing.title = ch.title.clone();
        } else {
            s.add_channel(ch.clone());
        }
    }

    // Patch the per-channel member cache. We only have the actor id, not a
    // resolved Actor row; build a MemberRow from `display_for` / `actor_kinds`
    // (populated by `refresh_actor_directory`) so the row renders nicely.
    let display = app
        .display_for
        .get(&actor_id)
        .cloned()
        .unwrap_or_else(|| actor_id.clone());
    let kind = match app.actor_kinds.get(&actor_id).map(String::as_str) {
        Some("agent") => proto::types::ActorKind::Agent,
        Some("service") => proto::types::ActorKind::Service,
        _ => proto::types::ActorKind::Human,
    };
    if let Some(s) = app.sidebar.as_mut() {
        s.add_member(
            &channel_id,
            MemberRow {
                actor_id: actor_id.clone(),
                display: display.clone(),
                kind,
            },
        );
    }

    // Surface the event. If the local actor is the invitee, prefer a strong
    // history line (it's actionable: they can now read/write that channel).
    let title = channel
        .as_ref()
        .map(|c| c.title.clone())
        .unwrap_or_else(|| channel_id.clone());
    if actor_id == app.actor_id {
        app.history
            .push_system(format!("you were added to #{}", title));
        app.set_status(format!("invited to #{}", title));
    } else {
        app.set_status(format!("{} joined #{}", display, title));
    }
}

/// Apply a `channel.revoked` actor-inbox push: drop the row from the cache
/// and (when we ourselves got revoked from the channel hosting the active
/// thread) surface a clear warning into chat history so the operator sees
/// why subsequent appends will fail.
fn apply_channel_revoked(app: &mut App, data: &serde_json::Value) {
    let actor_id = data
        .get("actorId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let channel_id = data
        .get("channelId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let title = if let Some(s) = app.sidebar.as_mut() {
        s.remove_member(&channel_id, &actor_id);
        s.channels
            .iter()
            .find(|c| c.id == channel_id)
            .map(|c| c.title.clone())
            .unwrap_or_else(|| channel_id.clone())
    } else {
        channel_id.clone()
    };

    if actor_id == app.actor_id {
        // If the active chat thread lives in this channel, surface a strong
        // warning in history; further `event/append` calls will be rejected.
        let in_revoked_channel = current_chat_channel(app).as_deref() == Some(channel_id.as_str());
        if in_revoked_channel {
            app.history.push_system(format!(
                "you were removed from #{} — this thread is no longer writable",
                title
            ));
        }
        app.set_status(format!("removed from #{}", title));
    } else {
        let display = app
            .display_for
            .get(&actor_id)
            .cloned()
            .unwrap_or_else(|| actor_id.clone());
        app.set_status(format!("{} left #{}", display, title));
    }
}

/// Turn/trace.update is owner-only; the server has already verified that this
/// connection is bound to the turn's actor before pushing the frame. We surface
/// it on the status line so the operator can see the agent's internal cursor
/// (tool starts, status transitions) without polluting history.
/// Apply a `turn/stream.update` notification: append a partial-text delta
/// into the streaming bubble for `(actorId, turnId)`. Drops frames whose
/// scope doesn't match the user's current scope; that bubble would belong
/// to a different room and the server still routed it to us because we're
/// also a subscriber there.
fn handle_stream_update(app: &mut App, scope: &ScopeRef, params: &serde_json::Value) {
    if let Some(s) = params.get("scope").cloned() {
        if let Ok(parsed) = serde_json::from_value::<ScopeRef>(s) {
            if &parsed != scope {
                return;
            }
        }
    }
    let turn_id = params.get("turnId").and_then(|v| v.as_str()).unwrap_or("");
    let actor_id = params.get("actorId").and_then(|v| v.as_str()).unwrap_or("");
    let delta = params
        .get("deltaText")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if turn_id.is_empty() || actor_id.is_empty() || delta.is_empty() {
        return;
    }
    app.history
        .append_stream_delta(actor_id, turn_id, delta, chrono::Utc::now());
    if app.auto_follow {
        app.jump_to_bottom();
    }
}

fn handle_trace_update(app: &mut App, params: &serde_json::Value) {
    let frame = match params.get("frame") {
        Some(f) => f,
        None => return,
    };
    let kind = frame.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    let body = frame
        .get("payload")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
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
    // Prompt modal eats every key when active. It deliberately runs before
    // Ctrl+B / sidebar so the user can't accidentally close the dialog.
    if app.prompt.is_some() {
        let outcome = app.prompt.as_mut().map(|p| p.handle_key(key));
        match outcome {
            Some(PromptOutcome::None) => {}
            Some(PromptOutcome::Cancelled) => app.close_prompt(),
            Some(PromptOutcome::SubmittedText { kind, value }) => {
                app.close_prompt();
                handle_prompt_submit(client, app, kind, value).await;
            }
            Some(PromptOutcome::Confirmed(kind)) => {
                app.close_prompt();
                handle_confirm(client, app, kind).await;
            }
            None => {}
        }
        return;
    }
    // Ctrl+B toggles the sidebar regardless of focus.
    if key.code == KeyCode::Char('b') && key.modifiers.contains(KeyModifiers::CONTROL) {
        if app.sidebar.is_none() {
            app.toggle_sidebar();
            initialize_sidebar(client, app).await;
        } else {
            app.toggle_sidebar();
        }
        return;
    }
    // Modal picker takes priority over the sidebar — even when the picker
    // was opened *from* the sidebar (e.g. InviteActor via `i` on the Members
    // pane), the sidebar must yield keyboard focus or it'll consume Enter
    // and cycle panes instead of letting the picker confirm.
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
                            PickerKind::InviteActor { channel_id } => PickerKind::InviteActor {
                                channel_id: channel_id.clone(),
                            },
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
    // Sidebar is focused whenever it's visible — keys go to it first.
    if app.sidebar.is_some() {
        if handle_sidebar_key(client, app, key).await {
            return;
        }
        // fall through if sidebar declined the key (none today, but keeps
        // the door open for hotkeys we want to reach the chat input)
    }
    if key.code == KeyCode::Char('r') && key.modifiers.contains(KeyModifiers::CONTROL) {
        app.open_reply_picker();
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
                        app.input = format!("{} ", cmd).into();
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
                        app.input = format!("@{} ", id).into();
                        app.at_menu = None;
                        return;
                    }
                }
                // Fall through to normal Enter handling if nothing selected.
            }
            _ => {}
        }
    }

    if key.code == KeyCode::Char('r') && key.modifiers.is_empty() && app.input.is_empty() {
        reply_selected_history(app);
        return;
    }

    match key.code {
        KeyCode::Esc => {
            // Priority order on Esc with empty input:
            //   1. Clear an armed reply target.
            //   2. Cancel an in-flight agent turn (selected one if any,
            //      otherwise "the only one" or a hint to disambiguate).
            //   3. Clear history selection (read-only nav reset).
            //   4. Otherwise: clear input + close any open picker.
            if app.input.is_empty() && app.reply_target.is_some() {
                app.clear_reply_target();
            } else if app.input.is_empty() && has_open_turn(app, scope) {
                cancel_in_scope(client, app, scope, None).await;
            } else if app.input.is_empty() && app.selected_history_idx.is_some() {
                app.clear_history_selection();
            } else {
                app.input.clear();
                app.slash_menu = None;
                app.at_menu = None;
            }
        }
        KeyCode::Up => app.select_older_history(),
        KeyCode::Down => app.select_newer_history(),
        KeyCode::Left if app.input.is_empty() => app.collapse_selected_history(),
        KeyCode::Right if app.input.is_empty() => app.expand_selected_history(),
        KeyCode::PageUp => {
            app.clear_history_selection();
            app.scroll_up(10);
        }
        KeyCode::PageDown => {
            app.clear_history_selection();
            app.scroll_down(10);
        }
        KeyCode::Home => {
            app.clear_history_selection();
            app.jump_to_top();
        }
        KeyCode::End => {
            app.clear_history_selection();
            app.jump_to_bottom();
        }
        KeyCode::Backspace => {
            app.input.pop();
            app.update_slash_menu();
            app.update_at_menu();
        }
        KeyCode::Enter => {
            if key.modifiers.intersects(KeyModifiers::ALT | KeyModifiers::SHIFT) {
                app.input.push_char('\n');
                app.update_slash_menu();
                app.update_at_menu();
                return;
            }
            let text = app.input.take_submission_text();
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
            if key.modifiers == KeyModifiers::CONTROL && c == 'j' {
                app.input.push_char('\n');
            } else {
                app.input.push_char(c);
            }
            app.update_slash_menu();
            app.update_at_menu();
        }
        _ => {}
    }
}

/// First-time sidebar load: pull all channels, then lazy-load threads for the
/// channel the current chat thread belongs to (if we can locate it). All other
/// channels' threads are fetched on demand when the user navigates to them.
async fn initialize_sidebar(client: &Arc<Client>, app: &mut App) {
    use proto::methods::{ChannelListResult, ThreadListResult};
    let channels = match client
        .call::<_, ChannelListResult>(method::CHANNEL_LIST, json!({}))
        .await
    {
        Ok(res) => res.channels,
        Err(e) => {
            app.set_status(format!("channel/list failed: {}", e));
            Vec::new()
        }
    };
    if let Some(s) = app.sidebar.as_mut() {
        s.replace_channels(channels.clone());
    }
    // Short-circuit: when bound to a channel's common area, we already
    // know the owning channel — skip the thread-scan and just refresh
    // that channel's members cache.
    if matches!(app.scope_kind, ScopeKind::Channel) && app.has_scope() {
        let ch_id = app.thread_id.clone();
        if let Some(s) = app.sidebar.as_mut() {
            if let Some(idx) = s.channels.iter().position(|c| c.id == ch_id) {
                s.selected_channel_idx = idx;
                s.sync_state();
            }
        }
        refresh_members(client, app, &ch_id).await;
        return;
    }
    // Thread-bound path: scan threads across channels to find the owning
    // channel so we can auto-focus it in the sidebar.
    let current_thread_id = app.thread_id.clone();
    if current_thread_id.is_empty() {
        return;
    }
    let mut owning_channel: Option<String> = None;
    for ch in &channels {
        let res = client
            .call::<_, ThreadListResult>(method::THREAD_LIST, json!({ "channelId": ch.id }))
            .await;
        match res {
            Ok(list) => {
                let hit = list.threads.iter().any(|t| t.id == current_thread_id);
                if let Some(s) = app.sidebar.as_mut() {
                    s.replace_threads(&ch.id, list.threads);
                }
                if hit {
                    if let Some(s) = app.sidebar.as_mut() {
                        s.focus_channel_of_current_thread();
                    }
                    owning_channel = Some(ch.id.clone());
                    break;
                }
            }
            Err(e) => {
                app.set_status(format!("thread/list failed: {}", e));
            }
        }
    }
    // Lazily fetch members for the active chat channel only — other channels
    // refresh on demand the first time the operator opens their Members pane
    // via `/members` or by switching channel focus.
    if let Some(ch_id) = owning_channel {
        refresh_members(client, app, &ch_id).await;
    }
}

/// Pull the latest member set for `channel_id` and patch the sidebar cache.
/// Best-effort: surfaces failure on the status line and leaves the existing
/// cache intact (so subsequent renders don't flicker an empty list).
async fn refresh_members(client: &Arc<Client>, app: &mut App, channel_id: &str) {
    use super::sidebar::MemberRow;
    use proto::methods::ChannelMembersResult;
    let res = client
        .call::<_, ChannelMembersResult>(
            method::CHANNEL_MEMBERS,
            json!({ "channelId": channel_id }),
        )
        .await;
    match res {
        Ok(r) => {
            // Side-effect: enrich the actor directory caches so future
            // @-mention picks and history rendering know each member's
            // display name + kind without a follow-up `actor/list` call.
            for a in &r.members {
                let name = if a.display_name.is_empty() {
                    a.id.clone()
                } else {
                    a.display_name.clone()
                };
                let kind_label = match a.kind {
                    proto::types::ActorKind::Human => "human",
                    proto::types::ActorKind::Agent => "agent",
                    proto::types::ActorKind::Service => "service",
                };
                app.actor_kinds.insert(a.id.clone(), kind_label.to_string());
                app.display_for.insert(a.id.clone(), name);
                if matches!(a.kind, proto::types::ActorKind::Agent) {
                    app.agent_ids.insert(a.id.clone());
                }
            }
            let rows: Vec<MemberRow> = r.members.iter().map(MemberRow::from_actor).collect();
            if let Some(s) = app.sidebar.as_mut() {
                s.replace_members(channel_id, rows);
            }
        }
        Err(e) => app.set_status(format!("channel/members failed: {}", e)),
    }
}

/// Sidebar key dispatch. Returns true if the key was consumed (so the chat
/// input handler should not see it). Esc / Ctrl+B closes the sidebar; other
/// keys either move the cursor, switch panes, open a CRUD prompt, or trigger
/// the actual channel/thread switch.
async fn handle_sidebar_key(client: &Arc<Client>, app: &mut App, key: KeyEvent) -> bool {
    use proto::methods::ThreadListResult;
    if matches!(key.code, KeyCode::Esc) {
        app.sidebar = None;
        return true;
    }
    match key.code {
        KeyCode::Up => {
            if let Some(s) = app.sidebar.as_mut() {
                s.move_up();
            }
            true
        }
        KeyCode::Down => {
            if let Some(s) = app.sidebar.as_mut() {
                s.move_down();
            }
            true
        }
        KeyCode::Tab => {
            if let Some(s) = app.sidebar.as_mut() {
                s.next_pane();
            }
            true
        }
        KeyCode::Enter => {
            // Pull out everything we need from the sidebar before any await,
            // since `.await` on `&mut app` would invalidate borrows.
            let (focus, channel_id, thread_id, has_threads_cached) = {
                let Some(s) = app.sidebar.as_ref() else {
                    return true;
                };
                let ch = s.selected_channel().map(|c| c.id.clone());
                let th = s.selected_thread().map(|t| t.id.clone());
                let cached = ch
                    .as_ref()
                    .map(|cid| s.threads_by_channel.contains_key(cid))
                    .unwrap_or(false);
                (s.focus, ch, th, cached)
            };
            match focus {
                SidebarFocus::Channels => {
                    if let Some(cid) = channel_id {
                        if !has_threads_cached {
                            match client
                                .call::<_, ThreadListResult>(
                                    method::THREAD_LIST,
                                    json!({ "channelId": cid }),
                                )
                                .await
                            {
                                Ok(res) => {
                                    if let Some(s) = app.sidebar.as_mut() {
                                        s.replace_threads(&cid, res.threads);
                                    }
                                }
                                Err(e) => {
                                    app.set_status(format!("thread/list failed: {}", e));
                                }
                            }
                        }
                        if let Some(s) = app.sidebar.as_mut() {
                            s.next_pane();
                        }
                    }
                    true
                }
                SidebarFocus::Threads => {
                    if let Some(tid) = thread_id {
                        let current = app.current_scope();
                        let target = ScopeRef {
                            kind: ScopeKind::Thread,
                            id: tid,
                        };
                        if Some(&target) != current.as_ref() {
                            app.pending_scope_switch = Some(target);
                        }
                        // Hide the sidebar after a switch so the chat fills the
                        // screen again — Ctrl+B toggles it back if needed.
                        app.sidebar = None;
                    }
                    true
                }
                SidebarFocus::Members => {
                    // Enter on a member is a no-op for v1 — the actionable
                    // keys live on `i`/`x`. Cycle back to Channels so the
                    // operator gets a visible response to their keystroke.
                    if let Some(s) = app.sidebar.as_mut() {
                        s.next_pane();
                    }
                    true
                }
            }
        }
        KeyCode::Char('c') => {
            // Bind chat to the selected channel's common area. Only
            // meaningful when the Channels pane has focus; ignore on
            // other panes so `c` remains free for their future use.
            let (focus, channel_id) = match app.sidebar.as_ref() {
                Some(s) => (s.focus, s.selected_channel().map(|c| c.id.clone())),
                None => return true,
            };
            if !matches!(focus, SidebarFocus::Channels) {
                return true;
            }
            let Some(cid) = channel_id else {
                return true;
            };
            let current = app.current_scope();
            let target = ScopeRef {
                kind: ScopeKind::Channel,
                id: cid,
            };
            if Some(&target) != current.as_ref() {
                app.pending_scope_switch = Some(target);
            }
            app.sidebar = None;
            true
        }
        KeyCode::Char('n') => {
            open_create_prompt(app);
            true
        }
        KeyCode::Char('r') => {
            open_rename_prompt(app);
            true
        }
        KeyCode::Char('d') => {
            open_delete_confirm(app);
            true
        }
        KeyCode::Char('i') => {
            // Picker-driven invite: only valid on the Members pane to avoid
            // accidentally opening it while navigating channels/threads.
            let on_members = app
                .sidebar
                .as_ref()
                .map(|s| matches!(s.focus, SidebarFocus::Members))
                .unwrap_or(false);
            if on_members {
                open_invite_actor_picker(client, app).await;
            }
            on_members
        }
        KeyCode::Char('I') => {
            // Free-text fallback for an actor id we don't have cached
            // (operator just registered an offline agent's spec, etc.).
            let on_members = app
                .sidebar
                .as_ref()
                .map(|s| matches!(s.focus, SidebarFocus::Members))
                .unwrap_or(false);
            if on_members {
                open_invite_by_id_prompt(app);
            }
            on_members
        }
        KeyCode::Char('x') => {
            let on_members = app
                .sidebar
                .as_ref()
                .map(|s| matches!(s.focus, SidebarFocus::Members))
                .unwrap_or(false);
            if on_members {
                open_revoke_confirm(app);
            }
            on_members
        }
        _ => false,
    }
}

fn open_invite_by_id_prompt(app: &mut App) {
    let Some(s) = app.sidebar.as_ref() else {
        return;
    };
    let Some(ch) = s.selected_channel() else {
        app.set_status("select a channel first");
        return;
    };
    app.prompt = Some(PromptModal::text(
        PromptKind::InviteToChannel {
            channel_id: ch.id.clone(),
        },
        format!("Invite into #{} (actor id)", ch.title),
        "",
    ));
}

fn open_revoke_confirm(app: &mut App) {
    let Some(s) = app.sidebar.as_ref() else {
        return;
    };
    let Some(ch) = s.selected_channel().cloned() else {
        app.set_status("select a channel first");
        return;
    };
    let Some(member) = s.selected_member().cloned() else {
        app.set_status("nothing selected");
        return;
    };
    if Some(member.actor_id.as_str()) == app.actor_id.as_str().into() {
        app.set_status("can't revoke yourself from chat — leave the channel from the CLI");
        return;
    }
    app.prompt = Some(PromptModal::confirm(
        ConfirmKind::RevokeFromChannel {
            channel_id: ch.id,
            actor_id: member.actor_id.clone(),
            display: member.display.clone(),
        },
        "Revoke member",
        format!("Remove {} from #{}?", member.display, ch.title),
    ));
}

async fn open_invite_actor_picker(client: &Arc<Client>, app: &mut App) {
    use proto::methods::ActorListResult;
    let Some(channel_id) = app
        .sidebar
        .as_ref()
        .and_then(|s| s.selected_channel().map(|c| c.id.clone()))
    else {
        app.set_status("select a channel first");
        return;
    };
    let already_members: std::collections::HashSet<String> = app
        .sidebar
        .as_ref()
        .and_then(|s| s.members_by_channel.get(&channel_id))
        .map(|rows| rows.iter().map(|r| r.actor_id.clone()).collect())
        .unwrap_or_default();

    let actors = match client
        .call::<_, ActorListResult>(method::ACTOR_LIST, json!({}))
        .await
    {
        Ok(r) => r.actors,
        Err(e) => {
            app.set_status(format!("actor/list failed: {}", e));
            return;
        }
    };
    let items: Vec<PickerItem> = actors
        .into_iter()
        .filter(|a| !already_members.contains(&a.id))
        .map(|a| {
            let kind_label = match a.kind {
                proto::types::ActorKind::Human => "human",
                proto::types::ActorKind::Agent => "agent",
                proto::types::ActorKind::Service => "service",
            };
            let label = if a.display_name.is_empty() {
                a.id.clone()
            } else {
                format!("{} ({})", a.display_name, kind_label)
            };
            PickerItem::new(a.id, label).with_hint(kind_label)
        })
        .collect();
    app.open_invite_picker(channel_id, items);
}

fn open_create_prompt(app: &mut App) {
    let Some(s) = app.sidebar.as_ref() else {
        return;
    };
    match s.focus {
        SidebarFocus::Channels => {
            app.prompt = Some(PromptModal::text(
                PromptKind::CreateChannel,
                "New channel title",
                "",
            ));
        }
        SidebarFocus::Threads => {
            let Some(ch) = s.selected_channel() else {
                app.set_status("select a channel first");
                return;
            };
            app.prompt = Some(PromptModal::text(
                PromptKind::CreateThread {
                    channel_id: ch.id.clone(),
                },
                format!("New thread in #{}", ch.title),
                "",
            ));
        }
        SidebarFocus::Members => {
            // No `n` semantic on Members — invite uses `i`/`I` (wired in
            // task #10). Hint the operator instead of opening a prompt.
            app.set_status("press i to invite a member, I to invite by id");
        }
    }
}

fn open_rename_prompt(app: &mut App) {
    let Some(s) = app.sidebar.as_ref() else {
        return;
    };
    match s.focus {
        SidebarFocus::Channels => {
            let Some(ch) = s.selected_channel() else {
                app.set_status("nothing selected");
                return;
            };
            app.prompt = Some(PromptModal::text(
                PromptKind::RenameChannel {
                    channel_id: ch.id.clone(),
                },
                format!("Rename channel #{}", ch.title),
                ch.title.clone(),
            ));
        }
        SidebarFocus::Threads => {
            let Some(th) = s.selected_thread() else {
                app.set_status("nothing selected");
                return;
            };
            app.prompt = Some(PromptModal::text(
                PromptKind::RenameThread {
                    thread_id: th.id.clone(),
                },
                format!("Rename thread '{}'", th.title),
                th.title.clone(),
            ));
        }
        SidebarFocus::Members => {
            // Members aren't renameable from chat — that's a property of
            // the actor row itself, not the channel membership.
            app.set_status("members aren't renameable here");
        }
    }
}

fn open_delete_confirm(app: &mut App) {
    let Some(s) = app.sidebar.as_ref() else {
        return;
    };
    match s.focus {
        SidebarFocus::Channels => {
            let Some(ch) = s.selected_channel() else {
                app.set_status("nothing selected");
                return;
            };
            app.prompt = Some(PromptModal::confirm(
                ConfirmKind::DeleteChannel {
                    channel_id: ch.id.clone(),
                },
                "Delete channel",
                format!(
                    "Delete channel #{}? Channel must be empty (delete its threads first)",
                    ch.title
                ),
            ));
        }
        SidebarFocus::Threads => {
            let Some(th) = s.selected_thread() else {
                app.set_status("nothing selected");
                return;
            };
            app.prompt = Some(PromptModal::confirm(
                ConfirmKind::DeleteThread {
                    thread_id: th.id.clone(),
                },
                "Delete thread",
                format!(
                    "Delete thread '{}'? Event history is left orphaned.",
                    th.title
                ),
            ));
        }
        SidebarFocus::Members => {
            // Use `x` (wired in task #10) to revoke; `d` is a destructive
            // shortcut bound to channel/thread deletion and would be
            // surprising here.
            app.set_status("press x to remove the selected member");
        }
    }
}

async fn handle_prompt_submit(
    client: &Arc<Client>,
    app: &mut App,
    kind: PromptKind,
    value: String,
) {
    use proto::methods::{
        ChannelCreateResult, ChannelUpdateResult, ThreadCreateResult, ThreadUpdateResult,
    };
    match kind {
        PromptKind::CreateChannel => {
            let res = client
                .call::<_, ChannelCreateResult>(method::CHANNEL_CREATE, json!({ "title": value }))
                .await;
            match res {
                Ok(r) => {
                    if let Some(s) = app.sidebar.as_mut() {
                        s.add_channel(r.channel.clone());
                    }
                    app.set_status(format!("created channel #{}", r.channel.title));
                }
                Err(e) => app.set_status(format!("channel/create failed: {}", e)),
            }
        }
        PromptKind::RenameChannel { channel_id } => {
            let res = client
                .call::<_, ChannelUpdateResult>(
                    method::CHANNEL_UPDATE,
                    json!({ "channelId": channel_id, "title": value }),
                )
                .await;
            match res {
                Ok(r) => {
                    if let Some(s) = app.sidebar.as_mut() {
                        s.rename_channel(r.channel.clone());
                    }
                    app.set_status(format!("renamed → #{}", r.channel.title));
                }
                Err(e) => app.set_status(format!("channel/update failed: {}", e)),
            }
        }
        PromptKind::CreateThread { channel_id } => {
            let res = client
                .call::<_, ThreadCreateResult>(
                    method::THREAD_CREATE,
                    json!({ "channelId": channel_id, "title": value }),
                )
                .await;
            match res {
                Ok(r) => {
                    let new_thread_id = r.thread.id.clone();
                    let new_thread_title = r.thread.title.clone();
                    if let Some(s) = app.sidebar.as_mut() {
                        s.add_thread(r.thread);
                    }
                    // Auto-bind into the newly created thread so the user
                    // can start chatting without a second keystroke. The
                    // main loop drains `pending_scope_switch` on the next
                    // frame and re-subscribes.
                    app.pending_scope_switch = Some(ScopeRef {
                        kind: ScopeKind::Thread,
                        id: new_thread_id,
                    });
                    app.sidebar = None;
                    app.set_status(format!("created thread '{}'", new_thread_title));
                }
                Err(e) => app.set_status(format!("thread/create failed: {}", e)),
            }
        }
        PromptKind::RenameThread { thread_id } => {
            let res = client
                .call::<_, ThreadUpdateResult>(
                    method::THREAD_UPDATE,
                    json!({ "threadId": thread_id, "title": value }),
                )
                .await;
            match res {
                Ok(r) => {
                    if let Some(s) = app.sidebar.as_mut() {
                        s.rename_thread(r.thread.clone());
                    }
                    app.set_status(format!("renamed → '{}'", r.thread.title));
                }
                Err(e) => app.set_status(format!("thread/update failed: {}", e)),
            }
        }
        PromptKind::InviteToChannel { channel_id } => {
            do_channel_invite(client, app, channel_id, value).await;
        }
    }
}

async fn handle_confirm(client: &Arc<Client>, app: &mut App, kind: ConfirmKind) {
    use proto::methods::{ChannelDeleteResult, ThreadDeleteResult};
    match kind {
        ConfirmKind::DeleteChannel { channel_id } => {
            let res = client
                .call::<_, ChannelDeleteResult>(
                    method::CHANNEL_DELETE,
                    json!({ "channelId": channel_id }),
                )
                .await;
            match res {
                Ok(_) => {
                    if let Some(s) = app.sidebar.as_mut() {
                        s.remove_channel(&channel_id);
                    }
                    app.set_status("channel deleted");
                }
                Err(e) => app.set_status(format!("channel/delete failed: {}", e)),
            }
        }
        ConfirmKind::RevokeFromChannel {
            channel_id,
            actor_id,
            display: _,
        } => {
            do_channel_revoke(client, app, channel_id, actor_id).await;
            return;
        }
        ConfirmKind::InviteThenHandoff {
            channel_id,
            actor_id,
            message,
        } => {
            // Snapshot the scope before re-borrowing app for status updates.
            // Falls back to thread scope for back-compat; the picker flow
            // only runs from inside a bound scope anyway.
            let scope = app.current_scope().unwrap_or(ScopeRef {
                kind: ScopeKind::Thread,
                id: app.thread_id.clone(),
            });
            do_channel_invite(client, app, channel_id, actor_id.clone()).await;
            // Even if invite failed, attempting the handoff surfaces the
            // server's PERMISSION_DENIED verbatim — useful signal for the
            // operator. So we don't gate on invite success.
            do_handoff_with_message(client, app, actor_id, message, &scope).await;
            return;
        }
        ConfirmKind::DeleteThread { thread_id } => {
            // We need the owning channel id to update the local cache. Look it
            // up from the sidebar before issuing the delete so we can still
            // patch the cache cleanly even if the thread vanishes server-side.
            let owning_channel = app.sidebar.as_ref().and_then(|s| {
                s.threads_by_channel.iter().find_map(|(ch, threads)| {
                    threads
                        .iter()
                        .find(|t| t.id == thread_id)
                        .map(|_| ch.clone())
                })
            });
            let res = client
                .call::<_, ThreadDeleteResult>(
                    method::THREAD_DELETE,
                    json!({ "threadId": thread_id }),
                )
                .await;
            match res {
                Ok(_) => {
                    if let (Some(s), Some(ch)) = (app.sidebar.as_mut(), owning_channel.as_ref()) {
                        s.remove_thread(ch, &thread_id);
                    }
                    app.set_status("thread deleted");
                }
                Err(e) => app.set_status(format!("thread/delete failed: {}", e)),
            }
        }
    }
}

fn handle_mouse(app: &mut App, mouse: MouseEvent) {
    match mouse.kind {
        MouseEventKind::ScrollUp => {
            app.clear_history_selection();
            app.scroll_up(3);
        }
        MouseEventKind::ScrollDown => {
            app.clear_history_selection();
            app.scroll_down(3);
        }
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
    // Best-effort membership pre-check: if the target isn't in the current
    // channel's member cache and the channel is private, route through a
    // confirm modal that does `channel/invite` then the handoff. This
    // pre-check is racy (membership cache can be stale) but the server is
    // authoritative — `event/append` will surface PERMISSION_DENIED if the
    // local cache lied to us, which we surface verbatim from `do_handoff`.
    if let Some(channel_id) = current_chat_channel(app) {
        if needs_invite_for(app, &channel_id, &target) {
            let display = app
                .display_for
                .get(&target)
                .cloned()
                .unwrap_or_else(|| target.clone());
            let title = app
                .sidebar
                .as_ref()
                .and_then(|s| s.channels.iter().find(|c| c.id == channel_id))
                .map(|c| c.title.clone())
                .unwrap_or_else(|| channel_id.clone());
            app.prompt = Some(PromptModal::confirm(
                ConfirmKind::InviteThenHandoff {
                    channel_id,
                    actor_id: target.clone(),
                    message,
                },
                "Invite first?",
                format!(
                    "@{} isn't in #{}. Invite them first, then send the handoff?",
                    display, title
                ),
            ));
            return;
        }
    }
    do_handoff_with_message(client, app, target, message, scope).await;
}

/// `true` iff we have enough cache to know that `actor_id` is NOT yet a
/// member of `channel_id` AND the channel is private (so the missing
/// membership actually matters). Returns `false` in the absence of a
/// definitive answer — better to attempt the handoff and let the server
/// reject than to spam the operator with bogus invite confirms.
fn needs_invite_for(app: &App, channel_id: &str, actor_id: &str) -> bool {
    let Some(s) = app.sidebar.as_ref() else {
        return false;
    };
    let Some(ch) = s.channels.iter().find(|c| c.id == channel_id) else {
        return false;
    };
    if !matches!(ch.visibility, proto::types::ChannelVisibility::Private) {
        return false;
    }
    // Prefer the resolved members_by_channel cache (populated lazily); fall
    // back to Channel.members which is always sent on the wire.
    if let Some(rows) = s.members_by_channel.get(channel_id) {
        !rows.iter().any(|r| r.actor_id == actor_id)
    } else {
        !ch.members.iter().any(|m| m == actor_id)
    }
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
        PickerKind::InviteActor { channel_id } => {
            do_channel_invite(client, app, channel_id, id).await;
        }
    }
}

async fn do_channel_invite(
    client: &Arc<Client>,
    app: &mut App,
    channel_id: String,
    actor_id: String,
) {
    use proto::methods::ChannelInviteResult;
    let res = client
        .call::<_, ChannelInviteResult>(
            method::CHANNEL_INVITE,
            json!({ "channelId": channel_id, "actorId": actor_id }),
        )
        .await;
    match res {
        Ok(r) => {
            // Optimistic local cache patch: the server will also broadcast a
            // `channel.invited` notification but echoing it now means the
            // sidebar refreshes immediately rather than after a round-trip.
            patch_member_cache_after_invite(app, &channel_id, &actor_id);
            // Replace channel meta so members list / visibility stays accurate.
            if let Some(s) = app.sidebar.as_mut() {
                s.rename_channel(r.channel.clone()); // reuses sort + cursor logic
                if let Some(ch) = s.channels.iter_mut().find(|c| c.id == r.channel.id) {
                    ch.members = r.channel.members.clone();
                    ch.visibility = r.channel.visibility;
                }
            }
            app.set_status(format!("invited {} into #{}", actor_id, r.channel.title));
        }
        Err(e) => app.set_status(format!("channel/invite failed: {}", e)),
    }
}

fn patch_member_cache_after_invite(app: &mut App, channel_id: &str, actor_id: &str) {
    use super::sidebar::MemberRow;
    let display = app
        .display_for
        .get(actor_id)
        .cloned()
        .unwrap_or_else(|| actor_id.to_string());
    let kind = match app.actor_kinds.get(actor_id).map(String::as_str) {
        Some("agent") => proto::types::ActorKind::Agent,
        Some("service") => proto::types::ActorKind::Service,
        _ => proto::types::ActorKind::Human,
    };
    if let Some(s) = app.sidebar.as_mut() {
        s.add_member(
            channel_id,
            MemberRow {
                actor_id: actor_id.to_string(),
                display,
                kind,
            },
        );
    }
}

async fn do_channel_revoke(
    client: &Arc<Client>,
    app: &mut App,
    channel_id: String,
    actor_id: String,
) {
    use proto::methods::ChannelRevokeResult;
    let res = client
        .call::<_, ChannelRevokeResult>(
            method::CHANNEL_REVOKE,
            json!({ "channelId": channel_id, "actorId": actor_id }),
        )
        .await;
    match res {
        Ok(r) => {
            if let Some(s) = app.sidebar.as_mut() {
                s.remove_member(&channel_id, &actor_id);
                if let Some(ch) = s.channels.iter_mut().find(|c| c.id == r.channel.id) {
                    ch.members = r.channel.members.clone();
                    ch.visibility = r.channel.visibility;
                }
            }
            app.set_status(format!("revoked {} from #{}", actor_id, r.channel.title));
        }
        Err(e) => app.set_status(format!("channel/revoke failed: {}", e)),
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
        "cancel" => {
            // `/cancel`               → the unique open turn (or selected one)
            // `/cancel @agent`        → most recent open turn for that actor
            let target = arg.strip_prefix('@').map(|s| s.trim().to_string());
            cancel_in_scope(client, app, scope, target).await;
        }
        "invite" => {
            // Resolve the current chat thread's owning channel — `/invite`
            // always targets the channel we're chatting in, not whatever the
            // sidebar happens to highlight (which may be another channel the
            // operator was browsing).
            let Some(channel_id) = current_chat_channel(app) else {
                app.set_status(
                    "can't resolve the current channel; open the sidebar (Ctrl+B) first",
                );
                return;
            };
            if arg.is_empty() {
                // Snapshot for the open helper to consume.
                if let Some(s) = app.sidebar.as_mut() {
                    if let Some(idx) = s.channels.iter().position(|c| c.id == channel_id) {
                        s.selected_channel_idx = idx;
                        s.focus = SidebarFocus::Members;
                    }
                }
                open_invite_actor_picker(client, app).await;
            } else {
                do_channel_invite(client, app, channel_id, arg).await;
            }
        }
        "members" => {
            let Some(channel_id) = current_chat_channel(app) else {
                app.set_status(
                    "can't resolve the current channel; open the sidebar (Ctrl+B) first",
                );
                return;
            };
            refresh_members(client, app, &channel_id).await;
            print_members_into_history(app, &channel_id);
        }
        "quit" | "q" | "exit" => app.should_quit = true,
        other => app.set_status(format!("unknown /{}", other)),
    }
}

fn current_chat_channel(app: &App) -> Option<String> {
    // Channel-bound mode: the bound scope id *is* the channel id.
    if matches!(app.scope_kind, ScopeKind::Channel) && app.has_scope() {
        return Some(app.thread_id.clone());
    }
    let s = app.sidebar.as_ref()?;
    s.threads_by_channel.iter().find_map(|(ch, threads)| {
        threads
            .iter()
            .find(|t| t.id == app.thread_id)
            .map(|_| ch.clone())
    })
}

fn print_members_into_history(app: &mut App, channel_id: &str) {
    let rows = app
        .sidebar
        .as_ref()
        .and_then(|s| s.members_by_channel.get(channel_id))
        .cloned()
        .unwrap_or_default();
    let title = app
        .sidebar
        .as_ref()
        .and_then(|s| s.channels.iter().find(|c| c.id == channel_id))
        .map(|c| c.title.clone())
        .unwrap_or_else(|| channel_id.to_string());
    if rows.is_empty() {
        app.history
            .push_system(format!("(no members for #{}, public channel?)", title));
        return;
    }
    app.history.push_system(format!("Members of #{}:", title));
    for r in rows {
        let kind = match r.kind {
            proto::types::ActorKind::Human => "human",
            proto::types::ActorKind::Agent => "agent",
            proto::types::ActorKind::Service => "service",
        };
        app.history
            .push_system(format!("  • {:<24} {} ({})", r.actor_id, r.display, kind));
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
    if !app.has_scope() {
        app.set_status("open a channel or thread first (Ctrl+B, then Enter/c)");
        return;
    }
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
    let Some(scope) = app.current_scope() else {
        app.set_status("no scope bound — cannot respond to action");
        return;
    };
    let payload = json!({
        "event": {
            "type": "action.response",
            "actorId": app.actor_id,
            "scope": scope,
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
    use proto::methods::{ActorListResult, AgentListResult};
    use std::collections::BTreeMap;

    // (id, display, status). v0 path fills status from agent/list; v1 path
    // (where the registry lives in `joi agent serve`) leaves status empty.
    let mut rows: BTreeMap<String, (String, String)> = BTreeMap::new();

    if let Ok(list) = client
        .call::<_, AgentListResult>(method::AGENT_LIST, json!({}))
        .await
    {
        for ai in list.agents {
            let actor = ai.spec.actor;
            rows.insert(actor.id.clone(), (actor.display_name, ai.status));
        }
    }
    if let Ok(list) = client
        .call::<_, ActorListResult>(method::ACTOR_LIST, json!({}))
        .await
    {
        for a in list.actors {
            if !matches!(a.kind, proto::types::ActorKind::Agent) {
                continue;
            }
            rows.entry(a.id).or_insert((a.display_name, String::new()));
        }
    }

    if rows.is_empty() {
        app.history.push_system("(no agents registered)");
        return;
    }
    app.history.push_system("Registered agents:".to_string());
    for (id, (display, status)) in rows {
        let line = if status.is_empty() {
            format!("  • {:<24} {}", id, display)
        } else {
            format!("  • {:<24} {:<20} ({})", id, display, status)
        };
        app.history.push_system(line);
    }
}

fn arm_reply_target(app: &mut App, event_id: String) {
    let preview = app
        .history
        .reply_targets(&|id| app.display_name_for(id))
        .into_iter()
        .find(|(id, _)| id == &event_id)
        .map(|(_, label)| label)
        .unwrap_or_else(|| "(unknown message)".to_string());
    if app.input.display_text().trim_start().starts_with("/reply") {
        app.input.clear();
        app.slash_menu = None;
        app.at_menu = None;
    }
    app.set_reply_target(event_id, preview);
}

fn reply_selected_history(app: &mut App) {
    let Some((event_id, _)) = app.selected_history_target() else {
        app.set_status("select a message with ↑/↓ first");
        return;
    };
    arm_reply_target(app, event_id);
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

async fn handle_paste(client: &Arc<Client>, app: &mut App, text: String, scope: &ScopeRef) {
    let normalized = normalize_pasted_text(&text);
    if normalized.is_empty() {
        return;
    }
    let char_count = normalized.chars().count();
    if char_count < 300 {
        app.input.push_str(&normalized);
    } else if char_count <= 5000 {
        let token_id = app.next_paste_token_id();
        app.input.push_pasted_chunk(token_id, normalized);
    } else {
        match save_pasted_content_to_workspace(client, app, scope, normalized).await {
            Ok(display) => app.input.push_saved_workspace_ref(display),
            Err(e) => {
                app.set_status(format!("save pasted content failed: {e}"));
                return;
            }
        }
    }
    app.update_slash_menu();
    app.update_at_menu();
}

fn normalize_pasted_text(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

async fn save_pasted_content_to_workspace(
    client: &Arc<Client>,
    app: &mut App,
    scope: &ScopeRef,
    text: String,
) -> anyhow::Result<String> {
    use anyhow::{anyhow, bail};
    use proto::methods::ArtifactPublishResult;

    if !app.has_scope() {
        bail!("open a channel or thread before pasting content larger than 5000 characters");
    }

    let res: ArtifactPublishResult = client
        .call(
            method::ARTIFACT_PUBLISH,
            json!({
                "createdBy": app.actor_id,
                "scope": scope,
                "ingress": {
                    "kind": "inline_text",
                    "name": "pasted-content.md",
                    "mediaType": "text/markdown",
                    "text": text,
                }
            }),
        )
        .await?;

    let entry_id = res
        .artifact
        ._meta
        .as_ref()
        .and_then(|meta| meta.get("workspaceEntryId"))
        .and_then(|value| value.as_u64())
        .ok_or_else(|| anyhow!("workspace entry id missing from artifact response"))?;

    Ok(format!(
        "[Saved pasted content to workspace ({}) id={entry_id}]",
        human_size(res.artifact.size)
    ))
}

fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    let bytes_f = bytes as f64;
    if bytes_f >= MB {
        format!("{:.1} MB", bytes_f / MB)
    } else if bytes_f >= KB {
        format!("{:.1} KB", bytes_f / KB)
    } else {
        format!("{bytes} B")
    }
}

async fn send_message(client: &Arc<Client>, app: &mut App, text: &str, scope: &ScopeRef) {
    use proto::methods::EventAppendResult;
    if !app.has_scope() {
        app.set_status("open a channel or thread first (Ctrl+B, then Enter/c)");
        return;
    }
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
            app.selected_history_idx = app.history.newest_replyable_index();
            app.reply_target = None;
            app.jump_to_bottom();
        }
        Err(e) => app.set_status(format!("send failed: {}", e)),
    }
}

/// True when the current scope has at least one open turn that's cancellable.
/// Source of truth is the `open_turns` registry (driven by `turn.opened` /
/// `turn.closed` notifications), not the chat history's streaming-bubble
/// state — agents may take a turn without ever emitting a stream delta.
fn has_open_turn(app: &App, scope: &ScopeRef) -> bool {
    !app.open_turns_in_scope(scope).is_empty()
}

/// Pick which in-flight turn to cancel.
///
/// - `target_actor: Some(actor)` → most recent open turn for that actor;
///   status hint if none found.
/// - `target_actor: None`        → the bubble at `selected_history_idx` if
///   it's tied to an open turn; otherwise the unique open turn in scope;
///   if multiple, status hint asking the user to disambiguate.
fn pick_cancel_target(
    app: &App,
    scope: &ScopeRef,
    target_actor: Option<&str>,
) -> Result<(String, String), String> {
    let open = app.open_turns_in_scope(scope);
    if open.is_empty() {
        return Err("no in-flight agent turn to cancel".into());
    }
    if let Some(actor) = target_actor {
        if let Some(t) = open.iter().rev().find(|t| t.actor_id == actor) {
            return Ok((t.actor_id.clone(), t.turn_id.clone()));
        }
        return Err(format!("no in-flight turn for @{actor}"));
    }
    // If the user has explicitly selected a bubble whose turn is still open,
    // prefer that — it matches the selection-driven mental model.
    if let Some(idx) = app.selected_history_idx {
        if let Some(b) = app.history.bubbles.get(idx) {
            if let Some(tid) = b.turn_id.as_ref() {
                if open.iter().any(|t| t.turn_id.as_str() == tid.as_str()) {
                    return Ok((b.actor_id.clone(), tid.clone()));
                }
            }
        }
    }
    if open.len() == 1 {
        let t = open[0];
        return Ok((t.actor_id.clone(), t.turn_id.clone()));
    }
    Err("multiple agents are working — select one with ↑/↓ or use /cancel @agent".into())
}

async fn cancel_in_scope(
    client: &Arc<Client>,
    app: &mut App,
    scope: &ScopeRef,
    target_actor: Option<String>,
) {
    let (actor, turn_id) = match pick_cancel_target(app, scope, target_actor.as_deref()) {
        Ok(t) => t,
        Err(msg) => {
            app.set_status(msg);
            return;
        }
    };
    let display = app.display_name_for(&actor);
    let res = client
        .call_raw(
            method::TURN_CLOSE,
            Some(json!({
                "turnId": turn_id,
                "status": "cancelled",
            })),
        )
        .await;
    match res {
        Ok(_) => {
            // Optimistic cleanup: drop our local entry so a quick second Esc
            // doesn't try to cancel the same already-cancelled turn before
            // the `turn.closed` notification round-trips back.
            app.open_turns.remove(&turn_id);
            app.set_status(format!("cancelled @{display}'s turn"))
        }
        Err(e) => app.set_status(format!("cancel failed: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::{arm_reply_target, message_relations, pick_cancel_target, reply_selected_history};
    use crate::cmd::chat::app::App;
    use crate::cmd::chat::history::{Bubble, BubbleKind, DeliveryState};
    use chrono::Utc;

    #[test]
    fn arm_reply_target_clears_reply_command_residue() {
        let mut app = App::new(
            "actor_human_current".into(),
            "thread_demo".into(),
            proto::types::ScopeKind::Thread,
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
            proto::types::ScopeKind::Thread,
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
            streaming: false,
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
            proto::types::ScopeKind::Thread,
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
            streaming: false,
        });
        app.set_reply_target("evt_123".into(), "evt_123".into());

        let (relations, _) = message_relations(&app);

        assert_eq!(relations.len(), 1);
        assert_eq!(relations[0]["kind"], "replies_to");
    }

    #[test]
    fn reply_selected_history_arms_selected_event() {
        let mut app = App::new(
            "actor_human_current".into(),
            "thread_demo".into(),
            proto::types::ScopeKind::Thread,
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
            streaming: false,
        });
        app.selected_history_idx = Some(0);

        reply_selected_history(&mut app);

        assert_eq!(
            app.reply_target
                .as_ref()
                .map(|target| target.event_id.as_str()),
            Some("evt_123")
        );
    }

    #[test]
    fn reply_selected_history_requires_selection() {
        let mut app = App::new(
            "actor_human_current".into(),
            "thread_demo".into(),
            proto::types::ScopeKind::Thread,
            "bojun.cbj".into(),
        );

        reply_selected_history(&mut app);

        assert_eq!(app.status, "select a message with ↑/↓ first");
    }

    fn test_scope() -> proto::types::ScopeRef {
        proto::types::ScopeRef {
            kind: proto::types::ScopeKind::Thread,
            id: "thread_demo".into(),
        }
    }

    fn app_with_open_turns(actors_and_turns: &[(&str, &str)]) -> App {
        use crate::cmd::chat::app::OpenTurn;
        let mut app = App::new(
            "actor_human_current".into(),
            "thread_demo".into(),
            proto::types::ScopeKind::Thread,
            "bojun.cbj".into(),
        );
        let scope = test_scope();
        for (a, t) in actors_and_turns {
            // Mirror real flow: register the open turn AND push a streaming
            // bubble. The bubble lets us exercise the selection-aware branch.
            app.open_turns.insert(
                (*t).into(),
                OpenTurn {
                    turn_id: (*t).into(),
                    actor_id: (*a).into(),
                    scope: scope.clone(),
                    opened_at: chrono::Utc::now(),
                },
            );
            app.history
                .append_stream_delta(a, t, "live", chrono::Utc::now());
        }
        app
    }

    #[test]
    fn pick_cancel_target_none_when_no_open_turns() {
        let app = app_with_open_turns(&[]);
        let err = pick_cancel_target(&app, &test_scope(), None).unwrap_err();
        assert!(err.contains("no in-flight"));
    }

    #[test]
    fn pick_cancel_target_picks_only_open_when_unspecified() {
        let app = app_with_open_turns(&[("Coder", "turn_1")]);
        let (a, t) = pick_cancel_target(&app, &test_scope(), None).unwrap();
        assert_eq!(a, "Coder");
        assert_eq!(t, "turn_1");
    }

    #[test]
    fn pick_cancel_target_requires_disambiguation_when_multiple() {
        let app = app_with_open_turns(&[("Coder", "turn_1"), ("OpenCode", "turn_2")]);
        let err = pick_cancel_target(&app, &test_scope(), None).unwrap_err();
        assert!(err.contains("/cancel @agent"));
    }

    #[test]
    fn pick_cancel_target_uses_selected_bubble() {
        let mut app = app_with_open_turns(&[("Coder", "turn_1"), ("OpenCode", "turn_2")]);
        // Select the first (Coder) bubble.
        app.selected_history_idx = Some(0);
        let (a, t) = pick_cancel_target(&app, &test_scope(), None).unwrap();
        assert_eq!(a, "Coder");
        assert_eq!(t, "turn_1");
    }

    #[test]
    fn pick_cancel_target_filters_by_actor() {
        let app = app_with_open_turns(&[("Coder", "turn_1"), ("OpenCode", "turn_2")]);
        let (a, t) = pick_cancel_target(&app, &test_scope(), Some("OpenCode")).unwrap();
        assert_eq!(a, "OpenCode");
        assert_eq!(t, "turn_2");
    }

    #[test]
    fn pick_cancel_target_actor_not_open_errors() {
        let app = app_with_open_turns(&[("Coder", "turn_1")]);
        let err = pick_cancel_target(&app, &test_scope(), Some("OpenCode")).unwrap_err();
        assert!(err.contains("@OpenCode"));
    }

    #[test]
    fn pick_cancel_target_works_without_streaming_bubbles() {
        // The whole point of the fix: an agent that hasn't emitted any
        // `turn/stream.update` deltas yet should still be cancellable.
        use crate::cmd::chat::app::OpenTurn;
        let mut app = App::new(
            "actor_human_current".into(),
            "thread_demo".into(),
            proto::types::ScopeKind::Thread,
            "bojun.cbj".into(),
        );
        app.open_turns.insert(
            "turn_silent".into(),
            OpenTurn {
                turn_id: "turn_silent".into(),
                actor_id: "Coder".into(),
                scope: test_scope(),
                opened_at: chrono::Utc::now(),
            },
        );
        // No streaming bubbles, no history at all — used to error before fix.
        let (a, t) = pick_cancel_target(&app, &test_scope(), None).unwrap();
        assert_eq!(a, "Coder");
        assert_eq!(t, "turn_silent");
    }

    #[test]
    fn pick_cancel_target_ignores_open_turns_in_other_scopes() {
        use crate::cmd::chat::app::OpenTurn;
        let mut app = App::new(
            "actor_human_current".into(),
            "thread_demo".into(),
            proto::types::ScopeKind::Thread,
            "bojun.cbj".into(),
        );
        let other = proto::types::ScopeRef {
            kind: proto::types::ScopeKind::Thread,
            id: "thread_other".into(),
        };
        app.open_turns.insert(
            "turn_x".into(),
            OpenTurn {
                turn_id: "turn_x".into(),
                actor_id: "Coder".into(),
                scope: other,
                opened_at: chrono::Utc::now(),
            },
        );
        let err = pick_cancel_target(&app, &test_scope(), None).unwrap_err();
        assert!(err.contains("no in-flight"));
    }
}
