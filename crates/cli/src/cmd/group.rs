use std::sync::Arc;

use anyhow::Result;
use proto::methods::*;
use serde_json::json;

use crate::client::Client;
use crate::render;

pub async fn create(
    client: Arc<Client>,
    channel_id: String,
    name: String,
    display_name: Option<String>,
    member_actor_ids: Vec<String>,
    wake_agents: bool,
) -> Result<()> {
    let res: ActorGroupCreateResult = client
        .call(
            method::ACTOR_GROUP_CREATE,
            json!({
                "channelId": channel_id,
                "name": name,
                "displayName": display_name,
                "memberActorIds": member_actor_ids,
                "wakeAgents": wake_agents,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!(
            "group {} @{} members={}",
            res.group.id,
            res.group.name,
            res.group.member_actor_ids.len()
        );
    }
    Ok(())
}

pub async fn list(client: Arc<Client>, channel_id: Option<String>) -> Result<()> {
    let mut params = json!({});
    if let Some(channel_id) = channel_id {
        params["channelId"] = json!(channel_id);
    }
    let res: ActorGroupListResult = client.call(method::ACTOR_GROUP_LIST, params).await?;
    if render::is_json() {
        render::print_json(&res);
        return Ok(());
    }
    if res.groups.is_empty() {
        println!("(no groups)");
        return Ok(());
    }
    for group in res.groups {
        println!(
            "{}\t#{}\t@{}\t{}\tmembers={}",
            group.id,
            group.channel_id,
            group.name,
            if group.wake_agents {
                "wake_agents"
            } else {
                "notify_only"
            },
            group.member_actor_ids.len()
        );
    }
    Ok(())
}

pub async fn add_member(client: Arc<Client>, group_id: String, actor_id: String) -> Result<()> {
    let res: ActorGroupMemberResult = client
        .call(
            method::ACTOR_GROUP_ADD_MEMBER,
            json!({
                "groupId": group_id,
                "actorId": actor_id,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!(
            "group {} members={}",
            res.group.id,
            res.group.member_actor_ids.len()
        );
    }
    Ok(())
}

pub async fn remove_member(client: Arc<Client>, group_id: String, actor_id: String) -> Result<()> {
    let res: ActorGroupMemberResult = client
        .call(
            method::ACTOR_GROUP_REMOVE_MEMBER,
            json!({
                "groupId": group_id,
                "actorId": actor_id,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!(
            "group {} members={}",
            res.group.id,
            res.group.member_actor_ids.len()
        );
    }
    Ok(())
}
