//! Instance request-file IO for `lifecycle = thread_bound` services.
//!
//! Per design §4.7.3 a thread-bound service is started by a caller
//! (typically a router/delivery agent) writing a small `request.json`
//! into the per-instance state dir:
//!
//! ```text
//! <data_root>/services/<service_id>/instances/<instance_id>/request.json
//! ```
//!
//! `joi service start --spec <id> --in <thread> --params {...}` is the
//! supported writer; `joi service stop --spec <id> --in <thread>`
//! removes the file. The running `joi service serve` host watches each
//! spec's `instances/` directory and on-create dispatches a per-instance
//! plugin task; on-delete it tears the task down.
//!
//! v1 keeps `instance_id == thread_id` (the only supported `bind.scope`
//! today). The schema permits a richer scope shape so we don't repaint
//! the on-disk format when channel/work-item-bound services land later.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::state::{instance_state_dir, state_dir};

/// On-disk request file. Versioned so a future host can refuse a newer
/// schema rather than misinterpret it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstanceRequest {
    /// Schema version. Always `1` in v1.
    #[serde(default = "default_version")]
    pub version: u32,
    /// `ServiceSpec.id` this instance belongs to. Redundant with the
    /// path but kept so a stray request file stays self-describing.
    pub spec_id: String,
    /// Scope the instance is bound to. v1 only `kind == "thread"`.
    pub scope: InstanceScope,
    /// Plugin parameters. Validated against `ServiceSpec.params_schema`
    /// at start time (host enforces; CLI does not).
    #[serde(default)]
    pub params: Value,
    /// ISO8601 timestamp for human inspection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

fn default_version() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct InstanceScope {
    /// `"thread"` in v1.
    pub kind: String,
    /// Thread id (for `kind = "thread"`).
    pub id: String,
    /// Parent channel of the thread, when known. Optional because some
    /// callers (e.g., `joi service start --in <thread>` without a
    /// `--channel` flag) defer the lookup to the host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,
}

/// `<root>/services/<spec_id>/instances/<instance_id>/request.json`.
pub fn request_path(root: &Path, spec_id: &str, instance_id: &str) -> PathBuf {
    instance_state_dir(root, spec_id, instance_id).join("request.json")
}

/// Write (create or overwrite) `request.json` for an instance. Creates
/// parent directories. Idempotent — calling twice with the same payload
/// is fine; the host watcher debounces by epoch.
pub fn write_request(root: &Path, req: &InstanceRequest) -> Result<PathBuf> {
    let instance_id = &req.scope.id;
    let dir = super::state::ensure_instance_state_dir(root, &req.spec_id, instance_id)?;
    let path = dir.join("request.json");
    let body =
        serde_json::to_string_pretty(req).context("serialize InstanceRequest")?;
    fs::write(&path, body)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}

/// Read the request file; returns `Ok(None)` if absent.
#[allow(dead_code)]
pub fn read_request(root: &Path, spec_id: &str, instance_id: &str) -> Result<Option<InstanceRequest>> {
    let path = request_path(root, spec_id, instance_id);
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path)
        .with_context(|| format!("read {}", path.display()))?;
    let req: InstanceRequest = serde_json::from_str(&text)
        .with_context(|| format!("parse InstanceRequest at {}", path.display()))?;
    Ok(Some(req))
}

/// Remove the request file. Returns `Ok(false)` if it was already
/// absent — `service stop` is safe to call on a stopped instance.
pub fn delete_request(root: &Path, spec_id: &str, instance_id: &str) -> Result<bool> {
    let path = request_path(root, spec_id, instance_id);
    if !path.exists() {
        return Ok(false);
    }
    fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
    Ok(true)
}

/// List the instance ids that currently have a `request.json` for the
/// given spec. Returns an empty vec when the spec has no instances dir
/// (e.g., never started).
pub fn list_instances(root: &Path, spec_id: &str) -> Result<Vec<String>> {
    let dir = state_dir(root, spec_id).join("instances");
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir).with_context(|| format!("read_dir {}", dir.display()))? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let req_path = entry.path().join("request.json");
        if req_path.exists() {
            if let Some(name) = entry.file_name().to_str() {
                out.push(name.to_string());
            }
        }
    }
    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp() -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "joi-instance-tests-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn req(spec: &str, thread: &str) -> InstanceRequest {
        InstanceRequest {
            version: 1,
            spec_id: spec.to_string(),
            scope: InstanceScope {
                kind: "thread".to_string(),
                id: thread.to_string(),
                channel_id: Some("ch_main".to_string()),
            },
            params: serde_json::json!({"mr_url": "https://example.test/mr/1"}),
            created_at: Some("2026-05-03T00:00:00Z".to_string()),
        }
    }

    #[test]
    fn write_then_read_round_trips() {
        let root = temp();
        let r = req("mr-detector", "thread_aaa");
        let path = write_request(&root, &r).unwrap();
        assert!(path.ends_with("request.json"));
        let back = read_request(&root, "mr-detector", "thread_aaa")
            .unwrap()
            .unwrap();
        assert_eq!(back, r);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn read_missing_returns_none() {
        let root = temp();
        let got = read_request(&root, "mr-detector", "ghost").unwrap();
        assert!(got.is_none());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn delete_is_idempotent() {
        let root = temp();
        let r = req("mr-detector", "thread_x");
        write_request(&root, &r).unwrap();
        assert!(delete_request(&root, "mr-detector", "thread_x").unwrap());
        assert!(!delete_request(&root, "mr-detector", "thread_x").unwrap());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn list_returns_only_instances_with_request_file() {
        let root = temp();
        write_request(&root, &req("mr-detector", "thread_a")).unwrap();
        write_request(&root, &req("mr-detector", "thread_b")).unwrap();
        // Make a third instance dir without a request file (e.g., a
        // stopped instance that left state behind).
        super::super::state::ensure_instance_state_dir(&root, "mr-detector", "thread_c").unwrap();
        let mut got = list_instances(&root, "mr-detector").unwrap();
        got.sort();
        assert_eq!(got, vec!["thread_a".to_string(), "thread_b".to_string()]);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn list_empty_for_unknown_spec() {
        let root = temp();
        let got = list_instances(&root, "nope").unwrap();
        assert!(got.is_empty());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn write_overwrites_existing_request() {
        let root = temp();
        let mut r = req("mr-detector", "thread_y");
        write_request(&root, &r).unwrap();
        r.params = serde_json::json!({"mr_url": "https://example.test/mr/2"});
        write_request(&root, &r).unwrap();
        let back = read_request(&root, "mr-detector", "thread_y")
            .unwrap()
            .unwrap();
        assert_eq!(back.params, r.params);
        fs::remove_dir_all(&root).ok();
    }
}
