use std::sync::Arc;

use anyhow::Result;
use proto::methods::*;
use serde_json::json;

use crate::client::Client;
use crate::render;

pub async fn create(client: Arc<Client>, title: String) -> Result<()> {
    let res: ChannelCreateResult = client
        .call(method::CHANNEL_CREATE, json!({ "title": title }))
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
