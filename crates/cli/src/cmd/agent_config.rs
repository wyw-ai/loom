use std::sync::Arc;

use anyhow::{Context, Result};
use proto::methods::*;
use serde_json::{json, Value};

use crate::client::Client;
use crate::render;

pub async fn publish(
    client: Arc<Client>,
    actor_id: String,
    version: Option<String>,
    model: Option<String>,
    adapter: Option<String>,
    prompt: Option<String>,
    tools_json: Option<String>,
) -> Result<()> {
    let res: AgentConfigPublishResult = client
        .call(
            method::AGENT_CONFIG_PUBLISH,
            json!({
                "actorId": actor_id,
                "version": version,
                "model": model.unwrap_or_default(),
                "adapter": adapter.unwrap_or_default(),
                "prompt": prompt.unwrap_or_default(),
                "tools": parse_json_or_null(tools_json, "--tools-json")?,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!("agent_config {}\t{}", res.version.id, res.version.version);
    }
    Ok(())
}

pub async fn activate(client: Arc<Client>, actor_id: String, version_id: String) -> Result<()> {
    let res: AgentConfigActivateResult = client
        .call(
            method::AGENT_CONFIG_ACTIVATE,
            json!({
                "actorId": actor_id,
                "versionId": version_id,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!(
            "agent_config active {}\t{}",
            res.activation.actor_id, res.activation.version_id
        );
    }
    Ok(())
}

fn parse_json_or_null(raw: Option<String>, label: &str) -> Result<Value> {
    raw.map(|value| serde_json::from_str(&value).with_context(|| format!("parse {label}")))
        .transpose()
        .map(|value| value.unwrap_or(Value::Null))
}
