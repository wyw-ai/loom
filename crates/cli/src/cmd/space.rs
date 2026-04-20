use std::sync::Arc;

use anyhow::Result;
use proto::methods::*;
use serde_json::json;

use crate::client::Client;

pub async fn create(client: Arc<Client>, title: String) -> Result<()> {
    let res: SpaceCreateResult = client
        .call(method::SPACE_CREATE, json!({ "title": title }))
        .await?;
    println!("space {}\t{}", res.space.id, res.space.title);
    Ok(())
}

pub async fn list(client: Arc<Client>) -> Result<()> {
    let res: SpaceListResult = client.call(method::SPACE_LIST, json!({})).await?;
    if res.spaces.is_empty() {
        println!("(no spaces)");
    }
    for s in res.spaces {
        println!("{}\t{}", s.id, s.title);
    }
    Ok(())
}
