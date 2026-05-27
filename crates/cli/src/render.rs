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

pub fn render_message(message: &Message) {
    let ts = message.created_at.with_timezone(&Local).format("%H:%M:%S");
    let actor = &message.author_actor_id;
    let target = &message.target;
    let body = message.body.trim_end();
    if body.is_empty() {
        println!("[{ts}] {actor} -> {target}: (attachment)");
    } else {
        println!("[{ts}] {actor} -> {target}: {body}");
    }
}
