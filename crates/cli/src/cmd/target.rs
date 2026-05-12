use std::sync::Arc;

use anyhow::{bail, Result};
use proto::methods::*;
use proto::types::{Channel, ScopeKind, ScopeRef};
use serde_json::json;

use crate::client::Client;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetMode {
    Read,
    Write,
}

#[derive(Debug, Clone)]
pub struct ResolvedTarget {
    pub scope: ScopeRef,
    pub direct_actor: Option<String>,
    pub thread_root_event_id: Option<String>,
}

pub async fn resolve_target(
    client: &Arc<Client>,
    local_actor: &str,
    target: &str,
    mode: TargetMode,
) -> Result<ResolvedTarget> {
    let target = target.trim();
    if target.is_empty() {
        bail!("target is empty");
    }

    if let Some(rest) = target.strip_prefix('#') {
        return resolve_hash_target(client, rest, mode).await;
    }
    if let Some(rest) = target.strip_prefix("dm:") {
        return resolve_dm_target(client, local_actor, rest, mode).await;
    }

    bail!(
        "unsupported target `{}` (use #<channel_id>, #<channel_id>:<root_event_id>, or dm:<actor_id>)",
        target
    )
}

fn scope(kind: ScopeKind, id: &str) -> ResolvedTarget {
    ResolvedTarget {
        scope: ScopeRef {
            kind,
            id: id.to_string(),
        },
        direct_actor: None,
        thread_root_event_id: None,
    }
}

async fn resolve_hash_target(
    client: &Arc<Client>,
    raw: &str,
    mode: TargetMode,
) -> Result<ResolvedTarget> {
    let raw = raw.trim();
    if raw.is_empty() {
        bail!("invalid target `#` (use #<channel_id>)");
    }
    let Some((channel_id, root_event_id)) = raw.split_once(':') else {
        return Ok(scope(ScopeKind::Channel, raw));
    };
    if channel_id.is_empty() || root_event_id.is_empty() || root_event_id.contains(':') {
        bail!(
            "invalid thread target `#{}` (use #<channel_id>:<root_event_id>)",
            raw
        );
    }
    let existing: ThreadListResult = client
        .call(method::THREAD_LIST, json!({ "channelId": channel_id }))
        .await?;
    if let Some(thread) = existing
        .threads
        .into_iter()
        .find(|t| t.root_event_id == root_event_id)
    {
        return Ok(ResolvedTarget {
            scope: ScopeRef {
                kind: ScopeKind::Thread,
                id: thread.id,
            },
            direct_actor: None,
            thread_root_event_id: Some(root_event_id.to_string()),
        });
    }
    if mode == TargetMode::Read {
        bail!("thread `#{channel_id}:{root_event_id}` does not exist");
    }
    let created: ThreadCreateResult = client
        .call(
            method::THREAD_CREATE,
            json!({
                "channelId": channel_id,
                "title": format!("thread {root_event_id}"),
                "rootEventId": root_event_id,
            }),
        )
        .await?;
    Ok(ResolvedTarget {
        scope: ScopeRef {
            kind: ScopeKind::Thread,
            id: created.thread.id,
        },
        direct_actor: None,
        thread_root_event_id: Some(root_event_id.to_string()),
    })
}

async fn resolve_dm_target(
    client: &Arc<Client>,
    local_actor: &str,
    raw: &str,
    mode: TargetMode,
) -> Result<ResolvedTarget> {
    if raw.is_empty() || raw.contains(':') {
        bail!("invalid DM target `dm:{}` (use dm:<actor_id>)", raw);
    }
    let actor_id = resolve_actor_id(client, raw).await?;
    let channel = resolve_direct_channel(client, local_actor, &actor_id, mode).await?;
    Ok(ResolvedTarget {
        scope: ScopeRef {
            kind: ScopeKind::Channel,
            id: channel.id,
        },
        direct_actor: Some(actor_id),
        thread_root_event_id: None,
    })
}

async fn resolve_actor_id(client: &Arc<Client>, raw: &str) -> Result<String> {
    let key = raw.trim();
    let res: ActorListResult = client.call(method::ACTOR_LIST, json!({})).await?;
    if res.actors.into_iter().any(|a| a.id == key) {
        Ok(key.to_string())
    } else {
        bail!("actor `{}` not found", raw)
    }
}

async fn resolve_direct_channel(
    client: &Arc<Client>,
    local_actor: &str,
    peer_actor: &str,
    mode: TargetMode,
) -> Result<Channel> {
    if local_actor == peer_actor {
        bail!("cannot create a direct message with yourself");
    }
    let title = direct_channel_title(local_actor, peer_actor);
    let existing: ChannelListResult = client.call(method::CHANNEL_LIST, json!({})).await?;
    if let Some(ch) = existing.channels.into_iter().find(|c| c.title == title) {
        return Ok(ch);
    }
    if mode == TargetMode::Read {
        bail!("direct message with `{}` does not exist", peer_actor);
    }
    let created: ChannelCreateResult = client
        .call(
            method::CHANNEL_CREATE,
            json!({ "title": title, "actorId": local_actor }),
        )
        .await?;
    let _: ChannelInviteResult = client
        .call(
            method::CHANNEL_INVITE,
            json!({
                "channelId": created.channel.id.clone(),
                "actorId": peer_actor,
            }),
        )
        .await?;
    Ok(created.channel)
}

fn direct_channel_title(a: &str, b: &str) -> String {
    let mut ids = [a, b];
    ids.sort();
    format!("dm:{}:{}", ids[0], ids[1])
}
