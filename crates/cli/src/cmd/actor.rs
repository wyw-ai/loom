use std::sync::Arc;

use anyhow::Result;
use proto::methods::*;
use serde_json::json;

use crate::client::Client;
use crate::render;

pub async fn list(client: Arc<Client>) -> Result<()> {
    let res: ActorListResult = client.call(method::ACTOR_LIST, json!({})).await?;
    if render::is_json() {
        render::print_json(&res);
        return Ok(());
    }
    if res.actors.is_empty() {
        println!("(no actors)");
    }
    for a in res.actors {
        let kind = serde_json::to_value(a.kind)
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_default();
        println!("{}\t{}\t{}", a.id, kind, a.display_name);
    }
    Ok(())
}
