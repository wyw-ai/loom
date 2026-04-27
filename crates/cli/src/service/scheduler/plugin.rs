// Until the host's plugin registry actually wires SchedulerPlugin
// (final step in S3), the per-spawn helpers look unused. Silence the
// noise to keep the diff focused.
#![allow(dead_code)]

//! `SchedulerPlugin` — the §6.2 long-process plugin that drives
//! [`super::spec::JobSpec`]s. One tokio task per job; each task sleeps
//! until the next 5-field UTC cron tick (§13 Q3) and fires the source
//! once.
//!
//! Per-tick steps:
//!
//! 1. `exec_source` to fetch the body (§8.3 command/http).
//! 2. Apply [`CursorBy`] diff: `body_hash` skips when `sha256(body)`
//!    matches the persisted cursor (§8.3 cursor semantics).
//! 3. `dedupe_once` with the §8.4 key (commits **before** the event
//!    append so a crash window cannot double-emit).
//! 4. `runtime.append_content` (or `handoff` if `target_agent` is set)
//!    with `_meta = { service: "scheduler", jobId, fireTimeUtc,
//!    sourceCursor? }`.
//! 5. Persist the new cursor (after success) and, if `await_reply`,
//!    block on `await_responds_to(handoff_event_id, timeout)`.
//!
//! Per-job `single_in_flight` (default true) rejects overlapping ticks:
//! the gate is held across the whole fire, so a slow source or a slow
//! agent cannot stack a queue of pending fires (§8.5).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, SecondsFormat, Utc};
use proto::types::{Meta, ScopeRef};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::task::JoinSet;

use crate::service::plugin::{ServiceContext, ServicePlugin, ShutdownSignal};
use crate::service::runtime::ServiceRuntime;

use super::cron::Schedule;
use super::source::exec_source;
use super::spec::{CursorBy, DedupeBy, JobSpec, SchedulerConfig, ScopeBinding, ScopeKind, Source};

/// Plugin-kind discriminator used by [`crate::service::ServiceHost`].
pub const KIND: &str = "scheduler";

#[derive(Default)]
pub struct SchedulerPlugin;

#[async_trait]
impl ServicePlugin for SchedulerPlugin {
    fn kind(&self) -> &'static str {
        KIND
    }

    async fn run(&self, ctx: ServiceContext) -> Result<()> {
        let config: SchedulerConfig = if ctx.spec.config.is_null() || ctx.spec.config == json!({}) {
            SchedulerConfig::default()
        } else {
            serde_json::from_value(ctx.spec.config.clone())
                .context("parse spec.config as SchedulerConfig")?
        };
        config.validate().context("validate scheduler config")?;

        let runtime = ctx.runtime.clone();
        let shutdown = ctx.shutdown.clone();

        // Pre-flight: make sure the actor can reach every scope channel.
        // best-effort; channel/invite is idempotent on the server.
        for job in &config.jobs {
            let channel_id = match (&job.scope.kind, &job.scope.channel_id) {
                (ScopeKind::Channel, _) => Some(job.scope.id.clone()),
                (ScopeKind::Thread, Some(c)) => Some(c.clone()),
                _ => None,
            };
            if let Some(cid) = channel_id {
                if let Err(e) = runtime.ensure_channel_member(&cid).await {
                    tracing::warn!(
                        job = %job.id,
                        channel = %cid,
                        error = ?e,
                        "ensure_channel_member failed (continuing anyway)",
                    );
                }
            }
        }

        if config.jobs.is_empty() {
            tracing::info!(
                service = %runtime.service_id(),
                "scheduler spec has no jobs; idling until shutdown",
            );
            wait_shutdown(shutdown).await;
            return Ok(());
        }

        let mut joinset: JoinSet<()> = JoinSet::new();
        for job in config.jobs {
            let runtime = runtime.clone();
            let shutdown = shutdown.clone();
            let job = Arc::new(job);
            joinset.spawn(async move {
                if let Err(e) = run_job_loop(job.clone(), runtime, shutdown).await {
                    tracing::error!(
                        job = %job.id,
                        error = ?e,
                        "scheduler job loop exited with error",
                    );
                }
            });
        }
        // Wait for either every job loop to exit (shutdown) or the
        // shutdown signal directly. The per-job loops observe shutdown
        // themselves; this just keeps `run` alive until they're done.
        while joinset.join_next().await.is_some() {}
        Ok(())
    }
}

/// Per-job state: the static spec + the in-flight gate.
struct JobState {
    spec: Arc<JobSpec>,
    schedule: Schedule,
    in_flight: AtomicBool,
}

async fn run_job_loop(
    job: Arc<JobSpec>,
    runtime: Arc<ServiceRuntime>,
    mut shutdown: ShutdownSignal,
) -> Result<()> {
    let schedule = Schedule::parse(&job.schedule)
        .with_context(|| format!("re-parse cron `{}`", job.schedule))?;
    let state = Arc::new(JobState {
        spec: job.clone(),
        schedule,
        in_flight: AtomicBool::new(false),
    });

    loop {
        let now = Utc::now();
        let next = match state.schedule.next_after(now) {
            Some(t) => t,
            None => {
                tracing::warn!(
                    job = %state.spec.id,
                    cron = %state.spec.schedule,
                    "cron expression has no future fire time within 5 years; exiting job loop",
                );
                return Ok(());
            }
        };
        let wait = (next - now).to_std().unwrap_or(Duration::from_secs(0));
        tokio::select! {
            _ = tokio::time::sleep(wait) => {}
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    tracing::info!(job = %state.spec.id, "shutdown received; job loop exiting");
                    return Ok(());
                }
            }
        }
        // §8.5: single_in_flight default true. We swap the gate before
        // entering the fire — a second tick that lands while the first
        // is still in flight loses the race and skips with a warning.
        if state.spec.single_in_flight && state.in_flight.swap(true, Ordering::AcqRel) {
            tracing::warn!(
                job = %state.spec.id,
                fire_time_utc = %next.to_rfc3339_opts(SecondsFormat::Secs, true),
                "previous fire still in flight; skipping",
            );
            continue;
        }
        let runtime = runtime.clone();
        let state_clone = state.clone();
        // Spawn the actual fire so the loop can sleep for the next tick
        // promptly; the gate is released at the end of the fire task.
        tokio::spawn(async move {
            let res = fire_once(state_clone.clone(), runtime, next).await;
            if state_clone.spec.single_in_flight {
                state_clone.in_flight.store(false, Ordering::Release);
            }
            if let Err(e) = res {
                tracing::warn!(
                    job = %state_clone.spec.id,
                    error = ?e,
                    "scheduler tick failed",
                );
            }
        });
    }
}

/// Single tick: source → cursor diff → dedupe → append → optional await.
///
/// Returns `Err` only on protocol-level failures the operator must
/// see (event/append fails, ServiceRuntime error). Source failures
/// (exit ≠ 0, HTTP non-2xx) are warnings, not errors — the next tick
/// will retry. This matches §8.3's "exit != 0 is failure but doesn't
/// emit an event"; we don't want a sticky source outage to terminate
/// the job loop.
async fn fire_once(
    state: Arc<JobState>,
    runtime: Arc<ServiceRuntime>,
    fire_time: DateTime<Utc>,
) -> Result<()> {
    let job = &state.spec;
    let body = match exec_source(&job.source).await {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(
                job = %job.id,
                error = ?e,
                "source failed; skipping tick",
            );
            return Ok(());
        }
    };
    let body_hash = sha256_hex(&body);

    // Cursor diff (§8.3): `body_hash` skips when unchanged. cursor_save
    // happens *after* successful append so the next tick's diff still
    // sees the prior value if append fails.
    let cursor_name = job.id.clone();
    if matches!(job.cursor_by, CursorBy::BodyHash) {
        if let Some(prev) = runtime.cursor_load(&cursor_name)? {
            if prev == body_hash {
                tracing::debug!(
                    job = %job.id,
                    fire_time_utc = %fire_time.to_rfc3339_opts(SecondsFormat::Secs, true),
                    "body unchanged; skipping",
                );
                return Ok(());
            }
        }
    }

    // Dedupe (§8.4) — must run *before* the append.
    let dedupe_key = build_dedupe_key(runtime.service_id(), job, fire_time, &body_hash);
    if let Some(key) = dedupe_key.as_deref() {
        if !runtime.dedupe_once(key)? {
            tracing::debug!(job = %job.id, key = %key, "dedupe hit; skipping");
            return Ok(());
        }
    }

    let scope = scope_ref(&job.scope);
    let body_text = stringify_body(&body);
    let meta = build_meta(job, fire_time, &body_hash);
    let event_id = if let Some(target) = job.target_agent.as_deref() {
        runtime
            .handoff(target, scope.clone(), body_text.clone(), Some(meta))
            .await
            .with_context(|| format!("scheduler handoff for job `{}`", job.id))?
    } else {
        runtime
            .append_content(scope.clone(), body_text.clone(), Vec::new(), Some(meta))
            .await
            .with_context(|| format!("scheduler append for job `{}`", job.id))?
    };

    // Cursor save after success.
    if matches!(job.cursor_by, CursorBy::BodyHash) {
        if let Err(e) = runtime.cursor_save(&cursor_name, &body_hash) {
            tracing::warn!(
                job = %job.id,
                error = ?e,
                "cursor_save failed (will re-fire on next tick)",
            );
        }
    }

    // Awaited mode: block until the agent answers (or timeout).
    if job.await_reply {
        let timeout = Duration::from_secs(job.await_timeout_secs);
        match runtime.await_responds_to(&event_id, timeout).await {
            Ok(events) if !events.is_empty() => {
                tracing::info!(
                    job = %job.id,
                    trigger = %event_id,
                    replies = events.len(),
                    "scheduler awaited reply received",
                );
            }
            Ok(_) => {
                tracing::warn!(
                    job = %job.id,
                    trigger = %event_id,
                    timeout_secs = job.await_timeout_secs,
                    "scheduler awaited reply timed out",
                );
            }
            Err(e) => {
                tracing::warn!(
                    job = %job.id,
                    error = ?e,
                    "await_responds_to error",
                );
            }
        }
    }

    Ok(())
}

fn scope_ref(s: &ScopeBinding) -> ScopeRef {
    let kind = match s.kind {
        ScopeKind::Thread => proto::types::ScopeKind::Thread,
        ScopeKind::Channel => proto::types::ScopeKind::Channel,
    };
    ScopeRef {
        kind,
        id: s.id.clone(),
    }
}

/// Construct the §8.4 dedupe key. Returns `None` when the job opts out
/// (`DedupeBy::None`).
///
/// `SourceEventId` falls back to the `payload_hash` shape today: until
/// S4 wires source-side event-id extraction we don't actually have a
/// stable id to key on, so the conservative behaviour is to dedupe on
/// the body-hash anyway. The schema field stays so specs needn't be
/// rewritten when extraction lands.
fn build_dedupe_key(
    service_id: &str,
    job: &JobSpec,
    fire_time: DateTime<Utc>,
    body_hash: &str,
) -> Option<String> {
    match job.dedupe_by {
        DedupeBy::None => None,
        DedupeBy::PayloadHash | DedupeBy::SourceEventId => Some(format!(
            "service:{service_id}:job:{}:fire:{}:hash:{}",
            job.id,
            fire_time.to_rfc3339_opts(SecondsFormat::Secs, true),
            body_hash
        )),
    }
}

fn build_meta(job: &JobSpec, fire_time: DateTime<Utc>, body_hash: &str) -> Meta {
    let mut m: BTreeMap<String, Value> = BTreeMap::new();
    m.insert("service".into(), Value::String("scheduler".into()));
    m.insert("jobId".into(), Value::String(job.id.clone()));
    m.insert(
        "fireTimeUtc".into(),
        Value::String(fire_time.to_rfc3339_opts(SecondsFormat::Secs, true)),
    );
    if matches!(job.cursor_by, CursorBy::BodyHash) {
        m.insert("sourceCursor".into(), Value::String(body_hash.to_string()));
    }
    // Source kind helps downstream consumers (and humans reading the
    // event) understand where the body came from without re-fetching.
    m.insert(
        "sourceKind".into(),
        Value::String(source_kind_name(&job.source).into()),
    );
    m
}

fn source_kind_name(s: &Source) -> &'static str {
    match s {
        Source::Command { .. } => "command",
        Source::Http { .. } => "http",
    }
}

fn stringify_body(body: &[u8]) -> String {
    // Output is `text/markdown`; treat the body as UTF-8 text. Lossy
    // conversion preserves whatever the source emitted — non-text APIs
    // belong outside scheduler's scope (§8.2).
    String::from_utf8_lossy(body).into_owned()
}

fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    hex::encode(h.finalize())
}

async fn wait_shutdown(mut shutdown: ShutdownSignal) {
    while shutdown.changed().await.is_ok() {
        if *shutdown.borrow() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use chrono::TimeZone;

    use super::super::spec::{ScopeBinding, Source};

    fn job(id: &str) -> JobSpec {
        JobSpec {
            id: id.into(),
            schedule: "*/5 * * * *".into(),
            source: Source::Command {
                command: "true".into(),
                args: vec![],
                env: BTreeMap::new(),
                timeout_ms: None,
            },
            scope: ScopeBinding {
                kind: ScopeKind::Channel,
                id: "chan_x".into(),
                channel_id: None,
            },
            target_agent: Some("actor_qa".into()),
            dedupe_by: DedupeBy::PayloadHash,
            cursor_by: CursorBy::BodyHash,
            single_in_flight: true,
            await_reply: false,
            await_timeout_secs: 60,
        }
    }

    #[test]
    fn sha256_hex_is_stable_for_known_input() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn dedupe_key_includes_service_job_fire_hash() {
        let j = job("ci_watch");
        let t = Utc.with_ymd_and_hms(2026, 4, 24, 9, 0, 0).unwrap();
        let key = build_dedupe_key("svc_scheduler", &j, t, "deadbeef").unwrap();
        assert_eq!(
            key,
            "service:svc_scheduler:job:ci_watch:fire:2026-04-24T09:00:00Z:hash:deadbeef"
        );
    }

    #[test]
    fn dedupe_key_none_for_opt_out() {
        let mut j = job("x");
        j.dedupe_by = DedupeBy::None;
        let t = Utc.with_ymd_and_hms(2026, 4, 24, 9, 0, 0).unwrap();
        assert!(build_dedupe_key("s", &j, t, "h").is_none());
    }

    #[test]
    fn meta_carries_required_fields() {
        let j = job("ci_watch");
        let t = Utc.with_ymd_and_hms(2026, 4, 24, 9, 0, 0).unwrap();
        let m = build_meta(&j, t, "abcd");
        assert_eq!(m.get("service").and_then(|v| v.as_str()), Some("scheduler"));
        assert_eq!(m.get("jobId").and_then(|v| v.as_str()), Some("ci_watch"));
        assert_eq!(
            m.get("fireTimeUtc").and_then(|v| v.as_str()),
            Some("2026-04-24T09:00:00Z")
        );
        assert_eq!(
            m.get("sourceCursor").and_then(|v| v.as_str()),
            Some("abcd"),
            "BodyHash cursor → meta carries hash",
        );
        assert_eq!(
            m.get("sourceKind").and_then(|v| v.as_str()),
            Some("command")
        );
    }

    #[test]
    fn meta_omits_source_cursor_when_cursor_by_none() {
        let mut j = job("x");
        j.cursor_by = CursorBy::None;
        let t = Utc.with_ymd_and_hms(2026, 4, 24, 9, 0, 0).unwrap();
        let m = build_meta(&j, t, "abcd");
        assert!(!m.contains_key("sourceCursor"));
    }

    #[test]
    fn scope_ref_maps_thread_and_channel() {
        let t = scope_ref(&ScopeBinding {
            kind: ScopeKind::Thread,
            id: "tid".into(),
            channel_id: Some("cid".into()),
        });
        assert_eq!(t.id, "tid");
        assert!(matches!(t.kind, proto::types::ScopeKind::Thread));
        let c = scope_ref(&ScopeBinding {
            kind: ScopeKind::Channel,
            id: "cid".into(),
            channel_id: None,
        });
        assert!(matches!(c.kind, proto::types::ScopeKind::Channel));
    }

    #[test]
    fn stringify_body_lossy_for_invalid_utf8() {
        // Should not panic on non-UTF-8 — replacement char.
        let s = stringify_body(&[0xFFu8, 0xFE, b'a']);
        assert!(s.contains('a'));
    }
}
