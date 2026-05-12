use std::io::{self, Write};
use std::sync::Arc;

use anyhow::{anyhow, bail, Result};
use proto::methods::*;
use proto::types::{Actor, ActorKind, Ref, RefKind, Relation, RelationKind, ScopeKind, ScopeRef};
use serde_json::json;

use crate::client::Client;
use crate::render;

use super::target::{resolve_target, TargetMode};

pub async fn run(
    client: Arc<Client>,
    actor_id: String,
    target_actor_id: Option<String>,
    scope_id: Option<String>,
    is_channel: bool,
    target_scope: Option<String>,
    message: String,
) -> Result<()> {
    let target = match target_actor_id {
        Some(t) if !t.is_empty() => t,
        _ => pick_target(&client).await?,
    };

    let (scope, root_event_id) = match target_scope {
        Some(raw_target) => {
            if scope_id.is_some() {
                bail!("use either --target or --in, not both");
            }
            if raw_target.trim().starts_with("dm:") {
                bail!("handoff --target supports #<channel_id> and #<channel_id>:<root_event_id>; use message send for dm:<actor_id>");
            }
            let resolved =
                resolve_target(&client, &actor_id, &raw_target, TargetMode::Write).await?;
            (resolved.scope, resolved.thread_root_event_id)
        }
        None => {
            let Some(scope_id) = scope_id else {
                bail!("missing destination scope: pass --target '#<channel_id>[:<root_event_id>]' or --in <scope_id>");
            };
            (
                ScopeRef {
                    kind: if is_channel {
                        ScopeKind::Channel
                    } else {
                        ScopeKind::Thread
                    },
                    id: scope_id,
                },
                None,
            )
        }
    };
    let mut relations = vec![Relation {
        kind: RelationKind::HandsOffTo,
        target: Ref {
            kind: RefKind::Actor,
            id: target.clone(),
            _meta: None,
        },
        _meta: None,
    }];
    if let Some(root_event_id) = root_event_id {
        relations.push(Relation {
            kind: RelationKind::RepliesTo,
            target: Ref {
                kind: RefKind::Event,
                id: root_event_id,
                _meta: None,
            },
            _meta: None,
        });
    }
    let payload = json!({
        "event": {
            "type": "content.add",
            "actorId": actor_id,
            "scope": scope,
            "payload": { "contentType": "text/markdown", "text": message },
            "relations": relations,
        }
    });
    let res: EventAppendResult = client.call(method::EVENT_APPEND, payload).await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!("handoff event {} → {}", res.event.id, target);
    }
    Ok(())
}

async fn pick_target(client: &Client) -> Result<String> {
    let actors: ActorListResult = client.call(method::ACTOR_LIST, json!({})).await?;
    let mut rows: Vec<PickRow> = actors.actors.into_iter().map(PickRow::from_actor).collect();
    if rows.is_empty() {
        return Err(anyhow!(
            "no actors known to the server (configure a machine agent and start `joi daemon` first)"
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
}

impl PickRow {
    fn from_actor(a: Actor) -> Self {
        Self {
            id: a.id,
            display: a.display_name,
            kind: a.kind,
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
        write!(f, "{}\t{} ({})", self.id, self.display, kind)
    }
}
