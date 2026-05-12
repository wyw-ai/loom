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
    offset: u64,
    max_bytes: u64,
) -> Result<()> {
    let res: ArtifactReadResult = client
        .call(
            method::ARTIFACT_READ,
            json!({ "artifactId": artifact_id, "offset": offset, "maxBytes": max_bytes }),
        )
        .await?;
    std::fs::write(&output, &res.bytes).with_context(|| format!("write {}", output.display()))?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!("{}", output.display());
        if res.truncated {
            eprintln!(
                "(read {} bytes from offset {}; next offset {})",
                res.bytes.len(),
                res.offset,
                res.next_offset
                    .unwrap_or(res.offset + res.bytes.len() as u64)
            );
        }
    }
    Ok(())
}

pub async fn download(
    client: Arc<Client>,
    artifact_id: String,
    output: PathBuf,
    chunk_bytes: u64,
) -> Result<()> {
    let chunk_bytes = chunk_bytes.max(1);
    let mut offset = 0_u64;
    let mut body = Vec::new();
    loop {
        let res: ArtifactReadResult = client
            .call(
                method::ARTIFACT_READ,
                json!({
                    "artifactId": artifact_id,
                    "offset": offset,
                    "maxBytes": chunk_bytes,
                }),
            )
            .await?;
        body.extend_from_slice(&res.bytes);
        match res.next_offset {
            Some(next) if res.truncated && next > offset => offset = next,
            _ => break,
        }
    }
    std::fs::write(&output, &body).with_context(|| format!("write {}", output.display()))?;
    if render::is_json() {
        render::print_json(&json!({
            "artifactId": artifact_id,
            "output": output,
            "bytes": body.len(),
        }));
    } else {
        println!("{}\t{} bytes", output.display(), body.len());
    }
    Ok(())
}
