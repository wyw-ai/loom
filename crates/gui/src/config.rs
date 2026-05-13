//! Local GUI config.
//!
//! Layout: `~/.joi-apps/desktop.toml`
//!
//! ```toml
//! active = "default"
//!
//! [account]
//! provider = "buc"
//! staff_id = "<staff_id>"
//! nickname = "bojun"
//! real_name = "Bo Jun"
//! email = "bojun@example.com"
//! actor_id = "actor_human_<staff_id>"
//! avatar_url = "//work.alibaba-inc.com/photo/<staff_id>.140x140.jpg"
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

use agent_runtime::discovery::AgentProviderOverride;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const ENV_CONFIG_DIR: &str = "JOI_CONFIG_DIR";

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
/// This is profile-only for now: BUC OAuth tokens are used only during login
/// and are not stored here. The provider field keeps the shape open for future
/// authentication providers without making workspace identity provider-specific.
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_actor_id: Option<String>,
    pub id: String,
    pub name: String,
    #[serde(default = "default_machine_kind")]
    pub kind: String,
    pub data_root: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub providers: Vec<AgentProviderOverride>,
    #[serde(default)]
    pub agents: Vec<MachineAgentConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineAgentConfig {
    pub provider_id: String,
    pub actor_id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub reasoning_effort: String,
    #[serde(default)]
    pub autostart: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DesktopConfig {
    #[serde(default)]
    pub active: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<HumanAccount>,
    #[serde(default)]
    pub workspaces: Vec<Workspace>,
    #[serde(default)]
    pub machines: Vec<MachineConfig>,
}

pub fn config_dir() -> PathBuf {
    if let Some(value) = std::env::var_os(ENV_CONFIG_DIR).filter(|value| !value.is_empty()) {
        return PathBuf::from(value);
    }
    dirs::home_dir()
        .map(|home| home.join(".joi-apps"))
        .unwrap_or_else(|| PathBuf::from(".joi-apps"))
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
            return Ok(with_default_machines(cfg));
        }
    }
    // First-run seeding: if the TUI has already been used, mirror its profile
    // as the initial "Local" workspace so the GUI picker is non-empty.
    let mut cfg = DesktopConfig::default();
    if let Some(seed) = try_seed_from_cli() {
        cfg.active = Some(seed.id.clone());
        cfg.workspaces.push(seed);
    }
    cfg = with_default_machines(cfg);
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
    let Some(legacy_root) = legacy_config_dir() else {
        return;
    };
    let new_root = config_dir();
    copy_legacy_file(&legacy_root, &new_root, "cli.toml");
    copy_legacy_file(&legacy_root, &new_root, "desktop.toml");
}

fn legacy_config_dir() -> Option<PathBuf> {
    dirs::config_dir()
        .map(|dir| dir.join("joi-apps"))
        .filter(|dir| dir != &config_dir())
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

pub fn generate_machine_id() -> String {
    format!("machine_{}", &Uuid::new_v4().simple().to_string()[..8])
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

pub fn human_actor_id_for_staff_id(staff_id: &str) -> String {
    format!("actor_human_{}", safe_config_key(staff_id.trim()))
}

pub fn human_avatar_url(staff_id: &str) -> String {
    format!(
        "//work.alibaba-inc.com/photo/{}.140x140.jpg",
        staff_id.trim()
    )
}

pub fn normalize_human_account(mut account: HumanAccount) -> HumanAccount {
    account.provider = first_non_empty([account.provider.as_str(), "unknown"]).to_string();
    account.staff_id = account.staff_id.trim().to_string();
    account.nickname = account.nickname.trim().to_string();
    account.real_name = account.real_name.trim().to_string();
    account.email = account.email.trim().to_string();
    account.actor_id = human_actor_id_for_staff_id(&account.staff_id);
    account.avatar_url = human_avatar_url(&account.staff_id);
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

pub fn default_agent_data_root() -> PathBuf {
    dirs::home_dir()
        .map(|d| d.join(".agentx"))
        .unwrap_or_else(|| PathBuf::from(".agentx"))
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

pub fn home_path_expr(path: &Path) -> String {
    abbreviate_home(path).unwrap_or_else(|| path.display().to_string())
}

pub fn default_agent_data_root_expr() -> String {
    "~/.agentx".into()
}

pub fn machine_data_root_expr(workspace_key: &str, machine_key: &str) -> String {
    format!("~/.agentx/machines/{workspace_key}/{machine_key}")
}

fn abbreviate_home(path: &Path) -> Option<String> {
    let home = dirs::home_dir()?;
    let rest = path.strip_prefix(home).ok()?;
    if rest.as_os_str().is_empty() {
        Some("~".into())
    } else {
        Some(format!("~/{}", rest.display()))
    }
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

    let active_workspace_id = active_workspace_id(cfg);
    let mut owners = cfg
        .machines
        .iter()
        .filter(|machine| machine.workspace_id.as_deref() == active_workspace_id)
        .filter_map(|machine| machine.owner_actor_id.as_deref())
        .map(str::trim)
        .filter(|owner| !owner.is_empty())
        .collect::<Vec<_>>();
    owners.sort_unstable();
    owners.dedup();
    if owners.len() == 1 {
        Some(owners[0].to_string())
    } else {
        None
    }
}

pub fn machine_belongs_to_active_workspace(machine: &MachineConfig, cfg: &DesktopConfig) -> bool {
    let owner_actor_id = active_owner_actor_id(cfg);
    machine_belongs_to_workspace_and_owner(
        machine,
        active_workspace_id(cfg),
        owner_actor_id.as_deref(),
    )
}

pub fn machine_belongs_to_workspace_and_owner(
    machine: &MachineConfig,
    workspace_id: Option<&str>,
    owner_actor_id: Option<&str>,
) -> bool {
    machine.workspace_id.as_deref() == workspace_id
        && machine.owner_actor_id.as_deref() == owner_actor_id
}

pub fn default_machine_for_workspace(
    workspace_id: &str,
    owner_actor_id: Option<&str>,
) -> MachineConfig {
    let workspace_key = safe_config_key(workspace_id);
    let suffix = workspace_key
        .strip_prefix("ws_")
        .unwrap_or(workspace_key.as_str());
    let owner_key = owner_actor_id.map(safe_config_key);
    let id_suffix = owner_key
        .as_deref()
        .map(|owner| format!("{suffix}_{owner}"))
        .unwrap_or_else(|| suffix.to_string());
    let data_key = owner_key
        .as_deref()
        .map(|owner| format!("{owner}/local"))
        .unwrap_or_else(|| "local".into());
    MachineConfig {
        workspace_id: Some(workspace_id.to_string()),
        owner_actor_id: owner_actor_id.map(ToString::to_string),
        id: format!("machine_{id_suffix}"),
        name: "Local Machine".into(),
        kind: default_machine_kind(),
        data_root: machine_data_root_expr(&workspace_key, &data_key),
        providers: Vec::new(),
        agents: Vec::new(),
    }
}

fn with_default_machines(mut cfg: DesktopConfig) -> DesktopConfig {
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

    for machine in &mut cfg.machines {
        if normalize_home_path_expr(&mut machine.data_root) {
            changed = true;
        }
    }

    if cfg.workspaces.is_empty() {
        if cfg.machines.is_empty() {
            cfg.machines.push(default_machine());
            changed = true;
        }
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

    for machine in &mut cfg.machines {
        if machine.workspace_id.is_none() {
            machine.workspace_id = Some(active_id.clone());
            changed = true;
        }
        if normalize_home_path_expr(&mut machine.data_root) {
            changed = true;
        }
    }

    let active_owner_actor_id = active_owner_actor_id(&cfg);
    let workspace_ids: Vec<String> = cfg
        .workspaces
        .iter()
        .map(|workspace| workspace.id.clone())
        .collect();
    for workspace_id in workspace_ids {
        if !cfg.machines.iter().any(|machine| {
            machine_belongs_to_workspace_and_owner(
                machine,
                Some(workspace_id.as_str()),
                active_owner_actor_id.as_deref(),
            )
        }) {
            cfg.machines.push(default_machine_for_workspace(
                &workspace_id,
                active_owner_actor_id.as_deref(),
            ));
            changed = true;
        }
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
            workspace.server_url = "ws://127.0.0.1:7878/rpc".into();
            changed = true;
        }
        if workspace.actor_id.trim().is_empty() {
            if let Some(actor_id) = account_actor_id.as_ref() {
                workspace.actor_id = actor_id.clone();
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

fn default_machine() -> MachineConfig {
    MachineConfig {
        workspace_id: None,
        owner_actor_id: None,
        id: "local".into(),
        name: "Local Machine".into(),
        kind: default_machine_kind(),
        data_root: default_agent_data_root_expr(),
        providers: Vec::new(),
        agents: Vec::new(),
    }
}

fn default_machine_kind() -> String {
    "local".into()
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

fn normalize_home_path_expr(value: &mut String) -> bool {
    let resolved = expand_home(value);
    if !resolved.is_absolute() {
        return false;
    }
    let next = home_path_expr(&resolved);
    if next == *value {
        false
    } else {
        *value = next;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_human_account_binds_actor_and_avatar_to_staff_id() {
        let account = normalize_human_account(HumanAccount {
            provider: "buc".into(),
            staff_id: " 12345 ".into(),
            nickname: " bojun ".into(),
            real_name: " Bo Jun ".into(),
            email: " bojun@example.com ".into(),
            actor_id: "actor_human_random".into(),
            avatar_url: "old".into(),
        });

        assert_eq!(account.staff_id, "12345");
        assert_eq!(account.actor_id, "actor_human_12345");
        assert_eq!(
            account.avatar_url,
            "//work.alibaba-inc.com/photo/12345.140x140.jpg"
        );
    }

    #[test]
    fn apply_account_identity_updates_workspace_human_identity() {
        let mut cfg = DesktopConfig {
            account: Some(normalize_human_account(HumanAccount {
                provider: "buc".into(),
                staff_id: "12345".into(),
                nickname: "bojun".into(),
                real_name: "Bo Jun".into(),
                email: "bojun@example.com".into(),
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
        assert_eq!(cfg.workspaces[0].actor_id, "actor_human_12345");
        assert_eq!(cfg.workspaces[0].display_name, "bojun");
    }

    #[test]
    fn machine_filter_requires_matching_account_owner() {
        let cfg = DesktopConfig {
            active: Some("default".into()),
            account: Some(normalize_human_account(HumanAccount {
                provider: "buc".into(),
                staff_id: "12345".into(),
                nickname: "bojun".into(),
                real_name: "Bo Jun".into(),
                email: "bojun@example.com".into(),
                actor_id: String::new(),
                avatar_url: String::new(),
            })),
            workspaces: vec![Workspace {
                id: "default".into(),
                name: "Local".into(),
                server_url: "ws://127.0.0.1:7878/rpc".into(),
                actor_id: "actor_human_12345".into(),
                display_name: "bojun".into(),
            }],
            ..DesktopConfig::default()
        };
        let machine = default_machine_for_workspace("default", Some("actor_human_12345"));
        let legacy_machine = MachineConfig {
            owner_actor_id: None,
            ..machine.clone()
        };
        let other_owner_machine = MachineConfig {
            owner_actor_id: Some("actor_human_other".into()),
            ..machine.clone()
        };

        assert!(machine_belongs_to_active_workspace(&machine, &cfg));
        assert!(!machine_belongs_to_active_workspace(&legacy_machine, &cfg));
        assert!(!machine_belongs_to_active_workspace(
            &other_owner_machine,
            &cfg
        ));
    }

    #[test]
    fn machine_filter_can_fallback_to_unique_machine_owner() {
        let cfg = DesktopConfig {
            active: Some("default".into()),
            account: None,
            workspaces: vec![Workspace {
                id: "default".into(),
                name: "Local".into(),
                server_url: "ws://127.0.0.1:7878/rpc".into(),
                actor_id: String::new(),
                display_name: String::new(),
            }],
            machines: vec![default_machine_for_workspace(
                "default",
                Some("actor_human_88084"),
            )],
        };
        let machine = default_machine_for_workspace("default", Some("actor_human_88084"));
        let other_owner_machine = MachineConfig {
            owner_actor_id: Some("actor_human_other".into()),
            ..machine.clone()
        };

        assert_eq!(
            active_owner_actor_id(&cfg).as_deref(),
            Some("actor_human_88084")
        );
        assert!(machine_belongs_to_active_workspace(&machine, &cfg));
        assert!(!machine_belongs_to_active_workspace(
            &other_owner_machine,
            &cfg
        ));
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
    }

    #[test]
    fn account_owned_default_machine_uses_owner_scoped_id_and_path() {
        let machine = default_machine_for_workspace("ws_local", Some("actor_human_12345"));

        assert_eq!(machine.owner_actor_id.as_deref(), Some("actor_human_12345"));
        assert_eq!(machine.id, "machine_local_actor_human_12345");
        assert_eq!(
            machine.data_root,
            "~/.agentx/machines/ws_local/actor_human_12345/local"
        );
    }
}
