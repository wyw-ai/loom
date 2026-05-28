//! Local agent CLI discovery and provider-spec synthesis.
//!
//! This module is intentionally small and side-effect free except for reading
//! `PATH`. The GUI uses it to present supported local CLIs, while `loom-daemon`
//! uses the same provider profiles to build runtime `AgentSpec`s from machine
//! config without requiring on-disk provider JSON specs.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

use proto::methods::{
    AgentActorDefaults, AgentActorSpec, AgentModelChoice, AgentModelSpec, AgentProviderInfo,
    AgentProviderRef, AgentProviderSpec, AgentSpec, AgentTransport,
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
    #[serde(default, skip)]
    pub transport_env: BTreeMap<String, String>,
    #[serde(default, skip)]
    pub transport: AgentTransport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    #[serde(default)]
    pub model_choices: Vec<AgentModelChoice>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentProviderOverride {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
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
    pub avatar_url: Option<String>,
}

pub fn detect_agent_cli_providers() -> Vec<DetectedAgentProvider> {
    detect_agent_cli_providers_in_path_with_config_dir(
        std::env::var_os("PATH").unwrap_or_default(),
        &crate::provider::loom_config_dir(),
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

pub fn apply_provider_overrides(
    mut providers: Vec<DetectedAgentProvider>,
    overrides: &[AgentProviderOverride],
) -> Vec<DetectedAgentProvider> {
    for override_config in overrides {
        let Some(provider) = providers
            .iter_mut()
            .find(|provider| provider.id == override_config.id)
        else {
            continue;
        };
        if let Some(command) = override_config
            .command
            .as_deref()
            .map(str::trim)
            .filter(|command| !command.is_empty())
        {
            provider.command = command.to_string();
            provider.transport.command = command.to_string();
        }
        if let Some(args) = &override_config.args {
            provider.args = args.clone();
            provider.transport.args = args.clone();
        }
        provider.transport_env.extend(
            override_config
                .env
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
        provider.transport.env.extend(override_config.env.clone());
    }
    providers
}

pub fn resolve_provider_ref_in_spec(spec: &mut AgentSpec) -> Result<(), String> {
    let Some(provider_ref) = spec.provider_ref.clone() else {
        return Ok(());
    };
    spec.transport = crate::provider::default_registry()?.resolve_transport(&provider_ref)?;
    Ok(())
}

impl DetectedAgentProvider {
    pub fn transport(&self) -> AgentTransport {
        let mut transport = self.transport.clone();
        if transport.command.trim().is_empty() {
            transport.command = self.command.clone();
        }
        if transport.args.is_empty() {
            transport.args = self.args.clone();
        }
        transport.env.extend(self.transport_env.clone());
        if transport.model.is_none() {
            transport.model = self.default_model.clone();
        }
        transport
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
                models: model_spec(self.default_model.as_deref(), &self.model_choices),
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
    meta.insert("createdBy".into(), json!("loom-daemon"));
    if let Some(reasoning_effort) = definition
        .reasoning_effort
        .as_deref()
        .map(str::trim)
        .filter(|reasoning_effort| !reasoning_effort.is_empty())
    {
        meta.insert("reasoningEffort".into(), json!(reasoning_effort));
    }
    if let Some(avatar_url) = definition
        .avatar_url
        .as_deref()
        .map(str::trim)
        .filter(|avatar_url| !avatar_url.is_empty())
    {
        meta.insert("avatarUrl".into(), json!(avatar_url));
    }
    if let Some(description) = definition
        .description
        .as_deref()
        .map(str::trim)
        .filter(|description| !description.is_empty())
    {
        meta.insert("description".into(), json!(description));
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
        identity: None,
        memory: None,
        announcement: None,
    }
}

fn detect_agent_cli_providers_in_path_with_config_dir(
    path: OsString,
    config_dir: &Path,
) -> Vec<DetectedAgentProvider> {
    crate::provider::detect_agent_cli_providers_with_config_dir_and_path(config_dir, path)
        .unwrap_or_default()
        .into_iter()
        .map(|provider| {
            let id = provider.id;
            let display_name = provider.display_name;
            let command = provider.command;
            let transport_kind = provider.transport_kind;
            let args = provider.args;
            let env = provider.env;
            let default_model = provider.default_model;
            let model_choices = provider.model_choices;
            let manifest_id = provider.manifest.id;
            let transport = crate::provider::ProviderRegistry::load(config_dir)
                .and_then(|registry| {
                    registry.resolve_transport(&AgentProviderRef {
                        id: manifest_id,
                        mode: Some("print".into()),
                        model: default_model.clone(),
                        reasoning_effort: None,
                    })
                })
                .unwrap_or_else(|_| AgentTransport {
                    kind: transport_kind.clone(),
                    command: command.clone(),
                    args: args.clone(),
                    env: env.clone(),
                    model: default_model.clone(),
                    ..Default::default()
                });
            DetectedAgentProvider {
                id,
                display_name,
                command,
                transport_kind,
                args,
                transport_env: env,
                transport,
                default_model,
                model_choices,
            }
        })
        .collect()
}

fn model_spec(default: Option<&str>, choices: &[AgentModelChoice]) -> Option<AgentModelSpec> {
    if default.is_none() && choices.is_empty() {
        return None;
    }
    Some(AgentModelSpec {
        default: default.map(ToOwned::to_owned),
        choices: choices.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proto::methods::{CommandOutputFormat, CommandSessionIdSource};
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "loom-discovery-{name}-{}",
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
        make_executable(&dir.join("claude"));
        make_executable(&dir.join("codex"));
        make_executable(&dir.join("copilot"));
        make_executable(&dir.join("opencode"));
        make_executable(&dir.join("qodercli"));

        let providers = detect_agent_cli_providers_in_path_with_config_dir(
            dir.clone().into_os_string(),
            &config_dir,
        );
        let ids = providers.iter().map(|p| p.id.as_str()).collect::<Vec<_>>();
        assert_eq!(ids, vec!["claude", "codex", "copilot", "opencode", "qoder"]);
        let claude = providers
            .iter()
            .find(|provider| provider.id == "claude")
            .expect("claude provider");
        assert!(claude.args.contains(&"--append-system-prompt".into()));
        assert!(claude.args.contains(&"{prompt.system}".into()));
        assert!(claude.args.contains(&"{prompt.user}".into()));
        let claude_transport = claude.transport();
        assert_eq!(
            claude_transport.output_format,
            Some(CommandOutputFormat::ClaudeStreamJson)
        );
        assert_eq!(
            claude_transport.session.as_ref().and_then(|s| s.id_source),
            Some(CommandSessionIdSource::LoomUuid)
        );
        assert_eq!(
            claude_transport
                .session
                .as_ref()
                .and_then(|s| s.first_run_capture.as_deref()),
            None
        );
        assert_eq!(
            claude_transport
                .session
                .as_ref()
                .and_then(|s| s.resume_args.as_ref())
                .map(|args| args.iter().map(String::as_str).collect::<Vec<_>>()),
            Some(vec![
                "--add-dir",
                "{loom.configDir}",
                "--permission-mode",
                "bypassPermissions",
                "--output-format",
                "stream-json",
                "--verbose",
                "--resume",
                "{session_id}",
                "--append-system-prompt",
                "{prompt.system}",
                "-p",
                "{prompt.user}",
            ])
        );
        let qoder = providers
            .iter()
            .find(|provider| provider.id == "qoder")
            .expect("qoder provider");
        assert!(qoder.args.contains(&"--append-system-prompt".into()));
        assert_eq!(
            qoder.transport().output_format,
            Some(CommandOutputFormat::ClaudeStreamJson)
        );
        let copilot = providers
            .iter()
            .find(|provider| provider.id == "copilot")
            .expect("copilot provider");
        assert!(copilot.args.contains(&"--resume".into()));
        assert!(copilot.args.contains(&"{prompt.full}".into()));
        assert_eq!(
            copilot.transport().output_format,
            Some(CommandOutputFormat::CopilotJson)
        );
        let codex = providers
            .iter()
            .find(|provider| provider.id == "codex")
            .expect("codex provider");
        assert!(codex.args.contains(&"{prompt.full}".into()));
        assert_eq!(
            codex.transport().output_format,
            Some(CommandOutputFormat::CodexStreamJson)
        );
        assert_eq!(
            codex
                .transport()
                .env
                .get("LOOM_NO_DAEMON")
                .map(String::as_str),
            Some("1")
        );
        let opencode = providers
            .iter()
            .find(|provider| provider.id == "opencode")
            .expect("opencode provider");
        assert!(opencode.args.contains(&"{prompt.full}".into()));
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
                "--sandbox".into(),
                "danger-full-access".into(),
                "-c".into(),
                "sandbox_workspace_write.network_access=true".into(),
                "--add-dir".into(),
                "/tmp/loom-config".into(),
            ],
            transport_env: BTreeMap::new(),
            transport: AgentTransport {
                kind: "command".into(),
                command: "/bin/codex".into(),
                args: vec![
                    "exec".into(),
                    "--skip-git-repo-check".into(),
                    "--sandbox".into(),
                    "danger-full-access".into(),
                    "-c".into(),
                    "sandbox_workspace_write.network_access=true".into(),
                    "--add-dir".into(),
                    "/tmp/loom-config".into(),
                ],
                env: BTreeMap::from([("LOOM_NO_DAEMON".into(), "1".into())]),
                ..Default::default()
            },
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
                avatar_url: None,
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
                "--sandbox",
                "danger-full-access",
                "-c",
                "sandbox_workspace_write.network_access=true",
                "--add-dir",
                "/tmp/loom-config"
            ]
        );
        assert_eq!(
            specs[0]
                .transport
                .env
                .get("LOOM_NO_DAEMON")
                .map(String::as_str),
            Some("1")
        );
    }

    #[test]
    fn provider_overrides_replace_command_args_and_merge_env() {
        let providers = vec![DetectedAgentProvider {
            id: "codex".into(),
            display_name: "Codex CLI".into(),
            command: "/usr/bin/codex".into(),
            transport_kind: "command".into(),
            args: vec!["exec".into(), "--skip-git-repo-check".into()],
            transport_env: BTreeMap::new(),
            transport: AgentTransport::default(),
            default_model: None,
            model_choices: Vec::new(),
        }];
        let providers = apply_provider_overrides(
            providers,
            &[AgentProviderOverride {
                id: "codex".into(),
                command: Some("/bin/bash".into()),
                args: Some(vec![
                    "-lc".into(),
                    "vpn && exec codex \"$@\"".into(),
                    "loom-codex".into(),
                ]),
                env: BTreeMap::from([("HTTPS_PROXY".into(), "http://127.0.0.1:7890".into())]),
            }],
        );
        let provider = &providers[0];
        assert_eq!(provider.command, "/bin/bash");
        assert_eq!(
            provider.args,
            vec!["-lc", "vpn && exec codex \"$@\"", "loom-codex"]
        );
        let transport = provider.transport();
        assert_eq!(
            transport.env.get("HTTPS_PROXY").map(String::as_str),
            Some("http://127.0.0.1:7890")
        );
    }
}
