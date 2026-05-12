use std::io::Read;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use proto::methods::*;
use proto::types::{Ref, RefKind, Relation, RelationKind, ScopeRef};
use serde_json::json;

use crate::client::Client;
use crate::render;

use super::target::{resolve_target, TargetMode};

pub async fn send(
    client: Arc<Client>,
    actor_id: String,
    target: String,
    text: Option<String>,
    attachment_ids: Vec<String>,
) -> Result<()> {
    let resolved = resolve_target(&client, &actor_id, &target, TargetMode::Write).await?;
    let body = match text {
        Some(t) => t,
        None => {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .context("read stdin")?;
            buf
        }
    };
    if body.trim().is_empty() && attachment_ids.is_empty() {
        bail!("message body is empty");
    }
    let mut relations = attachment_relations(attachment_ids);
    if let Some(root_event_id) = resolved.thread_root_event_id.as_ref() {
        relations.push(Relation {
            kind: RelationKind::RepliesTo,
            target: Ref {
                kind: RefKind::Event,
                id: root_event_id.clone(),
                _meta: None,
            },
            _meta: None,
        });
    }
    if let Some(actor) = resolved.direct_actor {
        relations.push(Relation {
            kind: RelationKind::HandsOffTo,
            target: Ref {
                kind: RefKind::Actor,
                id: actor,
                _meta: None,
            },
            _meta: None,
        });
    }
    append_content(client, &actor_id, resolved.scope, body, relations).await
}

pub async fn read(
    client: Arc<Client>,
    actor_id: String,
    target: String,
    limit: u32,
    before: Option<String>,
) -> Result<()> {
    let resolved = resolve_target(&client, &actor_id, &target, TargetMode::Read).await?;
    let mut params = json!({
        "scope": resolved.scope,
        "limit": limit,
    });
    if let Some(before) = before {
        params["beforeEventId"] = json!(before);
    }
    let res: ScopeReadResult = client.call(method::SCOPE_READ, params).await?;
    if render::is_json() {
        render::print_json(&res);
        return Ok(());
    }
    for event in &res.events {
        render::render_event(event);
    }
    if res.events.is_empty() {
        println!("(no messages)");
    }
    Ok(())
}

pub async fn check(client: Arc<Client>, actor_id: String, limit: u32, ack: bool) -> Result<()> {
    let res: DeliveryListResult = client
        .call(
            method::DELIVERY_LIST,
            json!({
                "actorId": actor_id,
                "state": "pending",
                "limit": limit,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else if res.deliveries.is_empty() {
        println!("(no pending direct messages)");
    } else {
        for entry in &res.deliveries {
            if let Some(event) = entry.event.as_ref() {
                render::render_event(event);
            } else {
                println!("pending delivery {}", entry.delivery.event_id);
            }
        }
    }
    if ack {
        for entry in res.deliveries {
            let _: ReceiptRecordResult = client
                .call(
                    method::RECEIPT_RECORD,
                    json!({
                        "eventId": entry.delivery.event_id,
                        "actorId": entry.delivery.actor_id,
                        "kind": "seen",
                    }),
                )
                .await?;
        }
    }
    Ok(())
}

pub async fn search(
    client: Arc<Client>,
    actor_id: String,
    query: String,
    target: Option<String>,
    limit: u32,
) -> Result<()> {
    let scope = match target {
        Some(target) => Some(
            resolve_target(&client, &actor_id, &target, TargetMode::Read)
                .await?
                .scope,
        ),
        None => None,
    };
    let res: MessageSearchResult = client
        .call(
            method::MESSAGE_SEARCH,
            json!({
                "query": query,
                "scope": scope,
                "limit": limit,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else if res.events.is_empty() {
        println!("(no matches)");
    } else {
        for event in &res.events {
            render::render_event(event);
        }
    }
    Ok(())
}

pub async fn append_content(
    client: Arc<Client>,
    actor_id: &str,
    scope: ScopeRef,
    text: String,
    relations: Vec<Relation>,
) -> Result<()> {
    let res: EventAppendResult = client
        .call(
            method::EVENT_APPEND,
            json!({
                "event": {
                    "type": "content.add",
                    "actorId": actor_id,
                    "scope": scope,
                    "payload": { "contentType": "text/markdown", "text": text },
                    "relations": relations,
                }
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!("event {}", res.event.id);
    }
    Ok(())
}

fn attachment_relations(ids: Vec<String>) -> Vec<Relation> {
    ids.into_iter()
        .map(|id| Relation {
            kind: RelationKind::AttachesArtifact,
            target: Ref {
                kind: RefKind::Artifact,
                id,
                _meta: None,
            },
            _meta: None,
        })
        .collect()
}
