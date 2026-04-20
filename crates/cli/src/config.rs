use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server_url: String,
    pub actor_id: String,
    #[serde(default)]
    pub display_name: String,
}

impl Default for Config {
    fn default() -> Self {
        let suffix = Uuid::new_v4().simple().to_string()[..8].to_string();
        Self {
            server_url: "ws://127.0.0.1:7878/rpc".into(),
            actor_id: format!("actor_human_{}", suffix),
            display_name: whoami_or("you"),
        }
    }
}

fn whoami_or(default: &str) -> String {
    std::env::var("USER")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default.into())
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .map(|d| d.join("joi-apps"))
        .unwrap_or_else(|| PathBuf::from(".joi-apps"))
}

pub fn config_path() -> PathBuf {
    config_dir().join("cli.toml")
}

pub fn load_or_init() -> Result<Config> {
    let path = config_path();
    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Ok(cfg) = toml::from_str::<Config>(&text) {
            return Ok(cfg);
        }
    }
    let cfg = Config::default();
    let _ = std::fs::create_dir_all(config_dir());
    let text = toml::to_string_pretty(&cfg)?;
    let _ = std::fs::write(&path, text);
    Ok(cfg)
}

pub fn resolve(
    server_url: Option<String>,
    actor_id: Option<String>,
) -> Result<Config> {
    let mut cfg = load_or_init()?;
    if let Some(s) = server_url.or_else(|| std::env::var("JOI_SERVER").ok()) {
        cfg.server_url = s;
    }
    if let Some(a) = actor_id.or_else(|| std::env::var("JOI_ACTOR").ok()) {
        cfg.actor_id = a;
    }
    Ok(cfg)
}
