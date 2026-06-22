use std::collections::HashMap;
use std::process::Command;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use reqwest::header::{ACCEPT, USER_AGENT};
use reqwest::Client as HttpClient;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;
use url::Url;
use uuid::Uuid;

use crate::config::{normalize_human_account, HumanAccount};

const CALLBACK_TIMEOUT: Duration = Duration::from_secs(300);
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const USER_AGENT_VALUE: &str = "loom-desktop";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuthProvider {
    Google,
    GitHub,
}

impl OAuthProvider {
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "google" => Ok(Self::Google),
            "github" => Ok(Self::GitHub),
            other => bail!("unsupported account provider: {other}"),
        }
    }

    fn display(self) -> &'static str {
        match self {
            Self::Google => "Google",
            Self::GitHub => "GitHub",
        }
    }

    fn auth_url(self) -> &'static str {
        match self {
            Self::Google => "https://accounts.google.com/o/oauth2/v2/auth",
            Self::GitHub => "https://github.com/login/oauth/authorize",
        }
    }

    fn token_url(self) -> &'static str {
        match self {
            Self::Google => "https://oauth2.googleapis.com/token",
            Self::GitHub => "https://github.com/login/oauth/access_token",
        }
    }

    fn scope(self) -> &'static str {
        match self {
            Self::Google => "openid email profile",
            Self::GitHub => "read:user user:email",
        }
    }

    fn client_id(self) -> Result<String> {
        let env_name = match self {
            Self::Google => "LOOM_GOOGLE_CLIENT_ID",
            Self::GitHub => "LOOM_GITHUB_CLIENT_ID",
        };
        std::env::var(env_name)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("{env_name} is required for {} login", self.display()))
    }

    fn client_secret(self) -> Option<String> {
        let env_name = match self {
            Self::Google => "LOOM_GOOGLE_CLIENT_SECRET",
            Self::GitHub => "LOOM_GITHUB_CLIENT_SECRET",
        };
        std::env::var(env_name)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }
}

pub async fn login_oauth(provider: &str) -> Result<HumanAccount> {
    let provider = OAuthProvider::parse(provider)?;
    let client_id = provider.client_id()?;
    let client_secret = provider.client_secret();
    let pkce = Pkce::new();
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .with_context(|| format!("starting {} callback listener", provider.display()))?;
    let redirect_uri = format!(
        "http://127.0.0.1:{}/callback",
        listener.local_addr()?.port()
    );
    let auth_url = authorization_url(provider, &client_id, &redirect_uri, &pkce)?;

    open_browser(auth_url.as_str()).with_context(|| {
        format!(
            "opening browser for {} login failed; visit this URL manually: {}",
            provider.display(),
            auth_url.as_str()
        )
    })?;

    let code = wait_for_callback(listener, &pkce.state, provider).await?;
    let client = HttpClient::builder().timeout(HTTP_TIMEOUT).build()?;
    let token = exchange_token(
        &client,
        provider,
        &client_id,
        client_secret.as_deref(),
        &code,
        &pkce.code_verifier,
        &redirect_uri,
    )
    .await?;
    provider_account(&client, provider, &token.access_token).await
}

fn authorization_url(
    provider: OAuthProvider,
    client_id: &str,
    redirect_uri: &str,
    pkce: &Pkce,
) -> Result<Url> {
    let mut url = Url::parse(provider.auth_url())?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("scope", provider.scope())
        .append_pair("state", &pkce.state)
        .append_pair("code_challenge", &pkce.code_challenge)
        .append_pair("code_challenge_method", "S256");
    Ok(url)
}

async fn wait_for_callback(
    listener: TcpListener,
    expected_state: &str,
    provider: OAuthProvider,
) -> Result<String> {
    let (mut stream, _) = timeout(CALLBACK_TIMEOUT, listener.accept())
        .await
        .map_err(|_| {
            anyhow!(
                "timed out waiting for {} authorization callback",
                provider.display()
            )
        })??;

    let request = read_http_request(&mut stream, provider).await?;
    let result = parse_callback_request(&request, expected_state);
    match &result {
        Ok(_) => {
            write_http_response(
                &mut stream,
                "200 OK",
                &format!(
                    "{} login complete. You can return to Loom.",
                    provider.display()
                ),
            )
            .await?;
        }
        Err(err) => {
            write_http_response(
                &mut stream,
                "400 Bad Request",
                &format!("{} login failed: {err}", provider.display()),
            )
            .await?;
        }
    }

    result
}

async fn read_http_request(stream: &mut TcpStream, provider: OAuthProvider) -> Result<String> {
    let mut buf = Vec::with_capacity(2048);
    let mut chunk = [0_u8; 1024];
    loop {
        let n = timeout(Duration::from_secs(10), stream.read(&mut chunk))
            .await
            .map_err(|_| anyhow!("timed out reading {} callback request", provider.display()))??;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|window| window == b"\r\n\r\n") || buf.len() > 16 * 1024 {
            break;
        }
    }
    String::from_utf8(buf).context("OAuth callback request was not UTF-8")
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
        "<!doctype html><meta charset=\"utf-8\"><title>Loom Login</title><body>{body}</body>"
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
    provider: OAuthProvider,
    client_id: &str,
    client_secret: Option<&str>,
    code: &str,
    code_verifier: &str,
    redirect_uri: &str,
) -> Result<TokenResponse> {
    let mut form = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("client_id", client_id),
        ("redirect_uri", redirect_uri),
        ("code_verifier", code_verifier),
    ];
    if let Some(secret) = client_secret {
        form.push(("client_secret", secret));
    }

    let response = client
        .post(provider.token_url())
        .header(ACCEPT, "application/json")
        .header(USER_AGENT, USER_AGENT_VALUE)
        .form(&form)
        .send()
        .await
        .with_context(|| format!("{} token exchange request failed", provider.display()))?;
    decode_json_response(response, &format!("{} token exchange", provider.display())).await
}

async fn provider_account(
    client: &HttpClient,
    provider: OAuthProvider,
    access_token: &str,
) -> Result<HumanAccount> {
    match provider {
        OAuthProvider::Google => google_account(client, access_token).await,
        OAuthProvider::GitHub => github_account(client, access_token).await,
    }
}

async fn google_account(client: &HttpClient, access_token: &str) -> Result<HumanAccount> {
    let response = client
        .get("https://openidconnect.googleapis.com/v1/userinfo")
        .bearer_auth(access_token)
        .header(ACCEPT, "application/json")
        .send()
        .await
        .context("Google user info request failed")?
        .error_for_status()
        .context("Google user info returned an error")?;
    let user: GoogleUser = response.json().await.context("parsing Google user info")?;

    if user.sub.trim().is_empty() {
        bail!("Google user info did not include subject");
    }
    let nickname = user
        .email
        .as_deref()
        .and_then(|email| email.split('@').next())
        .unwrap_or("")
        .to_string();
    Ok(normalize_human_account(HumanAccount {
        provider: "google".into(),
        staff_id: user.sub,
        nickname,
        real_name: user.name.unwrap_or_default(),
        email: user.email.unwrap_or_default(),
        actor_id: String::new(),
        avatar_url: user.picture.unwrap_or_default(),
    }))
}

async fn github_account(client: &HttpClient, access_token: &str) -> Result<HumanAccount> {
    let response = client
        .get("https://api.github.com/user")
        .bearer_auth(access_token)
        .header(ACCEPT, "application/vnd.github+json")
        .header(USER_AGENT, USER_AGENT_VALUE)
        .send()
        .await
        .context("GitHub user info request failed")?
        .error_for_status()
        .context("GitHub user info returned an error")?;
    let user: GitHubUser = response.json().await.context("parsing GitHub user info")?;

    let email = match user.email.filter(|email| !email.trim().is_empty()) {
        Some(email) => email,
        None => github_primary_email(client, access_token)
            .await
            .unwrap_or_default(),
    };
    Ok(normalize_human_account(HumanAccount {
        provider: "github".into(),
        staff_id: user.id.to_string(),
        nickname: user.login,
        real_name: user.name.unwrap_or_default(),
        email,
        actor_id: String::new(),
        avatar_url: user.avatar_url.unwrap_or_default(),
    }))
}

async fn github_primary_email(client: &HttpClient, access_token: &str) -> Result<String> {
    let response = client
        .get("https://api.github.com/user/emails")
        .bearer_auth(access_token)
        .header(ACCEPT, "application/vnd.github+json")
        .header(USER_AGENT, USER_AGENT_VALUE)
        .send()
        .await
        .context("GitHub emails request failed")?
        .error_for_status()
        .context("GitHub emails returned an error")?;
    let emails: Vec<GitHubEmail> = response.json().await.context("parsing GitHub emails")?;
    emails
        .iter()
        .find(|email| email.primary && email.verified)
        .or_else(|| emails.iter().find(|email| email.verified))
        .map(|email| email.email.clone())
        .filter(|email| !email.trim().is_empty())
        .ok_or_else(|| anyhow!("GitHub account did not expose a verified email"))
}

async fn decode_json_response<T: for<'de> Deserialize<'de>>(
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
    // FIXME(windows-compat-iter): GUI surface deferred per PRD §2.3
    // [G1: OS shell visibility] (PM-Arbitration-003 judgement).
    // These user-facing browser launchers must NOT use
    // `loom_platform::process::Command` until P1-Cmd-Sweep verifies that the
    // newtype's default Windows flags (CREATE_NO_WINDOW |
    // CREATE_BREAKAWAY_FROM_JOB | CREATE_NEW_PROCESS_GROUP) do not break the
    // browser/explorer launch UX. P0-D's `clippy::disallowed_methods` lint is
    // also deferred; the bare comment is sufficient signaling until then.
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
struct GoogleUser {
    sub: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    picture: Option<String>,
}

#[derive(Deserialize)]
struct GitHubUser {
    id: u64,
    login: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    avatar_url: Option<String>,
}

#[derive(Deserialize)]
struct GitHubEmail {
    email: String,
    primary: bool,
    verified: bool,
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
    fn parses_supported_providers() {
        assert_eq!(
            OAuthProvider::parse("google").unwrap(),
            OAuthProvider::Google
        );
        assert_eq!(
            OAuthProvider::parse("GitHub").unwrap(),
            OAuthProvider::GitHub
        );
        assert!(OAuthProvider::parse("internal").is_err());
    }
}
