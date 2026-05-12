use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use proto::methods::*;
use serde_json::json;

use crate::client::Client;
use crate::render;

use super::target::{resolve_target, TargetMode};

pub async fn upload(
    client: Arc<Client>,
    actor_id: String,
    target: String,
    path: PathBuf,
    media_type: Option<String>,
) -> Result<()> {
    let resolved = resolve_target(&client, &actor_id, &target, TargetMode::Write).await?;
    let bytes = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("attachment")
        .to_string();
    let res: ArtifactPublishResult = client
        .call(
            method::ARTIFACT_PUBLISH,
            json!({
                "createdBy": actor_id,
                "scope": resolved.scope,
                "ingress": {
                    "kind": "file_bytes",
                    "name": name,
                    "mediaType": media_type.unwrap_or_else(|| "application/octet-stream".into()),
                    "bytes": bytes,
                }
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!("Attachment ID: {}", res.artifact.id);
    }
    Ok(())
}

pub async fn view(
    client: Arc<Client>,
    artifact_id: String,
    output: PathBuf,
    max_bytes: u64,
) -> Result<()> {
    let res: ArtifactReadResult = client
        .call(
            method::ARTIFACT_READ,
            json!({ "artifactId": artifact_id, "maxBytes": max_bytes }),
        )
        .await?;
    std::fs::write(&output, &res.bytes).with_context(|| format!("write {}", output.display()))?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!("{}", output.display());
        if res.truncated {
            eprintln!(
                "(truncated to {} bytes; pass --max-bytes to fetch more)",
                max_bytes
            );
        }
    }
    Ok(())
}
