use std::sync::Arc;

use anyhow::Result;
use proto::methods::*;
use serde_json::json;

use crate::client::Client;
use crate::render;

pub async fn create(
    client: Arc<Client>,
    channel_id: String,
    root_event_id: String,
    title: String,
) -> Result<()> {
    let res: ThreadCreateResult = client
        .call(
            method::THREAD_CREATE,
            json!({ "channelId": channel_id, "rootEventId": root_event_id, "title": title }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!("thread {}\t{}", res.thread.id, res.thread.title);
    }
    Ok(())
}

pub async fn list(client: Arc<Client>, channel_id: Option<String>) -> Result<()> {
    let res: ThreadListResult = client
        .call(method::THREAD_LIST, json!({ "channelId": channel_id }))
        .await?;
    if render::is_json() {
        render::print_json(&res);
        return Ok(());
    }
    if res.threads.is_empty() {
        println!("(no threads)");
    }
    for t in res.threads {
        println!("{}\t{}\t{}", t.id, t.channel_id, t.title);
    }
    Ok(())
}
