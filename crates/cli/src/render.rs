use std::sync::OnceLock;

use chrono::Local;
use proto::types::*;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Pretty,
    Json,
}

static MODE: OnceLock<OutputMode> = OnceLock::new();

pub fn set_output_mode(mode: OutputMode) {
    let _ = MODE.set(mode);
}

pub fn output_mode() -> OutputMode {
    MODE.get().copied().unwrap_or(OutputMode::Pretty)
}

pub fn is_json() -> bool {
    output_mode() == OutputMode::Json
}

pub fn print_json<T: Serialize>(value: &T) {
    match serde_json::to_string(value) {
        Ok(s) => println!("{}", s),
        Err(e) => eprintln!("(json serialize failed: {})", e),
    }
}

pub fn render_event(event: &Event) {
    let ts = event.occurred_at.with_timezone(&Local).format("%H:%M:%S");
    let actor = &event.actor_id;
    let kind = &event.kind;
    match kind.as_str() {
        "content.add" => {
            let text = event
                .payload
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            // content.add carrying a HandsOffTo relation is a handoff —
            // surface the target so the line reads `... ↪ → target: text`.
            let handoff_target = event
                .relations
                .iter()
                .find(|r| matches!(r.kind, RelationKind::HandsOffTo))
                .map(|r| r.target.id.as_str());
            match handoff_target {
                Some(to) => println!("[{ts}] {actor} ↪ → {to}: {text}"),
                None => println!("[{ts}] {actor}: {text}"),
            }
        }
        "action.request" => {
            let title = event
                .payload
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("(action)");
            println!("[{ts}] {actor} ⌗ action.request id={}: {title}", event.id);
            if let Some(arr) = event.payload.get("choices").and_then(|v| v.as_array()) {
                for choice in arr {
                    let cid = choice.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    let label = choice.get("label").and_then(|v| v.as_str()).unwrap_or("");
                    println!("           - {cid}: {label}");
                }
            }
        }
        "action.response" => {
            let opt = event
                .payload
                .get("optionId")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            println!("[{ts}] {actor} ✓ action.response → {opt}");
        }
        "turn.close" => {
            let status = event
                .payload
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            println!("[{ts}] {actor} · turn closed ({status})");
        }
        other => {
            println!(
                "[{ts}] {actor} {other}: {}",
                serde_json::to_string(&event.payload).unwrap_or_default()
            );
        }
    }
}

/// Render a `turn/trace.update` notification. These frames are the agent's
/// private execution trail (tool calls, partial text, status, errors) and
/// only ever arrive at the turn owner — typically the agent itself when
/// connected as a debug client. Subscribed humans never see them.
pub fn render_trace_update(payload: &Value) {
    let turn_id = payload
        .get("turnId")
        .and_then(|v| v.as_str())
        .unwrap_or("?");
    let frame = match payload.get("frame") {
        Some(f) => f,
        None => return,
    };
    let kind = frame.get("kind").and_then(|v| v.as_str()).unwrap_or("?");
    let seq = frame.get("seq").and_then(|v| v.as_u64()).unwrap_or(0);
    let body = frame.get("payload").cloned().unwrap_or(Value::Null);
    let summary = match kind {
        "text.delta" => body
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        "tool.start" | "tool.update" | "tool.end" => {
            let name = body
                .get("toolName")
                .and_then(|v| v.as_str())
                .unwrap_or("tool");
            format!("{name}")
        }
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
        _ => serde_json::to_string(&body).unwrap_or_default(),
    };
    println!("· trace [{turn_id} #{seq}] {kind}: {summary}");
}

pub fn render_stream_update(payload: &Value) {
    let kind = payload.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    let data = payload.get("data").cloned().unwrap_or(Value::Null);
    match kind {
        "event.created" => {
            if let Some(ev) = data.get("event") {
                if let Ok(ev) = serde_json::from_value::<Event>(ev.clone()) {
                    render_event(&ev);
                }
            }
        }
        "turn.opened" => {
            if let Some(t) = data.get("turn") {
                if let Some(actor) = t.get("actorId").and_then(|v| v.as_str()) {
                    println!("· {} opened a turn", actor);
                }
            }
        }
        "turn.closed" => {
            if let Some(t) = data.get("turn") {
                if let Some(actor) = t.get("actorId").and_then(|v| v.as_str()) {
                    println!("· {} closed a turn", actor);
                }
            }
        }
        other => {
            println!("· stream/update {}: {}", other, data);
        }
    }
}
