use std::sync::Arc;

use anyhow::Result;
use proto::methods::*;
use serde_json::json;

use crate::client::Client;
use crate::render;

pub async fn create(
    client: Arc<Client>,
    actor_id: String,
    title: String,
    public: bool,
) -> Result<()> {
    let params = if public {
        json!({ "title": title, "public": true })
    } else {
        json!({ "title": title, "actorId": actor_id })
    };
    let res: ChannelCreateResult = client.call(method::CHANNEL_CREATE, params).await?;
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

pub async fn lookup(client: Arc<Client>, title: String) -> Result<()> {
    let res: ChannelLookupResult = client
        .call(method::CHANNEL_LOOKUP, json!({ "title": title }))
        .await?;
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

pub async fn update(
    client: Arc<Client>,
    channel_id: String,
    title: Option<String>,
    public: bool,
) -> Result<()> {
    let mut params = json!({ "channelId": channel_id });
    if let Some(title) = title {
        params["title"] = json!(title);
    }
    if public {
        params["visibility"] = json!("public");
    }
    let res: ChannelUpdateResult = client.call(method::CHANNEL_UPDATE, params).await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!("channel {}\t{}", res.channel.id, res.channel.title);
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
    workspace_dir: Option<String>,
    mention_ids: Vec<String>,
) -> Result<()> {
    let mention_ids = (!mention_ids.is_empty()).then_some(mention_ids);
    let res: ChannelMemberConfigSetResult = client
        .call(
            method::CHANNEL_MEMBER_CONFIG_SET,
            json!({
                "channelId": channel_id,
                "actorId": actor_id,
                "workspaceDir": workspace_dir,
                "mentionIds": mention_ids,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!(
            "member config set for {}: workspace={}, mentionIds={}",
            res.config.actor_id,
            res.config.workspace_dir.as_deref().unwrap_or("(default)"),
            res.config.mention_ids.len()
        );
    }
    Ok(())
}

pub async fn member_resolve(
    client: Arc<Client>,
    channel_id: String,
    mention_ids: Vec<String>,
) -> Result<()> {
    let res: ChannelMemberResolveResult = client
        .call(
            method::CHANNEL_MEMBER_RESOLVE,
            json!({
                "channelId": channel_id,
                "mentionIds": mention_ids,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
        return Ok(());
    }
    for member in res.members {
        println!(
            "{}\t{}\t{}",
            member.actor.id,
            member.actor.display_name,
            member.mention_ids.join(",")
        );
    }
    if !res.unresolved_mention_ids.is_empty() {
        println!("unresolved\t{}", res.unresolved_mention_ids.join(","));
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

// -----------------------------------------------------------------
// Channel skill management (file-based registry, agent data root)
// -----------------------------------------------------------------

pub async fn skill_add(channel_id: String, source: String, skill_id: Option<String>) -> Result<()> {
    let data_root = crate::cmd::agent_serve::default_data_root_pub();
    let id = skill_id.unwrap_or_else(|| {
        // Derive skill id from the source path's file name.
        std::path::Path::new(&source)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("skill")
            .to_string()
    });
    let registry = super::skill_registry::add_channel_skill(
        &data_root,
        &channel_id,
        id.clone(),
        source.clone(),
    )
    .map_err(|err| anyhow::anyhow!("write channel skill registry: {err}"))?;
    if render::is_json() {
        render::print_json(&registry);
    } else {
        println!("skill '{id}' added to channel {channel_id}");
    }
    Ok(())
}

pub async fn skill_remove(channel_id: String, skill_id: String) -> Result<()> {
    let data_root = crate::cmd::agent_serve::default_data_root_pub();
    let (registry, removed) =
        super::skill_registry::remove_channel_skill(&data_root, &channel_id, &skill_id)
            .map_err(|err| anyhow::anyhow!("read/remove channel skill registry: {err}"))?;
    if render::is_json() {
        render::print_json(&registry);
    } else if removed {
        println!("skill '{skill_id}' removed from channel {channel_id}");
    } else {
        println!("skill '{skill_id}' was not registered in channel {channel_id}");
    }
    Ok(())
}

pub async fn skill_list(channel_id: String) -> Result<()> {
    let data_root = crate::cmd::agent_serve::default_data_root_pub();
    let registry = super::skill_registry::read_channel_skills(&data_root, &channel_id)
        .map_err(|err| anyhow::anyhow!("read channel skill registry: {err}"))?;
    if render::is_json() {
        render::print_json(&registry);
    } else if registry.skills.is_empty() {
        println!("(no skills registered for channel {channel_id})");
    } else {
        for entry in &registry.skills {
            println!("{:<20} {}", entry.id, entry.source);
        }
    }
    Ok(())
}
