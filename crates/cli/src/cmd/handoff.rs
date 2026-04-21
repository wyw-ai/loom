use std::io::{self, Write};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use proto::methods::*;
use proto::types::{Actor, ActorKind};
use serde_json::json;

use crate::client::Client;

pub async fn run(
    client: Arc<Client>,
    actor_id: String,
    target_actor_id: Option<String>,
    thread_id: String,
    message: String,
) -> Result<()> {
    let target = match target_actor_id {
        Some(t) if !t.is_empty() => t,
        _ => pick_target(&client).await?,
    };
    let payload = json!({
        "event": {
            "type": "content.add",
            "actorId": actor_id,
            "scope": { "kind": "thread", "id": thread_id },
            "payload": { "contentType": "text/markdown", "text": message },
            "relations": [
                { "kind": "hands_off_to", "target": { "kind": "actor", "id": target } }
            ],
        }
    });
    let res: EventAppendResult = client.call(method::EVENT_APPEND, payload).await?;
    println!("handoff event {} → {}", res.event.id, target);
    Ok(())
}

async fn pick_target(client: &Client) -> Result<String> {
    let actors: ActorListResult = client.call(method::ACTOR_LIST, json!({})).await?;
    let agents: AgentListResult = client.call(method::AGENT_LIST, json!({})).await?;
    let mut rows: Vec<PickRow> = actors.actors.into_iter().map(PickRow::from_actor).collect();
    for a in agents.agents {
        if !rows.iter().any(|r| r.id == a.spec.actor.id) {
            rows.push(PickRow::from_agent(a.spec.actor, a.status));
        } else if let Some(r) = rows.iter_mut().find(|r| r.id == a.spec.actor.id) {
            r.status = Some(a.status);
        }
    }
    if rows.is_empty() {
        return Err(anyhow!(
            "no actors known to the server (try `joi agent install <id>` first)"
        ));
    }
    rows.sort_by(|a, b| a.kind_order().cmp(&b.kind_order()).then(a.id.cmp(&b.id)));
    println!("Pick a target:");
    for (i, r) in rows.iter().enumerate() {
        println!("  [{}] {}", i + 1, r);
    }
    print!("> ");
    io::stdout().flush().ok();
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    let trimmed = buf.trim();
    let idx: usize = trimmed
        .parse::<usize>()
        .map_err(|_| anyhow!("expected a number 1-{}", rows.len()))?;
    if idx == 0 || idx > rows.len() {
        return Err(anyhow!("out of range (1-{})", rows.len()));
    }
    Ok(rows[idx - 1].id.clone())
}

struct PickRow {
    id: String,
    display: String,
    kind: ActorKind,
    status: Option<String>,
}

impl PickRow {
    fn from_actor(a: Actor) -> Self {
        Self {
            id: a.id,
            display: a.display_name,
            kind: a.kind,
            status: None,
        }
    }
    fn from_agent(a: Actor, status: String) -> Self {
        Self {
            id: a.id,
            display: a.display_name,
            kind: ActorKind::Agent,
            status: Some(status),
        }
    }
    fn kind_order(&self) -> u8 {
        match self.kind {
            ActorKind::Agent => 0,
            ActorKind::Human => 1,
            ActorKind::Service => 2,
        }
    }
}

impl std::fmt::Display for PickRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match self.kind {
            ActorKind::Agent => "agent",
            ActorKind::Human => "human",
            ActorKind::Service => "service",
        };
        match &self.status {
            Some(s) => write!(f, "{}\t{} ({}, {})", self.id, self.display, kind, s),
            None => write!(f, "{}\t{} ({})", self.id, self.display, kind),
        }
    }
}
