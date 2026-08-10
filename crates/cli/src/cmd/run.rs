use std::future::Future;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{SecondsFormat, Utc};
use proto::methods::*;
use proto::types::{Run, RunStatus, ScopeKind, ScopeRef};
use serde_json::{json, Value};

use crate::client::Client;
use crate::render;

pub(crate) const LOOM_NO_REPLY_FILE_ENV: &str = "LOOM_NO_REPLY_FILE";
const WATCH_RECONNECT_INITIAL_DELAY: Duration = Duration::from_secs(1);
const WATCH_RECONNECT_MAX_DELAY: Duration = Duration::from_secs(5);

fn run_status_name(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Queued => "queued",
        RunStatus::PreparingContext => "preparing_context",
        RunStatus::Running => "running",
        RunStatus::WaitingTool => "waiting_tool",
        RunStatus::Completed => "completed",
        RunStatus::Failed => "failed",
        RunStatus::Canceled => "canceled",
    }
}

fn print_run_line(run: &Run) {
    let scope = match run.scope.kind {
        ScopeKind::Channel => format!("#{}", run.scope.id),
        ScopeKind::Thread => format!("thread:{}", run.scope.id),
    };
    let opened = run.opened_at.to_rfc3339_opts(SecondsFormat::Secs, true);
    let closed = run
        .closed_at
        .map(|ts| ts.to_rfc3339_opts(SecondsFormat::Secs, true))
        .unwrap_or_else(|| "-".into());
    let reason = run.start_reason.as_deref().unwrap_or("-");
    println!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}",
        run.id,
        run_status_name(run.status),
        run.actor_id,
        scope,
        opened,
        closed,
        reason
    );
}

pub async fn list(
    client: Arc<Client>,
    statuses: Vec<String>,
    actor_id: Option<String>,
    target: Option<String>,
) -> Result<()> {
    let statuses = statuses
        .iter()
        .map(|raw| parse_run_status(raw))
        .collect::<Result<Vec<_>>>()?;
    let res: RunListResult = client
        .call(
            method::RUN_LIST,
            json!({
                "statuses": if statuses.is_empty() { None } else { Some(statuses) },
                "actorId": actor_id,
                "target": target,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
        return Ok(());
    }
    if res.runs.is_empty() {
        println!("(no runs)");
    }
    for run in res.runs {
        print_run_line(&run);
    }
    Ok(())
}

pub async fn get(client: Arc<Client>, run_id: String) -> Result<()> {
    let res: RunGetResult = client
        .call(method::RUN_GET, json!({ "runId": run_id }))
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        print_run_line(&res.run);
    }
    Ok(())
}

pub async fn watch<F, Fut>(mut client: Arc<Client>, run_id: String, mut reconnect: F) -> Result<()>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Arc<Client>>>,
{
    let mut last_emitted = None;
    loop {
        if watch_connected_session(&client, &run_id, &mut last_emitted).await? {
            return Ok(());
        }

        // Release the dead transport before attempting a replacement. This is
        // especially important for daemon/local-socket clients whose writer
        // half can otherwise remain alive while reconnection is retried.
        drop(client);
        eprintln!("run watch connection lost; reconnecting...");
        let mut delay = WATCH_RECONNECT_INITIAL_DELAY;
        client = loop {
            match reconnect().await {
                Ok(reconnected) => {
                    break reconnected;
                }
                Err(err) => {
                    eprintln!(
                        "run watch reconnect failed: {err}; retrying in {}s",
                        delay.as_secs()
                    );
                    tokio::time::sleep(delay).await;
                    delay = delay.saturating_mul(2).min(WATCH_RECONNECT_MAX_DELAY);
                }
            }
        };
    }
}

/// Watches one established, initialized, actor-bound client session.
/// Returns `true` when the run is terminal and `false` when the stream closes.
async fn watch_connected_session(
    client: &Arc<Client>,
    run_id: &str,
    last_emitted: &mut Option<Value>,
) -> Result<bool> {
    let res: RunGetResult = client
        .call(method::RUN_GET, json!({ "runId": run_id }))
        .await
        .context("run.get before watch subscribe")?;
    let mut run = res.run;
    print_update_if_changed(&run, last_emitted)?;
    // run.updated only reaches subscribers of the run's scope — subscribe
    // before draining notifications or the watch never sees updates.
    if !is_terminal_run_status(run.status) {
        client
            .call_raw(method::SCOPE_SUBSCRIBE, Some(json!({ "scope": run.scope })))
            .await?;
        // Reconcile once after subscribing so an update that landed between
        // run.get and scope/subscribe is not missed.
        let res: RunGetResult = client
            .call(method::RUN_GET, json!({ "runId": run_id }))
            .await
            .context("run.get after watch subscribe")?;
        print_update_if_changed(&res.run, last_emitted)?;
        run = res.run;
    }
    while !is_terminal_run_status(run.status) {
        let notification = {
            let mut rx = client.notifications.lock().await;
            rx.recv().await
        };
        let Some(notification) = notification else {
            return Ok(false);
        };
        if notification.method != method::STREAM_UPDATE {
            continue;
        }
        let Some(params) = notification.params else {
            continue;
        };
        if params.get("kind").and_then(Value::as_str) != Some(stream_kind::RUN_UPDATED) {
            continue;
        }
        let Some(run_value) = params.get("data").and_then(|data| data.get("run")).cloned() else {
            continue;
        };
        let Ok(updated) = serde_json::from_value::<Run>(run_value) else {
            continue;
        };
        if updated.id != run.id {
            continue;
        }
        // A burst can queue multiple run.updated payloads before the
        // post-subscribe run.get completes. Re-read the canonical object so an
        // older queued payload cannot regress output after a newer snapshot.
        let res: RunGetResult = client
            .call(method::RUN_GET, json!({ "runId": run_id }))
            .await
            .context("run.get after run.updated")?;
        print_update_if_changed(&res.run, last_emitted)?;
        run = res.run;
    }
    Ok(true)
}

fn print_update_if_changed(run: &Run, last_emitted: &mut Option<Value>) -> Result<()> {
    let snapshot = serde_json::to_value(run)?;
    if last_emitted.as_ref() == Some(&snapshot) {
        return Ok(());
    }
    if render::is_json() {
        render::print_json(run);
    } else {
        print_run_line(run);
    }
    *last_emitted = Some(snapshot);
    Ok(())
}

pub async fn cancel(
    client: Arc<Client>,
    run_id: String,
    reason: Option<String>,
    yes: bool,
) -> Result<()> {
    if !confirm_cancel(&run_id, yes)? {
        eprintln!("run cancel aborted");
        return Ok(());
    }
    let res: RunCancelResult = client
        .call(
            method::RUN_CANCEL,
            json!({
                "runId": run_id,
                "reason": reason,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!("run {}\t{}", res.run.id, run_status_name(res.run.status));
    }
    Ok(())
}

fn confirm_cancel(run_id: &str, yes: bool) -> Result<bool> {
    let stdin = io::stdin();
    let interactive = stdin.is_terminal();
    let mut input = stdin.lock();
    let stderr = io::stderr();
    let mut output = stderr.lock();
    confirm_cancel_with_io(run_id, yes, interactive, &mut input, &mut output)
}

fn confirm_cancel_with_io<R: BufRead, W: Write>(
    run_id: &str,
    yes: bool,
    interactive: bool,
    input: &mut R,
    output: &mut W,
) -> Result<bool> {
    if yes {
        return Ok(true);
    }
    if !interactive {
        anyhow::bail!(
            "refusing to cancel run `{run_id}` without confirmation; pass --yes in non-interactive environments"
        );
    }

    write!(output, "Cancel run `{run_id}`? [y/N] ")?;
    output.flush()?;
    let mut answer = String::new();
    input.read_line(&mut answer)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn is_terminal_run_status(status: RunStatus) -> bool {
    matches!(
        status,
        RunStatus::Completed | RunStatus::Failed | RunStatus::Canceled
    )
}

pub async fn open(
    client: Arc<Client>,
    actor_id: String,
    target: String,
    delivery_id: Option<String>,
    start_reason: Option<String>,
    agent_config_version_id: String,
) -> Result<()> {
    let res: RunOpenResult = client
        .call(
            method::RUN_OPEN,
            json!({
                "actorId": actor_id,
                "scope": parse_scope_target(&target)?,
                "deliveryId": delivery_id,
                "startReason": start_reason,
                "agentConfigVersionId": agent_config_version_id,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!("run {}\t{}", res.run.id, run_status_name(res.run.status));
    }
    Ok(())
}

pub async fn append(
    client: Arc<Client>,
    run_id: String,
    status: Option<String>,
    frame_kind: String,
    payload_json: Option<String>,
) -> Result<()> {
    let status = status.map(|raw| parse_run_status(&raw)).transpose()?;
    let payload: Value = payload_json
        .map(|raw| serde_json::from_str(&raw).context("parse --payload-json"))
        .transpose()?
        .unwrap_or(Value::Null);
    let res: RunAppendResult = client
        .call(
            method::RUN_APPEND,
            json!({
                "runId": run_id,
                "status": status,
                "frameKind": frame_kind,
                "payload": payload,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!("run {}\tframe={}", res.run.id, res.frame.seq);
    }
    Ok(())
}

pub async fn ignore(
    client: Arc<Client>,
    run_id: Option<String>,
    reason: Option<String>,
) -> Result<()> {
    let run_id = run_id
        .or_else(|| std::env::var("LOOM_RUN_ID").ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .context("run ignore requires --run-id or LOOM_RUN_ID")?;
    let reason = reason
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "no_visible_reply_needed".into());
    let trigger_source_id = std::env::var("LOOM_TRIGGER_MESSAGE_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let payload = json!({
        "noReply": true,
        "replyMode": "none",
        "reason": reason,
        "triggerSourceId": trigger_source_id,
        "createdAt": Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
    });
    let local_marked = mark_local_no_reply(&run_id, &payload)
        .with_context(|| format!("write no-reply marker from {}", LOOM_NO_REPLY_FILE_ENV))?;
    let result: Result<RunAppendResult> = client
        .call(
            method::RUN_APPEND,
            json!({
                "runId": run_id,
                "frameKind": "control.no_reply",
                "payload": payload,
            }),
        )
        .await
        .context("run.append control.no_reply");
    match result {
        Ok(res) => {
            if render::is_json() {
                render::print_json(&json!({
                    "ignored": true,
                    "localMarked": local_marked,
                    "run": res.run,
                    "frame": res.frame,
                }));
            } else {
                println!("run {}\tignored", res.run.id);
            }
        }
        Err(err) if local_marked => {
            if render::is_json() {
                render::print_json(&json!({
                    "ignored": true,
                    "localMarked": true,
                    "auditRecorded": false,
                    "auditError": err.to_string(),
                }));
            } else {
                println!("run ignored locally (audit failed: {err})");
            }
        }
        Err(err) => return Err(err),
    }
    Ok(())
}

pub(crate) fn mark_local_no_reply(run_id: &str, payload: &Value) -> std::io::Result<bool> {
    let Some(path) = std::env::var_os(LOOM_NO_REPLY_FILE_ENV) else {
        return Ok(false);
    };
    let path = std::path::PathBuf::from(path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let marker = json!({
        "runId": run_id,
        "payload": payload,
    });
    let bytes = serde_json::to_vec_pretty(&marker)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))?;
    std::fs::write(path, bytes)?;
    Ok(true)
}

pub(crate) fn ensure_visible_output_allowed(allow_after_no_reply: bool) -> Result<()> {
    if allow_after_no_reply {
        return Ok(());
    }
    let Some(path) = std::env::var_os(LOOM_NO_REPLY_FILE_ENV) else {
        return Ok(());
    };
    ensure_visible_output_allowed_for_path(PathBuf::from(path).as_path())
}

fn ensure_visible_output_allowed_for_path(path: &std::path::Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    anyhow::bail!(
        "this agent run is already marked no-reply or has completed its assignment handoff; \
         sending another message now can duplicate an outcome or wake. Send any visible result \
         before the terminal assignment update, or pass --allow-after-no-reply when a deliberate \
         post-handoff message is required"
    );
}

pub async fn close(client: Arc<Client>, run_id: String, status: String) -> Result<()> {
    let status = parse_run_status(&status)?;
    let res: RunCloseResult = client
        .call(
            method::RUN_CLOSE,
            json!({
                "runId": run_id,
                "status": status,
            }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!("run {}\t{}", res.run.id, run_status_name(res.run.status));
    }
    Ok(())
}

fn parse_scope_target(target: &str) -> Result<ScopeRef> {
    let target = target.trim();
    if let Some(channel_id) = target.strip_prefix('#') {
        if channel_id.is_empty() || channel_id.contains(':') {
            anyhow::bail!("run target must be a channel scope like #chan_id or thread:thread_id");
        }
        return Ok(ScopeRef {
            kind: ScopeKind::Channel,
            id: channel_id.into(),
        });
    }
    if let Some(thread_id) = target.strip_prefix("thread:") {
        if thread_id.trim().is_empty() {
            anyhow::bail!("thread target cannot be empty");
        }
        return Ok(ScopeRef {
            kind: ScopeKind::Thread,
            id: thread_id.trim().into(),
        });
    }
    anyhow::bail!("run target must be #channel or thread:<thread_id>");
}

fn parse_run_status(raw: &str) -> Result<RunStatus> {
    serde_json::from_value(json!(raw.trim())).with_context(|| {
        "invalid run status; expected queued, preparing_context, running, waiting_tool, completed, failed, or canceled"
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_status_names_match_canonical_wire_values() {
        let cases = [
            (RunStatus::Queued, "queued"),
            (RunStatus::PreparingContext, "preparing_context"),
            (RunStatus::Running, "running"),
            (RunStatus::WaitingTool, "waiting_tool"),
            (RunStatus::Completed, "completed"),
            (RunStatus::Failed, "failed"),
            (RunStatus::Canceled, "canceled"),
        ];
        for (status, expected) in cases {
            assert_eq!(run_status_name(status), expected);
            assert_eq!(serde_json::to_value(status).unwrap(), json!(expected));
        }
    }

    #[test]
    fn cancel_yes_is_safe_for_non_interactive_scripts() {
        let mut input = io::Cursor::new(Vec::<u8>::new());
        let mut output = Vec::new();
        assert!(confirm_cancel_with_io("run_1", true, false, &mut input, &mut output).unwrap());
        assert!(output.is_empty());
    }

    #[test]
    fn cancel_without_yes_is_rejected_when_non_interactive() {
        let mut input = io::Cursor::new(Vec::<u8>::new());
        let mut output = Vec::new();
        let err = confirm_cancel_with_io("run_1", false, false, &mut input, &mut output)
            .expect_err("non-interactive cancellation must require --yes");
        assert!(err.to_string().contains("--yes"));
        assert!(output.is_empty());
    }

    #[test]
    fn interactive_cancel_requires_explicit_yes() {
        for (answer, expected) in [
            ("y\n", true),
            ("YES\n", true),
            ("\n", false),
            ("no\n", false),
        ] {
            let mut input = io::Cursor::new(answer.as_bytes());
            let mut output = Vec::new();
            assert_eq!(
                confirm_cancel_with_io("run_1", false, true, &mut input, &mut output).unwrap(),
                expected
            );
            assert_eq!(
                String::from_utf8(output).unwrap(),
                "Cancel run `run_1`? [y/N] "
            );
        }
    }

    fn temp_marker(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "loom-{name}-{}-{}.json",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ))
    }

    #[test]
    fn visible_output_guard_allows_missing_marker() {
        let marker = temp_marker("missing-no-reply");
        assert!(ensure_visible_output_allowed_for_path(&marker).is_ok());
    }

    #[test]
    fn visible_output_guard_rejects_existing_marker() {
        let marker = temp_marker("existing-no-reply");
        std::fs::write(&marker, "{}").expect("write marker");

        let err = ensure_visible_output_allowed_for_path(&marker).expect_err("guard should reject");

        assert!(err.to_string().contains("--allow-after-no-reply"));
        std::fs::remove_file(marker).ok();
    }
}
