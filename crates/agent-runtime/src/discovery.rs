//! Local agent CLI discovery and provider-spec synthesis.
//!
//! This module is intentionally small and side-effect free except for reading
//! `PATH`. The GUI uses it to present supported local CLIs, while `joi daemon`
//! uses the same provider profiles to build runtime `AgentSpec`s from machine
//! config without requiring on-disk provider JSON specs.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use proto::methods::{
    AgentActorDefaults, AgentActorSpec, AgentModelChoice, AgentProviderInfo, AgentProviderSpec,
    AgentTransport, CommandOutputFormat, IdentityFiles, IdentityScaffoldSpec, IdentitySpec,
    PromptVia,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedAgentProvider {
    pub id: String,
    pub display_name: String,
    pub command: String,
    pub transport_kind: String,
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    #[serde(default)]
    pub model_choices: Vec<AgentModelChoice>,
}

#[derive(Debug, Clone)]
pub struct AgentDefinition {
    pub provider_id: String,
    pub actor_id: String,
    pub display_name: String,
    pub description: Option<String>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub autostart: bool,
}

struct ProviderDef {
    id: &'static str,
    display_name: &'static str,
    candidates: &'static [&'static str],
    args: &'static [&'static str],
}

const PROVIDER_DEFS: &[ProviderDef] = &[
    ProviderDef {
        id: "claude",
        display_name: "Claude Code",
        candidates: &["claude"],
        args: &["-p"],
    },
    ProviderDef {
        id: "qoder",
        display_name: "Qoder CLI",
        candidates: &["qodercli"],
        args: &["-p"],
    },
    ProviderDef {
        id: "copilot",
        display_name: "GitHub Copilot CLI",
        candidates: &["copilot", "copilotcli"],
        args: &["-p"],
    },
    ProviderDef {
        id: "codex",
        display_name: "Codex CLI",
        candidates: &["codex", "codexcli"],
        args: &["exec", "--skip-git-repo-check"],
    },
];

pub fn detect_agent_cli_providers() -> Vec<DetectedAgentProvider> {
    detect_agent_cli_providers_in_path_with_config_dir(
        std::env::var_os("PATH").unwrap_or_default(),
        &joi_config_dir(),
    )
}

pub fn provider_specs_from_agent_definitions(
    providers: &[DetectedAgentProvider],
    definitions: &[AgentDefinition],
) -> Vec<AgentProviderSpec> {
    let mut specs = Vec::new();
    for provider in providers {
        let actors = definitions
            .iter()
            .filter(|definition| definition.provider_id == provider.id)
            .map(|definition| actor_spec_from_definition(provider, definition))
            .collect::<Vec<_>>();
        if actors.is_empty() {
            continue;
        }
        specs.push(provider.to_provider_spec(actors));
    }
    specs
}

impl DetectedAgentProvider {
    pub fn transport(&self) -> AgentTransport {
        AgentTransport {
            kind: self.transport_kind.clone(),
            command: self.command.clone(),
            args: self.args.clone(),
            env: BTreeMap::new(),
            auth_method: None,
            model: self.default_model.clone(),
            session: None,
            output_format: Some(CommandOutputFormat::Text),
            prompt_via: PromptVia::Args,
            interactive: None,
            provider: None,
        }
    }

    pub fn to_provider_spec(&self, actors: Vec<AgentActorSpec>) -> AgentProviderSpec {
        AgentProviderSpec {
            provider: AgentProviderInfo {
                id: self.id.clone(),
                display_name: self.display_name.clone(),
            },
            transport: self.transport(),
            defaults: AgentActorDefaults {
                autostart: false,
                models: None,
                bundle: None,
                identity: None,
                memory: None,
                announcement: None,
            },
            actors,
        }
    }
}

fn actor_spec_from_definition(
    provider: &DetectedAgentProvider,
    definition: &AgentDefinition,
) -> AgentActorSpec {
    let mut meta = BTreeMap::new();
    meta.insert("providerId".into(), json!(provider.id.clone()));
    meta.insert("providerName".into(), json!(provider.display_name.clone()));
    meta.insert(
        "transportKind".into(),
        json!(provider.transport_kind.clone()),
    );
    meta.insert("createdBy".into(), json!("joi-daemon"));
    if let Some(reasoning_effort) = definition
        .reasoning_effort
        .as_deref()
        .map(str::trim)
        .filter(|reasoning_effort| !reasoning_effort.is_empty())
    {
        meta.insert("reasoningEffort".into(), json!(reasoning_effort));
    }

    AgentActorSpec {
        id: definition.actor_id.clone(),
        display_name: Some(definition.display_name.clone()),
        capabilities: None,
        meta: Some(meta),
        transport: None,
        autostart: Some(definition.autostart),
        model: definition.model.clone(),
        models: None,
        bundle: None,
        identity: definition
            .description
            .as_ref()
            .map(|description| IdentitySpec {
                files: IdentityFiles::default(),
                description: Some(description.clone()),
                scaffold: Some(IdentityScaffoldSpec {
                    identity: Some(description.clone()),
                    soul: None,
                }),
            }),
        memory: None,
        announcement: None,
    }
}

fn detect_agent_cli_providers_in_path_with_config_dir(
    path: OsString,
    config_dir: &Path,
) -> Vec<DetectedAgentProvider> {
    PROVIDER_DEFS
        .iter()
        .filter_map(|def| {
            let command = find_command_in_path(def.candidates, &path)?;
            Some(DetectedAgentProvider {
                id: def.id.into(),
                display_name: def.display_name.into(),
                command: command.display().to_string(),
                transport_kind: "command".into(),
                args: provider_args(def, config_dir),
                default_model: None,
                model_choices: Vec::new(),
            })
        })
        .collect()
}

fn provider_args(def: &ProviderDef, config_dir: &Path) -> Vec<String> {
    let joi_config_dir = absolute_path(config_dir);
    let mut args = Vec::new();
    match def.id {
        "claude" => {
            append_add_dir_arg(&mut args, &joi_config_dir);
            args.push("--allowed-tools".into());
            args.push("Bash(joi *)".into());
            args.extend(def.args.iter().map(|arg| (*arg).to_string()));
        }
        "qoder" => {
            append_add_dir_arg(&mut args, &joi_config_dir);
            args.push("--allowed-tools".into());
            args.push("Bash(joi *)".into());
            args.extend(def.args.iter().map(|arg| (*arg).to_string()));
        }
        "copilot" => {
            append_add_dir_arg(&mut args, &joi_config_dir);
            args.push("--allow-tool=shell(joi:*)".into());
            args.extend(def.args.iter().map(|arg| (*arg).to_string()));
        }
        "codex" => {
            args.extend(def.args.iter().map(|arg| (*arg).to_string()));
            append_add_dir_arg(&mut args, &joi_config_dir);
        }
        _ => args.extend(def.args.iter().map(|arg| (*arg).to_string())),
    }
    args
}

fn append_add_dir_arg(args: &mut Vec<String>, dir: &Path) {
    args.push("--add-dir".into());
    args.push(dir.display().to_string());
}

fn joi_config_dir() -> PathBuf {
    if let Some(value) = std::env::var_os("JOI_CONFIG_DIR").filter(|value| !value.is_empty()) {
        return PathBuf::from(value);
    }
    if let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(home).join(".joi-apps");
    }
    PathBuf::from(".joi-apps")
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(path)
}

fn find_command_in_path(candidates: &[&str], path: &OsString) -> Option<PathBuf> {
    let path_dirs = std::env::split_paths(path).collect::<Vec<_>>();
    for candidate in candidates {
        let candidate_path = Path::new(candidate);
        if candidate_path.components().count() > 1 && is_executable(candidate_path) {
            return Some(candidate_path.to_path_buf());
        }
        for dir in &path_dirs {
            let path = dir.join(candidate);
            if is_executable(&path) {
                return Some(path);
            }
        }
    }
    None
}

fn is_executable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "joi-discovery-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock drift")
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).expect("create temp dir");
        path
    }

    #[cfg(unix)]
    fn make_executable(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(path, "#!/bin/sh\n").expect("write executable");
        let mut perms = std::fs::metadata(path).expect("metadata").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms).expect("chmod");
    }

    #[cfg(not(unix))]
    fn make_executable(path: &Path) {
        std::fs::write(path, "").expect("write executable");
    }

    #[test]
    fn detects_supported_cli_binaries_from_path() {
        let dir = temp_dir("path");
        let config_dir = temp_dir("config");
        make_executable(&dir.join("codex"));
        make_executable(&dir.join("copilot"));
        make_executable(&dir.join("qodercli"));

        let providers = detect_agent_cli_providers_in_path_with_config_dir(
            dir.clone().into_os_string(),
            &config_dir,
        );
        let ids = providers.iter().map(|p| p.id.as_str()).collect::<Vec<_>>();
        let config_dir_arg = config_dir.display().to_string();
        let add_dir = vec!["--add-dir", config_dir_arg.as_str()];

        assert_eq!(ids, vec!["qoder", "copilot", "codex"]);
        let qoder = providers
            .iter()
            .find(|provider| provider.id == "qoder")
            .expect("qoder provider");
        let mut expected = add_dir.clone();
        expected.extend(["--allowed-tools", "Bash(joi *)"]);
        expected.push("-p");
        assert_eq!(qoder.args, expected);
        let copilot = providers
            .iter()
            .find(|provider| provider.id == "copilot")
            .expect("copilot provider");
        let mut expected = add_dir.clone();
        expected.push("--allow-tool=shell(joi:*)");
        expected.push("-p");
        assert_eq!(copilot.args, expected);
        let codex = providers
            .iter()
            .find(|provider| provider.id == "codex")
            .expect("codex provider");
        let mut expected = vec!["exec", "--skip-git-repo-check"];
        expected.extend(add_dir);
        assert_eq!(codex.args, expected);
        std::fs::remove_dir_all(dir).ok();
        std::fs::remove_dir_all(config_dir).ok();
    }

    #[test]
    fn builds_provider_specs_from_machine_agent_definitions() {
        let provider = DetectedAgentProvider {
            id: "codex".into(),
            display_name: "Codex CLI".into(),
            command: "/bin/codex".into(),
            transport_kind: "command".into(),
            args: vec![
                "exec".into(),
                "--skip-git-repo-check".into(),
                "--add-dir".into(),
                "/tmp/joi-config".into(),
            ],
            default_model: None,
            model_choices: Vec::new(),
        };
        let specs = provider_specs_from_agent_definitions(
            &[provider],
            &[AgentDefinition {
                provider_id: "codex".into(),
                actor_id: "actor_agent_builder".into(),
                display_name: "Builder".into(),
                description: Some("Builds patches".into()),
                model: None,
                reasoning_effort: None,
                autostart: true,
            }],
        );

        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].provider.id, "codex");
        assert_eq!(specs[0].actors[0].id, "actor_agent_builder");
        assert_eq!(
            specs[0].transport.args,
            vec![
                "exec",
                "--skip-git-repo-check",
                "--add-dir",
                "/tmp/joi-config"
            ]
        );
    }
}
