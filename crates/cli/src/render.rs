use std::sync::OnceLock;

use chrono::Local;
use proto::types::*;
use serde::Serialize;

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

