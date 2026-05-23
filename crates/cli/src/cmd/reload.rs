//! Spec / bundle hot-reload markers. See design §7.1.
//!
//! `loom agent reload <actor_id>` and `loom service reload <service_id>`
//! drop a small `reload-epoch.json` file under the host data dir for
//! that actor / service. The owning host process polls this file and
//! when the epoch advances it tears down the running worker, re-reads
//! the spec from disk, and respawns. Provider sessions whose §6.3
//! signature changes are dropped on the next first-turn dispatch.
//!
//! Pure filesystem; no server contact and no IPC. The host doing the
//! polling is the `loom-daemon` process. Reading / writing the marker is the
//! shared contract.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde_json::{json, Value};

/// `<agent_data_root>/agents/<actor_id>/reload-epoch.json`.
pub fn agent_marker_path(data_root: &Path, actor_id: &str) -> PathBuf {
    data_root
        .join("agents")
        .join(actor_id)
        .join("reload-epoch.json")
}

/// `<service_data_root>/services/<service_id>/reload-epoch.json`.
pub fn service_marker_path(data_root: &Path, service_id: &str) -> PathBuf {
    data_root
        .join("services")
        .join(service_id)
        .join("reload-epoch.json")
}

/// Read the current epoch (millis since unix epoch). Returns 0 if the
/// marker is missing or unparseable — both are equivalent to "never
/// reloaded", so a cold-start host can use 0 as its baseline.
pub fn read_epoch(path: &Path) -> u64 {
    let body = match std::fs::read_to_string(path) {
        Ok(b) => b,
        Err(_) => return 0,
    };
    match serde_json::from_str::<Value>(&body) {
        Ok(v) => v.get("epoch_ms").and_then(|n| n.as_u64()).unwrap_or(0),
        Err(_) => 0,
    }
}

/// Bump the marker to `now_ms`. Idempotent across concurrent writers in
/// the sense that the *latest* writer wins; readers compare strictly
/// greater than their last-seen epoch so equal writes don't trigger a
/// reload spuriously.
pub fn bump(path: &Path) -> Result<u64> {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("mkdir -p {}", parent.display()))?;
    }
    let payload = json!({
        "epoch_ms": now_ms,
        "iso": chrono::Utc::now().to_rfc3339(),
    });
    let body = serde_json::to_string_pretty(&payload)?;
    std::fs::write(path, body).with_context(|| format!("write {}", path.display()))?;
    Ok(now_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "loom-reload-tests-{}",
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn read_missing_returns_zero() {
        let dir = temp();
        let path = dir.join("does-not-exist.json");
        assert_eq!(read_epoch(&path), 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn bump_then_read_round_trips() {
        let dir = temp();
        let path = dir.join("reload-epoch.json");
        let written = bump(&path).expect("bump");
        let read = read_epoch(&path);
        assert!(read >= written - 1 && read <= written + 1);
        // Subsequent bump must be >= previous.
        std::thread::sleep(std::time::Duration::from_millis(2));
        let again = bump(&path).expect("bump again");
        assert!(again >= written);
        assert!(read_epoch(&path) >= read);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_unparseable_returns_zero() {
        let dir = temp();
        let path = dir.join("reload-epoch.json");
        std::fs::write(&path, b"not json").unwrap();
        assert_eq!(read_epoch(&path), 0);
        std::fs::remove_dir_all(&dir).ok();
    }
}
