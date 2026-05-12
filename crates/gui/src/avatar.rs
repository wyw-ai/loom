use std::path::PathBuf;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use sha2::{Digest, Sha256};
use url::Url;

use crate::config::{self, HumanAccount};

const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_AVATAR_BYTES: usize = 2 * 1024 * 1024;
const WORK_AVATAR_HOST: &str = "work.alibaba-inc.com";

pub async fn prefetch_account_avatar(account: &HumanAccount) -> Result<()> {
    cached_avatar_data_url(&account.avatar_url)
        .await
        .map(|_| ())
}

pub async fn cached_avatar_data_url(source: &str) -> Result<String> {
    let source = source.trim();
    if source.starts_with("data:") {
        return Ok(source.to_string());
    }

    let url = normalize_work_avatar_url(source)?;
    let path = cache_path_for_url(url.as_str());
    let mime = mime_for_path(url.path());

    if let Ok(bytes) = std::fs::read(&path) {
        if !bytes.is_empty() {
            return Ok(data_url(mime, &bytes));
        }
    }

    let bytes = download_avatar(url.as_str()).await?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create avatar cache {}", parent.display()))?;
    }
    std::fs::write(&path, &bytes)
        .with_context(|| format!("write avatar cache {}", path.display()))?;
    Ok(data_url(mime, &bytes))
}

fn normalize_work_avatar_url(source: &str) -> Result<Url> {
    let normalized = if source.starts_with("//") {
        format!("https:{source}")
    } else {
        source.to_string()
    };
    let url = Url::parse(&normalized).with_context(|| format!("parse avatar URL {source}"))?;
    match url.scheme() {
        "http" | "https" => {}
        other => bail!("unsupported avatar URL scheme: {other}"),
    }
    if url.host_str() != Some(WORK_AVATAR_HOST) {
        bail!("unsupported avatar URL host");
    }
    if !url.path().starts_with("/photo/") {
        bail!("unsupported work avatar path");
    }
    Ok(url)
}

async fn download_avatar(url: &str) -> Result<Vec<u8>> {
    let client = reqwest::Client::builder().timeout(HTTP_TIMEOUT).build()?;
    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("download avatar {url}"))?;
    if !response.status().is_success() {
        return Err(anyhow!("avatar request failed: {}", response.status()));
    }
    let bytes = response.bytes().await.context("read avatar response")?;
    if bytes.len() > MAX_AVATAR_BYTES {
        bail!("avatar response too large");
    }
    Ok(bytes.to_vec())
}

fn cache_path_for_url(url: &str) -> PathBuf {
    config::config_dir()
        .join("avatars")
        .join(format!("{}.img", hex_sha256(url.as_bytes())))
}

fn data_url(mime: &str, bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", base64_standard(bytes))
}

fn mime_for_path(path: &str) -> &'static str {
    let path = path.to_ascii_lowercase();
    if path.ends_with(".png") {
        "image/png"
    } else if path.ends_with(".webp") {
        "image/webp"
    } else {
        "image/jpeg"
    }
}

fn hex_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn base64_standard(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        let n = ((b0 as u32) << 16) | ((b1 as u32) << 8) | b2 as u32;
        out.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((n >> 6) & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(n & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_accepts_scheme_relative_work_avatar() {
        let url = normalize_work_avatar_url("//work.alibaba-inc.com/photo/123.140x140.jpg")
            .expect("normalize");

        assert_eq!(
            url.as_str(),
            "https://work.alibaba-inc.com/photo/123.140x140.jpg"
        );
    }

    #[test]
    fn base64_standard_pads_output() {
        assert_eq!(base64_standard(b""), "");
        assert_eq!(base64_standard(b"f"), "Zg==");
        assert_eq!(base64_standard(b"fo"), "Zm8=");
        assert_eq!(base64_standard(b"foo"), "Zm9v");
    }
}
