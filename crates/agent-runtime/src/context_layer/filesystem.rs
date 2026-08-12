//! FileSystemProvider — mounts workspace files as context resources.
//!
//! Reads from the scope's workspace directory, respecting agentcontext.yml
//! path declarations. Uses UNC-aware path handling on Windows.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

use proto::types::ScopeRef;

use super::{ResourceContent, ResourceHandle, ResourceProvider};

/// Maximum bytes for a single file read. Mirrors the agent_serve.rs
/// AGENT_PROMPT_FILE_MAX_BYTES constant.
const MAX_READ_BYTES: u64 = 128 * 1024;

/// Mounts workspace files as context resources. Stateless — all paths
/// are derived from scope + profile_dir at call time.
pub struct FileSystemProvider {
    /// The mount path relative to the workspace root, or absolute.
    /// Typically resolved from agentcontext.yml `config.path`.
    mount_path: String,
    /// Maximum number of files to enumerate in list().
    max_files: usize,
}

impl FileSystemProvider {
    /// Create a new FileSystemProvider with the given mount path.
    /// `mount_path` may contain `${workspace.dir}` which is expanded
    /// by the caller before reaching here.
    pub fn new(mount_path: impl Into<String>, max_files: usize) -> Self {
        Self {
            mount_path: mount_path.into(),
            max_files,
        }
    }

    /// Resolve the mount path to an absolute directory.
    /// If `mount_path` is absolute, use it directly; otherwise treat it
    /// as relative to `workspace_dir`.
    fn resolve_base(&self, workspace_dir: &Path) -> PathBuf {
        let p = Path::new(&self.mount_path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            workspace_dir.join(&self.mount_path)
        }
    }

    /// Infer a simple media type from a file extension.
    fn infer_media_type(path: &Path) -> String {
        match path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|e| e.to_lowercase())
            .as_deref()
        {
            Some("md") => "text/markdown".to_string(),
            Some("txt") => "text/plain".to_string(),
            Some("json") => "application/json".to_string(),
            Some("yaml" | "yml") => "application/yaml".to_string(),
            Some("toml") => "application/toml".to_string(),
            Some("rs") => "text/rust".to_string(),
            Some("py") => "text/x-python".to_string(),
            Some("js" | "ts") => "text/javascript".to_string(),
            _ => "application/octet-stream".to_string(),
        }
    }
}

impl ResourceProvider for FileSystemProvider {
    fn scheme(&self) -> &str {
        "file"
    }

    fn list(&self, scope: &ScopeRef, profile_dir: &Path) -> Result<Vec<ResourceHandle>> {
        // Workspace is under profile_dir/../workspace or configured per scope.
        // For the provider, we use profile_dir as the base for relative paths.
        let workspace_dir = profile_dir.join("workspace");
        let base = self.resolve_base(&workspace_dir);

        if !base.exists() {
            return Ok(Vec::new());
        }

        let mut handles = Vec::new();
        let mut count = 0;

        let entries = std::fs::read_dir(&base)
            .with_context(|| format!("list directory {}", base.display()))?;

        for entry in entries {
            if count >= self.max_files {
                break;
            }
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let metadata = entry.metadata()?;
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();
            let uri = format!("file:{}/{}", scope.id, name);

            handles.push(ResourceHandle {
                uri,
                name,
                size_bytes: metadata.len(),
            });
            count += 1;
        }

        Ok(handles)
    }

    fn read(&self, uri: &str, scope: &ScopeRef, profile_dir: &Path) -> Result<ResourceContent> {
        // Parse URI: file:{scope_id}/{filename}
        let prefix = format!("file:{}/", scope.id);
        let filename = uri
            .strip_prefix(&prefix)
            .or_else(|| uri.strip_prefix("file:"))
            .ok_or_else(|| anyhow!("invalid file URI: {uri}"))?;

        let workspace_dir = profile_dir.join("workspace");
        let base = self.resolve_base(&workspace_dir);
        let path = base.join(filename);

        // Security: ensure the resolved path is within the base directory.
        let canonical_base = base.canonicalize().unwrap_or_else(|_| base.clone());
        let canonical_path = path.canonicalize().unwrap_or_else(|_| path.clone());
        if !canonical_path.starts_with(&canonical_base) {
            return Err(anyhow!("path traversal blocked: {filename}"));
        }

        let metadata = std::fs::metadata(&path)
            .with_context(|| format!("read file metadata {}", path.display()))?;

        if metadata.len() > MAX_READ_BYTES {
            return Err(anyhow!(
                "file {} ({} bytes) exceeds max read size ({} bytes)",
                path.display(),
                metadata.len(),
                MAX_READ_BYTES
            ));
        }

        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("read file {}", path.display()))?;

        let media_type = Self::infer_media_type(&path);

        Ok(ResourceContent {
            uri: uri.to_string(),
            media_type,
            text,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proto::types::ScopeKind;

    #[test]
    fn scheme_is_file() {
        let provider = FileSystemProvider::new(".", 10);
        assert_eq!(provider.scheme(), "file");
    }

    #[test]
    fn infer_media_type_basic() {
        assert_eq!(
            FileSystemProvider::infer_media_type(Path::new("foo.md")),
            "text/markdown"
        );
        assert_eq!(
            FileSystemProvider::infer_media_type(Path::new("foo.json")),
            "application/json"
        );
        assert_eq!(
            FileSystemProvider::infer_media_type(Path::new("foo.unknown")),
            "application/octet-stream"
        );
    }
}
