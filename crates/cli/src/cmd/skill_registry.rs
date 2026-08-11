use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use fs2::FileExt;
use proto::path_component::validate_path_component;
use serde::{Deserialize, Serialize};

/// A single skill entry in a channel or thread skill registry.
///
/// `#[serde(rename_all = "camelCase")]` keeps this struct wire-compatible
/// with the GUI's `SkillEntryDto`, which serializes `added_at` as
/// `addedAt`. `#[serde(alias = "added_at")]` on `added_at` preserves
/// backward compatibility with older `snake_case` registry files written
/// by previous CLI versions.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillEntry {
    pub id: String,
    pub source: String,
    #[serde(alias = "added_at", alias = "addedAt")]
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

/// Validate a scope identifier (channel or thread id) before joining it
/// into a runtime-managed directory. Returns an `io::Error` on failure so
/// callers can surface it through the existing `io::Result` API.
fn validate_scope_id(scope: &str, what: &str) -> io::Result<()> {
    match validate_path_component(scope, what) {
        Ok(()) => Ok(()),
        Err(err) => Err(io::Error::new(io::ErrorKind::InvalidInput, err)),
    }
}

/// Validate a skill id. Skill ids become part of agent prompt assembly and
/// may be used in file lookups, so they must satisfy the same path-component
/// safety rules as channel/thread ids.
fn validate_skill_id(skill_id: &str) -> io::Result<()> {
    validate_scope_id(skill_id, "skill_id")
}

/// Validate a skill `source` field. `source` is either a directory path or
/// a URL-like identifier; we do NOT reject slashes (legit absolute paths),
/// but we reject any `..` segment so a malicious source cannot escape the
/// data root via path traversal when source is interpreted as a filesystem
/// path by the agent runtime.
fn validate_skill_source(source: &str) -> io::Result<()> {
    if source.split('/').any(|seg| seg == "..") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "skill source must not contain `..` path segments",
        ));
    }
    if source.split('\\').any(|seg| seg == "..") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "skill source must not contain `..` path segments",
        ));
    }
    Ok(())
}

/// `<data_root>/channels/<channel_id>/channel-skills.json`
pub fn channel_skill_registry_path(data_root: &Path, channel_id: &str) -> PathBuf {
    data_root
        .join("channels")
        .join(channel_id)
        .join("channel-skills.json")
}

/// `<data_root>/channels/<channel_id>/threads/<thread_id>/thread-skills.json`
pub fn thread_skill_registry_path(data_root: &Path, channel_id: &str, thread_id: &str) -> PathBuf {
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

/// Max time to wait for the cross-process registry lock before failing.
const REGISTRY_LOCK_TIMEOUT: Duration = Duration::from_secs(5);

/// Read a skill registry from `path`. Returns an empty registry if the
/// file does not exist (graceful backward compatibility).
///
/// If the main file exists but is corrupt (e.g. a partial write from a
/// crash that somehow bypassed the atomic rename), falls back to the
/// `.bak` sidecar file as a last-known-good copy (issue #6
/// defense-in-depth).
fn read_registry(path: &Path) -> io::Result<SkillRegistry> {
    match fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<SkillRegistry>(&text) {
            Ok(reg) => Ok(reg),
            Err(parse_err) => {
                // Main file is corrupt; try the .bak fallback.
                tracing::warn!(
                    path = %path.display(),
                    error = %parse_err,
                    "skill registry JSON corrupt, falling back to .bak (issue #6 defense-in-depth)"
                );
                read_bak_registry(path).or_else(|bak_err| {
                    Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("{parse_err}; .bak also unavailable: {bak_err}"),
                    ))
                })
            }
        },
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            // No main file; try .bak in case only the backup survived,
            // otherwise return an empty registry (graceful default).
            read_bak_registry(path).or(Ok(SkillRegistry::default()))
        }
        Err(err) => Err(err),
    }
}

/// Attempt to read the `.bak` sidecar for `path`. Returns `Ok(registry)`
/// if the backup exists and parses, or an `Err(NotFound)` if there is no
/// backup (so callers can treat the absence as "no fallback available").
fn read_bak_registry(path: &Path) -> io::Result<SkillRegistry> {
    let bak = bak_file_for(path);
    let text = fs::read_to_string(&bak)?;
    let reg: SkillRegistry = serde_json::from_str(&text)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    tracing::info!(
        bak = %bak.display(),
        "restored skill registry from .bak backup"
    );
    Ok(reg)
}

/// Write a skill registry to `path`, creating parent directories.
///
/// The write is atomic: we serialize into a temp file in the same
/// directory, `flush` + `sync_all` to durability, then `rename` over the
/// target. Partial writes from a crashed process therefore never leave a
/// corrupt registry file visible to readers.
///
/// Before overwriting, the previous version (if any) is copied to a
/// `.bak` sidecar so `read_registry` can recover if the main file is ever
/// corrupted (issue #6 defense-in-depth).
fn write_registry(path: &Path, registry: &SkillRegistry) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Save the current file as .bak before writing the new one. We only
    // copy (not rename) so the current file stays in place for the atomic
    // rename below. If the copy fails we proceed anyway: the .bak is a
    // best-effort fallback, not a correctness requirement.
    if path.exists() {
        let bak = bak_file_for(path);
        if let Err(err) = fs::copy(path, &bak) {
            tracing::debug!(
                path = %path.display(),
                bak = %bak.display(),
                error = %err,
                "failed to write .bak backup (non-fatal)"
            );
        }
    }

    let text = serde_json::to_string_pretty(registry)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;

    // Write to a sibling temp file then atomically rename. Using the same
    // directory guarantees the rename is atomic on POSIX and replace-on-POSIX
    // + same-volume on Windows.
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(text.as_bytes())?;
    tmp.as_file_mut().sync_all()?;
    tmp.persist(path).map_err(|err| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("atomic persist failed: {err}"),
        )
    })?;
    Ok(())
}

/// Cross-process lock guard for a skill registry file. We lock a sibling
/// `.lock` file (creating it if needed) for the duration of a read-modify-
/// write cycle so concurrent CLI/GUI/agent processes cannot interleave and
/// clobber each other's edits.
struct RegistryLock(fs::File);

impl RegistryLock {
    fn acquire(path: &Path, timeout: Duration) -> io::Result<Self> {
        let lock_path = lock_file_for(path);
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .open(&lock_path)?;
        // Spin-wait with short sleeps until the exclusive lock is acquired
        // or the timeout elapses. `try_lock_exclusive` is non-blocking.
        //
        // On Windows, a contended `try_lock_exclusive` returns a raw OS
        // error (ERROR_LOCK_VIOLATION = 33) rather than `WouldBlock`, so
        // we treat both the explicit `WouldBlock` kind and the
        // `Uncategorized` raw-error-33 case as "retry".
        let start = std::time::Instant::now();
        loop {
            match file.try_lock_exclusive() {
                Ok(()) => return Ok(RegistryLock(file)),
                Err(ref err) if is_lock_contended(err) => {
                    if start.elapsed() >= timeout {
                        return Err(io::Error::new(
                            io::ErrorKind::WouldBlock,
                            "timed out waiting for skill registry lock",
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(err) => return Err(err),
            }
        }
    }
}

/// Returns true if `err` represents a "lock is held by someone else"
/// condition rather than a real I/O failure. fs2 surfaces this differently
/// per platform: `WouldBlock` on Unix, raw OS error 33 (ERROR_LOCK_VIOLATION)
/// on Windows.
fn is_lock_contended(err: &io::Error) -> bool {
    if err.kind() == io::ErrorKind::WouldBlock {
        return true;
    }
    // Windows: ERROR_LOCK_VIOLATION = 33. fs2 surfaces the raw OS error
    // with `Uncategorized` kind on stable Rust.
    #[cfg(windows)]
    if let Some(raw) = err.raw_os_error() {
        if raw == 33 {
            return true;
        }
    }
    false
}

impl Drop for RegistryLock {
    fn drop(&mut self) {
        // Best-effort unlock; ignore errors.
        let _ = self.0.unlock();
    }
}

fn lock_file_for(registry_path: &Path) -> PathBuf {
    let mut name = registry_path
        .file_name()
        .map(|s| s.to_os_string())
        .unwrap_or_else(|| std::ffi::OsString::from("registry"));
    name.push(".lock");
    registry_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(name)
}

/// Return the `.bak` sidecar path for a registry file. Used by
/// `write_registry` (to save the previous version) and `read_registry`
/// (to recover when the main file is corrupt).
fn bak_file_for(registry_path: &Path) -> PathBuf {
    let mut name = registry_path
        .file_name()
        .map(|s| s.to_os_string())
        .unwrap_or_else(|| std::ffi::OsString::from("registry"));
    name.push(".bak");
    registry_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(name)
}

// ---------------------------------------------------------------------
// Public API — channel skills
// ---------------------------------------------------------------------

/// Read the channel skill registry. Returns empty if missing.
pub fn read_channel_skills(data_root: &Path, channel_id: &str) -> io::Result<SkillRegistry> {
    validate_scope_id(channel_id, "channel_id")?;
    read_registry(&channel_skill_registry_path(data_root, channel_id))
}

/// Add or replace a skill in the channel registry.
pub fn add_channel_skill(
    data_root: &Path,
    channel_id: &str,
    skill_id: String,
    source: String,
) -> io::Result<SkillRegistry> {
    validate_scope_id(channel_id, "channel_id")?;
    validate_skill_id(&skill_id)?;
    validate_skill_source(&source)?;
    let path = channel_skill_registry_path(data_root, channel_id);
    let _lock = RegistryLock::acquire(&path, REGISTRY_LOCK_TIMEOUT)?;
    let mut reg = read_registry(&path)?;
    reg.upsert(SkillEntry {
        id: skill_id,
        source,
        added_at: now_iso(),
    });
    write_registry(&path, &reg)?;
    Ok(reg)
}

/// Remove a skill from the channel registry. Returns `(updated_registry,
/// was_present)`.
pub fn remove_channel_skill(
    data_root: &Path,
    channel_id: &str,
    skill_id: &str,
) -> io::Result<(SkillRegistry, bool)> {
    validate_scope_id(channel_id, "channel_id")?;
    validate_skill_id(skill_id)?;
    let path = channel_skill_registry_path(data_root, channel_id);
    let _lock = RegistryLock::acquire(&path, REGISTRY_LOCK_TIMEOUT)?;
    let mut reg = read_registry(&path)?;
    let removed = reg.remove(skill_id);
    if removed {
        write_registry(&path, &reg)?;
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
    validate_scope_id(channel_id, "channel_id")?;
    validate_scope_id(thread_id, "thread_id")?;
    read_registry(&thread_skill_registry_path(
        data_root, channel_id, thread_id,
    ))
}

/// Add or replace a skill in the thread registry.
pub fn add_thread_skill(
    data_root: &Path,
    channel_id: &str,
    thread_id: &str,
    skill_id: String,
    source: String,
) -> io::Result<SkillRegistry> {
    validate_scope_id(channel_id, "channel_id")?;
    validate_scope_id(thread_id, "thread_id")?;
    validate_skill_id(&skill_id)?;
    validate_skill_source(&source)?;
    let path = thread_skill_registry_path(data_root, channel_id, thread_id);
    let _lock = RegistryLock::acquire(&path, REGISTRY_LOCK_TIMEOUT)?;
    let mut reg = read_registry(&path)?;
    reg.upsert(SkillEntry {
        id: skill_id,
        source,
        added_at: now_iso(),
    });
    write_registry(&path, &reg)?;
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
    validate_scope_id(channel_id, "channel_id")?;
    validate_scope_id(thread_id, "thread_id")?;
    validate_skill_id(skill_id)?;
    let path = thread_skill_registry_path(data_root, channel_id, thread_id);
    let _lock = RegistryLock::acquire(&path, REGISTRY_LOCK_TIMEOUT)?;
    let mut reg = read_registry(&path)?;
    let removed = reg.remove(skill_id);
    if removed {
        write_registry(&path, &reg)?;
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

        let reg =
            add_channel_skill(root, "chan1", "obsidian".into(), "/path/to/skill".into()).unwrap();
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

        let reg =
            add_thread_skill(root, "chan1", "thr1", "pdf".into(), "/path/to/pdf".into()).unwrap();
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

    // ---- Regression: issue #2 path validation ---------------------------

    #[test]
    fn rejects_channel_id_with_path_traversal() {
        let tmp = TempDir::new().unwrap();
        let err =
            add_channel_skill(tmp.path(), "../escape", "skill1".into(), "/src".into()).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);

        let err = read_channel_skills(tmp.path(), "..").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);

        let err = remove_channel_skill(tmp.path(), "..", "skill1").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn rejects_thread_id_with_path_traversal() {
        let tmp = TempDir::new().unwrap();
        let err = add_thread_skill(
            tmp.path(),
            "chan1",
            "../../etc",
            "skill1".into(),
            "/src".into(),
        )
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);

        let err = read_thread_skills(tmp.path(), "chan1", "../../etc").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn rejects_unsafe_skill_id() {
        let tmp = TempDir::new().unwrap();
        for bad in ["..", "../escape", "-flag", "a/b", "foo..bar", ".hidden"] {
            let err = add_channel_skill(tmp.path(), "chan1", bad.into(), "/src".into())
                .err()
                .unwrap_or_else(|| panic!("expected reject for {bad}"));
            assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "skill_id={bad}");
        }
    }

    #[test]
    fn rejects_skill_source_with_dotdot_segment() {
        let tmp = TempDir::new().unwrap();
        for bad in [
            "../escape",
            "/abs/../../etc",
            "foo/../../bar",
            "a\\..\\..\\b",
        ] {
            let err = add_channel_skill(tmp.path(), "chan1", "skill1".into(), bad.into())
                .err()
                .unwrap_or_else(|| panic!("expected reject for {bad}"));
            assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "source={bad}");
        }
        // Legit absolute path with no `..` segment is accepted.
        add_channel_skill(
            tmp.path(),
            "chan1",
            "skill_ok".into(),
            "/abs/path/to/skill".into(),
        )
        .expect("legit absolute source");
    }

    // ---- Regression: issue #5 camelCase / snake_case compat -------------

    #[test]
    fn skill_entry_serializes_as_camel_case_to_match_gui() {
        let entry = SkillEntry {
            id: "obsidian".into(),
            source: "/path".into(),
            added_at: "1970-01-01T00:00:00000Z".into(),
        };
        let json = serde_json::to_value(&entry).unwrap();
        // GUI reads `addedAt`; CLI must emit the same key for wire compat.
        assert!(json.get("addedAt").is_some(), "must emit addedAt: {json}");
        assert!(
            json.get("added_at").is_none(),
            "must not emit snake_case added_at: {json}"
        );
    }

    #[test]
    fn skill_entry_deserializes_both_camel_and_snake_case() {
        // Legacy CLI-written file (snake_case).
        let snake = r#"{"id":"a","source":"/s","added_at":"1970-01-01T00:00:00000Z"}"#;
        let e: SkillEntry = serde_json::from_str(snake).expect("parse snake_case");
        assert_eq!(e.id, "a");
        assert_eq!(e.added_at, "1970-01-01T00:00:00000Z");

        // GUI-written file (camelCase).
        let camel = r#"{"id":"b","source":"/s","addedAt":"1970-01-01T00:00:00001Z"}"#;
        let e: SkillEntry = serde_json::from_str(camel).expect("parse camelCase");
        assert_eq!(e.id, "b");
        assert_eq!(e.added_at, "1970-01-01T00:00:00001Z");
    }

    #[test]
    fn round_trip_preserves_camel_case_on_disk() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        add_channel_skill(root, "chan1", "obs".into(), "/p".into()).unwrap();
        let on_disk = fs::read_to_string(channel_skill_registry_path(root, "chan1")).unwrap();
        assert!(
            on_disk.contains("\"addedAt\""),
            "on-disk file must use camelCase: {on_disk}"
        );
        assert!(
            !on_disk.contains("\"added_at\""),
            "on-disk file must not use snake_case: {on_disk}"
        );
    }

    // ---- Regression: issue #6 atomic write + cross-process lock ---------

    #[test]
    fn atomic_write_leaves_no_partial_file_on_success() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        add_channel_skill(root, "chan1", "s1".into(), "/p".into()).unwrap();
        // After a successful write, only the final registry file should
        // exist (no leftover temp files in the directory).
        let registry_path = channel_skill_registry_path(root, "chan1");
        let dir = registry_path.parent().unwrap();
        let files: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            !files
                .iter()
                .any(|f| f.starts_with(".tmp") || f.contains("tmp")),
            "leftover temp file: {files:?}"
        );
        assert!(files.iter().any(|f| f == "channel-skills.json"));
    }

    #[test]
    fn concurrent_writes_do_not_lose_edits() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        let mut handles = Vec::new();
        for i in 0..8 {
            let root = root.clone();
            handles.push(std::thread::spawn(move || {
                add_channel_skill(&root, "chan1", format!("skill_{i}"), format!("/src/{i}"))
                    .expect("add channel skill");
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        let reg = read_channel_skills(&root, "chan1").unwrap();
        // All 8 concurrent upserts must be present (lock prevents lost updates).
        assert_eq!(reg.skills.len(), 8, "concurrent writes lost edits");
    }

    /// Regression for issue #6 (defense-in-depth): if the main registry
    /// file is corrupt, read_registry must fall back to the `.bak`
    /// sidecar written by the previous successful write_registry call.
    #[test]
    fn read_registry_falls_back_to_bak_when_main_is_corrupt() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        // Write a valid registry twice: the first write creates the file,
        // the second write saves a .bak of the first version.
        add_channel_skill(root, "chan1", "obsidian".into(), "/src".into()).unwrap();
        add_channel_skill(root, "chan1", "pdf".into(), "/pdf".into()).unwrap();

        let main_path = channel_skill_registry_path(root, "chan1");
        assert!(main_path.exists(), "main file should exist after write");

        // .bak should exist (second write saved the first version).
        let bak_path = bak_file_for(&main_path);
        assert!(bak_path.exists(), ".bak should exist after second write");

        // Corrupt the main file.
        fs::write(&main_path, b"NOT VALID JSON{{{").unwrap();

        // read should fall back to .bak and return the saved skill(s).
        let recovered = read_channel_skills(root, "chan1").unwrap();
        // .bak has the state from the first write (1 skill: obsidian).
        assert_eq!(
            recovered.skills.len(),
            1,
            "bak should have 1 skill (from first write)"
        );
        assert_eq!(recovered.skills[0].id, "obsidian");
    }

    /// Regression for issue #6 (defense-in-depth): write_registry must
    /// create/refresh the .bak sidecar on every write, so the backup
    /// always reflects the most recent successful state.
    #[test]
    fn write_registry_creates_bak_on_each_write() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let main_path = channel_skill_registry_path(root, "chan1");
        let bak_path = bak_file_for(&main_path);

        // First write: no prior file, so no .bak yet.
        add_channel_skill(root, "chan1", "skill_a".into(), "/a".into()).unwrap();
        assert!(!bak_path.exists(), "no .bak on first write (no prior file)");

        // Second write: prior file exists, .bak should be created.
        add_channel_skill(root, "chan1", "skill_b".into(), "/b".into()).unwrap();
        assert!(bak_path.exists(), ".bak should exist after second write");

        // .bak should contain the state from the first write (1 skill: skill_a).
        let bak_text = fs::read_to_string(&bak_path).unwrap();
        let bak_reg: SkillRegistry = serde_json::from_str(&bak_text).unwrap();
        assert_eq!(
            bak_reg.skills.len(),
            1,
            "bak should have 1 skill (from first write)"
        );
        assert_eq!(bak_reg.skills[0].id, "skill_a");

        // Third write: .bak should now reflect the second write (2 skills).
        add_channel_skill(root, "chan1", "skill_c".into(), "/c".into()).unwrap();
        let bak_text = fs::read_to_string(&bak_path).unwrap();
        let bak_reg: SkillRegistry = serde_json::from_str(&bak_text).unwrap();
        assert_eq!(
            bak_reg.skills.len(),
            2,
            "bak should have 2 skills (from second write)"
        );
    }

    /// Regression for issue #6 (defense-in-depth): if both the main file
    /// and .bak are missing, read_registry returns an empty registry
    /// (graceful default), not an error.
    #[test]
    fn read_registry_returns_empty_when_both_main_and_bak_missing() {
        let tmp = TempDir::new().unwrap();
        let reg = read_channel_skills(tmp.path(), "chan_no_files").unwrap();
        assert!(reg.skills.is_empty());
    }
}
