//! Scaffolding for `{agent.profile}/identity.md` + `soul.md` + `memory/`.
//!
//! Called on every agent spawn. Files that already exist are preserved
//! verbatim — we only write templates when the file is missing, so user
//! edits are never clobbered. The `memory/records/` dir is just `mkdir -p`.

use std::io;
use std::path::{Path, PathBuf};

/// Parameters for scaffolding. Paths are joined against `profile_dir`; if a
/// caller passes an absolute path for `identity_file` / `soul_file` we treat
/// it as-is (matches the spec's relative-or-absolute semantics).
#[derive(Debug)]
pub struct ProfileScaffold<'a> {
    pub profile_dir: &'a Path,
    pub actor_id: &'a str,
    pub display_name: &'a str,
    /// Marketplace description or empty string. Drives the "Role:" line of
    /// the scaffolded identity template. Purely cosmetic.
    pub description: &'a str,
    /// Relative-or-absolute path for identity.md. Use spec default
    /// (`identity.md`) if unset.
    pub identity_file: &'a str,
    /// Relative-or-absolute path for soul.md.
    pub soul_file: &'a str,
    /// Relative-or-absolute path for memory records dir.
    pub memory_root: &'a str,
}

/// Ensure the profile-dir layout exists, writing templates only where
/// files are absent. Returns any IO error on the first failure — callers
/// should warn-and-continue (a failed scaffold is not fatal to the spawn).
pub fn ensure_profile_scaffold(params: &ProfileScaffold<'_>) -> io::Result<()> {
    std::fs::create_dir_all(params.profile_dir)?;

    let identity_path = resolve_relative(params.profile_dir, params.identity_file);
    if !identity_path.exists() {
        if let Some(parent) = identity_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&identity_path, scaffold_identity_md(params))?;
    }

    let soul_path = resolve_relative(params.profile_dir, params.soul_file);
    if !soul_path.exists() {
        if let Some(parent) = soul_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&soul_path, scaffold_soul_md(params))?;
    }

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

/// Read an identity/soul markdown file. Returns empty string if the file
/// doesn't exist — callers use that to skip the prompt section.
pub fn read_markdown_file(profile_dir: &Path, rel: &str) -> String {
    let path = resolve_relative(profile_dir, rel);
    std::fs::read_to_string(path).unwrap_or_default()
}

fn resolve_relative(profile_dir: &Path, rel: &str) -> PathBuf {
    let p = Path::new(rel);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        profile_dir.join(p)
    }
}

fn scaffold_identity_md(p: &ProfileScaffold<'_>) -> String {
    let display = if p.display_name.trim().is_empty() {
        p.actor_id
    } else {
        p.display_name
    };
    let desc = p.description.trim();
    if desc.is_empty() {
        format!(
            "# {display}\n\
             \n\
             - Role: <one-line role for this actor>\n\
             - Primary responsibility: describe what you own in this Joi workspace.\n\
             - Non-goals: do not invent access to tools or memory you cannot see.\n\
             \n\
             Edit this file freely — it is loaded on every turn as the agent identity section.\n"
        )
    } else {
        format!(
            "# {display}\n\
             \n\
             - Role: {desc}\n\
             - Primary responsibility: handle tasks that fit this actor's strengths inside Joi.\n\
             - Non-goals: do not invent access to tools or memory you cannot see.\n\
             \n\
             Edit this file freely — it is loaded on every turn as the agent identity section.\n"
        )
    }
}

fn scaffold_soul_md(_p: &ProfileScaffold<'_>) -> String {
    "# Operating Style\n\
     \n\
     - Prefer precise, technical communication.\n\
     - Surface risks and tradeoffs early.\n\
     - Use platform memory and conversation tools when available instead of guessing.\n\
     - Separate assumptions from observed facts.\n\
     - Keep collaboration explicit when handing work to another actor.\n\
     \n\
     Edit this file freely — it is loaded on every turn as the agent soul section.\n"
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir() -> PathBuf {
        let p = std::env::temp_dir().join(format!("joi-profile-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn params<'a>(dir: &'a Path, desc: &'a str) -> ProfileScaffold<'a> {
        ProfileScaffold {
            profile_dir: dir,
            actor_id: "actor_test",
            display_name: "Test",
            description: desc,
            identity_file: "identity.md",
            soul_file: "soul.md",
            memory_root: "./memory/records",
        }
    }

    #[test]
    fn creates_all_files_on_first_run() {
        let dir = tmpdir();
        ensure_profile_scaffold(&params(&dir, "Coding assistant")).unwrap();
        assert!(dir.join("identity.md").exists());
        assert!(dir.join("soul.md").exists());
        assert!(dir.join("memory/records").is_dir());
        assert!(dir.join("memory/meta.json").exists());
        let id = std::fs::read_to_string(dir.join("identity.md")).unwrap();
        assert!(id.contains("Coding assistant"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn preserves_user_edited_identity() {
        let dir = tmpdir();
        ensure_profile_scaffold(&params(&dir, "desc")).unwrap();
        std::fs::write(dir.join("identity.md"), "# user override\n\ncustom content").unwrap();
        ensure_profile_scaffold(&params(&dir, "desc")).unwrap();
        let id = std::fs::read_to_string(dir.join("identity.md")).unwrap();
        assert!(id.contains("user override"));
        assert!(id.contains("custom content"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn absolute_paths_are_respected() {
        let dir = tmpdir();
        let alt = tmpdir().join("custom-identity.md");
        let p = ProfileScaffold {
            profile_dir: &dir,
            actor_id: "a",
            display_name: "A",
            description: "",
            identity_file: alt.to_str().unwrap(),
            soul_file: "soul.md",
            memory_root: "./memory/records",
        };
        ensure_profile_scaffold(&p).unwrap();
        assert!(alt.exists());
        assert!(!dir.join("identity.md").exists());
        std::fs::remove_dir_all(dir).ok();
    }
}
