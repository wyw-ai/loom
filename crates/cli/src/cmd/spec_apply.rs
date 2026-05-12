//! `joi spec apply --action <event_id>` — classroom教学循环最后一公里
//! (design §4.4 / §7.1, e2e-readiness-review M5).
//!
//! Consumes an `action.response` event whose `responds_to` request is
//! `requestType = approval.spec_apply` and whose payload says the user
//! accepted. Reads the lesson-plan artifact attached to the request,
//! pulls its JSON frontmatter, and if the frontmatter carries a
//! `spec_apply` block, applies the plan: deep-merges `spec_patch` onto
//! the on-disk AgentSpec / ServiceSpec, writes any `bundle_writes`
//! files under the spec's `bundle/` sibling, then bumps the
//! reload-epoch marker so a running `joi {agent,service} serve` host
//! re-spawns the worker. Records a `runtime_receipt` artifact + a
//! `status.update` event in the original action.request scope so the
//! decision is replayable from timeline alone.
//!
//! Lesson-plan frontmatter contract for the apply path (extends
//! `docs/artifact-contracts.md` §4):
//!
//! ```json
//! {
//!   "schema_version": "1",
//!   "producer": "teacher",
//!   "task_id": "...",
//!   "skills": [...],
//!   "spec_apply": {
//!     "target": { "kind": "agent" | "service", "id": "<id>" },
//!     "spec_patch": { /* JSON deep-merged onto the existing spec.json */ },
//!     "bundle_writes": [
//!       { "path": "rel/path.txt", "contents": "raw text" }
//!     ]
//!   }
//! }
//! ```
//!
//! Without a `spec_apply` block the response is treated as
//! informational and the command exits OK without touching disk —
//! matches the v1 promise that lesson-plans are first and foremost
//! human-readable.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use proto::methods::*;
use proto::types::{Event, RefKind, RelationKind, ScopeRef};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::client::Client;

const REQUEST_TYPE: &str = "approval.spec_apply";

/// Entry point for `joi spec apply --action <event_id>`.
pub async fn run(
    client: Arc<Client>,
    actor_id: String,
    action_event_id: String,
    dry_run: bool,
) -> Result<()> {
    let response = find_event(&client, &action_event_id)
        .await?
        .ok_or_else(|| anyhow!("event {action_event_id} not found in any thread"))?;
    if response.kind != "action.response" {
        bail!(
            "event {action_event_id} is `{}`, expected `action.response`",
            response.kind
        );
    }
    let response_kind = response
        .payload
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("");
    if response_kind != "accepted" {
        bail!(
            "action.response payload.kind = `{response_kind}` (expected `accepted`); refusing to apply"
        );
    }

    let request_id = response
        .relations
        .iter()
        .find(|r| matches!(r.kind, RelationKind::RespondsTo) && r.target.kind == RefKind::Event)
        .map(|r| r.target.id.clone())
        .ok_or_else(|| anyhow!("action.response has no responds_to → event relation"))?;

    let request = find_event(&client, &request_id)
        .await?
        .ok_or_else(|| anyhow!("action.request {request_id} not found"))?;
    let req_type = request
        .payload
        .get("requestType")
        .and_then(Value::as_str)
        .unwrap_or("");
    if req_type != REQUEST_TYPE {
        bail!(
            "action.request {request_id} has requestType = `{req_type}`, expected `{REQUEST_TYPE}`"
        );
    }

    let artifact_id = request
        .relations
        .iter()
        .find(|r| {
            matches!(r.kind, RelationKind::AttachesArtifact) && r.target.kind == RefKind::Artifact
        })
        .map(|r| r.target.id.clone())
        .ok_or_else(|| {
            anyhow!(
                "action.request {request_id} has no attaches_artifact relation; \
                 cannot find lesson-plan to apply"
            )
        })?;

    let body = read_artifact_text(&client, &artifact_id).await?;
    let frontmatter = parse_lesson_plan_frontmatter(&body)
        .with_context(|| format!("parse lesson-plan frontmatter (artifact {artifact_id})"))?;

    let plan = match frontmatter.get("spec_apply") {
        Some(v) if !v.is_null() => SpecApplyPlan::from_value(v)
            .with_context(|| format!("parse spec_apply block in lesson-plan {artifact_id}"))?,
        _ => {
            println!(
                "lesson-plan {artifact_id} has no spec_apply block — accepted but nothing to apply"
            );
            return Ok(());
        }
    };

    let outcome = if dry_run {
        plan.preview()?
    } else {
        plan.apply()?
    };

    if dry_run {
        println!(
            "dry-run preview for {} `{}`:",
            plan.target_kind_str(),
            plan.target_id
        );
        println!("{}", serde_json::to_string_pretty(&outcome)?);
        return Ok(());
    }

    // Receipt artifact + status event so the decision lives in timeline.
    let receipt = json!({
        "schema_version": "1",
        "producer": "joi-spec-apply",
        "action_event_id": action_event_id,
        "request_event_id": request_id,
        "lesson_plan_artifact_id": artifact_id,
        "target": { "kind": plan.target_kind_str(), "id": plan.target_id },
        "outcome": outcome,
    });
    let receipt_text = serde_json::to_string_pretty(&receipt)?;
    let receipt_name = format!("runtime-receipt-{}.json", action_event_id);

    let publish_params = ArtifactPublishParams {
        ingress: ArtifactIngress::InlineText(InlineTextIngress {
            name: receipt_name.clone(),
            media_type: "application/json".into(),
            text: receipt_text,
        }),
        created_by: actor_id.clone(),
        scope: Some(request.scope.clone()),
    };
    let publish: ArtifactPublishResult = client
        .call(method::ARTIFACT_PUBLISH, publish_params)
        .await
        .context("publish runtime_receipt artifact")?;

    let status_payload = json!({
        "kind": "spec_apply.completed",
        "target": { "kind": outcome["target_kind"], "id": outcome["target_id"] },
        "epoch_ms": outcome["epoch_ms"],
    });
    let status_event = json!({
        "type": "status.update",
        "actorId": actor_id,
        "scope": scope_value(&request.scope),
        "payload": status_payload,
        "relations": [
            { "kind": "responds_to", "target": { "kind": "event", "id": action_event_id } },
            { "kind": "attaches_artifact", "target": { "kind": "artifact", "id": publish.artifact.id } },
        ],
    });
    let _: EventAppendResult = client
        .call(method::EVENT_APPEND, json!({"event": status_event}))
        .await
        .context("append spec_apply.completed status.update")?;

    println!(
        "applied spec_apply for {}:{}; epoch_ms={}; receipt artifact={}",
        plan.target_kind_str(),
        plan.target_id,
        outcome["epoch_ms"],
        publish.artifact.id
    );
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TargetKind {
    Agent,
    Service,
}

impl TargetKind {
    fn parse(s: &str) -> Result<Self> {
        match s {
            "agent" => Ok(Self::Agent),
            "service" => Ok(Self::Service),
            other => bail!("spec_apply.target.kind = `{other}`, expected `agent` | `service`"),
        }
    }
    fn as_str(&self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Service => "service",
        }
    }
}

#[derive(Debug, Clone)]
struct BundleWrite {
    path: PathBuf,
    contents: String,
}

#[derive(Debug, Clone)]
struct SpecApplyPlan {
    target_kind: TargetKind,
    target_id: String,
    spec_patch: Option<Value>,
    bundle_writes: Vec<BundleWrite>,
}

impl SpecApplyPlan {
    fn target_kind_str(&self) -> &'static str {
        self.target_kind.as_str()
    }

    fn from_value(v: &Value) -> Result<Self> {
        #[derive(Deserialize)]
        struct Raw {
            target: RawTarget,
            #[serde(default)]
            spec_patch: Option<Value>,
            #[serde(default)]
            bundle_writes: Vec<RawBundleWrite>,
        }
        #[derive(Deserialize)]
        struct RawTarget {
            kind: String,
            id: String,
        }
        #[derive(Deserialize)]
        struct RawBundleWrite {
            path: String,
            contents: String,
        }
        let raw: Raw = serde_json::from_value(v.clone())?;
        let target_kind = TargetKind::parse(&raw.target.kind)?;
        if raw.target.id.trim().is_empty() {
            bail!("spec_apply.target.id is empty");
        }
        let mut writes = Vec::with_capacity(raw.bundle_writes.len());
        for w in raw.bundle_writes {
            let pb = PathBuf::from(&w.path);
            if pb.is_absolute() || w.path.contains("..") {
                bail!(
                    "spec_apply.bundle_writes[].path `{}` must be a relative path under bundle/",
                    w.path
                );
            }
            writes.push(BundleWrite {
                path: pb,
                contents: w.contents,
            });
        }
        Ok(Self {
            target_kind,
            target_id: raw.target.id,
            spec_patch: raw.spec_patch,
            bundle_writes: writes,
        })
    }

    fn preview(&self) -> Result<Value> {
        let (spec_path, bundle_root) = self.locate()?;
        Ok(json!({
            "target_kind": self.target_kind_str(),
            "target_id": self.target_id,
            "spec_path": spec_path.display().to_string(),
            "bundle_root": bundle_root.display().to_string(),
            "spec_patch": self.spec_patch,
            "bundle_writes": self.bundle_writes.iter().map(|w| json!({
                "path": w.path.display().to_string(),
                "bytes": w.contents.len(),
            })).collect::<Vec<_>>(),
            "dry_run": true,
        }))
    }

    fn apply(&self) -> Result<Value> {
        let (spec_path, bundle_root) = self.locate()?;
        let backup_dir = self.make_backup(&spec_path, &bundle_root)?;

        if let Some(patch) = &self.spec_patch {
            let current_text = std::fs::read_to_string(&spec_path)
                .with_context(|| format!("read {}", spec_path.display()))?;
            let mut current: Value = serde_json::from_str(&current_text)
                .with_context(|| format!("parse {}", spec_path.display()))?;
            deep_merge(&mut current, patch.clone());
            let new_text = serde_json::to_string_pretty(&current)?;
            atomic_write(&spec_path, new_text.as_bytes())?;
        }

        for w in &self.bundle_writes {
            let dest = bundle_root.join(&w.path);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("mkdir -p {}", parent.display()))?;
            }
            atomic_write(&dest, w.contents.as_bytes())?;
        }

        // Reload epoch.
        let epoch = self.bump_reload()?;

        Ok(json!({
            "target_kind": self.target_kind_str(),
            "target_id": self.target_id,
            "spec_path": spec_path.display().to_string(),
            "bundle_root": bundle_root.display().to_string(),
            "backup_dir": backup_dir.display().to_string(),
            "epoch_ms": epoch,
            "bundle_files_written": self.bundle_writes.iter()
                .map(|w| w.path.display().to_string())
                .collect::<Vec<_>>(),
            "spec_patch_applied": self.spec_patch.is_some(),
        }))
    }

    /// Returns `(spec_path, bundle_root)`. Spec is searched in nested
    /// then flat layout, matching the behavior of the loaders in
    /// `cmd::service::load_specs` and `cmd::agent_serve::load_specs`.
    fn locate(&self) -> Result<(PathBuf, PathBuf)> {
        let dir = match self.target_kind {
            TargetKind::Agent => super::agent::default_specs_dir(),
            TargetKind::Service => super::service::default_specs_dir(),
        };
        let nested = dir.join(&self.target_id).join("spec.json");
        let flat = dir.join(format!("{}.json", &self.target_id));
        let (spec_path, bundle_root) = if nested.exists() {
            (nested.clone(), nested.parent().unwrap().join("bundle"))
        } else if flat.exists() {
            // Flat layout puts bundle/<id>/ next to the .json file.
            (flat, dir.join("bundle").join(&self.target_id))
        } else {
            bail!(
                "no {} spec found for `{}` under {} (looked for `<id>/spec.json` and `<id>.json`)",
                self.target_kind_str(),
                self.target_id,
                dir.display()
            );
        };
        Ok((spec_path, bundle_root))
    }

    fn make_backup(&self, spec_path: &Path, bundle_root: &Path) -> Result<PathBuf> {
        let ts = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        let backup_dir = spec_path
            .parent()
            .ok_or_else(|| anyhow!("spec path has no parent: {}", spec_path.display()))?
            .join(".backups")
            .join(&ts);
        std::fs::create_dir_all(&backup_dir)
            .with_context(|| format!("mkdir -p {}", backup_dir.display()))?;
        if spec_path.exists() {
            std::fs::copy(spec_path, backup_dir.join("spec.json"))
                .with_context(|| format!("backup {}", spec_path.display()))?;
        }
        // Backup any bundle file we're about to overwrite.
        for w in &self.bundle_writes {
            let src = bundle_root.join(&w.path);
            if src.exists() {
                let dest = backup_dir.join("bundle").join(&w.path);
                if let Some(p) = dest.parent() {
                    std::fs::create_dir_all(p).ok();
                }
                std::fs::copy(&src, &dest).with_context(|| format!("backup {}", src.display()))?;
            }
        }
        Ok(backup_dir)
    }

    fn bump_reload(&self) -> Result<u64> {
        let path = match self.target_kind {
            TargetKind::Agent => {
                let data_root = super::agent_serve::default_data_root_pub();
                super::reload::agent_marker_path(&data_root, &self.target_id)
            }
            TargetKind::Service => {
                let data_root = crate::service::state::default_data_root();
                super::reload::service_marker_path(&data_root, &self.target_id)
            }
        };
        super::reload::bump(&path)
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("mkdir -p {}", parent.display()))?;
    }
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|s| s.to_str()).unwrap_or("write")
    ));
    std::fs::write(&tmp, bytes).with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("rename {} → {}", tmp.display(), path.display()))?;
    Ok(())
}

/// JSON deep-merge: object values recurse, anything else replaces.
/// Arrays are replaced wholesale because lesson-plans authoring "patch
/// element 3 of jobs[]" is more error-prone than rewriting the whole
/// jobs array.
pub(crate) fn deep_merge(target: &mut Value, patch: Value) {
    match (target, patch) {
        (Value::Object(t), Value::Object(p)) => {
            for (k, v) in p {
                match t.get_mut(&k) {
                    Some(slot) => deep_merge(slot, v),
                    None => {
                        t.insert(k, v);
                    }
                }
            }
        }
        (slot, p) => {
            *slot = p;
        }
    }
}

/// Walk every thread the server knows about and return the first event
/// with `id == event_id`. Mirrors `cmd::action::fetch_event_scope` but
/// returns the full event so we can read payload + relations.
async fn find_event(client: &Client, event_id: &str) -> Result<Option<Event>> {
    let lst: ThreadListResult = client.call(method::THREAD_LIST, json!({})).await?;
    for t in lst.threads {
        let res: ScopeReadResult = client
            .call(
                method::SCOPE_READ,
                json!({ "scope": { "kind": "thread", "id": t.id }, "limit": 500 }),
            )
            .await?;
        if let Some(ev) = res.events.into_iter().find(|e| e.id == event_id) {
            return Ok(Some(ev));
        }
    }
    Ok(None)
}

async fn read_artifact_text(client: &Client, artifact_id: &str) -> Result<String> {
    let params = ArtifactReadParams {
        artifact_id: artifact_id.into(),
        offset: 0,
        max_bytes: 1024 * 1024,
    };
    let res: ArtifactReadResult = client
        .call(method::ARTIFACT_READ, params)
        .await
        .with_context(|| format!("artifact/read {artifact_id}"))?;
    if res.truncated {
        bail!(
            "artifact {artifact_id} body exceeded read limit; refuse to apply partial lesson-plan"
        );
    }
    Ok(res.content)
}

fn scope_value(scope: &ScopeRef) -> Value {
    serde_json::to_value(scope).unwrap_or(Value::Null)
}

/// Extract the JSON object from the first ```` ```json ```` fenced
/// block at the top of `body`. Tolerates leading whitespace / blank
/// lines; rejects bodies whose first non-blank content isn't a json
/// fence (matches `docs/artifact-contracts.md` §4 "exactly one such
/// block as the first non-blank content").
pub(crate) fn parse_lesson_plan_frontmatter(body: &str) -> Result<Map<String, Value>> {
    let trimmed = body.trim_start();
    let after_open = trimmed
        .strip_prefix("```json")
        .ok_or_else(|| anyhow!("lesson-plan body does not start with a ```json fenced block"))?;
    let after_open = after_open
        .strip_prefix('\n')
        .or_else(|| after_open.strip_prefix("\r\n"))
        .ok_or_else(|| anyhow!("lesson-plan ```json fence not followed by a newline"))?;
    let close = after_open
        .find("\n```")
        .ok_or_else(|| anyhow!("lesson-plan ```json block missing closing fence"))?;
    let json_text = &after_open[..close];
    let v: Value = serde_json::from_str(json_text).context("parse lesson-plan frontmatter JSON")?;
    match v {
        Value::Object(m) => Ok(m),
        _ => bail!("lesson-plan frontmatter is not a JSON object"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deep_merge_recurses_objects_replaces_scalars() {
        let mut t = json!({"a": 1, "b": {"c": 2, "d": 3}});
        deep_merge(&mut t, json!({"b": {"c": 20, "e": 4}, "f": 5}));
        assert_eq!(t, json!({"a": 1, "b": {"c": 20, "d": 3, "e": 4}, "f": 5}));
    }

    #[test]
    fn deep_merge_replaces_arrays_wholesale() {
        let mut t = json!({"jobs": [1, 2, 3]});
        deep_merge(&mut t, json!({"jobs": [9]}));
        assert_eq!(t, json!({"jobs": [9]}));
    }

    #[test]
    fn parse_frontmatter_extracts_json_block() {
        let body = "```json\n{\"schema_version\": \"1\", \"producer\": \"teacher\"}\n```\n\n# Lesson 1\n\nbody";
        let m = parse_lesson_plan_frontmatter(body).expect("parse");
        assert_eq!(m["schema_version"], json!("1"));
        assert_eq!(m["producer"], json!("teacher"));
    }

    #[test]
    fn parse_frontmatter_rejects_missing_fence() {
        let body = "# Lesson 1\n\nbody without json fence";
        assert!(parse_lesson_plan_frontmatter(body).is_err());
    }

    #[test]
    fn spec_apply_plan_rejects_traversal() {
        let v = json!({
            "target": {"kind": "agent", "id": "delivery"},
            "bundle_writes": [{"path": "../etc/passwd", "contents": "x"}]
        });
        assert!(SpecApplyPlan::from_value(&v).is_err());
    }

    #[test]
    fn spec_apply_plan_rejects_unknown_target_kind() {
        let v = json!({"target": {"kind": "channel", "id": "x"}});
        assert!(SpecApplyPlan::from_value(&v).is_err());
    }

    #[test]
    fn spec_apply_plan_parses_minimal() {
        let v = json!({
            "target": {"kind": "service", "id": "mr-detector"},
            "spec_patch": {"config": {"jobs": []}}
        });
        let p = SpecApplyPlan::from_value(&v).expect("parse");
        assert_eq!(p.target_kind_str(), "service");
        assert_eq!(p.target_id, "mr-detector");
        assert!(p.spec_patch.is_some());
        assert!(p.bundle_writes.is_empty());
    }
}
