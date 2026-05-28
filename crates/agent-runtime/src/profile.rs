//! Scaffolding for `{agent.profile}/memory/`.
//!
//! Called on every agent spawn when memory is enabled. Existing files are
//! preserved; the scaffold only ensures the memory records directory and its
//! marker file exist.

use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct ProfileScaffold<'a> {
    pub profile_dir: &'a Path,
    /// Relative-or-absolute path for memory records dir.
    pub memory_root: &'a str,
}

pub fn ensure_profile_scaffold(params: &ProfileScaffold<'_>) -> io::Result<()> {
    std::fs::create_dir_all(params.profile_dir)?;

    let memory_root = resolve_relative(params.profile_dir, params.memory_root);
    std::fs::create_dir_all(&memory_root)?;

    // Drop a memory/meta.json marker next to records/ so the dir layout is
    // self-describing. Same convention as AgentX.
    let memory_meta = memory_root
        .parent()
        .unwrap_or(&memory_root)
        .join("meta.json");
    if !memory_meta.exists() {
        if let Some(parent) = memory_meta.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let meta = serde_json::json!({
            "schemaVersion": 1,
            "storeType": "jsonl",
            "shardBy": "month",
        });
        std::fs::write(
            &memory_meta,
            serde_json::to_string_pretty(&meta).unwrap_or_else(|_| "{}".into()),
        )?;
    }

    Ok(())
}

fn resolve_relative(profile_dir: &Path, rel: &str) -> PathBuf {
    let p = Path::new(rel);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        profile_dir.join(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir() -> PathBuf {
        let p = std::env::temp_dir().join(format!("loom-profile-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn params<'a>(dir: &'a Path) -> ProfileScaffold<'a> {
        ProfileScaffold {
            profile_dir: dir,
            memory_root: "./memory/records",
        }
    }

    #[test]
    fn creates_memory_layout_on_first_run() {
        let dir = tmpdir();
        ensure_profile_scaffold(&params(&dir)).unwrap();
        assert!(dir.join("memory/records").is_dir());
        assert!(dir.join("memory/meta.json").exists());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn preserves_existing_meta_file() {
        let dir = tmpdir();
        std::fs::create_dir_all(dir.join("memory")).unwrap();
        std::fs::write(dir.join("memory/meta.json"), "{\"custom\":true}").unwrap();
        ensure_profile_scaffold(&params(&dir)).unwrap();
        let meta = std::fs::read_to_string(dir.join("memory/meta.json")).unwrap();
        assert_eq!(meta, "{\"custom\":true}");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn absolute_memory_root_is_respected() {
        let dir = tmpdir();
        let alt = tmpdir().join("custom-memory");
        let p = ProfileScaffold {
            profile_dir: &dir,
            memory_root: alt.to_str().unwrap(),
        };
        ensure_profile_scaffold(&p).unwrap();
        assert!(alt.is_dir());
        assert!(!dir.join("memory/records").exists());
        std::fs::remove_dir_all(dir).ok();
        std::fs::remove_dir_all(alt).ok();
    }
}
