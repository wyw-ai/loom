use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use proto::methods::*;
use proto::types::{ScopeKind, ScopeRef};
use serde_json::json;

use crate::client::Client;
use crate::render;

pub async fn publish(
    client: Arc<Client>,
    actor_id: String,
    name: String,
    media_type: Option<String>,
    text: Option<String>,
    file: Option<PathBuf>,
    scope_id: Option<String>,
    is_channel: bool,
) -> Result<()> {
    let body = match (text, file) {
        (Some(_), Some(_)) => bail!("--text and --file are mutually exclusive"),
        (Some(t), None) => t,
        (None, Some(p)) => {
            std::fs::read_to_string(&p).with_context(|| format!("read {}", p.display()))?
        }
        (None, None) => {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .context("read stdin")?;
            if buf.is_empty() {
                bail!("artifact body is empty (provide --text, --file, or pipe stdin)");
            }
            buf
        }
    };
    let media_type = media_type.unwrap_or_else(|| "text/markdown".into());
    let scope = scope_id.map(|id| ScopeRef {
        kind: if is_channel {
            ScopeKind::Channel
        } else {
            ScopeKind::Thread
        },
        id,
    });
    let params = json!({
        "createdBy": actor_id,
        "scope": scope,
        "ingress": {
            "kind": "inline_text",
            "name": name,
            "mediaType": media_type,
            "text": body,
        }
    });
    let res: ArtifactPublishResult = client.call(method::ARTIFACT_PUBLISH, params).await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        let a = &res.artifact;
        println!("{}\t{}\t{} bytes\t{}", a.id, a.media_type, a.size, a.uri);
    }
    Ok(())
}

pub async fn get(client: Arc<Client>, id_or_uri: String) -> Result<()> {
    let params = if id_or_uri.starts_with("artifact://") {
        json!({ "artifactUri": id_or_uri })
    } else {
        json!({ "artifactId": id_or_uri })
    };
    let res: ArtifactGetResult = client.call(method::ARTIFACT_GET, params).await?;
    if render::is_json() {
        render::print_json(&res);
        return Ok(());
    }
    let a = &res.artifact;
    println!("id        {}", a.id);
    println!("uri       {}", a.uri);
    println!("name      {}", a.name);
    println!("kind      {:?}", a.kind);
    println!("media     {}", a.media_type);
    println!("size      {} bytes", a.size);
    println!("checksum  {}", a.checksum);
    println!("createdBy {}", a.created_by);
    println!("createdAt {}", a.created_at);
    Ok(())
}

pub async fn read(
    client: Arc<Client>,
    artifact_id: String,
    offset: u64,
    max_bytes: u64,
) -> Result<()> {
    let res: ArtifactReadResult = client
        .call(
            method::ARTIFACT_READ,
            json!({ "artifactId": artifact_id, "offset": offset, "maxBytes": max_bytes }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
        return Ok(());
    }
    print!("{}", res.content);
    if !res.content.ends_with('\n') {
        println!();
    }
    if res.truncated {
        eprintln!(
            "(read {} bytes from offset {}; next offset {})",
            res.bytes.len(),
            res.offset,
            res.next_offset
                .unwrap_or(res.offset + res.bytes.len() as u64)
        );
    }
    Ok(())
}
