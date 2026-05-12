// `joi service am-handler` orchestrator. First user is the CLI
// subcommand wired in `crate::cmd::service`; some helpers are exported
// for tests but not yet used outside this module.
#![allow(dead_code)]

//! Per-invocation orchestrator for the AM bridge. One process per
//! `am listen --script` callback. Reads the message JSON from stdin,
//! resolves scope, writes a `hands_off_to` event, optionally waits for
//! the agent reply, and either prints a DingTalk callback JSON to
//! stdout or sends via `am`.
//!
//! Two entry shapes:
//!
//! * `run_normal` — the default: stdin → handoff → reply (callback /
//!   send / async_send dispatch).
//! * `run_async_reply` — invoked by ourselves when async_send mode
//!   spawned a detached child. Skips the handoff (already done in the
//!   parent) and only waits-then-sends.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use proto::methods::ServiceSpec;
use proto::types::{Meta, ScopeKind, ScopeRef};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::client::Client;
use crate::service::runtime::ServiceRuntime;
use crate::service::state;

use super::reply::ReplyMode;
use super::scope::ScopeMode;
use super::{extract, reply, scope, AmConfig};

/// Resolve the spec path: `<dir>/<service_id>.json` first, then a fall
/// back scan if the file name doesn't match the id (operators sometimes
/// rename without renaming the file). The validate step ensures the
/// returned spec passed §6.1's actor.kind check.
pub fn load_spec(dir: &Path, service_id: &str) -> Result<ServiceSpec> {
    let direct = dir.join(format!("{service_id}.json"));
    if direct.exists() {
        let text =
            fs::read_to_string(&direct).with_context(|| format!("read {}", direct.display()))?;
        let mut spec: ServiceSpec = serde_json::from_str(&text)
            .with_context(|| format!("parse {} as ServiceSpec", direct.display()))?;
        spec.normalize();
        spec.validate()?;
        if spec.id != service_id {
            // The file was named after `service_id` but holds a spec
            // with a different id. Surface as an error rather than
            // silently using the file's id — operator likely intended
            // the filename to be authoritative.
            bail!(
                "spec at {} has id `{}`, expected `{service_id}`",
                direct.display(),
                spec.id,
            );
        }
        return Ok(spec);
    }
    // Fallback: scan directory for any spec whose internal id matches.
    if !dir.exists() {
        bail!("specs directory {} does not exist", dir.display());
    }
    for entry in fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(mut spec) = serde_json::from_str::<ServiceSpec>(&text) else {
            continue;
        };
        if spec.id == service_id {
            spec.normalize();
            spec.validate()
                .with_context(|| format!("validate {}", path.display()))?;
            return Ok(spec);
        }
    }
    bail!(
        "ServiceSpec `{service_id}` not found under {} (no <id>.json file and no fallback match)",
        dir.display()
    );
}

/// Parse `spec.config` as [`AmConfig`]. An absent or `null` `config`
/// yields the defaults (mirroring §6.1's "config is optional but
/// plugin-specific").
pub fn parse_am_config(spec: &ServiceSpec) -> Result<AmConfig> {
    if spec.config.is_null() {
        return Ok(AmConfig::default());
    }
    serde_json::from_value(spec.config.clone()).context("parse spec.config as AmConfig")
}

/// Read all of stdin into a string. Returns "" on empty stdin (Python
/// `parse_stdin` falls back to argv / env in that case; the Rust port
/// treats empty stdin as "no events" and prints `{}` to satisfy
/// `am listen` callback expectations).
fn read_stdin() -> Result<String> {
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .context("read stdin")?;
    Ok(buf)
}

/// Parse the listener payload. Mirrors Python `parse_stdin`:
///
/// * Try whole-buffer JSON first (object → 1 event, array → N events).
/// * Otherwise split by lines, parse each as JSON, and on per-line
///   parse failure synthesize `{"text": <line>, "_raw": <full input>}`
///   so downstream `extract::extract_text` still has something to work
///   with.
pub fn parse_events(raw: &str) -> Vec<Value> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
        return match parsed {
            Value::Array(arr) => arr,
            other => vec![other],
        };
    }
    let mut out = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(line) {
            Ok(v) => out.push(v),
            Err(_) => out.push(json!({ "text": line, "_raw": raw })),
        }
    }
    out
}

/// Build the prompt body the agent sees. Includes a `Source: ...` line
/// when sender/conversation are known, then the user message verbatim.
/// Format matches the Python reference so agent persona prompts that
/// inspect the prefix continue to work.
pub fn format_question(event: &Value, text: &str) -> String {
    let sender = extract::sender(event);
    let conversation = extract::conversation(event);
    let mut source: Vec<String> = Vec::new();
    if !sender.is_empty() {
        source.push(format!("sender={sender}"));
    }
    if !conversation.is_empty() {
        source.push(format!("conversation={conversation}"));
    }
    let mut lines: Vec<String> = Vec::new();
    if !source.is_empty() {
        lines.push(format!(
            "Source: DingTalk bot message ({})",
            source.join(", ")
        ));
        lines.push(String::new());
    }
    lines.push("User message:".into());
    lines.push(text.into());
    lines.join("\n").trim().to_string()
}

fn build_meta(event: &Value) -> Meta {
    let mut meta: Meta = Default::default();
    meta.insert("service".into(), json!("am"));
    let mut external = serde_json::Map::new();
    let conv = extract::conversation(event);
    let sender = extract::sender(event);
    let msg_id = extract::message_id(event);
    if !conv.is_empty() {
        external.insert("conversationId".into(), json!(conv));
    }
    if !sender.is_empty() {
        external.insert("senderStaffId".into(), json!(sender));
    }
    if !msg_id.is_empty() {
        external.insert("messageId".into(), json!(msg_id));
    }
    if !external.is_empty() {
        meta.insert("external".into(), Value::Object(external));
    }
    meta
}

/// Resolve `(scope_kind, scope_id)` per [`ScopeMode`]. For
/// `AutoThread` this loads / mutates / saves the thread map (§7.2,
/// state under `<state_dir>/thread-map.json`).
async fn resolve_scope(
    am_cfg: &AmConfig,
    channel_id: &str,
    event: &Value,
    runtime: &ServiceRuntime,
    state_dir: &Path,
) -> Result<(ScopeKind, String)> {
    match am_cfg.scope {
        ScopeMode::Channel => Ok((ScopeKind::Channel, channel_id.into())),
        ScopeMode::Thread => {
            let tid = am_cfg
                .thread_id
                .clone()
                .ok_or_else(|| anyhow!("scope=thread requires AmConfig.threadId"))?;
            Ok((ScopeKind::Thread, tid))
        }
        ScopeMode::AutoThread => {
            // Per Python: a fixed thread_id wins even when scope is
            // auto_thread (lets operators pin a single thread for testing).
            if let Some(tid) = &am_cfg.thread_id {
                return Ok((ScopeKind::Thread, tid.clone()));
            }
            let key = scope::thread_key(event);
            let map_path = scope::thread_map_path(state_dir);
            let mut map = scope::load(&map_path)?;
            let chan_map = map.channels.entry(channel_id.into()).or_default();
            if let Some(entry) = chan_map.get(&key) {
                return Ok((ScopeKind::Thread, entry.thread_id.clone()));
            }
            let title = scope::thread_title(event, &key);
            let root_event_id = runtime
                .append_content(
                    ScopeRef {
                        kind: ScopeKind::Channel,
                        id: channel_id.into(),
                    },
                    title.clone(),
                    vec![],
                    None,
                )
                .await?;
            let thread = runtime
                .create_thread(channel_id, &root_event_id, &title)
                .await?;
            chan_map.insert(
                key,
                scope::ThreadEntry {
                    thread_id: thread.id.clone(),
                    title: thread.title.clone(),
                    created_at: chrono::Utc::now().timestamp(),
                },
            );
            scope::save(&map_path, &map)?;
            tracing::info!(
                thread_id = %thread.id,
                channel = %channel_id,
                "am-joi: auto-created thread",
            );
            Ok((ScopeKind::Thread, thread.id))
        }
    }
}

async fn wait_for_answer(
    runtime: &ServiceRuntime,
    trigger_id: &str,
    timeout_secs: u64,
) -> Result<String> {
    let timeout = Duration::from_secs(timeout_secs);
    let events = runtime.await_responds_to(trigger_id, timeout).await?;
    for event in events {
        if let Some(text) = event.payload.get("text").and_then(|v| v.as_str()) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Ok(trimmed.to_string());
            }
        }
    }
    bail!("timed out waiting for response to {trigger_id}")
}

/// async_send parent path: spawn a detached `joi service am-handler
/// --service-id <id> --async-reply <payload>` child that will wait for
/// the agent reply and send via `am`. Parent prints the pending
/// callback and exits.
fn spawn_async_reply(
    service_id: &str,
    source_event: &Value,
    trigger_id: &str,
    scope_kind: &str,
    scope_id: &str,
    am_cfg: &AmConfig,
) -> Result<()> {
    let payload = json!({
        "sourceEvent": source_event,
        "triggerId": trigger_id,
        "scopeKind": scope_kind,
        "scopeId": scope_id,
    });
    let binary = std::env::current_exe().context("locate current binary for async-reply spawn")?;
    let log_path = am_cfg.async_log_path.clone().unwrap_or_else(|| {
        state::default_data_root()
            .join("services")
            .join(service_id)
            .join("logs")
            .join("async-reply.log")
    });
    reply::spawn_detached_async_reply(&binary, service_id, &payload, &log_path)
}

/// Top-level entry for the CLI subcommand. Branches on `async_reply`:
/// non-None means we're the detached child re-invoking ourselves to
/// finish an async_send turn; None means a fresh listener callback.
pub async fn run_handler(
    server_url: String,
    service_id: String,
    specs_dir: PathBuf,
    async_reply: Option<String>,
) -> Result<()> {
    let spec = load_spec(&specs_dir, &service_id)?;
    let am_cfg = parse_am_config(&spec)?;

    let data_root = state::default_data_root();
    let state_path = state::state_dir(&data_root, &service_id);
    // One-shot legacy import (idempotent on subsequent runs).
    let _ = scope::migrate_legacy(
        &scope::thread_map_path(&state_path),
        &scope::legacy_thread_map_path(),
    );

    if let Some(payload) = async_reply {
        return run_async_reply(server_url, spec, am_cfg, &data_root, &payload).await;
    }
    run_normal(server_url, spec, am_cfg, &data_root).await
}

async fn run_normal(
    server_url: String,
    spec: ServiceSpec,
    am_cfg: AmConfig,
    data_root: &Path,
) -> Result<()> {
    let raw = read_stdin()?;
    let events = parse_events(&raw);

    let actor_id = spec.actor.id.clone();
    let display = spec.actor.display_name.clone();
    let display_opt = if display.is_empty() {
        None
    } else {
        Some(display.as_str())
    };

    let client = Client::connect(&server_url)
        .await
        .with_context(|| format!("ws connect {server_url}"))?;
    client.initialize().await?;
    client
        .open_connection_as(&actor_id, "service", display_opt)
        .await?;
    let runtime = ServiceRuntime::start(spec.id.clone(), actor_id, client.clone(), data_root)?;
    runtime.actor_upsert(spec.actor.clone()).await?;

    let channel_id = spec
        .channel_id
        .clone()
        .ok_or_else(|| anyhow!("AM plugin requires spec.channelId"))?;
    let target_agent = spec
        .target_agent
        .clone()
        .ok_or_else(|| anyhow!("AM plugin requires spec.targetAgent for handoff"))?;

    if am_cfg.auto_invite {
        let _ = runtime.ensure_channel_member(&channel_id).await;
        if let Err(e) = runtime.invite_member(&channel_id, &target_agent).await {
            tracing::warn!(error = %e, target = %target_agent, "auto_invite of target agent failed; continuing");
        }
    }

    let state_dir = state::state_dir(data_root, runtime.service_id());
    let mut wrote_callback = false;
    for event in events {
        let raw_text = event.get("_raw").and_then(|v| v.as_str()).unwrap_or("");
        let user_text = extract::extract_text(&event, raw_text);
        if user_text.is_empty() {
            tracing::warn!("am-joi: skipping empty message");
            continue;
        }

        let (scope_kind, scope_id) =
            resolve_scope(&am_cfg, &channel_id, &event, &runtime, &state_dir).await?;
        let body = format_question(&event, &user_text);
        let meta = build_meta(&event);
        let trigger_id = runtime
            .handoff(
                &target_agent,
                ScopeRef {
                    kind: scope_kind,
                    id: scope_id.clone(),
                },
                body,
                Some(meta),
            )
            .await?;
        tracing::info!(%trigger_id, scope = %scope_id, "am-joi: handoff written");

        if !am_cfg.reply {
            continue;
        }
        match am_cfg.reply_mode {
            ReplyMode::Callback => {
                let answer =
                    wait_for_answer(&runtime, &trigger_id, am_cfg.reply_timeout_secs).await?;
                let mut stdout = std::io::stdout();
                reply::write_callback(&mut stdout, &event, &answer)?;
                wrote_callback = true;
            }
            ReplyMode::Send => {
                let answer =
                    wait_for_answer(&runtime, &trigger_id, am_cfg.reply_timeout_secs).await?;
                reply::send_via_am(&am_cfg.am_send_config(), &event, &answer)?;
            }
            ReplyMode::AsyncSend => {
                let scope_kind_str = match scope_kind {
                    ScopeKind::Channel => "channel",
                    ScopeKind::Thread => "thread",
                };
                spawn_async_reply(
                    runtime.service_id(),
                    &event,
                    &trigger_id,
                    scope_kind_str,
                    &scope_id,
                    &am_cfg,
                )?;
                let mut stdout = std::io::stdout();
                reply::write_callback(&mut stdout, &event, &am_cfg.pending_text)?;
                wrote_callback = true;
            }
        }
    }

    if !wrote_callback {
        // `am listen --script` expects SOMETHING on stdout per
        // invocation; emit an empty object as a no-op acknowledgement.
        println!("{{}}");
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AsyncPayload {
    source_event: Value,
    trigger_id: String,
    /// `"channel"` or `"thread"`. Currently unused on the receive side
    /// (await_responds_to filters by trigger_id only) but kept in the
    /// payload for human debuggability of the log file.
    #[serde(default)]
    #[allow(dead_code)]
    scope_kind: String,
    #[serde(default)]
    #[allow(dead_code)]
    scope_id: String,
}

async fn run_async_reply(
    server_url: String,
    spec: ServiceSpec,
    am_cfg: AmConfig,
    data_root: &Path,
    payload_str: &str,
) -> Result<()> {
    let payload: AsyncPayload =
        serde_json::from_str(payload_str).context("parse --async-reply payload")?;

    let actor_id = spec.actor.id.clone();
    let display = spec.actor.display_name.clone();
    let display_opt = if display.is_empty() {
        None
    } else {
        Some(display.as_str())
    };

    let client = Client::connect(&server_url).await?;
    client.initialize().await?;
    client
        .open_connection_as(&actor_id, "service", display_opt)
        .await?;
    let runtime = ServiceRuntime::start(spec.id.clone(), actor_id, client, data_root)?;

    let answer = wait_for_answer(&runtime, &payload.trigger_id, am_cfg.reply_timeout_secs).await?;
    reply::send_via_am(&am_cfg.am_send_config(), &payload.source_event, &answer)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_events_handles_single_object() {
        let evs = parse_events(r#"{"text":"hi"}"#);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0]["text"], "hi");
    }

    #[test]
    fn parse_events_handles_json_array() {
        let evs = parse_events(r#"[{"text":"a"},{"text":"b"}]"#);
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[1]["text"], "b");
    }

    #[test]
    fn parse_events_handles_jsonl() {
        let raw = "{\"text\":\"a\"}\n{\"text\":\"b\"}";
        let evs = parse_events(raw);
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0]["text"], "a");
        assert_eq!(evs[1]["text"], "b");
    }

    #[test]
    fn parse_events_synthesizes_text_for_unparseable_lines() {
        // Real-world `am listen` may emit a free-form line on misparse;
        // wrap it as `{text, _raw}` so extract_text can fall back.
        let raw = "not json line";
        let evs = parse_events(raw);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0]["text"], "not json line");
        assert_eq!(evs[0]["_raw"], "not json line");
    }

    #[test]
    fn parse_events_skips_blank_lines() {
        let raw = "\n  \n{\"text\":\"a\"}\n\n";
        let evs = parse_events(raw);
        assert_eq!(evs.len(), 1);
    }

    #[test]
    fn parse_events_returns_empty_for_blank_input() {
        assert!(parse_events("").is_empty());
        assert!(parse_events("   \n  ").is_empty());
    }

    #[test]
    fn format_question_includes_source_when_known() {
        let event = json!({"conversationId": "conv_x", "senderStaffId": "u1"});
        let q = format_question(&event, "请帮我看一下 CR");
        assert!(q.starts_with("Source: DingTalk bot message"));
        assert!(q.contains("sender=u1"));
        assert!(q.contains("conversation=conv_x"));
        assert!(q.contains("User message:"));
        assert!(q.ends_with("请帮我看一下 CR"));
    }

    #[test]
    fn format_question_omits_source_block_when_unknown() {
        let q = format_question(&json!({}), "raw text");
        assert!(!q.contains("Source:"));
        assert!(q.starts_with("User message:"));
        assert!(q.ends_with("raw text"));
    }

    #[test]
    fn build_meta_carries_external_routing() {
        let event = json!({"conversationId":"c","senderStaffId":"u","messageId":"m"});
        let meta = build_meta(&event);
        assert_eq!(meta["service"], "am");
        let ext = &meta["external"];
        assert_eq!(ext["conversationId"], "c");
        assert_eq!(ext["senderStaffId"], "u");
        assert_eq!(ext["messageId"], "m");
    }

    #[test]
    fn build_meta_omits_external_when_empty() {
        let meta = build_meta(&json!({}));
        assert_eq!(meta["service"], "am");
        assert!(meta.get("external").is_none());
    }

    #[test]
    fn parse_am_config_defaults_when_config_null() {
        let spec = ServiceSpec {
            id: "x".into(),
            kind: "am".into(),
            actor: proto::types::Actor {
                id: "svc_x".into(),
                kind: proto::types::ActorKind::Service,
                display_name: String::new(),
                capabilities: None,
                _meta: None,
            },
            autostart: true,
            channel_id: None,
            target_agent: None,
            lifecycle: proto::methods::ServiceLifecycle::ChannelSingleton,
            bind: None,
            params_schema: None,
            config: Value::Null,
        };
        let cfg = parse_am_config(&spec).expect("ok");
        assert_eq!(cfg.am_bin, "am");
        assert_eq!(cfg.scope, ScopeMode::AutoThread);
        assert_eq!(cfg.reply_mode, ReplyMode::Callback);
    }

    #[test]
    fn parse_am_config_overrides_from_spec() {
        let spec = ServiceSpec {
            id: "x".into(),
            kind: "am".into(),
            actor: proto::types::Actor {
                id: "svc_x".into(),
                kind: proto::types::ActorKind::Service,
                display_name: String::new(),
                capabilities: None,
                _meta: None,
            },
            autostart: true,
            channel_id: None,
            target_agent: None,
            lifecycle: proto::methods::ServiceLifecycle::ChannelSingleton,
            bind: None,
            params_schema: None,
            config: json!({
                "amBin": "/usr/local/bin/am",
                "scope": "channel",
                "replyMode": "async_send",
                "reply": true,
                "replyTimeoutSecs": 60,
                "pendingText": "稍等"
            }),
        };
        let cfg = parse_am_config(&spec).expect("ok");
        assert_eq!(cfg.am_bin, "/usr/local/bin/am");
        assert_eq!(cfg.scope, ScopeMode::Channel);
        assert_eq!(cfg.reply_mode, ReplyMode::AsyncSend);
        assert!(cfg.reply);
        assert_eq!(cfg.reply_timeout_secs, 60);
        assert_eq!(cfg.pending_text, "稍等");
        // Untouched fields keep their defaults.
        assert!(cfg.send_plain_text);
        assert_eq!(cfg.send_max_chars, 1800);
    }

    #[test]
    fn load_spec_finds_by_filename() {
        let dir =
            std::env::temp_dir().join(format!("joi-am-handler-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("am1.json");
        std::fs::write(
            &path,
            r#"{"id":"am1","kind":"am","actor":{"id":"svc","kind":"service"}}"#,
        )
        .unwrap();
        let spec = load_spec(&dir, "am1").expect("ok");
        assert_eq!(spec.id, "am1");
    }

    #[test]
    fn load_spec_rejects_id_filename_mismatch() {
        let dir =
            std::env::temp_dir().join(format!("joi-am-handler-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("renamed.json");
        std::fs::write(
            &path,
            r#"{"id":"original","kind":"am","actor":{"id":"svc","kind":"service"}}"#,
        )
        .unwrap();
        let err = load_spec(&dir, "renamed").expect_err("must fail");
        assert!(format!("{err}").contains("expected"), "{err}");
    }
}
