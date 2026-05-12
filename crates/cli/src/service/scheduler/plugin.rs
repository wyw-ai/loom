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

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, SecondsFormat, Utc};
use proto::methods::ServiceLifecycle;
use proto::types::{Meta, ScopeRef};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::Notify;
use tokio::task::JoinSet;

use crate::service::plugin::{ServiceContext, ServicePlugin, ShutdownSignal};
use crate::service::runtime::ServiceRuntime;

use super::cron::Schedule;
use super::source::exec_source;
use super::spec::{
    CursorBy, DedupeBy, EmitConfig, EmitMode, JobSpec, SchedulerConfig, ScopeBinding, ScopeKind,
    Source,
};

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
        // §4.7.3 placeholder substitution. For thread-bound instances
        // (and harmlessly for channel-singletons too) interpolate
        // `{thread.id}` / `{channel.id}` / `{channel.workspace}` /
        // `{params.X}` / `{instance.data_dir}` / `{service.data_dir}`
        // everywhere inside spec.config so jobs
        // can reference the per-instance binding without the host
        // having to teach every plugin a separate templating layer.
        let subs = build_substitutions(&ctx);
        let mut config_json = ctx.spec.config.clone();
        if !subs.is_empty() {
            substitute_in_value(&mut config_json, &subs);
        }
        let config: SchedulerConfig = if config_json.is_null() || config_json == json!({}) {
            SchedulerConfig::default()
        } else {
            serde_json::from_value(config_json).context("parse spec.config as SchedulerConfig")?
        };
        config.validate().context("validate scheduler config")?;

        let runtime = ctx.runtime.clone();
        let shutdown = ctx.shutdown.clone();

        // §4.7.3: thread-bound services tear down their instance when
        // any event in `bind.auto_stop_on` is observed. Today the
        // self_complete sentinel is the only signal scheduler raises;
        // future expansions (e.g., listening for `thread.closed` on
        // delivery_list) reuse the same Notify.
        let stop_on_self_complete = matches!(ctx.spec.lifecycle, ServiceLifecycle::ThreadBound)
            && ctx
                .spec
                .bind
                .as_ref()
                .map(|b| b.auto_stop_on.iter().any(|k| k == "service.self_complete"))
                .unwrap_or(false);
        let self_complete = Arc::new(Notify::new());

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
            let self_complete = self_complete.clone();
            let job = Arc::new(job);
            joinset.spawn(async move {
                if let Err(e) = run_job_loop(job.clone(), runtime, shutdown, self_complete).await {
                    tracing::error!(
                        job = %job.id,
                        error = ?e,
                        "scheduler job loop exited with error",
                    );
                }
            });
        }
        // Wait for either every job loop to exit (shutdown) or, for a
        // thread-bound spec opted into `service.self_complete`, the
        // first time a job emits the sentinel. In the latter case we
        // cancel the remaining jobs so the scheduler instance can
        // unwind cleanly and the host's per-instance task ends.
        if stop_on_self_complete {
            tokio::select! {
                _ = async { while joinset.join_next().await.is_some() {} } => {}
                _ = self_complete.notified() => {
                    tracing::info!(
                        service = %runtime.service_id(),
                        instance = ?runtime.instance_id(),
                        "service.self_complete observed; aborting remaining jobs",
                    );
                    joinset.abort_all();
                    while joinset.join_next().await.is_some() {}
                }
            }
        } else {
            while joinset.join_next().await.is_some() {}
        }
        Ok(())
    }
}

/// Build the §4.7.3 substitution map from a [`ServiceContext`].
///
/// Keys are the literal placeholder strings (`{thread.id}`,
/// `{channel.id}`, `{channel.workspace}`, `{instance.data_dir}`,
/// `{service.data_dir}`, `{params.<name>}`); values are their concrete
/// replacements pulled from `ctx.instance` / `ctx.spec` / `ctx.runtime`.
/// Scope-keyed entries are only inserted when the context carries the
/// corresponding channel, thread, or params, so unrelated placeholders are
/// left intact for the next layer (or, more usually, don't appear at all).
///
/// `params` are flattened one level deep — a top-level object keyed
/// by name, value rendered as: strings used verbatim, everything
/// else `to_string()` (so booleans become "true"/"false", numbers
/// their decimal form, nested objects/arrays their compact JSON).
fn build_substitutions(ctx: &ServiceContext) -> HashMap<String, String> {
    let mut subs: HashMap<String, String> = HashMap::new();
    let service_data_dir = ctx.runtime.state_dir().display().to_string();
    subs.insert("{service.data_dir}".to_string(), service_data_dir.clone());
    subs.insert("{instance.data_dir}".to_string(), service_data_dir);
    if let Some(path) = ctx.spec_path.as_ref() {
        if let Some(dir) = path.parent() {
            let dir_str = dir.display().to_string();
            subs.insert("{spec.dir}".to_string(), dir_str.clone());
            subs.insert("{bundle.dir}".to_string(), format!("{dir_str}/bundle"));
        }
    }
    let channel_id = ctx
        .instance
        .as_ref()
        .and_then(|inst| inst.scope.channel_id.as_deref())
        .or(ctx.spec.channel_id.as_deref());
    if let Some(channel) = channel_id {
        subs.insert("{channel.id}".to_string(), channel.to_string());
        subs.insert(
            "{channel.workspace}".to_string(),
            channel_workspace_dir(channel).display().to_string(),
        );
    }
    if let Some(inst) = &ctx.instance {
        subs.insert("{thread.id}".to_string(), inst.scope.id.clone());
        if let Some(params) = inst.params.as_object() {
            for (k, v) in params {
                let rendered = match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                subs.insert(format!("{{params.{k}}}"), rendered);
            }
        }
    }
    subs
}

fn channel_workspace_dir(channel_id: &str) -> std::path::PathBuf {
    crate::cmd::agent_serve::default_data_root_pub()
        .join("channels")
        .join(channel_id)
        .join("shared")
}

/// Apply the substitution map to every string within `v`, recursing
/// into arrays and objects in place. O(N · M) where N = total bytes
/// of strings, M = number of placeholders; both are tiny (<10 each
/// in practice) so the plain `String::contains` + `String::replace`
/// pair beats a regex/aho-corasick build cost here.
fn substitute_in_value(v: &mut Value, subs: &HashMap<String, String>) {
    match v {
        Value::String(s) => {
            for (placeholder, replacement) in subs {
                if s.contains(placeholder) {
                    *s = s.replace(placeholder, replacement);
                }
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                substitute_in_value(item, subs);
            }
        }
        Value::Object(obj) => {
            for (_, val) in obj.iter_mut() {
                substitute_in_value(val, subs);
            }
        }
        _ => {}
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
    self_complete: Arc<Notify>,
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
        let self_complete = self_complete.clone();
        // Spawn the actual fire so the loop can sleep for the next tick
        // promptly; the gate is released at the end of the fire task.
        tokio::spawn(async move {
            let res = fire_once(state_clone.clone(), runtime, next, self_complete).await;
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
    self_complete: Arc<Notify>,
) -> Result<()> {
    let job = &state.spec;
    let raw_body = match exec_source(&job.source).await {
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
    // §4.7.3: a service plugin (e.g., mr-detector bundle) may signal
    // "this instance is done" by writing a sentinel JSON object as the
    // last line of stdout. We strip the sentinel from the body so it
    // doesn't leak into the user-visible event, then publish a separate
    // `service.self_complete` event and notify the run() loop so the
    // scheduler can tear the instance down when `auto_stop_on` opts in.
    let (body_vec, self_complete_signal) = strip_self_complete(&raw_body);
    let body = body_vec.as_slice();
    let body_hash = sha256_hex(body);

    let scope = scope_ref(&job.scope);
    let mut event_id: Option<String> = None;

    // If the body is empty after stripping the sentinel, skip the
    // regular content event (a self_complete-only tick has nothing
    // user-visible to log). Otherwise run the cursor diff / dedupe /
    // append / await chain like a normal tick.
    if !body.is_empty() {
        // Cursor diff (§8.3): `body_hash` skips when unchanged.
        // cursor_save happens *after* successful append so the next
        // tick's diff still sees the prior value if append fails.
        let mut cursor_skip = false;
        let cursor_name = job.id.clone();
        if matches!(job.cursor_by, CursorBy::BodyHash) {
            if let Some(prev) = runtime.cursor_load(&cursor_name)? {
                if prev == body_hash {
                    tracing::debug!(
                        job = %job.id,
                        fire_time_utc = %fire_time.to_rfc3339_opts(SecondsFormat::Secs, true),
                        "body unchanged; skipping",
                    );
                    cursor_skip = true;
                }
            }
        }

        if !cursor_skip {
            // Dedupe (§8.4) — must run *before* the append.
            let dedupe_key = build_dedupe_key(runtime.service_id(), job, fire_time, &body_hash);
            let dedupe_skip = if let Some(key) = dedupe_key.as_deref() {
                if !runtime.dedupe_once(key)? {
                    tracing::debug!(job = %job.id, key = %key, "dedupe hit; skipping");
                    true
                } else {
                    false
                }
            } else {
                false
            };

            if !dedupe_skip {
                let id = if let Some(emit) = job.emit.as_ref() {
                    emit_per_line(
                        emit,
                        body,
                        &runtime,
                        scope.clone(),
                        job,
                        fire_time,
                        &body_hash,
                    )
                    .await
                    .with_context(|| format!("scheduler emit-per-line for job `{}`", job.id))?
                } else {
                    let body_text = stringify_body(body);
                    let meta = build_meta(job, fire_time, &body_hash);
                    if let Some(target) = job.target_agent.as_deref() {
                        runtime
                            .handoff(target, scope.clone(), body_text.clone(), Some(meta))
                            .await
                            .with_context(|| format!("scheduler handoff for job `{}`", job.id))?
                    } else {
                        runtime
                            .append_content(
                                scope.clone(),
                                body_text.clone(),
                                Vec::new(),
                                Some(meta),
                            )
                            .await
                            .with_context(|| format!("scheduler append for job `{}`", job.id))?
                    }
                };
                event_id = Some(id);

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
            }
        }
    }

    // §4.7.3: emit the self_complete event regardless of cursor /
    // dedupe outcome — the bundle saying "I'm done" is itself the
    // event that matters, and a stuck cursor must not silence it.
    if let Some(signal) = self_complete_signal {
        match runtime
            .publish_self_complete(scope.clone(), signal.reason.clone())
            .await
        {
            Ok(id) => {
                tracing::info!(
                    job = %job.id,
                    instance = ?runtime.instance_id(),
                    reason = %signal.reason,
                    event_id = %id,
                    "service.self_complete published",
                );
            }
            Err(e) => {
                tracing::warn!(
                    job = %job.id,
                    error = ?e,
                    "publish_self_complete failed (continuing)",
                );
            }
        }
        // Wake up `run()`'s select; a no-op when no one is waiting.
        self_complete.notify_waiters();
    }

    // Awaited mode: block until the agent answers (or timeout). Only
    // meaningful when the tick actually emitted an event.
    if let Some(event_id) = event_id.as_deref() {
        if job.await_reply {
            let timeout = Duration::from_secs(job.await_timeout_secs);
            match runtime.await_responds_to(event_id, timeout).await {
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

/// Emit one artifact + one `status.update` event per non-empty JSON
/// line in `body`. Returns the id of the *last* event appended so the
/// caller can drive the regular cursor / await_responds_to bookkeeping.
/// A line that fails JSON parse is logged and skipped — one bad line
/// must not block subsequent transitions in the same tick. When zero
/// lines are emittable, returns the body-hash-stamped no-op event id
/// to preserve cursor invariants.
async fn emit_per_line(
    emit: &EmitConfig,
    body: &[u8],
    runtime: &ServiceRuntime,
    scope: ScopeRef,
    job: &JobSpec,
    fire_time: DateTime<Utc>,
    body_hash: &str,
) -> Result<String> {
    if !matches!(emit.mode, EmitMode::ArtifactPerJsonLine) {
        // Defensive: future EmitMode variants must extend this.
        anyhow::bail!("unsupported emit mode for job `{}`", job.id);
    }
    let text = String::from_utf8_lossy(body);
    let mut last_event_id: Option<String> = None;
    let template = emit
        .artifact_name_template
        .clone()
        .unwrap_or_else(|| format!("{}-{{event_kind}}.json", job.id));
    let mut emitted = 0usize;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let payload: Value = match serde_json::from_str::<Value>(trimmed) {
            Ok(v) if v.is_object() => v,
            Ok(_) => {
                tracing::warn!(
                    job = %job.id,
                    "emit-per-line: skipping non-object JSON line",
                );
                continue;
            }
            Err(e) => {
                tracing::warn!(
                    job = %job.id,
                    error = %e,
                    "emit-per-line: skipping unparseable line",
                );
                continue;
            }
        };
        let name = render_artifact_name(&template, &payload);
        let body_text =
            serde_json::to_string(&payload).with_context(|| "serialize artifact body")?;
        let (artifact_id, _uri) = runtime
            .publish_artifact(
                scope.clone(),
                name.clone(),
                Some("application/json".into()),
                body_text,
            )
            .await
            .with_context(|| format!("publish_artifact for job `{}`", job.id))?;
        let mut meta = build_meta(job, fire_time, body_hash);
        meta.insert("artifactName".into(), Value::String(name.clone()));
        let event_id = runtime
            .append_status(
                scope.clone(),
                emit.status_event_type.clone(),
                payload,
                Some(&artifact_id),
                Some(meta),
            )
            .await
            .with_context(|| format!("append_status for job `{}`", job.id))?;
        last_event_id = Some(event_id);
        emitted += 1;
    }
    if emitted == 0 {
        // No usable lines — fall back to a single empty status.update so
        // cursor / await_responds_to plumbing remains coherent.
        let meta = build_meta(job, fire_time, body_hash);
        let id = runtime
            .append_status(
                scope,
                emit.status_event_type.clone(),
                json!({"empty": true}),
                None,
                Some(meta),
            )
            .await?;
        return Ok(id);
    }
    Ok(last_event_id.expect("emitted > 0 implies last_event_id set"))
}

/// Substitute `{key}` tokens in `template` with the matching top-level
/// string field from `payload`. Missing or non-string fields render as
/// the literal placeholder so the artifact name flags the gap rather
/// than silently coalescing distinct events.
fn render_artifact_name(template: &str, payload: &Value) -> String {
    let mut out = String::with_capacity(template.len());
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            if let Some(close) = template[i..].find('}') {
                let key = &template[i + 1..i + close];
                let replacement = payload
                    .get(key)
                    .and_then(|v| v.as_str())
                    .map(sanitize_name_segment)
                    .unwrap_or_else(|| format!("{{{key}}}"));
                out.push_str(&replacement);
                i += close + 1;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn sanitize_name_segment(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Sentinel parsed off the last line of a tick's stdout. See §4.7.3.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct SelfCompleteSentinel {
    #[serde(rename = "service.self_complete")]
    service_self_complete: bool,
    #[serde(default)]
    reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SelfCompleteSignal {
    reason: String,
}

/// If the last non-empty line of `body` parses as `{"service.self_complete":
/// true, "reason": "..."}`, return the rest of the body (sentinel
/// stripped, with any trailing blank lines trimmed) plus the parsed
/// signal. Otherwise return `body` unchanged. Tolerates JSON whose
/// `reason` is omitted; rejects sentinels whose flag is false or whose
/// JSON shape doesn't match — those are passed through as ordinary
/// stdout so the operator sees them.
fn strip_self_complete(body: &[u8]) -> (Vec<u8>, Option<SelfCompleteSignal>) {
    let text = match std::str::from_utf8(body) {
        Ok(s) => s,
        Err(_) => return (body.to_vec(), None),
    };
    // Walk lines from the end skipping pure whitespace; the first
    // non-empty line is the candidate.
    let trimmed_end = text.trim_end_matches(['\n', '\r', ' ', '\t']);
    let last_newline = trimmed_end.rfind('\n');
    let (head, last_line) = match last_newline {
        Some(i) => (&trimmed_end[..i], trimmed_end[i + 1..].trim()),
        None => ("", trimmed_end.trim()),
    };
    if last_line.is_empty() || !last_line.starts_with('{') {
        return (body.to_vec(), None);
    }
    let parsed: SelfCompleteSentinel = match serde_json::from_str::<SelfCompleteSentinel>(last_line)
    {
        Ok(p) if p.service_self_complete => p,
        _ => return (body.to_vec(), None),
    };
    let reason = if parsed.reason.is_empty() {
        "unspecified".to_string()
    } else {
        parsed.reason
    };
    let mut head_bytes = head.trim_end_matches(['\n', '\r']).as_bytes().to_vec();
    if !head_bytes.is_empty() {
        head_bytes.push(b'\n');
    }
    (head_bytes, Some(SelfCompleteSignal { reason }))
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
            emit: None,
        }
    }

    #[test]
    fn render_artifact_name_substitutes_top_level_fields() {
        let payload = json!({"event_kind": "merged", "mr_id": 42});
        let out = render_artifact_name("mr-event-{event_kind}-{mr_id}.json", &payload);
        // mr_id is a number, not a string, so it's left as the literal placeholder.
        assert_eq!(out, "mr-event-merged-{mr_id}.json");
    }

    #[test]
    fn render_artifact_name_sanitizes_unsafe_chars() {
        let payload = json!({"event_kind": "build/passed?", "mr_id": "12"});
        let out = render_artifact_name("mr-{event_kind}-{mr_id}.json", &payload);
        assert_eq!(out, "mr-build_passed_-12.json");
    }

    #[test]
    fn render_artifact_name_keeps_unknown_placeholders() {
        let payload = json!({"event_kind": "merged"});
        let out = render_artifact_name("mr-{event_kind}-{missing}.json", &payload);
        assert_eq!(out, "mr-merged-{missing}.json");
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

    #[test]
    fn strip_self_complete_detects_sentinel_only_body() {
        let body = b"{\"service.self_complete\":true,\"reason\":\"merged\"}\n";
        let (rest, sig) = strip_self_complete(body);
        assert!(rest.is_empty(), "sentinel-only body strips to empty");
        let sig = sig.expect("signal parsed");
        assert_eq!(sig.reason, "merged");
    }

    #[test]
    fn strip_self_complete_keeps_preceding_stdout() {
        // Real bundle shape: emit one tick event line then the sentinel
        // as the trailing line. The tick event must survive untouched.
        let body = b"{\"event\":\"opened\",\"raw_event_fingerprint\":\"sha256:abc\"}\n{\"service.self_complete\":true,\"reason\":\"closed\"}\n";
        let (rest, sig) = strip_self_complete(body);
        let rest_str = String::from_utf8(rest).unwrap();
        assert!(rest_str.contains("\"event\":\"opened\""));
        assert!(!rest_str.contains("self_complete"));
        assert_eq!(sig.unwrap().reason, "closed");
    }

    #[test]
    fn strip_self_complete_passes_through_when_absent() {
        let body = b"hello world\n";
        let (rest, sig) = strip_self_complete(body);
        assert_eq!(rest, body);
        assert!(sig.is_none());
    }

    #[test]
    fn strip_self_complete_ignores_false_flag() {
        let body = b"{\"service.self_complete\":false,\"reason\":\"x\"}\n";
        let (rest, sig) = strip_self_complete(body);
        assert_eq!(rest, body);
        assert!(sig.is_none());
    }

    #[test]
    fn strip_self_complete_defaults_reason_when_missing() {
        let body = b"{\"service.self_complete\":true}\n";
        let (_rest, sig) = strip_self_complete(body);
        assert_eq!(sig.unwrap().reason, "unspecified");
    }

    #[test]
    fn strip_self_complete_tolerates_non_utf8_input() {
        let body = &[0xFF, 0xFE, b'\n'][..];
        let (rest, sig) = strip_self_complete(body);
        assert_eq!(rest, body.to_vec());
        assert!(sig.is_none());
    }

    #[test]
    fn substitute_replaces_thread_and_params_in_strings() {
        let mut subs: HashMap<String, String> = HashMap::new();
        subs.insert("{thread.id}".into(), "thr_42".into());
        subs.insert("{params.mr_url}".into(), "https://x/y".into());

        let mut v = json!({
            "scope": { "id": "{thread.id}" },
            "command": "fetch {params.mr_url} into {thread.id}",
            "args": ["{thread.id}", "literal", "{params.mr_url}/path"],
        });
        substitute_in_value(&mut v, &subs);

        assert_eq!(v["scope"]["id"], json!("thr_42"));
        assert_eq!(v["command"], json!("fetch https://x/y into thr_42"));
        assert_eq!(v["args"][0], json!("thr_42"));
        assert_eq!(v["args"][1], json!("literal"));
        assert_eq!(v["args"][2], json!("https://x/y/path"));
    }

    #[test]
    fn substitute_leaves_unmatched_placeholders_untouched() {
        let subs: HashMap<String, String> = HashMap::new();
        let mut v = json!({ "x": "{thread.id} stays" });
        substitute_in_value(&mut v, &subs);
        assert_eq!(v["x"], json!("{thread.id} stays"));
    }

    #[test]
    fn substitute_renders_non_string_param_values() {
        let mut subs: HashMap<String, String> = HashMap::new();
        // Mirror what build_substitutions does for non-string values.
        subs.insert("{params.flag}".into(), Value::Bool(true).to_string());
        subs.insert("{params.n}".into(), Value::from(7).to_string());

        let mut v = json!({
            "args": ["--flag={params.flag}", "--n={params.n}"],
        });
        substitute_in_value(&mut v, &subs);
        assert_eq!(v["args"][0], json!("--flag=true"));
        assert_eq!(v["args"][1], json!("--n=7"));
    }

    #[test]
    fn substitute_replaces_channel_and_service_placeholders() {
        let mut subs: HashMap<String, String> = HashMap::new();
        subs.insert("{channel.id}".into(), "chan_repo".into());
        subs.insert(
            "{channel.workspace}".into(),
            "/data/channels/chan_repo/shared".into(),
        );
        subs.insert("{service.data_dir}".into(), "/svc/repo-cache".into());

        let mut v = json!({
            "args": [
                "--channel={channel.id}",
                "{channel.workspace}/.joi/repos/manifest.json",
                "{service.data_dir}/cache"
            ]
        });
        substitute_in_value(&mut v, &subs);

        assert_eq!(v["args"][0], json!("--channel=chan_repo"));
        assert_eq!(
            v["args"][1],
            json!("/data/channels/chan_repo/shared/.joi/repos/manifest.json")
        );
        assert_eq!(v["args"][2], json!("/svc/repo-cache/cache"));
    }

    #[test]
    fn channel_workspace_dir_points_at_channel_shared() {
        let suffix = std::path::Path::new("channels")
            .join("chan_repo")
            .join("shared");
        assert!(channel_workspace_dir("chan_repo").ends_with(&suffix));
    }
}
