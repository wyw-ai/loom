//! Local GUI config.
//!
//! Layout: `~/.loom-apps/desktop.toml`
//!
//! Daemon runtime configs are written separately below
//! `~/.loom-apps/d/<workspace>/<owner-short>/<machine>/desktop.toml`.
//! The GUI can track multiple server profiles in one desktop config, while a
//! daemon should see only the workspace/machine it is launched for.
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

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use agent_runtime::discovery::AgentProviderOverride;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const ENV_CONFIG_DIR: &str = "LOOM_CONFIG_DIR";
const LEGACY_ENV_CONFIG_DIR: &str = "JOI_CONFIG_DIR";

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
    #[serde(default)]
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
    #[serde(default)]
    pub machines: Vec<MachineConfig>,
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

pub fn daemon_config_dir_for_machine(machine: &MachineConfig) -> PathBuf {
    let workspace_key = machine
        .workspace_id
        .as_deref()
        .map(|value| short_config_key(value, 16))
        .unwrap_or_else(|| "unassigned".into());
    let owner_key = machine
        .owner_actor_id
        .as_deref()
        .map(|value| short_config_key(value, 16))
        .unwrap_or_else(|| "unowned".into());
    let machine_key = short_config_key(&machine.id, 24);
    config_dir()
        .join("d")
        .join(workspace_key)
        .join(owner_key)
        .join(machine_key)
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
    let owner_key = owner_actor_id.map(safe_config_key);
    let data_key = owner_key
        .as_deref()
        .map(|owner| format!("{owner}/local"))
        .unwrap_or_else(|| "local".into());
    MachineConfig {
        workspace_id: Some(workspace_id.to_string()),
        owner_actor_id: owner_actor_id.map(ToString::to_string),
        id: default_machine_id_for_workspace(workspace_id),
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
    if cleanup_machine_configs(&mut cfg) {
        changed = true;
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

fn default_machine_id_for_workspace(workspace_id: &str) -> String {
    let workspace_key = safe_config_key(workspace_id);
    let suffix = workspace_key
        .strip_prefix("ws_")
        .unwrap_or(workspace_key.as_str());
    format!("machine_{suffix}")
}

fn cleanup_machine_configs(cfg: &mut DesktopConfig) -> bool {
    let before_len = cfg.machines.len();
    let active_workspace = active_workspace_id(cfg).map(ToString::to_string);
    let active_owner = active_owner_actor_id(cfg);
    let known_workspaces = cfg
        .workspaces
        .iter()
        .map(|workspace| workspace.id.clone())
        .collect::<HashSet<_>>();
    let mut changed = false;
    let mut cleaned: Vec<MachineConfig> = Vec::new();

    for mut machine in std::mem::take(&mut cfg.machines) {
        if machine.workspace_id.is_none() && !cfg.workspaces.is_empty() {
            machine.workspace_id = active_workspace.clone();
            changed = true;
        }
        if let Some(workspace_id) = machine.workspace_id.as_deref() {
            if known_workspaces.contains(workspace_id) {
                if let Some(owner_actor_id) = active_owner.as_deref() {
                    if machine
                        .owner_actor_id
                        .as_deref()
                        .map(str::trim)
                        .filter(|owner| !owner.is_empty())
                        .is_none()
                    {
                        machine.owner_actor_id = Some(owner_actor_id.to_string());
                        changed = true;
                    }
                    if should_canonicalize_machine_id(&machine.id, workspace_id, owner_actor_id) {
                        let next_id = default_machine_id_for_workspace(workspace_id);
                        if machine.id != next_id {
                            machine.id = next_id;
                            changed = true;
                        }
                    }
                }
            }
        }

        let before_agents = machine.agents.len();
        machine
            .agents
            .retain(|agent| is_supported_agent_actor_id(&agent.actor_id));
        if machine.agents.len() != before_agents {
            changed = true;
        }

        if merge_machine_config(&mut cleaned, machine) {
            changed = true;
        }
    }

    changed |= cleaned.len() != before_len;
    cfg.machines = cleaned;
    changed
}

fn should_canonicalize_machine_id(
    machine_id: &str,
    workspace_id: &str,
    owner_actor_id: &str,
) -> bool {
    machine_id == "local"
        || machine_id == legacy_owner_scoped_machine_id(workspace_id, owner_actor_id)
        || is_legacy_owner_scoped_machine_id(machine_id)
}

fn legacy_owner_scoped_machine_id(workspace_id: &str, owner_actor_id: &str) -> String {
    let workspace_key = safe_config_key(workspace_id);
    let suffix = workspace_key
        .strip_prefix("ws_")
        .unwrap_or(workspace_key.as_str());
    format!("machine_{}_{}", suffix, safe_config_key(owner_actor_id))
}

fn is_legacy_owner_scoped_machine_id(machine_id: &str) -> bool {
    machine_id.starts_with("machine_") && machine_id.contains("_actor_human_")
}

fn is_supported_agent_actor_id(actor_id: &str) -> bool {
    let trimmed = actor_id.trim();
    !trimmed.is_empty()
        && trimmed.len() <= 64
        && trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':'))
}

fn merge_machine_config(machines: &mut Vec<MachineConfig>, machine: MachineConfig) -> bool {
    let Some(existing) = machines.iter_mut().find(|existing| {
        existing.workspace_id == machine.workspace_id
            && existing.owner_actor_id == machine.owner_actor_id
            && existing.id == machine.id
    }) else {
        machines.push(machine);
        return false;
    };

    if existing.name.trim().is_empty() || existing.name == "Local Machine" {
        existing.name = machine.name;
    }
    if existing.kind.trim().is_empty() {
        existing.kind = machine.kind;
    }
    if existing.data_root.trim().is_empty() || existing.data_root == default_agent_data_root_expr()
    {
        existing.data_root = machine.data_root;
    }
    for provider in machine.providers {
        if !existing
            .providers
            .iter()
            .any(|existing_provider| existing_provider.id == provider.id)
        {
            existing.providers.push(provider);
        }
    }
    for agent in machine.agents {
        if !existing
            .agents
            .iter()
            .any(|existing_agent| existing_agent.actor_id == agent.actor_id)
        {
            existing.agents.push(agent);
        }
    }
    true
}

fn repair_workspace_fields(cfg: &mut DesktopConfig) -> bool {
    let mut changed = false;
    let account_display = cfg.account.as_ref().map(account_display_name);
    let account_actor_id = active_account_actor_id(cfg).map(ToString::to_string);
    let workspace_owner_fallbacks = cfg
        .workspaces
        .iter()
        .map(|workspace| {
            (
                workspace.id.clone(),
                unique_machine_owner_for_workspace(cfg, &workspace.id),
            )
        })
        .collect::<Vec<_>>();
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
            } else if let Some((_, Some(actor_id))) = workspace_owner_fallbacks
                .iter()
                .find(|(workspace_id, _)| workspace_id == &workspace.id)
            {
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

fn unique_machine_owner_for_workspace(cfg: &DesktopConfig, workspace_id: &str) -> Option<String> {
    let mut owners = cfg
        .machines
        .iter()
        .filter(|machine| machine.workspace_id.as_deref() == Some(workspace_id))
        .filter_map(|machine| machine.owner_actor_id.as_deref())
        .map(str::trim)
        .filter(|owner| !owner.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    owners.sort();
    owners.dedup();
    if owners.len() == 1 {
        owners.pop()
    } else {
        None
    }
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

fn short_config_key(value: &str, max_prefix_len: usize) -> String {
    let key = safe_config_key(value);
    if key.len() <= max_prefix_len {
        return key;
    }
    format!("{}_{}", &key[..max_prefix_len], short_hash(value))
}

fn short_hash(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    digest[..4]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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
    fn machine_filter_requires_matching_account_owner() {
        let cfg = DesktopConfig {
            active: Some("default".into()),
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
                actor_id: "actor_human_github_12345".into(),
                display_name: "octocat".into(),
            }],
            ..DesktopConfig::default()
        };
        let machine = default_machine_for_workspace("default", Some("actor_human_github_12345"));
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
        assert_eq!(cfg.workspaces[0].actor_id, "actor_human_local_default");
        assert_eq!(cfg.workspaces[0].display_name, "actor_human_local_default");
    }

    #[test]
    fn account_owned_default_machine_uses_owner_scoped_id_and_path() {
        let machine = default_machine_for_workspace("ws_local", Some("actor_human_github_12345"));

        assert_eq!(
            machine.owner_actor_id.as_deref(),
            Some("actor_human_github_12345")
        );
        assert_eq!(machine.id, "machine_local");
        assert_eq!(
            machine.data_root,
            "~/.agentx/machines/ws_local/actor_human_github_12345/local"
        );
    }

    #[test]
    fn daemon_config_dir_is_scoped_and_short_enough_for_unix_socket() {
        let machine = MachineConfig {
            workspace_id: Some("ws_2ca56331".into()),
            owner_actor_id: Some("actor_human_local_ws_2ca56331".into()),
            id: "machine_c3a89b06".into(),
            name: "CanfengMac".into(),
            kind: "local".into(),
            data_root: "~/.agentx".into(),
            providers: Vec::new(),
            agents: Vec::new(),
        };

        let dir = daemon_config_dir_for_machine(&machine);
        let path = dir.join("daemon").join("daemon.sock");
        assert!(path.display().to_string().len() < 104);
        assert!(dir.ends_with("d/ws_2ca56331/actor_human_loca_120ccc37/machine_c3a89b06"));
    }

    #[test]
    fn load_rewrites_legacy_owner_scoped_machine_ids() {
        let cfg = DesktopConfig {
            active: Some("ws_abbb0e0b".into()),
            account: None,
            workspaces: vec![Workspace {
                id: "ws_abbb0e0b".into(),
                name: "Local".into(),
                server_url: "ws://127.0.0.1:7878/rpc".into(),
                actor_id: "actor_human_local_ws_abbb0e0b".into(),
                display_name: "boyd".into(),
            }],
            machines: vec![
                MachineConfig {
                    workspace_id: Some("ws_abbb0e0b".into()),
                    owner_actor_id: None,
                    id: "local".into(),
                    name: "Local Machine".into(),
                    kind: "local".into(),
                    data_root: "~/.agentx".into(),
                    providers: Vec::new(),
                    agents: Vec::new(),
                },
                MachineConfig {
                    workspace_id: Some("ws_abbb0e0b".into()),
                    owner_actor_id: Some("actor_human_local_ws_abbb0e0b".into()),
                    id: "machine_abbb0e0b_actor_human_local_ws_abbb0e0b".into(),
                    name: "Local Machine".into(),
                    kind: "local".into(),
                    data_root: "~/.agentx/machines/ws_abbb0e0b/actor_human_local_ws_abbb0e0b/local"
                        .into(),
                    providers: Vec::new(),
                    agents: vec![MachineAgentConfig {
                        provider_id: "codex".into(),
                        actor_id:
                            "actor_agent_machine_abbb0e0b_actor_human_local_ws_abbb0e0b_45b7a479"
                                .into(),
                        name: "legacy".into(),
                        description: String::new(),
                        model: String::new(),
                        reasoning_effort: String::new(),
                        autostart: false,
                        avatar_url: String::new(),
                    }],
                },
            ],
        };

        let mut cfg = cfg;
        assert!(cleanup_machine_configs(&mut cfg));

        assert_eq!(cfg.machines.len(), 1);
        assert_eq!(cfg.machines[0].id, "machine_abbb0e0b");
        assert_eq!(
            cfg.machines[0].owner_actor_id.as_deref(),
            Some("actor_human_local_ws_abbb0e0b")
        );
        assert!(cfg.machines[0].agents.is_empty());
    }
}
