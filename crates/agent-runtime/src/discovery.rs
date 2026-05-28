//! Local agent CLI discovery.
//!
//! This module is intentionally small and side-effect free except for reading
//! `PATH`. The GUI and `loom-daemon` use it to present the provider inventory
//! exposed by the daemon-local ProviderManifest registry.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

use proto::methods::{AgentModelChoice, AgentProviderRef, AgentTransport};
use serde::{Deserialize, Serialize};

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

pub fn detect_agent_cli_providers() -> Vec<DetectedAgentProvider> {
    detect_agent_cli_providers_in_path_with_config_dir(
        std::env::var_os("PATH").unwrap_or_default(),
        &crate::provider::loom_config_dir(),
    )
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
            Some(CommandOutputFormat::NdjsonLines)
        );
        assert!(copilot
            .transport()
            .decoder
            .as_ref()
            .and_then(|decoder| decoder.reduce.as_ref())
            .and_then(|reduce| reduce.final_text.as_ref())
            .is_some());
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
}
