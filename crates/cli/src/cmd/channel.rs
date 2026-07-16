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

pub async fn delete(client: Arc<Client>, channel_id: String) -> Result<()> {
    let res: ChannelDeleteResult = client
        .call(
            method::CHANNEL_DELETE,
            json!({ "channelId": channel_id, "cascade": true }),
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

pub async fn member_config_list(client: Arc<Client>, channel_id: String) -> Result<()> {
    let res: ChannelMemberConfigListResult = client
        .call(
            method::CHANNEL_MEMBER_CONFIG_LIST,
            json!({ "channelId": channel_id }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
        return Ok(());
    }
    if res.configs.is_empty() {
        println!("(no member workspace overrides)");
        return Ok(());
    }
    for config in res.configs {
        println!(
            "{}\t{}",
            config.actor_id,
            config.workspace_dir.as_deref().unwrap_or("(default)")
        );
    }
    Ok(())
}

pub async fn member_config_get(
    client: Arc<Client>,
    channel_id: String,
    actor_id: String,
) -> Result<()> {
    let res: ChannelMemberConfigGetResult = client
        .call(
            method::CHANNEL_MEMBER_CONFIG_GET,
            json!({ "channelId": channel_id, "actorId": actor_id }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
        return Ok(());
    }
    match res.config {
        Some(config) => println!(
            "{}\t{}",
            config.actor_id,
            config.workspace_dir.as_deref().unwrap_or("(default)")
        ),
        None => println!("(default)"),
    }
    Ok(())
}

pub async fn member_config_set(
    client: Arc<Client>,
    channel_id: String,
    actor_id: String,
    workspace_dir: String,
) -> Result<()> {
    let res: ChannelMemberConfigSetResult = client
        .call(
            method::CHANNEL_MEMBER_CONFIG_SET,
            json!({
                "channelId": channel_id,
                "actorId": actor_id,
                "workspaceDir": workspace_dir,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!(
            "workspace override set for {}: {}",
            res.config.actor_id,
            res.config.workspace_dir.as_deref().unwrap_or("(default)")
        );
    }
    Ok(())
}

pub async fn member_config_clear(
    client: Arc<Client>,
    channel_id: String,
    actor_id: String,
) -> Result<()> {
    let res: ChannelMemberConfigClearResult = client
        .call(
            method::CHANNEL_MEMBER_CONFIG_CLEAR,
            json!({ "channelId": channel_id, "actorId": actor_id }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else if res.cleared {
        println!("workspace override cleared");
    } else {
        println!("workspace override was already default");
    }
    Ok(())
}

pub async fn set_instruction(
    client: Arc<Client>,
    channel_id: String,
    instructions: String,
) -> Result<()> {
    let res: ChannelSetInstructionResult = client
        .call(
            method::CHANNEL_SET_INSTRUCTION,
            json!({ "channelId": channel_id, "instructions": instructions }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!("instructions set for channel {}", res.channel.id);
    }
    Ok(())
}

pub async fn get_instruction(client: Arc<Client>, channel_id: String) -> Result<()> {
    let res: ChannelGetInstructionResult = client
        .call(
            method::CHANNEL_GET_INSTRUCTION,
            json!({ "channelId": channel_id }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
        return Ok(());
    }
    match res.instructions {
        Some(instructions) => println!("{}", instructions),
        None => println!("(none)"),
    }
    Ok(())
}

pub async fn clear_instruction(client: Arc<Client>, channel_id: String) -> Result<()> {
    let res: ChannelClearInstructionResult = client
        .call(
            method::CHANNEL_CLEAR_INSTRUCTION,
            json!({ "channelId": channel_id }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else if res.cleared {
        println!("channel instructions cleared");
    } else {
        println!("channel instructions were already empty");
    }
    Ok(())
}
