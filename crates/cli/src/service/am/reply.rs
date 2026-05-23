// First user is the orchestrator in mod.rs (S2-5).
#![allow(dead_code)]

//! Three reply paths back to DingTalk, mirroring §7.4:
//!
//! * [`ReplyMode::Callback`] — print the DingTalk callback JSON to
//!   stdout. `am listen --script` reads it and replies for us.
//! * [`ReplyMode::Send`] — invoke `am group <conv> <text>` (or
//!   `am chat <sender> <text>` if no conversation) synchronously, with
//!   retry/backoff and an optional fallback to chat.
//! * [`ReplyMode::AsyncSend`] — print a "received, working on it"
//!   callback, then spawn a detached child of `loom service am-handler
//!   --async-reply <payload>`. The child waits for the agent reply and
//!   sends via `am`.
//!
//! Pure functions where possible; subprocess + filesystem live in
//! [`send_via_am`] and [`spawn_detached_async_reply`].

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{extract, text};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplyMode {
    Callback,
    Send,
    AsyncSend,
}

impl Default for ReplyMode {
    fn default() -> Self {
        Self::Callback
    }
}

/// Knobs for the `Send` and `AsyncSend` paths. Defaults match the
/// Python reference so behavior stays 1:1 unless an operator overrides.
#[derive(Debug, Clone)]
pub struct AmSendConfig {
    pub am_bin: String,
    pub am_config_path: Option<PathBuf>,
    pub send_plain_text: bool,
    pub send_max_chars: usize,
    pub send_attempts: u32,
    pub send_retry_delay_secs: f64,
    pub send_retry_backoff: f64,
    pub send_fallback_chat: bool,
    pub reply_strict: bool,
    pub dry_run: bool,
}

impl Default for AmSendConfig {
    fn default() -> Self {
        Self {
            am_bin: "am".into(),
            am_config_path: None,
            send_plain_text: true,
            send_max_chars: 1800,
            send_attempts: 4,
            send_retry_delay_secs: 1.0,
            send_retry_backoff: 1.8,
            send_fallback_chat: true,
            reply_strict: false,
            dry_run: false,
        }
    }
}

/// Build the DingTalk callback JSON line (no trailing newline).
/// Mirrors Python `reply_callback`. Public for tests and for the
/// orchestrator's pending-callback path in async_send mode.
pub fn callback_line(source_payload: &Value, answer: &str) -> String {
    let sender_id = am_staff_id(&extract::sender(source_payload));
    let conversation_id = extract::conversation(source_payload);
    let mut msg_param = serde_json::Map::new();
    msg_param.insert("content".into(), json!(answer.trim()));
    if !sender_id.is_empty() {
        msg_param.insert("senderStaffId".into(), json!(sender_id));
    }
    if !conversation_id.is_empty() {
        msg_param.insert("conversationId".into(), json!(conversation_id));
    }
    let body = json!({
        "msgKey": "sampleText",
        "msgParam": Value::Object(msg_param).to_string()
    });
    body.to_string()
}

pub fn write_callback<W: Write>(
    out: &mut W,
    source_payload: &Value,
    answer: &str,
) -> io::Result<()> {
    writeln!(out, "{}", callback_line(source_payload, answer))?;
    out.flush()
}

/// Synchronously invoke `am group|chat` with retry/backoff. On group
/// failure with `send_fallback_chat = true`, retry as `am chat`.
/// Returns `Err` only when `reply_strict` is set AND every attempt
/// (incl. fallback) failed. Otherwise logs and returns `Ok` so the
/// listener callback path doesn't blow up.
pub fn send_via_am(cfg: &AmSendConfig, source_payload: &Value, answer: &str) -> Result<()> {
    let conversation_id = extract::conversation(source_payload);
    let sender_id = am_staff_id(&extract::sender(source_payload));
    let outbound = if cfg.send_plain_text {
        text::plain_am_text(answer, cfg.send_max_chars)
    } else {
        answer.trim().to_string()
    };
    if conversation_id.is_empty() && sender_id.is_empty() {
        tracing::warn!("am reply: no conversation or sender — nothing to send to");
        return Ok(());
    }
    if cfg.dry_run {
        // Mirror the Python AM_LOOM_DRY_RUN: print to stdout and skip.
        println!("{}", outbound);
        return Ok(());
    }

    let primary: Vec<String> = if !conversation_id.is_empty() {
        let mut a = vec!["group".into(), conversation_id.clone(), outbound.clone()];
        if !sender_id.is_empty() {
            a.push("--at".into());
            a.push(sender_id.clone());
        }
        a
    } else {
        vec!["chat".into(), sender_id.clone(), outbound.clone()]
    };

    let primary_err = match run_am_with_retry(cfg, &primary, "am reply") {
        Ok(()) => return Ok(()),
        Err(e) => e,
    };

    let should_fallback =
        !conversation_id.is_empty() && !sender_id.is_empty() && cfg.send_fallback_chat;
    if !should_fallback {
        return fail_strict(cfg, &primary_err.to_string());
    }
    let fallback: Vec<String> = vec!["chat".into(), sender_id, outbound];
    match run_am_with_retry(cfg, &fallback, "am fallback chat") {
        Ok(()) => {
            tracing::info!("am fallback chat succeeded after group failure");
            Ok(())
        }
        Err(fallback_err) => fail_strict(cfg, &format!("{primary_err}\n{fallback_err}")),
    }
}

fn run_am_with_retry(cfg: &AmSendConfig, args: &[String], label: &str) -> Result<()> {
    let attempts = cfg.send_attempts.max(1);
    let mut delay = cfg.send_retry_delay_secs.max(0.0);
    let backoff = cfg.send_retry_backoff.max(1.0);
    let mut last_err: Option<String> = None;
    for attempt in 1..=attempts {
        let mut cmd = Command::new(&cfg.am_bin);
        if let Some(cfg_path) = &cfg.am_config_path {
            cmd.arg("--config-path").arg(cfg_path);
        }
        cmd.args(args);
        let output = cmd.output().with_context(|| {
            format!("spawn `{}` (attempt {}/{})", cfg.am_bin, attempt, attempts)
        })?;
        if output.status.success() {
            if attempt > 1 {
                tracing::info!(label, attempt, attempts, "am send succeeded after retry");
            }
            return Ok(());
        }
        let detail = format!(
            "{label} (exit {}): {}",
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(if output.stderr.is_empty() {
                &output.stdout
            } else {
                &output.stderr
            })
            .trim()
        );
        tracing::warn!(label, attempt, attempts, %detail, "am send failed; retrying");
        last_err = Some(detail);
        if attempt < attempts && delay > 0.0 {
            std::thread::sleep(Duration::from_secs_f64(delay));
            delay *= backoff;
        }
    }
    Err(anyhow!(
        "{}",
        last_err.unwrap_or_else(|| format!("{label}: unknown failure"))
    ))
}

fn fail_strict(cfg: &AmSendConfig, message: &str) -> Result<()> {
    tracing::error!(%message, "am reply failed; strict={}", cfg.reply_strict);
    if cfg.reply_strict {
        bail!("{}", message);
    }
    Ok(())
}

/// DingTalk staff ids that look numeric come back zero-padded
/// occasionally; strip leading zeros for the `--at` flag. Mirrors
/// Python `am_staff_id`. Non-numeric ids pass through.
pub(crate) fn am_staff_id(s: &str) -> String {
    if s.is_empty() || !s.chars().all(|c| c.is_ascii_digit()) {
        return s.to_string();
    }
    let stripped = s.trim_start_matches('0');
    if stripped.is_empty() {
        s.to_string()
    } else {
        stripped.to_string()
    }
}

/// Spawn a detached `loom service am-handler --service-id <sid>
/// --async-reply <payload>` child. The parent (listener-callback
/// process) returns immediately after; the child waits for the agent
/// reply and sends via `am`.
///
/// Detachment uses `process_group(0)` on Unix so SIGHUP from the
/// listener exiting doesn't kill the child. Stdout/stderr are
/// redirected to `log_path` (append, parent dirs created).
#[cfg(unix)]
pub fn spawn_detached_async_reply(
    binary_path: &Path,
    service_id: &str,
    payload: &Value,
    log_path: &Path,
) -> Result<()> {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;

    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create log dir {}", parent.display()))?;
    }
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .with_context(|| format!("open async-reply log {}", log_path.display()))?;
    let log_err = log.try_clone()?;
    let payload_str = serde_json::to_string(payload)?;

    let mut cmd = Command::new(binary_path);
    cmd.args([
        "service",
        "am-handler",
        "--service-id",
        service_id,
        "--async-reply",
        &payload_str,
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::from(log))
    .stderr(Stdio::from(log_err))
    .process_group(0);
    cmd.spawn().context("spawn detached async-reply child")?;
    Ok(())
}

/// Windows fallback — no `process_group(0)`. Spawns the child without
/// detachment; the parent is expected to exit quickly so the OS treats
/// the child as a normal background process. Untested on Windows in
/// S2; flagged here so we don't promise behavior we don't deliver.
#[cfg(not(unix))]
pub fn spawn_detached_async_reply(
    binary_path: &Path,
    service_id: &str,
    payload: &Value,
    log_path: &Path,
) -> Result<()> {
    use std::process::Stdio;
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    let log_err = log.try_clone()?;
    let payload_str = serde_json::to_string(payload)?;
    Command::new(binary_path)
        .args([
            "service",
            "am-handler",
            "--service-id",
            service_id,
            "--async-reply",
            &payload_str,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err))
        .spawn()
        .context("spawn detached async-reply child (windows)")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn am_staff_id_strips_leading_zeros_for_numeric() {
        assert_eq!(am_staff_id("00012345"), "12345");
        assert_eq!(am_staff_id("12345"), "12345");
    }

    #[test]
    fn am_staff_id_preserves_non_numeric() {
        assert_eq!(am_staff_id("abc123"), "abc123");
        assert_eq!(am_staff_id(""), "");
    }

    #[test]
    fn am_staff_id_keeps_all_zeros_intact() {
        // Stripping all zeros would produce "" which would lose the id;
        // keep the literal value instead.
        assert_eq!(am_staff_id("0000"), "0000");
    }

    #[test]
    fn callback_line_includes_content_and_known_routing_fields() {
        let event = json!({
            "conversationId": "conv_x",
            "senderStaffId": "00007"
        });
        let line = callback_line(&event, "  hi there  ");
        let parsed: Value = serde_json::from_str(&line).expect("valid json");
        assert_eq!(parsed["msgKey"], "sampleText");
        let inner: Value =
            serde_json::from_str(parsed["msgParam"].as_str().expect("msgParam string"))
                .expect("inner is json");
        assert_eq!(inner["content"], "hi there");
        assert_eq!(inner["conversationId"], "conv_x");
        // Numeric leading zero stripped.
        assert_eq!(inner["senderStaffId"], "7");
    }

    #[test]
    fn callback_line_omits_routing_when_event_lacks_them() {
        let event = json!({});
        let line = callback_line(&event, "hi");
        let parsed: Value = serde_json::from_str(&line).unwrap();
        let inner: Value = serde_json::from_str(parsed["msgParam"].as_str().unwrap()).unwrap();
        assert_eq!(inner["content"], "hi");
        assert!(inner.get("senderStaffId").is_none());
        assert!(inner.get("conversationId").is_none());
    }

    #[test]
    fn write_callback_appends_newline_and_flushes() {
        let mut buf = Vec::new();
        write_callback(&mut buf, &json!({"conversationId": "c"}), "hi").unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.ends_with('\n'));
        let line = s.trim();
        let parsed: Value = serde_json::from_str(line).unwrap();
        assert_eq!(parsed["msgKey"], "sampleText");
    }

    #[test]
    fn send_via_am_skips_when_no_targeting_present() {
        // No conversationId, no senderStaffId, no `am` invocation.
        // We don't actually run `am` here — the fast-path returns Ok
        // before any subprocess spawn.
        let cfg = AmSendConfig::default();
        let res = send_via_am(&cfg, &json!({}), "hello");
        assert!(res.is_ok());
    }

    #[test]
    fn send_via_am_dry_run_prints_text_and_skips_subprocess() {
        // dry_run path also avoids spawning `am`. Stdout side effect
        // not asserted (would need redirection setup), but we verify
        // no error path is taken when `am_bin` is intentionally
        // invalid.
        let cfg = AmSendConfig {
            am_bin: "/nonexistent/binary".into(),
            dry_run: true,
            ..AmSendConfig::default()
        };
        let event = json!({"conversationId": "c", "senderStaffId": "u"});
        let res = send_via_am(&cfg, &event, "ans");
        assert!(res.is_ok(), "dry_run skips spawn so bad bin doesn't matter");
    }

    #[test]
    fn send_via_am_strict_failure_propagates() {
        // Strict mode: when every retry of an unspawnable bin fails,
        // we surface the error. (Non-strict would log and Ok.)
        let cfg = AmSendConfig {
            am_bin: "/definitely/not/a/real/binary/loom-am-test".into(),
            send_attempts: 1,
            send_retry_delay_secs: 0.0,
            reply_strict: true,
            ..AmSendConfig::default()
        };
        let event = json!({"conversationId": "c", "senderStaffId": "u"});
        let res = send_via_am(&cfg, &event, "ans");
        assert!(res.is_err(), "strict mode must propagate");
    }

    #[test]
    fn send_via_am_non_strict_swallows_failure() {
        let cfg = AmSendConfig {
            am_bin: "/definitely/not/a/real/binary/loom-am-test".into(),
            send_attempts: 1,
            send_retry_delay_secs: 0.0,
            send_fallback_chat: false,
            reply_strict: false,
            ..AmSendConfig::default()
        };
        let event = json!({"conversationId": "c", "senderStaffId": "u"});
        let res = send_via_am(&cfg, &event, "ans");
        assert!(res.is_ok(), "non-strict swallows the error");
    }
}
