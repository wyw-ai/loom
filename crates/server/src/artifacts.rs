use std::path::{Path, PathBuf};

use chrono::Utc;
use proto::methods::{ArtifactIngress, ArtifactPublishParams, ArtifactReadResult};
use proto::types::*;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::store::{Store, StoreError, StoreResult};

pub struct ArtifactStore {
    root: PathBuf,
}

impl ArtifactStore {
    pub fn new(root: impl Into<PathBuf>) -> std::io::Result<Self> {
        let root: PathBuf = root.into();
        std::fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn publish(&self, store: &Store, params: ArtifactPublishParams) -> StoreResult<Artifact> {
        let id = format!("art_{}", short_id());
        let dir = self.root.join(&id);
        std::fs::create_dir_all(&dir)?;
        let (name, media_type, bytes) = match params.ingress {
            ArtifactIngress::InlineText(t) => {
                let media = if t.media_type.is_empty() {
                    "text/markdown".to_string()
                } else {
                    t.media_type
                };
                (t.name, media, t.text.into_bytes())
            }
        };
        let safe_name = sanitize_name(&name);
        let path = dir.join(&safe_name);
        std::fs::write(&path, &bytes)?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let checksum = format!("sha256:{}", hex::encode(hasher.finalize()));
        let uri = format!("artifact://{}/{}", id, safe_name);
        let artifact = Artifact {
            id,
            uri,
            kind: ArtifactKind::File,
            name,
            media_type,
            size: bytes.len() as u64,
            checksum,
            created_by: params.created_by,
            created_at: Utc::now(),
            _meta: None,
        };
        store.put_artifact(artifact.clone())?;
        Ok(artifact)
    }

    pub fn read(&self, artifact: &Artifact, max_bytes: u64) -> StoreResult<ArtifactReadResult> {
        let safe = sanitize_name(&artifact.name);
        let path = self.root.join(&artifact.id).join(&safe);
        let bytes = std::fs::read(&path)
            .map_err(|e| StoreError::NotFound(format!("artifact body {}: {}", artifact.id, e)))?;
        let max = max_bytes.max(1) as usize;
        let truncated = bytes.len() > max;
        let slice = if truncated { &bytes[..max] } else { &bytes[..] };
        let content = String::from_utf8_lossy(slice).to_string();
        Ok(ArtifactReadResult {
            artifact_id: artifact.id.clone(),
            media_type: artifact.media_type.clone(),
            truncated,
            content,
        })
    }
}

fn sanitize_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push_str("artifact");
    }
    out
}

fn short_id() -> String {
    Uuid::new_v4().simple().to_string()[..12].to_string()
}
