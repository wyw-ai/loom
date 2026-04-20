use std::sync::Arc;

use anyhow::Result;
use proto::methods::*;
use serde_json::json;

use crate::client::Client;
use crate::render;

pub async fn create(client: Arc<Client>, space_id: String, title: String) -> Result<()> {
    let res: ConversationCreateResult = client
        .call(
            method::CONVERSATION_CREATE,
            json!({ "spaceId": space_id, "title": title }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!("conv {}\t{}", res.conversation.id, res.conversation.title);
    }
    Ok(())
}

pub async fn list(client: Arc<Client>, space_id: Option<String>) -> Result<()> {
    let res: ConversationListResult = client
        .call(method::CONVERSATION_LIST, json!({ "spaceId": space_id }))
        .await?;
    if render::is_json() {
        render::print_json(&res);
        return Ok(());
    }
    if res.conversations.is_empty() {
        println!("(no conversations)");
    }
    for c in res.conversations {
        println!("{}\t{}\t{}", c.id, c.space_id, c.title);
    }
    Ok(())
}
