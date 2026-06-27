//! Local GUI config.
//!
//! Layout: `~/.loom-apps/desktop.toml`
//!
//! The GUI desktop config is a local connection profile: account identity,
//! server profiles, and the active server. Machines and agents are discovered
//! from the connected server inventory instead of treated as local truth.
//!
//! Daemon runtime config is deliberately separate. A daemon reads its own
//! `daemon.toml` from `LOOM_CONFIG_DIR`; this GUI file does not own daemon
//! machine identity, providers, agents, profile, memory, or runtime state.
//!
//! ```toml
//! active = "default"
//!
//! [account]
//! provider = "github"
//! staff_id = "<provider_subject>"
//! nickname = "octocat"
//! real_name = "Octo Cat"
//! email = "octocat@example.com"
//! actor_id = "actor_human_github_<provider_subject>"
//! avatar_url = "https://avatars.githubusercontent.com/u/..."
//!
//! [[workspaces]]
//! id = "default"
//! name = "Local"
//! server_url = "ws://127.0.0.1:7878/rpc"
//! actor_id = "actor_human_<staff_id>"
//! display_name = "bojun"
//! ```
//!
//! We deliberately keep this file separate from the TUI's `cli.toml`. The
//! TUI only knows about a single profile, and forcing it to share storage
//! with the GUI's multi-workspace world would risk a migration nightmare.
//! On first launch we migrate the TUI's `cli.toml` into a starter workspace
//! named "Local" so the operator doesn't face an empty picker.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

pub const ENV_CONFIG_DIR: &str = "LOOM_GUI_CONFIG_DIR";
const LEGACY_ENV_CONFIG_DIR: &str = "JOI_GUI_CONFIG_DIR";
pub const DEFAULT_SERVER_URL: &str = "ws://127.0.0.1:7878/rpc";
pub const DEFAULT_SERVER_PORT: u16 = 7878;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub server_url: String,
    #[serde(default)]
    pub actor_id: String,
    #[serde(default)]
    pub display_name: String,
}

/// Locally persisted human identity.
///
/// This is profile-only for now: OAuth tokens are used only during login and
/// are not stored here. `staff_id` is retained as the serialized profile
/// subject for backward compatibility with existing desktop configs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HumanAccount {
    pub provider: String,
    pub staff_id: String,
    pub nickname: String,
    pub real_name: String,
    pub email: String,
    pub actor_id: String,
    pub avatar_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DesktopConfig {
    #[serde(default)]
    pub active: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<HumanAccount>,
    #[serde(default)]
    pub workspaces: Vec<Workspace>,
}

pub fn config_dir() -> PathBuf {
    if let Some(value) = std::env::var_os(ENV_CONFIG_DIR).filter(|value| !value.is_empty()) {
        return PathBuf::from(value);
    }
    if let Some(value) = std::env::var_os(LEGACY_ENV_CONFIG_DIR).filter(|value| !value.is_empty()) {
        return PathBuf::from(value);
    }
    dirs::home_dir()
        .map(|home| home.join(".loom-apps"))
        .unwrap_or_else(|| PathBuf::from(".loom-apps"))
}

pub fn desktop_config_path() -> PathBuf {
    config_dir().join("desktop.toml")
}

fn legacy_cli_config_path() -> PathBuf {
    config_dir().join("cli.toml")
}

pub fn load_or_init() -> Result<DesktopConfig> {
    migrate_legacy_configs();
    let path = desktop_config_path();
    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Ok(cfg) = toml::from_str::<DesktopConfig>(&text) {
            let cfg = normalize_desktop_config(cfg);
            if has_legacy_desktop_machine_state(&text) {
                save(&cfg).ok();
            }
            return Ok(cfg);
        }
    }
    // First-run seeding: if the TUI has already been used, mirror its profile
    // as the initial "Local" workspace so the GUI picker is non-empty.
    let mut cfg = DesktopConfig::default();
    if let Some(seed) = try_seed_from_cli() {
        cfg.active = Some(seed.id.clone());
        cfg.workspaces.push(seed);
    }
    cfg = normalize_desktop_config(cfg);
    save(&cfg).ok();
    Ok(cfg)
}

pub fn save(cfg: &DesktopConfig) -> Result<()> {
    let _ = std::fs::create_dir_all(config_dir());
    let text = toml::to_string_pretty(cfg)?;
    std::fs::write(desktop_config_path(), text)?;
    Ok(())
}

fn migrate_legacy_configs() {
    let new_root = config_dir();
    for legacy_root in legacy_config_dirs() {
        copy_legacy_file(&legacy_root, &new_root, "cli.toml");
        copy_legacy_file(&legacy_root, &new_root, "desktop.toml");
    }
}

fn legacy_config_dirs() -> Vec<PathBuf> {
    let new_root = config_dir();
    let mut roots = Vec::new();
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join(".joi-apps"));
    }
    if let Some(config) = dirs::config_dir() {
        roots.push(config.join("joi-apps"));
    }
    roots.retain(|dir| dir != &new_root);
    roots.sort();
    roots.dedup();
    roots
}

fn copy_legacy_file(legacy_root: &Path, new_root: &Path, file_name: &str) {
    let source = legacy_root.join(file_name);
    let dest = new_root.join(file_name);
    if dest.exists() || !source.is_file() {
        return;
    }
    if let Some(parent) = dest.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::copy(source, dest);
}

fn has_legacy_desktop_machine_state(text: &str) -> bool {
    toml::from_str::<toml::Value>(text)
        .ok()
        .and_then(|value| value.as_table().cloned())
        .is_some_and(|table| table.contains_key("machines"))
}

fn try_seed_from_cli() -> Option<Workspace> {
    #[derive(Deserialize)]
    struct CliLike {
        server_url: String,
        actor_id: String,
        #[serde(default)]
        display_name: String,
    }
    let text = std::fs::read_to_string(legacy_cli_config_path()).ok()?;
    let parsed: CliLike = toml::from_str(&text).ok()?;
    Some(Workspace {
        id: "default".into(),
        name: "Local".into(),
        server_url: parsed.server_url,
        actor_id: parsed.actor_id,
        display_name: parsed.display_name,
    })
}

pub fn generate_id() -> String {
    format!("ws_{}", &Uuid::new_v4().simple().to_string()[..8])
}

pub fn normalize_workspace_server_url(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(DEFAULT_SERVER_URL.to_string());
    }

    let candidate = if let Some(idx) = trimmed.find("://") {
        let scheme = trimmed[..idx].to_ascii_lowercase();
        let rest = &trimmed[idx + 3..];
        match scheme.as_str() {
            "wss" | "https" | "rpcs" => format!("wss://{rest}"),
            "ws" | "http" | "rpc" => format!("ws://{rest}"),
            other => return Err(anyhow!("server URL scheme `{other}` is not supported")),
        }
    } else {
        format!("ws://{trimmed}")
    };

    let mut url =
        Url::parse(&candidate).map_err(|err| anyhow!("invalid server URL `{raw}`: {err}"))?;
    match url.scheme() {
        "ws" | "wss" => {}
        other => return Err(anyhow!("server URL scheme `{other}` is not supported")),
    }
    if url.host_str().is_none() {
        return Err(anyhow!("server host is required"));
    }
    if url.port().is_none() {
        url.set_port(Some(DEFAULT_SERVER_PORT))
            .map_err(|_| anyhow!("invalid server port"))?;
    }
    if url.path().is_empty() || url.path() == "/" {
        url.set_path("/rpc");
    } else if !url.path().contains("/rpc") {
        let path = format!("/rpc{}", url.path().trim_end_matches('/'));
        url.set_path(&path);
    }

    Ok(url.to_string())
}

pub fn account_display_name(account: &HumanAccount) -> String {
    first_non_empty([
        account.nickname.as_str(),
        account.real_name.as_str(),
        account.staff_id.as_str(),
        account.actor_id.as_str(),
    ])
    .to_string()
}

pub fn human_actor_id_for_subject(provider: &str, subject: &str) -> String {
    format!(
        "actor_human_{}_{}",
        safe_config_key(provider.trim()),
        safe_config_key(subject.trim())
    )
}

pub fn normalize_human_account(mut account: HumanAccount) -> HumanAccount {
    account.provider = first_non_empty([account.provider.as_str(), "unknown"]).to_string();
    account.staff_id = account.staff_id.trim().to_string();
    account.nickname = account.nickname.trim().to_string();
    account.real_name = account.real_name.trim().to_string();
    account.email = account.email.trim().to_string();
    account.actor_id = human_actor_id_for_subject(&account.provider, &account.staff_id);
    account.avatar_url = account.avatar_url.trim().to_string();
    account
}

pub fn apply_account_identity(cfg: &mut DesktopConfig) -> bool {
    let Some(account) = cfg.account.as_ref() else {
        return false;
    };
    let display_name = account_display_name(account);
    let mut changed = false;
    for workspace in &mut cfg.workspaces {
        if workspace.actor_id != account.actor_id {
            workspace.actor_id = account.actor_id.clone();
            changed = true;
        }
        if workspace.display_name != display_name {
            workspace.display_name = display_name.clone();
            changed = true;
        }
    }
    changed
}

pub fn expand_home(value: &str) -> PathBuf {
    if value == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(value));
    }
    if let Some(rest) = value.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(value)
}

pub fn active_workspace_id(cfg: &DesktopConfig) -> Option<&str> {
    cfg.active
        .as_deref()
        .and_then(|id| cfg.workspaces.iter().find(|workspace| workspace.id == id))
        .or_else(|| cfg.workspaces.first())
        .map(|workspace| workspace.id.as_str())
}

pub fn active_account_actor_id(cfg: &DesktopConfig) -> Option<&str> {
    cfg.account
        .as_ref()
        .map(|account| account.actor_id.as_str())
}

pub fn active_owner_actor_id(cfg: &DesktopConfig) -> Option<String> {
    if let Some(actor_id) = active_account_actor_id(cfg)
        .map(str::trim)
        .filter(|actor_id| !actor_id.is_empty())
    {
        return Some(actor_id.to_string());
    }
    if let Some(actor_id) = active_workspace_id(cfg)
        .and_then(|id| cfg.workspaces.iter().find(|workspace| workspace.id == id))
        .map(|workspace| workspace.actor_id.trim())
        .filter(|actor_id| !actor_id.is_empty())
    {
        return Some(actor_id.to_string());
    }
    None
}

fn normalize_desktop_config(mut cfg: DesktopConfig) -> DesktopConfig {
    let mut changed = false;

    if let Some(account) = cfg.account.clone() {
        let normalized = normalize_human_account(account);
        if cfg.account.as_ref() != Some(&normalized) {
            cfg.account = Some(normalized);
            changed = true;
        }
    }
    if apply_account_identity(&mut cfg) {
        changed = true;
    }

    if cfg.workspaces.is_empty() {
        if changed {
            save(&cfg).ok();
        }
        return cfg;
    }

    let active_id = active_workspace_id(&cfg)
        .map(ToString::to_string)
        .expect("workspaces is not empty");
    if cfg.active.as_deref() != Some(active_id.as_str()) {
        cfg.active = Some(active_id.clone());
        changed = true;
    }
    if repair_workspace_fields(&mut cfg) {
        changed = true;
    }

    if changed {
        save(&cfg).ok();
    }
    cfg
}

fn repair_workspace_fields(cfg: &mut DesktopConfig) -> bool {
    let mut changed = false;
    let account_display = cfg.account.as_ref().map(account_display_name);
    let account_actor_id = active_account_actor_id(cfg).map(ToString::to_string);
    for workspace in &mut cfg.workspaces {
        if workspace.name.trim().is_empty() {
            workspace.name = "Local".into();
            changed = true;
        }
        if workspace.server_url.trim().is_empty() {
            workspace.server_url = DEFAULT_SERVER_URL.into();
            changed = true;
        } else if let Ok(normalized) = normalize_workspace_server_url(&workspace.server_url) {
            if workspace.server_url != normalized {
                workspace.server_url = normalized;
                changed = true;
            }
        }
        if workspace.actor_id.trim().is_empty() {
            if let Some(actor_id) = account_actor_id.as_ref() {
                workspace.actor_id = actor_id.clone();
                changed = true;
            } else {
                workspace.actor_id = local_actor_id_for_workspace(&workspace.id);
                changed = true;
            }
        }
        if workspace.display_name.trim().is_empty() {
            if let Some(display_name) = account_display.as_ref() {
                workspace.display_name = display_name.clone();
                changed = true;
            } else if !workspace.actor_id.trim().is_empty() {
                workspace.display_name = workspace.actor_id.clone();
                changed = true;
            }
        }
    }
    changed
}

fn local_actor_id_for_workspace(workspace_id: &str) -> String {
    format!("actor_human_local_{}", safe_config_key(workspace_id))
}

fn safe_config_key(value: &str) -> String {
    let key: String = value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '-')
        .collect();
    if key.is_empty() {
        "workspace".into()
    } else {
        key
    }
}

fn first_non_empty<'a, const N: usize>(values: [&'a str; N]) -> &'a str {
    values
        .into_iter()
        .find(|value| !value.trim().is_empty())
        .map(str::trim)
        .unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_human_account_binds_actor_to_provider_subject() {
        let account = normalize_human_account(HumanAccount {
            provider: "github".into(),
            staff_id: " 12345 ".into(),
            nickname: " octocat ".into(),
            real_name: " Octo Cat ".into(),
            email: " octocat@example.com ".into(),
            actor_id: "actor_human_random".into(),
            avatar_url: " https://example.test/avatar.png ".into(),
        });

        assert_eq!(account.staff_id, "12345");
        assert_eq!(account.actor_id, "actor_human_github_12345");
        assert_eq!(account.avatar_url, "https://example.test/avatar.png");
    }

    #[test]
    fn apply_account_identity_updates_workspace_human_identity() {
        let mut cfg = DesktopConfig {
            account: Some(normalize_human_account(HumanAccount {
                provider: "github".into(),
                staff_id: "12345".into(),
                nickname: "octocat".into(),
                real_name: "Octo Cat".into(),
                email: "octocat@example.com".into(),
                actor_id: String::new(),
                avatar_url: String::new(),
            })),
            workspaces: vec![Workspace {
                id: "default".into(),
                name: "Local".into(),
                server_url: "ws://127.0.0.1:7878/rpc".into(),
                actor_id: "actor_human_old".into(),
                display_name: "old".into(),
            }],
            ..DesktopConfig::default()
        };

        assert!(apply_account_identity(&mut cfg));
        assert_eq!(cfg.workspaces[0].actor_id, "actor_human_github_12345");
        assert_eq!(cfg.workspaces[0].display_name, "octocat");
    }

    #[test]
    fn server_url_normalization_accepts_bare_host_inputs() {
        assert_eq!(
            normalize_workspace_server_url("loom.example.com").expect("normalize host"),
            "ws://loom.example.com:7878/rpc"
        );
        assert_eq!(
            normalize_workspace_server_url("192.168.1.20:9000").expect("normalize host port"),
            "ws://192.168.1.20:9000/rpc"
        );
    }

    #[test]
    fn server_url_normalization_maps_friendly_schemes_to_websocket_rpc() {
        assert_eq!(
            normalize_workspace_server_url("rpc://loom.example.com").expect("normalize rpc alias"),
            "ws://loom.example.com:7878/rpc"
        );
        assert_eq!(
            normalize_workspace_server_url("https://loom.example.com:9443").expect("normalize https"),
            "wss://loom.example.com:9443/rpc"
        );
    }

    #[test]
    fn workspace_fields_default_when_daemon_saved_lossy_config() {
        let cfg: DesktopConfig = toml::from_str(
            r#"
active = "default"

[[workspaces]]
id = "default"
"#,
        )
        .expect("parse lossy desktop config");

        let mut cfg = cfg;
        assert!(repair_workspace_fields(&mut cfg));
        assert_eq!(cfg.workspaces[0].name, "Local");
        assert_eq!(cfg.workspaces[0].server_url, "ws://127.0.0.1:7878/rpc");
        assert_eq!(cfg.workspaces[0].actor_id, "actor_human_local_default");
        assert_eq!(cfg.workspaces[0].display_name, "actor_human_local_default");
    }

    #[test]
    fn workspace_fields_normalize_non_empty_server_urls() {
        let mut cfg = DesktopConfig {
            workspaces: vec![Workspace {
                id: "default".into(),
                name: "Local".into(),
                server_url: "loom.example.com".into(),
                actor_id: "actor_human_local_default".into(),
                display_name: "Local".into(),
            }],
            ..DesktopConfig::default()
        };

        assert!(repair_workspace_fields(&mut cfg));
        assert_eq!(cfg.workspaces[0].server_url, "ws://loom.example.com:7878/rpc");
    }

    #[test]
    fn normalize_desktop_config_does_not_seed_gui_machine_truth() {
        let cfg = DesktopConfig {
            active: Some("default".into()),
            account: Some(normalize_human_account(HumanAccount {
                provider: "github".into(),
                staff_id: "12345".into(),
                nickname: "octocat".into(),
                real_name: String::new(),
                email: String::new(),
                actor_id: String::new(),
                avatar_url: String::new(),
            })),
            workspaces: vec![Workspace {
                id: "default".into(),
                name: "Local".into(),
                server_url: "ws://127.0.0.1:7878/rpc".into(),
                actor_id: "actor_human_github_12345".into(),
                display_name: "octocat".into(),
            }],
        };

        let cfg = normalize_desktop_config(cfg);

        assert_eq!(cfg.workspaces[0].actor_id, "actor_human_github_12345");
    }

    #[test]
    fn legacy_machine_fields_are_not_serialized_back_to_desktop_config() {
        let text = r#"
active = "default"

[[workspaces]]
id = "default"
name = "Local"
server_url = "ws://127.0.0.1:7878/rpc"
actor_id = "actor_human_local_default"
display_name = "Local"

[[machines]]
id = "machine_legacy"
name = "Legacy"
kind = "local"
data_root = "~/.agentx"
"#;
        let cfg: DesktopConfig = toml::from_str(text).expect("parse legacy desktop config");

        let text = toml::to_string_pretty(&normalize_desktop_config(cfg))
            .expect("serialize desktop config");

        assert!(!text.contains("[[machines]]"));
        assert!(!text.contains("machine_legacy"));
    }

    #[test]
    fn legacy_machine_state_is_detected_for_load_scrub() {
        assert!(has_legacy_desktop_machine_state(
            r#"
active = "default"

[[machines]]
id = "machine_legacy"
"#
        ));
        assert!(!has_legacy_desktop_machine_state(
            r#"
active = "default"

[[workspaces]]
id = "default"
"#
        ));
    }
}
