//! Source executors for scheduler jobs (§8.3 Source kinds).
//!
//! Each tick of the cron loop calls [`exec_source`] which dispatches by
//! `Source::kind`:
//!
//! * `command` — spawn a child via `tokio::process::Command`, capture
//!   stdout, treat exit ≠ 0 as failure (with stderr in the error msg).
//!   stderr lines stream to `tracing` so operators can see what the
//!   subprocess is doing.
//! * `http` — single GET/POST via reqwest, treat non-2xx as failure.
//!   Body is returned as raw bytes.
//!
//! Both honor the per-source timeout (default 30s). Failure raises an
//! `anyhow::Error` so the caller (the per-job loop in `plugin.rs`)
//! decides whether to skip-and-retry or escalate.

use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::Method;
use tokio::process::Command;

use super::spec::Source;

/// Run the source once and return its body (bytes).
///
/// Errors carry enough context for the operator to debug from the log:
/// command line, exit code, stderr tail (capped) for command sources;
/// status, URL, body tail for HTTP. The plugin logs and skips on error;
/// retries become the next cron tick (intentionally — scheduler does
/// not internally retry within a tick).
pub async fn exec_source(source: &Source) -> Result<Vec<u8>> {
    let timeout = source.effective_timeout();
    match source {
        Source::Command {
            command,
            args,
            env,
            ..
        } => exec_command(command, args, env, timeout).await,
        Source::Http {
            url,
            method,
            headers,
            body,
            ..
        } => exec_http(url, method, headers, body.as_deref(), timeout).await,
    }
}

async fn exec_command(
    command: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
    timeout: Duration,
) -> Result<Vec<u8>> {
    let mut cmd = Command::new(command);
    cmd.args(args);
    for (k, v) in env {
        cmd.env(k, v);
    }
    // Inherit parent stdin closed; capture stdout + stderr.
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let child = cmd
        .spawn()
        .with_context(|| format!("spawn `{command}`"))?;
    let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(r) => r.with_context(|| format!("wait `{command}`"))?,
        Err(_) => {
            return Err(anyhow!(
                "command `{}` timed out after {:?}",
                command,
                timeout
            ));
        }
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "command `{} {}` exited with status {} (stderr: {})",
            command,
            args.join(" "),
            output.status,
            tail(&stderr, 512)
        ));
    }
    if !output.stderr.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::debug!(command = %command, stderr = %tail(&stderr, 1024), "command stderr");
    }
    Ok(output.stdout)
}

async fn exec_http(
    url: &str,
    method: &str,
    headers: &BTreeMap<String, String>,
    body: Option<&str>,
    timeout: Duration,
) -> Result<Vec<u8>> {
    let m = Method::from_bytes(method.to_ascii_uppercase().as_bytes())
        .with_context(|| format!("parse HTTP method `{method}`"))?;
    let mut header_map = HeaderMap::new();
    for (k, v) in headers {
        let name = HeaderName::from_bytes(k.as_bytes())
            .with_context(|| format!("invalid header name `{k}`"))?;
        let val = HeaderValue::from_str(v)
            .with_context(|| format!("invalid header value for `{k}`"))?;
        header_map.insert(name, val);
    }
    // A new client per call is fine for the tick-rate the scheduler
    // runs at (jobs/minute, not jobs/second). Connection-pool sharing
    // would only matter once we're firing the same URL at sub-second
    // cadence — well outside §13 Q3's scope.
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .context("build reqwest client")?;
    let mut req = client.request(m, url).headers(header_map);
    if let Some(b) = body {
        req = req.body(b.to_string());
    }
    let resp = req
        .send()
        .await
        .with_context(|| format!("HTTP {method} {url}"))?;
    let status = resp.status();
    let bytes = resp
        .bytes()
        .await
        .with_context(|| format!("read body of {method} {url}"))?;
    if !status.is_success() {
        let body_str = String::from_utf8_lossy(&bytes);
        return Err(anyhow!(
            "HTTP {} {} returned status {} (body: {})",
            method,
            url,
            status,
            tail(&body_str, 512)
        ));
    }
    Ok(bytes.to_vec())
}

fn tail(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let suffix = &s[s.len() - max..];
        format!("…{suffix}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(c: &str, args: &[&str]) -> Source {
        Source::Command {
            command: c.into(),
            args: args.iter().map(|s| s.to_string()).collect(),
            env: BTreeMap::new(),
            timeout_ms: Some(5_000),
        }
    }

    #[tokio::test]
    async fn command_returns_stdout() {
        let body = exec_source(&cmd("echo", &["hello", "world"])).await.unwrap();
        assert_eq!(String::from_utf8(body).unwrap().trim(), "hello world");
    }

    #[tokio::test]
    async fn command_propagates_nonzero_exit() {
        // `false` is the canonical exit-1 unix utility.
        let err = exec_source(&cmd("false", &[])).await.unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("exited with status"), "{msg}");
    }

    #[tokio::test]
    async fn command_passes_env() {
        let mut env = BTreeMap::new();
        env.insert("MY_TEST_VAR".into(), "joi-scheduler".into());
        let s = Source::Command {
            command: "sh".into(),
            args: vec!["-c".into(), "echo $MY_TEST_VAR".into()],
            env,
            timeout_ms: Some(5_000),
        };
        let body = exec_source(&s).await.unwrap();
        assert_eq!(String::from_utf8(body).unwrap().trim(), "joi-scheduler");
    }

    #[tokio::test]
    async fn command_respects_timeout() {
        let s = Source::Command {
            command: "sh".into(),
            args: vec!["-c".into(), "sleep 5".into()],
            env: BTreeMap::new(),
            timeout_ms: Some(150),
        };
        let err = exec_source(&s).await.unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("timed out"), "{msg}");
    }

    #[tokio::test]
    async fn command_missing_binary_errs() {
        let s = cmd("/no/such/binary/joi-cron", &[]);
        let err = exec_source(&s).await.unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("spawn"), "{msg}");
    }

    #[test]
    fn tail_truncates_left() {
        assert_eq!(tail("abcdef", 3), "…def");
        assert_eq!(tail("abc", 5), "abc");
    }
}
