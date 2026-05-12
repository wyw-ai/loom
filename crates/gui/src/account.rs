use std::collections::HashMap;
use std::process::Command;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use reqwest::Client as HttpClient;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;
use url::Url;
use uuid::Uuid;

use crate::config::{normalize_human_account, HumanAccount};

const BUC_BASE_URL: &str = "https://login.alibaba-inc.com";
const BUC_CLIENT_ID: &str = "aone-cli";
const BUC_AUTH_PATH: &str = "/oauth2/auth.htm";
const BUC_TOKEN_PATH: &str = "/rpc/oauth2/access_token.json";
const BUC_USER_INFO_PATH: &str = "/rpc/oauth2/user_info.json";
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(300);
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

pub async fn login_buc() -> Result<HumanAccount> {
    let pkce = Pkce::new();
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .context("starting BUC callback listener")?;
    let redirect_uri = format!(
        "http://127.0.0.1:{}/callback",
        listener.local_addr()?.port()
    );
    let base_url = buc_base_url();
    let auth_url = authorization_url(&base_url, &redirect_uri, &pkce)?;

    open_browser(auth_url.as_str()).with_context(|| {
        format!(
            "opening browser for BUC login failed; visit this URL manually: {}",
            auth_url.as_str()
        )
    })?;

    let code = wait_for_callback(listener, &pkce.state).await?;
    let client = HttpClient::builder().timeout(HTTP_TIMEOUT).build()?;
    let token = exchange_token(
        &client,
        &base_url,
        &code,
        &pkce.code_verifier,
        &redirect_uri,
    )
    .await?;
    let user = get_user_info(&client, &base_url, &token.access_token).await?;
    user.into_account()
}

fn buc_base_url() -> String {
    std::env::var("JOI_BUC_BASE_URL")
        .or_else(|_| std::env::var("A1_BUC_BASE_URL"))
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            let trimmed = value.trim().trim_end_matches('/').to_string();
            if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
                trimmed
            } else {
                format!("http://{trimmed}")
            }
        })
        .unwrap_or_else(|| BUC_BASE_URL.into())
}

fn authorization_url(base_url: &str, redirect_uri: &str, pkce: &Pkce) -> Result<Url> {
    // Reuse the a1-cli BUC OAuth app so users approve the same internal app.
    let mut url = Url::parse(&format!("{base_url}{BUC_AUTH_PATH}"))?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", BUC_CLIENT_ID)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("scope", "profile")
        .append_pair("state", &pkce.state)
        .append_pair("code_challenge", &pkce.code_challenge)
        .append_pair("code_challenge_method", "S256");
    Ok(url)
}

async fn wait_for_callback(listener: TcpListener, expected_state: &str) -> Result<String> {
    let (mut stream, _) = timeout(CALLBACK_TIMEOUT, listener.accept())
        .await
        .map_err(|_| anyhow!("timed out waiting for BUC authorization callback"))??;

    let request = read_http_request(&mut stream).await?;
    let result = parse_callback_request(&request, expected_state);
    match &result {
        Ok(_) => {
            write_http_response(
                &mut stream,
                "200 OK",
                "BUC login complete. You can return to Joi.",
            )
            .await?;
        }
        Err(err) => {
            write_http_response(
                &mut stream,
                "400 Bad Request",
                &format!("BUC login failed: {err}"),
            )
            .await?;
        }
    }

    result
}

async fn read_http_request(stream: &mut TcpStream) -> Result<String> {
    let mut buf = Vec::with_capacity(2048);
    let mut chunk = [0_u8; 1024];
    loop {
        let n = timeout(Duration::from_secs(10), stream.read(&mut chunk))
            .await
            .map_err(|_| anyhow!("timed out reading BUC callback request"))??;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|window| window == b"\r\n\r\n") || buf.len() > 16 * 1024 {
            break;
        }
    }
    String::from_utf8(buf).context("BUC callback request was not UTF-8")
}

fn parse_callback_request(request: &str, expected_state: &str) -> Result<String> {
    let first_line = request
        .lines()
        .next()
        .ok_or_else(|| anyhow!("empty callback request"))?;
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts
        .next()
        .ok_or_else(|| anyhow!("callback request missing target"))?;
    if method != "GET" {
        bail!("callback method must be GET");
    }

    let callback_url = if target.starts_with("http://") || target.starts_with("https://") {
        Url::parse(target)?
    } else {
        Url::parse(&format!("http://127.0.0.1{target}"))?
    };
    let query: HashMap<String, String> = callback_url.query_pairs().into_owned().collect();

    if let Some(error) = query.get("error") {
        let description = query
            .get("error_description")
            .map(String::as_str)
            .unwrap_or(error);
        bail!("{description}");
    }

    let state = query
        .get("state")
        .ok_or_else(|| anyhow!("callback missing state"))?;
    if state != expected_state {
        bail!("callback state mismatch");
    }

    query
        .get("code")
        .filter(|code| !code.trim().is_empty())
        .cloned()
        .ok_or_else(|| anyhow!("callback missing authorization code"))
}

async fn write_http_response(stream: &mut TcpStream, status: &str, body: &str) -> Result<()> {
    let html = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>Joi Login</title><body>{body}</body>"
    );
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        html.len(),
        html
    );
    stream.write_all(response.as_bytes()).await?;
    let _ = stream.shutdown().await;
    Ok(())
}

async fn exchange_token(
    client: &HttpClient,
    base_url: &str,
    code: &str,
    code_verifier: &str,
    redirect_uri: &str,
) -> Result<TokenResponse> {
    let body = client
        .post(format!("{base_url}{BUC_TOKEN_PATH}"))
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("client_id", BUC_CLIENT_ID),
            ("redirect_uri", redirect_uri),
            ("code_verifier", code_verifier),
        ])
        .send()
        .await
        .context("BUC token exchange request failed")?;
    decode_buc_response(body, "BUC token exchange").await
}

async fn get_user_info(client: &HttpClient, base_url: &str, access_token: &str) -> Result<BucUser> {
    let body = client
        .post(format!("{base_url}{BUC_USER_INFO_PATH}"))
        .form(&[("access_token", access_token)])
        .send()
        .await
        .context("BUC user info request failed")?;
    decode_buc_response(body, "BUC user info").await
}

async fn decode_buc_response<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
    label: &str,
) -> Result<T> {
    let status = response.status();
    let text = response.text().await?;
    if !status.is_success() {
        bail!("{label} failed (HTTP {status}): {text}");
    }
    serde_json::from_str(&text).with_context(|| format!("parsing {label} response"))
}

fn open_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut cmd = Command::new("open");
        cmd.arg(url);
        cmd
    };

    #[cfg(target_os = "linux")]
    let mut cmd = {
        let mut cmd = Command::new("xdg-open");
        cmd.arg(url);
        cmd
    };

    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "start", "", url]);
        cmd
    };

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = url;
        bail!("opening a browser is not supported on this platform");
    }

    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    {
        cmd.spawn()?;
        Ok(())
    }
}

struct Pkce {
    code_verifier: String,
    code_challenge: String,
    state: String,
}

impl Pkce {
    fn new() -> Self {
        let code_verifier = random_url_token();
        let code_challenge = s256_challenge(&code_verifier);
        let state = random_url_token();
        Self {
            code_verifier,
            code_challenge,
            state,
        }
    }
}

fn random_url_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

fn s256_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    base64_url_no_pad(&digest)
}

fn base64_url_no_pad(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity((bytes.len() * 4).div_ceil(3));
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        let n = ((b0 as u32) << 16) | ((b1 as u32) << 8) | b2 as u32;
        out.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((n >> 6) & 0x3f) as usize] as char);
        }
        if chunk.len() > 2 {
            out.push(TABLE[(n & 0x3f) as usize] as char);
        }
    }
    out
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
}

#[derive(Deserialize)]
struct BucUser {
    #[serde(default, alias = "empId", alias = "employeeId")]
    emp_id: String,
    #[serde(default, alias = "realName")]
    name: String,
    #[serde(default, alias = "nickName")]
    nickname: String,
    #[serde(default)]
    email: String,
}

impl BucUser {
    fn into_account(self) -> Result<HumanAccount> {
        let staff_id = self.emp_id.trim();
        if staff_id.is_empty() {
            bail!("BUC user info did not include employee id");
        }
        Ok(normalize_human_account(HumanAccount {
            provider: "buc".into(),
            staff_id: staff_id.into(),
            nickname: self.nickname,
            real_name: self.name,
            email: self.email,
            actor_id: String::new(),
            avatar_url: String::new(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_url_no_pad_omits_padding() {
        assert_eq!(base64_url_no_pad(b""), "");
        assert_eq!(base64_url_no_pad(b"f"), "Zg");
        assert_eq!(base64_url_no_pad(b"fo"), "Zm8");
        assert_eq!(base64_url_no_pad(b"foo"), "Zm9v");
    }

    #[test]
    fn parse_callback_rejects_bad_state() {
        let request = "GET /callback?code=abc&state=wrong HTTP/1.1\r\n\r\n";
        let err = parse_callback_request(request, "expected").unwrap_err();
        assert!(err.to_string().contains("state mismatch"));
    }

    #[test]
    fn buc_user_account_uses_only_profile_fields() {
        let user = BucUser {
            emp_id: "12345".into(),
            name: "Bo Jun".into(),
            nickname: "bojun".into(),
            email: "bojun@example.com".into(),
        };
        let account = user.into_account().unwrap();
        assert_eq!(account.provider, "buc");
        assert_eq!(account.staff_id, "12345");
        assert_eq!(account.nickname, "bojun");
        assert_eq!(account.real_name, "Bo Jun");
        assert_eq!(account.email, "bojun@example.com");
    }
}
