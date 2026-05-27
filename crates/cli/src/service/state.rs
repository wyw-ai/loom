// Public S1 surface; runtime calls into these but no plugin yet. Silence
// the dead-code warnings until S2 reaches the first cursor/dedupe site.
#![allow(dead_code)]

//! Per-service file-system state. Pure I/O, no WS, no async — easy to
//! unit test in isolation. The §10 layout under
//! `~/.local/share/loom/service-host/services/<service_id>/` is owned by
//! this module; runtime + host route through these helpers so layout
//! changes only happen here.

use std::collections::HashSet;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use parking_lot::Mutex;

/// Resolve the service-host data root with the standard precedence:
///
/// 1. `LOOM_SERVICE_HOST_DATA` env var (used by tests and ops overrides).
/// 2. `dirs::data_local_dir()/loom/service-host` — Linux
///    `~/.local/share/loom/service-host`, macOS
///    `~/Library/Application Support/loom/service-host`. Matches §10.
/// 3. `./.loom-service-host` as a last-resort relative fallback so the
///    binary still runs in a sandbox without a usable HOME.
pub fn default_data_root() -> PathBuf {
    if let Ok(s) = std::env::var("LOOM_SERVICE_HOST_DATA") {
        return PathBuf::from(s);
    }
    dirs::data_local_dir()
        .map(|p| p.join("loom").join("service-host"))
        .unwrap_or_else(|| PathBuf::from("./.loom-service-host"))
}

/// Per-service directory: `<root>/services/<service_id>/`. Pure path
/// composition — does not touch the filesystem.
pub fn state_dir(root: &Path, service_id: &str) -> PathBuf {
    root.join("services").join(service_id)
}

/// Per-instance directory for a `lifecycle = thread_bound` service
/// (`<root>/services/<service_id>/instances/<instance_id>/`). Pure path
/// composition — does not touch the filesystem. See design §4.7.3.
pub fn instance_state_dir(root: &Path, service_id: &str, instance_id: &str) -> PathBuf {
    state_dir(root, service_id)
        .join("instances")
        .join(instance_id)
}

/// Materialize the service directory and its standard subdirs so plugins
/// can write without each creating their own paths. Idempotent.
pub fn ensure_state_dir(root: &Path, service_id: &str) -> Result<PathBuf> {
    let dir = state_dir(root, service_id);
    fs::create_dir_all(&dir).with_context(|| format!("create service dir at {}", dir.display()))?;
    fs::create_dir_all(dir.join("cursors"))?;
    fs::create_dir_all(dir.join("logs"))?;
    Ok(dir)
}

/// Materialize a per-instance state dir under the parent service. Each
/// thread-bound instance owns its own `cursors/` and `logs/` so cursor /
/// dedupe state is scoped to that thread.
pub fn ensure_instance_state_dir(
    root: &Path,
    service_id: &str,
    instance_id: &str,
) -> Result<PathBuf> {
    // Make sure the parent service dir exists too so listing
    // `instances/` makes sense even when no channel-singleton instance
    // ran first.
    let _ = ensure_state_dir(root, service_id)?;
    let dir = instance_state_dir(root, service_id, instance_id);
    fs::create_dir_all(&dir)
        .with_context(|| format!("create instance dir at {}", dir.display()))?;
    fs::create_dir_all(dir.join("cursors"))?;
    fs::create_dir_all(dir.join("logs"))?;
    Ok(dir)
}

/// Append-only dedupe set persisted at `<service_dir>/dedupe.jsonl`. One
/// JSON object per line: `{"key": "..."}`. Loaded into a HashSet on
/// open; `record(key)` is the §8.4 primitive — true means "this key is
/// new, you must process the source", false means "we've seen this; skip".
///
/// Per §8.4 the dedup write must commit *before* the corresponding event
/// append, so a crash window cannot double-emit. That ordering is the
/// caller's responsibility — `record` only guarantees the write hits disk
/// before returning.
pub struct DedupeStore {
    path: PathBuf,
    seen: Mutex<HashSet<String>>,
}

impl DedupeStore {
    pub fn open(service_dir: &Path) -> Result<Self> {
        let path = service_dir.join("dedupe.jsonl");
        let mut seen = HashSet::new();
        if path.exists() {
            let f = fs::File::open(&path)
                .with_context(|| format!("open dedupe at {}", path.display()))?;
            for (lineno, line) in BufReader::new(f).lines().enumerate() {
                let line = line?;
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let v: serde_json::Value = serde_json::from_str(trimmed).with_context(|| {
                    format!("parse dedupe line {} in {}", lineno + 1, path.display())
                })?;
                if let Some(k) = v.get("key").and_then(|v| v.as_str()) {
                    seen.insert(k.to_string());
                }
            }
        }
        Ok(Self {
            path,
            seen: Mutex::new(seen),
        })
    }

    /// Returns `Ok(true)` on first record, `Ok(false)` if the key was
    /// already present. Persists the new key to disk before returning so
    /// a process crash between this call and the subsequent event append
    /// will still see the key on next boot (and skip the source).
    pub fn record(&self, key: &str) -> Result<bool> {
        let mut seen = self.seen.lock();
        if !seen.insert(key.to_string()) {
            return Ok(false);
        }
        let line = serde_json::json!({ "key": key }).to_string();
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .with_context(|| format!("append dedupe at {}", self.path.display()))?;
        writeln!(f, "{line}")
            .with_context(|| format!("write dedupe line at {}", self.path.display()))?;
        f.sync_data().ok();
        Ok(true)
    }

    #[cfg(test)]
    pub(crate) fn contains(&self, key: &str) -> bool {
        self.seen.lock().contains(key)
    }
}

/// Load a named cursor. Returns `None` when the file is absent (first
/// run) or the file contains no `value` field. Read errors propagate.
pub fn cursor_load(service_dir: &Path, name: &str) -> Result<Option<String>> {
    let path = cursor_path(service_dir, name);
    if !path.exists() {
        return Ok(None);
    }
    let raw =
        fs::read_to_string(&path).with_context(|| format!("read cursor {}", path.display()))?;
    let v: serde_json::Value =
        serde_json::from_str(&raw).with_context(|| format!("parse cursor {}", path.display()))?;
    Ok(v.get("value")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string()))
}

/// Atomic cursor save: writes to `<name>.json.tmp` then renames over the
/// final path so a crash mid-write leaves either the previous value or
/// the new one — never a half-written file.
pub fn cursor_save(service_dir: &Path, name: &str, value: &str) -> Result<()> {
    let path = cursor_path(service_dir, name);
    let parent = path.parent().expect("cursor path always has a parent");
    fs::create_dir_all(parent)?;
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::json!({ "value": value }).to_string();
    fs::write(&tmp, body).with_context(|| format!("write tmp cursor {}", tmp.display()))?;
    fs::rename(&tmp, &path)
        .with_context(|| format!("rename cursor {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}

fn cursor_path(service_dir: &Path, name: &str) -> PathBuf {
    service_dir.join("cursors").join(format!("{name}.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root() -> PathBuf {
        std::env::temp_dir().join(format!("loom-svc-state-{}", uuid::Uuid::new_v4().simple()))
    }

    #[test]
    fn ensure_state_dir_creates_subtree() {
        let root = temp_root();
        let dir = ensure_state_dir(&root, "svc_x").expect("create");
        assert!(dir.exists());
        assert!(dir.join("cursors").exists());
        assert!(dir.join("logs").exists());
        // Idempotent — second call must not fail.
        let dir2 = ensure_state_dir(&root, "svc_x").expect("again");
        assert_eq!(dir, dir2);
    }

    #[test]
    fn dedupe_persists_across_reopen() {
        // The whole point of dedupe is restart safety: a key recorded
        // before crash must still be remembered after the new process
        // reopens the same path.
        let root = temp_root();
        let dir = ensure_state_dir(&root, "svc_x").expect("ensure");

        let store = DedupeStore::open(&dir).expect("open");
        assert!(store.record("k1").expect("first"));
        assert!(!store.record("k1").expect("dup"));
        drop(store);

        let store2 = DedupeStore::open(&dir).expect("reopen");
        assert!(store2.contains("k1"), "key survives reopen");
        assert!(!store2.record("k1").expect("dup after reopen"));
        assert!(store2.record("k2").expect("new key"));
    }

    #[test]
    fn cursor_round_trips_value() {
        let root = temp_root();
        let dir = ensure_state_dir(&root, "svc_x").expect("ensure");
        assert!(cursor_load(&dir, "ci").expect("load missing").is_none());

        cursor_save(&dir, "ci", "run_42").expect("save");
        assert_eq!(
            cursor_load(&dir, "ci").expect("load"),
            Some("run_42".into())
        );

        // Overwrite must replace, not append.
        cursor_save(&dir, "ci", "run_43").expect("overwrite");
        assert_eq!(
            cursor_load(&dir, "ci").expect("load2"),
            Some("run_43".into())
        );
    }

    #[test]
    fn cursor_atomic_write_leaves_no_tmp() {
        let root = temp_root();
        let dir = ensure_state_dir(&root, "svc_x").expect("ensure");
        cursor_save(&dir, "ci", "v1").expect("save");
        let tmp = dir.join("cursors").join("ci.json.tmp");
        assert!(!tmp.exists(), "tmp file should be renamed away");
    }

    #[test]
    fn ensure_instance_state_dir_isolates_per_thread() {
        // Two thread-bound instances of the same spec must get disjoint
        // cursor / dedupe directories so their state cannot collide.
        let root = temp_root();
        let a = ensure_instance_state_dir(&root, "mr-detector", "thread_aaa").expect("a");
        let b = ensure_instance_state_dir(&root, "mr-detector", "thread_bbb").expect("b");
        assert_ne!(a, b);
        assert!(a.exists() && b.exists());
        assert!(a.join("cursors").exists() && b.join("cursors").exists());
        assert!(a.join("logs").exists() && b.join("logs").exists());
        let parent = state_dir(&root, "mr-detector");
        assert!(parent.exists(), "parent service dir is materialized too");
        assert_eq!(a.parent().unwrap(), parent.join("instances"));
    }

    #[test]
    fn instance_dedupe_state_is_per_instance() {
        // Recording the same key under two different instance dirs must
        // not cross-pollinate — each instance has its own DedupeStore.
        let root = temp_root();
        let dir_a = ensure_instance_state_dir(&root, "svc", "inst_a").expect("a");
        let dir_b = ensure_instance_state_dir(&root, "svc", "inst_b").expect("b");
        let store_a = DedupeStore::open(&dir_a).expect("a open");
        let store_b = DedupeStore::open(&dir_b).expect("b open");
        assert!(store_a.record("k1").expect("a first"));
        assert!(store_b.record("k1").expect("b first"));
    }
}
