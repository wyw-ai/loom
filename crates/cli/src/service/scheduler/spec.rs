//! Plugin-specific deserialization of `ServiceSpec.config` for
//! `kind = "scheduler"`. The host treats the JSON as opaque and hands it
//! to the plugin, which then projects it into typed structs that drive
//! the cron loop and per-job execution.
//!
//! Schema follows `docs/service-plugin-system-design.md` §6.1 (scheduler
//! example) and §8.3 (job field table). Fields not in the doc table are
//! ergonomic extensions called out inline.

use std::collections::{BTreeMap, HashSet};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::cron::Schedule;

/// Top-level shape of `spec.config` for a scheduler service. One spec
/// owns N jobs; each job has its own cron line and source.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SchedulerConfig {
    /// Per-job definitions. Empty list = nothing to schedule (the
    /// plugin still runs but emits no events; useful as a placeholder
    /// during staged rollout).
    pub jobs: Vec<JobSpec>,
}

/// One scheduled job. Field semantics in §8.3.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobSpec {
    /// Unique within the spec; doubles as `cursors/<id>.json` filename
    /// stem, dedupe-key segment, and `_meta.jobId` on emitted events.
    pub id: String,
    /// 5-field cron, UTC (§13 Q3). Parsed by [`super::cron::Schedule`].
    pub schedule: String,
    /// Where the body comes from each tick.
    pub source: Source,
    /// Where to write the resulting Loom event.
    pub scope: ScopeBinding,
    /// Optional directed message target. When set, emitted output is sent
    /// with `audience=actor:<targetAgent>` and `deliveryPolicy=wake_agent`.
    /// When unset, the output is pure logging (still appended, no agent invocation).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_agent: Option<String>,
    /// Dedupe strategy. Default `payload_hash` per §8.4.
    #[serde(default)]
    pub dedupe_by: DedupeBy,
    /// Cursor strategy. Default `none` per §8.3.
    #[serde(default)]
    pub cursor_by: CursorBy,
    /// Reject overlapping fires of the same job (§8.5). Default true —
    /// watcher jobs should never have two ticks in flight.
    #[serde(default = "default_true")]
    pub single_in_flight: bool,
    /// Wait for a message reply after directed send (§8.5 awaited mode). Off by
    /// default — fire-and-forget is the common case.
    #[serde(default)]
    pub await_reply: bool,
    /// Bound on `await_reply`. Doc recommends ≤ 1/2 the tick interval.
    /// Default 60s; only consulted when `await_reply = true`.
    #[serde(default = "default_await_timeout_secs")]
    pub await_timeout_secs: u64,
    /// Optional output mode. When unset (`None`), the scheduler emits a
    /// single `content.add` event per tick whose body is the entire
    /// stdout (legacy behavior). When set to
    /// `EmitMode::ArtifactPerJsonLine`, each non-empty stdout line is
    /// parsed as a JSON object, published as its own artifact, and
    /// announced via a `status.update` event with an `attaches_artifact`
    /// relation. See `docs/artifact-contracts.md` §6.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emit: Option<EmitConfig>,
}

/// How the scheduler turns one fire's stdout into Loom events.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmitConfig {
    /// Mode discriminator. Currently only `artifact_per_json_line` adds
    /// behavior beyond the default content-event path.
    pub mode: EmitMode,
    /// Template for the artifact name. Placeholders inside `{}` are
    /// substituted from each parsed JSON line's top-level keys
    /// (e.g. `{event_kind}` reads `payload.event_kind`). Plain text
    /// outside `{}` is used verbatim.
    #[serde(default)]
    pub artifact_name_template: Option<String>,
    /// Event kind to emit for each artifact. Defaults to `status.update`.
    #[serde(default = "default_status_event_type")]
    pub status_event_type: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EmitMode {
    ArtifactPerJsonLine,
}

fn default_status_event_type() -> String {
    "status.update".to_string()
}

/// `command` or `http`. Tagged on `kind` to keep specs human-readable.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    Command {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        /// Extra env vars merged into the child's environment. Inherits
        /// the parent's env; entries in this map override.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        env: BTreeMap<String, String>,
        /// Default 30000.
        #[serde(
            default,
            rename = "timeoutMs",
            alias = "timeout_ms",
            skip_serializing_if = "Option::is_none"
        )]
        timeout_ms: Option<u64>,
    },
    Http {
        url: String,
        /// `GET` (default) or `POST`.
        #[serde(default = "default_http_method")]
        method: String,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        headers: BTreeMap<String, String>,
        /// Raw request body (used by POST). Usually the operator wraps
        /// JSON in a string here.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
        #[serde(
            default,
            rename = "timeoutMs",
            alias = "timeout_ms",
            skip_serializing_if = "Option::is_none"
        )]
        timeout_ms: Option<u64>,
    },
}

impl Source {
    /// `timeout_ms` with the §8.3 default (30s) folded in.
    pub fn effective_timeout(&self) -> Duration {
        let ms = match self {
            Source::Command { timeout_ms, .. } => *timeout_ms,
            Source::Http { timeout_ms, .. } => *timeout_ms,
        };
        Duration::from_millis(ms.unwrap_or(30_000))
    }
}

/// Static per-job scope (§8.5 — no auto-thread for scheduler). The
/// service actor must already be a member of the channel reachable
/// from this scope; `SchedulerPlugin::run` calls
/// `runtime.ensure_channel_member` before the loop starts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeBinding {
    pub kind: ScopeKind,
    pub id: String,
    /// Required when `kind = thread` so we can `ensure_channel_member`
    /// on the parent channel before posting. Optional for `kind =
    /// channel` (the channel id *is* the scope id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScopeKind {
    Thread,
    Channel,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DedupeBy {
    /// `service:<sid>:job:<jid>:fire:<fire_time>:hash:<body_hash>`
    #[default]
    PayloadHash,
    /// `service:<sid>:job:<jid>:source_payload:<source_id>` — only
    /// meaningful for sources that expose a stable id (HTTP responses
    /// the operator parses out, etc). Plugin currently treats this as
    /// `payload_hash` until S4 adds source-message extraction; left in
    /// the schema so specs don't need to change later.
    SourceMessageId,
    /// Skip dedupe entirely. The cron expression itself becomes the
    /// rate limiter; suitable for "fire every N min" notifiers.
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CursorBy {
    /// No cursor diff — every tick fires (subject to dedupe).
    #[default]
    None,
    /// Persist sha256(body); skip the next tick if the new body hashes
    /// to the same value. The §8.3 "state polling" pattern.
    BodyHash,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SchedulerSpecError {
    #[error("scheduler config has no jobs (set jobs=[] explicitly to silence)")]
    EmptyJobs,
    #[error("job `{0}` has empty id")]
    EmptyJobId(String),
    #[error("job `{0}`: duplicate id within the same spec")]
    DuplicateJobId(String),
    #[error("job `{0}`: invalid cron `{1}`: {2}")]
    InvalidSchedule(String, String, String),
    #[error("job `{0}`: scope.id must be non-empty")]
    EmptyScopeId(String),
    #[error("job `{0}`: scope.kind=thread requires scope.channelId")]
    ThreadMissingChannel(String),
    #[error("job `{0}`: source URL must be non-empty")]
    EmptyHttpUrl(String),
    #[error("job `{0}`: source command must be non-empty")]
    EmptyCommand(String),
    #[error("job `{0}`: HTTP method `{1}` not supported (use GET or POST)")]
    UnsupportedHttpMethod(String, String),
    #[error("job `{0}`: awaitReply requires targetAgent")]
    AwaitWithoutTarget(String),
}

impl SchedulerConfig {
    /// Validate the whole config in one pass. Caller (the plugin) runs
    /// this once at startup and propagates the error so the host can
    /// log + skip the spec.
    ///
    /// Empty `jobs` is intentionally allowed — a freshly bootstrapped
    /// scheduler spec with `{ "jobs": [] }` should boot without error so
    /// operators can iterate. The early-error case (`EmptyJobs`) is
    /// reserved for *missing* jobs vs explicit empty.
    pub fn validate(&self) -> Result<(), SchedulerSpecError> {
        let mut seen = HashSet::new();
        for job in &self.jobs {
            job.validate()?;
            if !seen.insert(job.id.as_str()) {
                return Err(SchedulerSpecError::DuplicateJobId(job.id.clone()));
            }
        }
        Ok(())
    }
}

impl JobSpec {
    pub fn validate(&self) -> Result<(), SchedulerSpecError> {
        if self.id.trim().is_empty() {
            return Err(SchedulerSpecError::EmptyJobId(self.id.clone()));
        }
        // Parse cron eagerly; cheap and surfaces typos at boot, not at
        // first tick.
        if let Err(e) = Schedule::parse(&self.schedule) {
            return Err(SchedulerSpecError::InvalidSchedule(
                self.id.clone(),
                self.schedule.clone(),
                e.to_string(),
            ));
        }
        if self.scope.id.trim().is_empty() {
            return Err(SchedulerSpecError::EmptyScopeId(self.id.clone()));
        }
        if matches!(self.scope.kind, ScopeKind::Thread) && self.scope.channel_id.is_none() {
            return Err(SchedulerSpecError::ThreadMissingChannel(self.id.clone()));
        }
        match &self.source {
            Source::Command { command, .. } if command.trim().is_empty() => {
                return Err(SchedulerSpecError::EmptyCommand(self.id.clone()));
            }
            Source::Http { url, method, .. } => {
                if url.trim().is_empty() {
                    return Err(SchedulerSpecError::EmptyHttpUrl(self.id.clone()));
                }
                let m = method.to_ascii_uppercase();
                if m != "GET" && m != "POST" {
                    return Err(SchedulerSpecError::UnsupportedHttpMethod(
                        self.id.clone(),
                        method.clone(),
                    ));
                }
            }
            _ => {}
        }
        if self.await_reply && self.target_agent.is_none() {
            return Err(SchedulerSpecError::AwaitWithoutTarget(self.id.clone()));
        }
        Ok(())
    }
}

fn default_true() -> bool {
    true
}
fn default_await_timeout_secs() -> u64 {
    60
}
fn default_http_method() -> String {
    "GET".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ok_job() -> JobSpec {
        JobSpec {
            id: "ci".into(),
            schedule: "*/5 * * * *".into(),
            source: Source::Command {
                command: "echo".into(),
                args: vec!["hi".into()],
                env: BTreeMap::new(),
                timeout_ms: None,
            },
            scope: ScopeBinding {
                kind: ScopeKind::Thread,
                id: "thread_x".into(),
                channel_id: Some("chan_x".into()),
            },
            target_agent: Some("actor_qa".into()),
            dedupe_by: DedupeBy::PayloadHash,
            cursor_by: CursorBy::None,
            single_in_flight: true,
            await_reply: false,
            await_timeout_secs: 60,
            emit: None,
        }
    }

    #[test]
    fn deserialize_doc_example() {
        // The §6.1 scheduler spec config block.
        let v = json!({
            "jobs": [
                {
                    "id": "ci_watch",
                    "schedule": "*/10 * * * *",
                    "source": {
                        "kind": "command",
                        "command": "a1",
                        "args": ["ci", "run", "list"]
                    },
                    "scope": {
                        "kind": "thread",
                        "id": "thread_ci",
                        "channelId": "chan_ops"
                    }
                }
            ]
        });
        let cfg: SchedulerConfig = serde_json::from_value(v).expect("parse");
        cfg.validate().expect("doc example valid");
        assert_eq!(cfg.jobs.len(), 1);
        let job = &cfg.jobs[0];
        assert_eq!(job.id, "ci_watch");
        assert!(matches!(job.dedupe_by, DedupeBy::PayloadHash));
        assert!(matches!(job.cursor_by, CursorBy::None));
        assert!(job.single_in_flight, "default true");
    }

    #[test]
    fn http_source_round_trip() {
        let v = json!({
            "jobs": [
                {
                    "id": "ping",
                    "schedule": "* * * * *",
                    "source": {
                        "kind": "http",
                        "url": "https://example.com/api/ci",
                        "method": "GET",
                        "headers": { "Authorization": "Bearer x" }
                    },
                    "scope": { "kind": "channel", "id": "chan_ops" },
                    "cursorBy": "body_hash"
                }
            ]
        });
        let cfg: SchedulerConfig = serde_json::from_value(v).expect("parse");
        cfg.validate().expect("valid");
        let job = &cfg.jobs[0];
        match &job.source {
            Source::Http {
                url,
                method,
                headers,
                ..
            } => {
                assert_eq!(url, "https://example.com/api/ci");
                assert_eq!(method, "GET");
                assert_eq!(
                    headers.get("Authorization").map(String::as_str),
                    Some("Bearer x")
                );
            }
            _ => panic!("wrong source kind"),
        }
        assert!(matches!(job.cursor_by, CursorBy::BodyHash));
    }

    #[test]
    fn validate_rejects_duplicate_job_ids() {
        let cfg = SchedulerConfig {
            jobs: vec![ok_job(), ok_job()],
        };
        assert_eq!(
            cfg.validate(),
            Err(SchedulerSpecError::DuplicateJobId("ci".into()))
        );
    }

    #[test]
    fn validate_rejects_bad_cron() {
        let mut job = ok_job();
        job.schedule = "not a cron".into();
        let cfg = SchedulerConfig { jobs: vec![job] };
        match cfg.validate() {
            Err(SchedulerSpecError::InvalidSchedule(id, expr, _)) => {
                assert_eq!(id, "ci");
                assert_eq!(expr, "not a cron");
            }
            other => panic!("expected InvalidSchedule, got {other:?}"),
        }
    }

    #[test]
    fn validate_rejects_thread_scope_without_channel() {
        let mut job = ok_job();
        job.scope.channel_id = None;
        let cfg = SchedulerConfig { jobs: vec![job] };
        assert_eq!(
            cfg.validate(),
            Err(SchedulerSpecError::ThreadMissingChannel("ci".into()))
        );
    }

    #[test]
    fn validate_rejects_await_without_target() {
        let mut job = ok_job();
        job.target_agent = None;
        job.await_reply = true;
        let cfg = SchedulerConfig { jobs: vec![job] };
        assert_eq!(
            cfg.validate(),
            Err(SchedulerSpecError::AwaitWithoutTarget("ci".into()))
        );
    }

    #[test]
    fn validate_rejects_unsupported_http_method() {
        let mut job = ok_job();
        job.source = Source::Http {
            url: "https://x".into(),
            method: "PUT".into(),
            headers: BTreeMap::new(),
            body: None,
            timeout_ms: None,
        };
        let cfg = SchedulerConfig { jobs: vec![job] };
        assert!(matches!(
            cfg.validate(),
            Err(SchedulerSpecError::UnsupportedHttpMethod(_, _))
        ));
    }

    #[test]
    fn empty_jobs_validates_ok() {
        // Bootstrap: operator creates a spec with no jobs to confirm
        // wiring; should not error.
        let cfg = SchedulerConfig { jobs: Vec::new() };
        cfg.validate().expect("empty jobs is fine");
    }

    #[test]
    fn effective_timeout_applies_default() {
        let s = Source::Command {
            command: "true".into(),
            args: vec![],
            env: BTreeMap::new(),
            timeout_ms: None,
        };
        assert_eq!(s.effective_timeout(), Duration::from_millis(30_000));
        let s2 = Source::Http {
            url: "x".into(),
            method: "GET".into(),
            headers: BTreeMap::new(),
            body: None,
            timeout_ms: Some(5_000),
        };
        assert_eq!(s2.effective_timeout(), Duration::from_millis(5_000));
    }

    #[test]
    fn source_timeout_accepts_documented_camel_case_and_legacy_snake_case() {
        let command: Source = serde_json::from_value(json!({
            "kind": "command",
            "command": "true",
            "timeoutMs": 60_000
        }))
        .expect("parse command timeoutMs");
        assert_eq!(command.effective_timeout(), Duration::from_secs(60));

        let http: Source = serde_json::from_value(json!({
            "kind": "http",
            "url": "https://example.com",
            "timeout_ms": 45_000
        }))
        .expect("parse legacy http timeout_ms");
        assert_eq!(http.effective_timeout(), Duration::from_secs(45));

        let serialized = serde_json::to_value(command).expect("serialize command source");
        assert_eq!(serialized["timeoutMs"], json!(60_000));
        assert!(serialized.get("timeout_ms").is_none());
    }
}
