use chrono::Local;
use proto::types::*;
use serde_json::Value;

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
            println!("[{ts}] {actor}: {text}");
        }
        "tool.report" => {
            let name = event
                .payload
                .get("toolName")
                .and_then(|v| v.as_str())
                .unwrap_or("tool");
            let status = event
                .payload
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            println!("[{ts}] {actor} ↯ tool {name} ({status})");
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
        "handoff.offer" => {
            let msg = event
                .payload
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let to = event
                .relations
                .iter()
                .find(|r| matches!(r.kind, RelationKind::HandsOffTo))
                .map(|r| r.target.id.clone())
                .unwrap_or_default();
            println!("[{ts}] {actor} ↪ handoff → {to}: {msg}");
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

pub fn render_stream_update(payload: &Value) {
    let kind = payload.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    let data = payload.get("data").cloned().unwrap_or(Value::Null);
    match kind {
        "event.created" | "handoff.created" => {
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
