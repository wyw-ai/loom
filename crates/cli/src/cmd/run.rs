use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::{SecondsFormat, Utc};
use proto::methods::*;
use proto::types::{RunStatus, ScopeKind, ScopeRef};
use serde_json::{json, Value};

use crate::client::Client;
use crate::render;

const LOOM_NO_REPLY_FILE_ENV: &str = "LOOM_NO_REPLY_FILE";

pub async fn open(
    client: Arc<Client>,
    actor_id: String,
    target: String,
    delivery_id: Option<String>,
    start_reason: Option<String>,
    agent_config_version_id: String,
) -> Result<()> {
    let res: RunOpenResult = client
        .call(
            method::RUN_OPEN,
            json!({
                "actorId": actor_id,
                "scope": parse_scope_target(&target)?,
                "deliveryId": delivery_id,
                "startReason": start_reason,
                "agentConfigVersionId": agent_config_version_id,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!("run {}\t{:?}", res.run.id, res.run.status);
    }
    Ok(())
}

pub async fn append(
    client: Arc<Client>,
    run_id: String,
    status: Option<String>,
    frame_kind: String,
    payload_json: Option<String>,
) -> Result<()> {
    let status = status.map(|raw| parse_run_status(&raw)).transpose()?;
    let payload: Value = payload_json
        .map(|raw| serde_json::from_str(&raw).context("parse --payload-json"))
        .transpose()?
        .unwrap_or(Value::Null);
    let res: RunAppendResult = client
        .call(
            method::RUN_APPEND,
            json!({
                "runId": run_id,
                "status": status,
                "frameKind": frame_kind,
                "payload": payload,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!("run {}\tframe={}", res.run.id, res.frame.seq);
    }
    Ok(())
}

pub async fn ignore(
    client: Arc<Client>,
    run_id: Option<String>,
    reason: Option<String>,
) -> Result<()> {
    let run_id = run_id
        .or_else(|| std::env::var("LOOM_RUN_ID").ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .context("run ignore requires --run-id or LOOM_RUN_ID")?;
    let reason = reason
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "no_visible_reply_needed".into());
    let trigger_source_id = std::env::var("LOOM_TRIGGER_MESSAGE_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let payload = json!({
        "noReply": true,
        "replyMode": "none",
        "reason": reason,
        "triggerSourceId": trigger_source_id,
        "createdAt": Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
    });
    let local_marked = mark_local_no_reply(&run_id, &payload)
        .with_context(|| format!("write no-reply marker from {}", LOOM_NO_REPLY_FILE_ENV))?;
    let result: Result<RunAppendResult> = client
        .call(
            method::RUN_APPEND,
            json!({
                "runId": run_id,
                "frameKind": "control.no_reply",
                "payload": payload,
            }),
        )
        .await
        .context("run.append control.no_reply");
    match result {
        Ok(res) => {
            if render::is_json() {
                render::print_json(&json!({
                    "ignored": true,
                    "localMarked": local_marked,
                    "run": res.run,
                    "frame": res.frame,
                }));
            } else {
                println!("run {}\tignored", res.run.id);
            }
        }
        Err(err) if local_marked => {
            if render::is_json() {
                render::print_json(&json!({
                    "ignored": true,
                    "localMarked": true,
                    "auditRecorded": false,
                    "auditError": err.to_string(),
                }));
            } else {
                println!("run ignored locally (audit failed: {err})");
            }
        }
        Err(err) => return Err(err),
    }
    Ok(())
}

fn mark_local_no_reply(run_id: &str, payload: &Value) -> std::io::Result<bool> {
    let Some(path) = std::env::var_os(LOOM_NO_REPLY_FILE_ENV) else {
        return Ok(false);
    };
    let path = std::path::PathBuf::from(path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let marker = json!({
        "runId": run_id,
        "payload": payload,
    });
    let bytes = serde_json::to_vec_pretty(&marker)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))?;
    std::fs::write(path, bytes)?;
    Ok(true)
}

pub async fn close(client: Arc<Client>, run_id: String, status: String) -> Result<()> {
    let status = parse_run_status(&status)?;
    let res: RunCloseResult = client
        .call(
            method::RUN_CLOSE,
            json!({
                "runId": run_id,
                "status": status,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!("run {}\t{:?}", res.run.id, res.run.status);
    }
    Ok(())
}

fn parse_scope_target(target: &str) -> Result<ScopeRef> {
    let target = target.trim();
    if let Some(channel_id) = target.strip_prefix('#') {
        if channel_id.is_empty() || channel_id.contains(':') {
            anyhow::bail!("run target must be a channel scope like #chan_id or thread:thread_id");
        }
        return Ok(ScopeRef {
            kind: ScopeKind::Channel,
            id: channel_id.into(),
        });
    }
    if let Some(thread_id) = target.strip_prefix("thread:") {
        if thread_id.trim().is_empty() {
            anyhow::bail!("thread target cannot be empty");
        }
        return Ok(ScopeRef {
            kind: ScopeKind::Thread,
            id: thread_id.trim().into(),
        });
    }
    anyhow::bail!("run target must be #channel or thread:<thread_id>");
}

fn parse_run_status(raw: &str) -> Result<RunStatus> {
    serde_json::from_value(json!(raw.trim())).with_context(|| {
        "invalid run status; expected queued, preparing_context, running, waiting_tool, completed, failed, or canceled"
    })
}
