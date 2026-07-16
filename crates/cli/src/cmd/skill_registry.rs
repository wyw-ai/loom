use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A single skill entry in a channel or thread skill registry.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillEntry {
    pub id: String,
    pub source: String,
    pub added_at: String,
}

/// The on-disk registry file format.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SkillRegistry {
    #[serde(default)]
    pub skills: Vec<SkillEntry>,
}

impl SkillRegistry {
    /// Look up a skill by id.
    pub fn find(&self, id: &str) -> Option<&SkillEntry> {
        self.skills.iter().find(|s| s.id == id)
    }

    /// Remove a skill by id, returning true if it was present.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.skills.len();
        self.skills.retain(|s| s.id != id);
        self.skills.len() != before
    }

    /// Insert or replace a skill by id.
    pub fn upsert(&mut self, entry: SkillEntry) {
        if let Some(existing) = self.skills.iter_mut().find(|s| s.id == entry.id) {
            *existing = entry;
        } else {
            self.skills.push(entry);
        }
    }
}

// ---------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------

/// `<data_root>/channels/<channel_id>/channel-skills.json`
pub fn channel_skill_registry_path(data_root: &Path, channel_id: &str) -> PathBuf {
    data_root
        .join("channels")
        .join(channel_id)
        .join("channel-skills.json")
}

/// `<data_root>/channels/<channel_id>/threads/<thread_id>/thread-skills.json`
pub fn thread_skill_registry_path(
    data_root: &Path,
    channel_id: &str,
    thread_id: &str,
) -> PathBuf {
    data_root
        .join("channels")
        .join(channel_id)
        .join("threads")
        .join(thread_id)
        .join("thread-skills.json")
}

// ---------------------------------------------------------------------
// Read / write
// ---------------------------------------------------------------------

/// Read a skill registry from `path`. Returns an empty registry if the
/// file does not exist (graceful backward compatibility).
fn read_registry(path: &Path) -> io::Result<SkillRegistry> {
    match fs::read_to_string(path) {
        Ok(text) => {
            let reg: SkillRegistry = serde_json::from_str(&text)
                .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
            Ok(reg)
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(SkillRegistry::default()),
        Err(err) => Err(err),
    }
}

/// Write a skill registry to `path`, creating parent directories.
fn write_registry(path: &Path, registry: &SkillRegistry) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(registry)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    fs::write(path, text)
}

// ---------------------------------------------------------------------
// Public API — channel skills
// ---------------------------------------------------------------------

/// Read the channel skill registry. Returns empty if missing.
pub fn read_channel_skills(data_root: &Path, channel_id: &str) -> io::Result<SkillRegistry> {
    read_registry(&channel_skill_registry_path(data_root, channel_id))
}

/// Add or replace a skill in the channel registry.
pub fn add_channel_skill(
    data_root: &Path,
    channel_id: &str,
    skill_id: String,
    source: String,
) -> io::Result<SkillRegistry> {
    let mut reg = read_channel_skills(data_root, channel_id)?;
    reg.upsert(SkillEntry {
        id: skill_id,
        source,
        added_at: now_iso(),
    });
    write_registry(&channel_skill_registry_path(data_root, channel_id), &reg)?;
    Ok(reg)
}

/// Remove a skill from the channel registry. Returns `(updated_registry,
/// was_present)`.
pub fn remove_channel_skill(
    data_root: &Path,
    channel_id: &str,
    skill_id: &str,
) -> io::Result<(SkillRegistry, bool)> {
    let mut reg = read_channel_skills(data_root, channel_id)?;
    let removed = reg.remove(skill_id);
    if removed {
        write_registry(&channel_skill_registry_path(data_root, channel_id), &reg)?;
    }
    Ok((reg, removed))
}

// ---------------------------------------------------------------------
// Public API — thread skills
// ---------------------------------------------------------------------

/// Read the thread skill registry. Returns empty if missing.
pub fn read_thread_skills(
    data_root: &Path,
    channel_id: &str,
    thread_id: &str,
) -> io::Result<SkillRegistry> {
    read_registry(&thread_skill_registry_path(data_root, channel_id, thread_id))
}

/// Add or replace a skill in the thread registry.
pub fn add_thread_skill(
    data_root: &Path,
    channel_id: &str,
    thread_id: &str,
    skill_id: String,
    source: String,
) -> io::Result<SkillRegistry> {
    let mut reg = read_thread_skills(data_root, channel_id, thread_id)?;
    reg.upsert(SkillEntry {
        id: skill_id,
        source,
        added_at: now_iso(),
    });
    write_registry(
        &thread_skill_registry_path(data_root, channel_id, thread_id),
        &reg,
    )?;
    Ok(reg)
}

/// Remove a skill from the thread registry. Returns `(updated_registry,
/// was_present)`.
pub fn remove_thread_skill(
    data_root: &Path,
    channel_id: &str,
    thread_id: &str,
    skill_id: &str,
) -> io::Result<(SkillRegistry, bool)> {
    let mut reg = read_thread_skills(data_root, channel_id, thread_id)?;
    let removed = reg.remove(skill_id);
    if removed {
        write_registry(
            &thread_skill_registry_path(data_root, channel_id, thread_id),
            &reg,
        )?;
    }
    Ok((reg, removed))
}

// ---------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------

fn now_iso() -> String {
    // Use a simple UTC timestamp.  We avoid pulling in chrono just for
    // this — the format matches RFC3339 UTC.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("1970-01-01T00:00:{secs:05}Z")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn missing_registry_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let reg = read_channel_skills(tmp.path(), "chan_missing").unwrap();
        assert!(reg.skills.is_empty());
    }

    #[test]
    fn channel_skill_round_trip() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        let reg = add_channel_skill(root, "chan1", "obsidian".into(), "/path/to/skill".into())
            .unwrap();
        assert_eq!(reg.skills.len(), 1);
        assert_eq!(reg.skills[0].id, "obsidian");

        let reg2 = read_channel_skills(root, "chan1").unwrap();
        assert_eq!(reg2.skills.len(), 1);
        assert_eq!(reg2.skills[0].source, "/path/to/skill");
    }

    #[test]
    fn upsert_replaces_same_id() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        add_channel_skill(root, "chan1", "skill1".into(), "/old".into()).unwrap();
        add_channel_skill(root, "chan1", "skill1".into(), "/new".into()).unwrap();
        let reg = read_channel_skills(root, "chan1").unwrap();
        assert_eq!(reg.skills.len(), 1);
        assert_eq!(reg.skills[0].source, "/new");
    }

    #[test]
    fn remove_returns_false_when_absent() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        let (_, removed) = remove_channel_skill(root, "chan1", "nope").unwrap();
        assert!(!removed);
    }

    #[test]
    fn thread_skill_round_trip() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        let reg = add_thread_skill(
            root,
            "chan1",
            "thr1",
            "pdf".into(),
            "/path/to/pdf".into(),
        )
        .unwrap();
        assert_eq!(reg.skills.len(), 1);

        let reg2 = read_thread_skills(root, "chan1", "thr1").unwrap();
        assert_eq!(reg2.skills[0].id, "pdf");
    }

    #[test]
    fn thread_and_channel_registries_are_independent() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        add_channel_skill(root, "chan1", "shared".into(), "/channel".into()).unwrap();
        add_thread_skill(root, "chan1", "thr1", "shared".into(), "/thread".into()).unwrap();

        let ch = read_channel_skills(root, "chan1").unwrap();
        let th = read_thread_skills(root, "chan1", "thr1").unwrap();

        assert_eq!(ch.skills[0].source, "/channel");
        assert_eq!(th.skills[0].source, "/thread");
    }
}
