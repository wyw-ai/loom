use std::sync::Arc;

use anyhow::{anyhow, Result};
use proto::methods::*;
use proto::types::{Actor, ActorKind};
use serde_json::json;

use crate::client::Client;
use crate::render;

pub async fn list(client: Arc<Client>) -> Result<()> {
    let res: ActorListResult = client.call(method::ACTOR_LIST, json!({})).await?;
    if render::is_json() {
        render::print_json(&res);
        return Ok(());
    }
    if res.actors.is_empty() {
        println!("(no actors)");
    }
    for a in res.actors {
        println!("{}\t{}\t{}", a.id, actor_kind_label(a.kind), a.display_name);
    }
    Ok(())
}

pub async fn upsert(
    client: Arc<Client>,
    actor_id: String,
    kind: String,
    display: Option<String>,
) -> Result<()> {
    let kind = parse_actor_kind(&kind)?;
    let actor = Actor {
        id: actor_id.clone(),
        kind,
        display_name: display.unwrap_or_else(|| actor_id.clone()),
        capabilities: None,
        _meta: None,
    };
    let res: ActorUpsertResult = client
        .call(method::ACTOR_UPSERT, json!({ "actor": actor }))
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        let kind = actor_kind_label(res.actor.kind);
        println!(
            "actor {}\t{}\t{}",
            res.actor.id, kind, res.actor.display_name
        );
    }
    Ok(())
}

fn parse_actor_kind(kind: &str) -> Result<ActorKind> {
    match kind.to_ascii_lowercase().as_str() {
        "human" => Ok(ActorKind::Human),
        "agent" => Ok(ActorKind::Agent),
        "service" => Ok(ActorKind::Service),
        other => Err(anyhow!(
            "invalid actor kind `{}` (expected human, agent, or service)",
            other
        )),
    }
}

fn actor_kind_label(kind: ActorKind) -> &'static str {
    match kind {
        ActorKind::Human => "human",
        ActorKind::Agent => "agent",
        ActorKind::Service => "service",
    }
}
