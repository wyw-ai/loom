use std::sync::Arc;

use anyhow::{Context, Result};
use proto::methods::*;
use proto::types::{RunStatus, ScopeKind, ScopeRef};
use serde_json::{json, Value};

use crate::client::Client;
use crate::render;

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
