use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::Utc;
use parking_lot::Mutex;
use proto::methods::{ArtifactIngress, ArtifactPublishParams, ArtifactReadResult};
use proto::types::*;
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::store::{Store, StoreError, StoreResult};

pub struct ArtifactStore {
    root: PathBuf,
    workspace_root: PathBuf,
    scope_seq: Mutex<HashMap<String, u64>>,
}

impl ArtifactStore {
    pub fn new(
        root: impl Into<PathBuf>,
        workspace_root: impl Into<PathBuf>,
    ) -> std::io::Result<Self> {
        let root: PathBuf = root.into();
        let workspace_root: PathBuf = workspace_root.into();
        std::fs::create_dir_all(&root)?;
        std::fs::create_dir_all(&workspace_root)?;
        Ok(Self {
            root,
            workspace_root,
            scope_seq: Mutex::new(HashMap::new()),
        })
    }

    pub fn publish(&self, store: &Store, params: ArtifactPublishParams) -> StoreResult<Artifact> {
        let id = format!("art_{}", short_id());
        let dir = self.root.join(&id);
        std::fs::create_dir_all(&dir)?;
        let created_by = params.created_by.clone();
        let scope = params.scope.clone();
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
        let workspace_meta = scope
            .as_ref()
            .map(|scope| self.save_workspace_copy(scope, &name, &bytes))
            .transpose()?;
        let artifact = Artifact {
            id,
            uri,
            kind: ArtifactKind::File,
            name,
            media_type,
            size: bytes.len() as u64,
            checksum,
            created_by,
            created_at: Utc::now(),
            _meta: workspace_meta.map(|workspace| {
                let mut meta = Meta::new();
                meta.insert("workspaceEntryId".into(), json!(workspace.entry_id));
                meta.insert("workspaceFilename".into(), json!(workspace.filename));
                meta.insert("workspacePath".into(), json!(workspace.path));
                meta.insert("workspaceScopeKind".into(), json!(scope_kind_name(workspace.kind)));
                meta.insert("workspaceScopeId".into(), json!(workspace.scope_id));
                meta
            }),
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

    fn save_workspace_copy(
        &self,
        scope: &ScopeRef,
        name: &str,
        bytes: &[u8],
    ) -> std::io::Result<WorkspaceFile> {
        let dir = self
            .workspace_root
            .join(scope_kind_name(scope.kind))
            .join(sanitize_name(&scope.id));
        std::fs::create_dir_all(&dir)?;
        let entry_id = self.next_workspace_entry_id(scope, &dir)?;
        let ext = extension_for(name);
        let filename = format!("pasted-content-{entry_id}{ext}");
        let path = dir.join(&filename);
        std::fs::write(&path, bytes)?;
        Ok(WorkspaceFile {
            entry_id,
            filename,
            path: path.display().to_string(),
            kind: scope.kind,
            scope_id: scope.id.clone(),
        })
    }

    fn next_workspace_entry_id(&self, scope: &ScopeRef, dir: &Path) -> std::io::Result<u64> {
        let key = format!("{}:{}", scope_kind_name(scope.kind), scope.id);
        let mut cache = self.scope_seq.lock();
        let next = if let Some(next) = cache.get_mut(&key) {
            let current = *next;
            *next = next.saturating_add(1);
            current
        } else {
            let current = scan_next_workspace_entry_id(dir)?;
            cache.insert(key, current.saturating_add(1));
            current
        };
        Ok(next)
    }
}

struct WorkspaceFile {
    entry_id: u64,
    filename: String,
    path: String,
    kind: ScopeKind,
    scope_id: String,
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

fn extension_for(name: &str) -> String {
    Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .filter(|ext| !ext.is_empty())
        .map(|ext| format!(".{ext}"))
        .unwrap_or_else(|| ".txt".to_string())
}

fn scope_kind_name(kind: ScopeKind) -> &'static str {
    match kind {
        ScopeKind::Channel => "channel",
        ScopeKind::Thread => "thread",
    }
}

fn scan_next_workspace_entry_id(dir: &Path) -> std::io::Result<u64> {
    let mut next = 1u64;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(rest) = name.strip_prefix("pasted-content-") else {
            continue;
        };
        let id_text = rest.split('.').next().unwrap_or(rest);
        if let Ok(id) = id_text.parse::<u64>() {
            next = next.max(id.saturating_add(1));
        }
    }
    Ok(next)
}

fn short_id() -> String {
    Uuid::new_v4().simple().to_string()[..12].to_string()
}
