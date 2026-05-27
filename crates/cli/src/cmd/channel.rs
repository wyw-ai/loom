use std::sync::Arc;

use anyhow::Result;
use proto::methods::*;
use serde_json::json;

use crate::client::Client;
use crate::render;

pub async fn create(client: Arc<Client>, actor_id: String, title: String) -> Result<()> {
    let res: ChannelCreateResult = client
        .call(
            method::CHANNEL_CREATE,
            json!({ "title": title, "actorId": actor_id }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!("channel {}\t{}", res.channel.id, res.channel.title);
    }
    Ok(())
}

pub async fn list(client: Arc<Client>) -> Result<()> {
    let res: ChannelListResult = client.call(method::CHANNEL_LIST, json!({})).await?;
    if render::is_json() {
        render::print_json(&res);
        return Ok(());
    }
    if res.channels.is_empty() {
        println!("(no channels)");
    }
    for c in res.channels {
        println!("{}\t{}", c.id, c.title);
    }
    Ok(())
}

pub async fn delete(client: Arc<Client>, channel_id: String, cascade: bool) -> Result<()> {
    let res: ChannelDeleteResult = client
        .call(
            method::CHANNEL_DELETE,
            json!({ "channelId": channel_id, "cascade": cascade }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else if res.deleted {
        if res.deleted_threads > 0 {
            println!(
                "deleted channel (cascade: {} thread(s))",
                res.deleted_threads
            );
        } else {
            println!("deleted channel");
        }
    } else {
        println!("channel not deleted");
    }
    Ok(())
}

pub async fn invite(client: Arc<Client>, channel_id: String, actor_id: String) -> Result<()> {
    let res: ChannelInviteResult = client
        .call(
            method::CHANNEL_INVITE,
            json!({ "channelId": channel_id, "actorId": actor_id }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!(
            "invited {} into {} (members: {})",
            actor_id,
            res.channel.id,
            res.channel.members.join(",")
        );
    }
    Ok(())
}

pub async fn revoke(client: Arc<Client>, channel_id: String, actor_id: String) -> Result<()> {
    let res: ChannelRevokeResult = client
        .call(
            method::CHANNEL_REVOKE,
            json!({ "channelId": channel_id, "actorId": actor_id }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!(
            "revoked {} from {} (members: {})",
            actor_id,
            res.channel.id,
            res.channel.members.join(",")
        );
    }
    Ok(())
}

pub async fn members(client: Arc<Client>, channel_id: String) -> Result<()> {
    let res: ChannelMembersResult = client
        .call(method::CHANNEL_MEMBERS, json!({ "channelId": channel_id }))
        .await?;
    if render::is_json() {
        render::print_json(&res);
        return Ok(());
    }
    if res.members.is_empty() {
        println!("(no members)");
    }
    for a in res.members {
        println!("{}\t{}\t{:?}", a.id, a.display_name, a.kind);
    }
    Ok(())
}
