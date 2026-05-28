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

use crate::provider::{ProviderRegistry, ProviderRuntimePlan};

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
    pub runtime_plan: Option<ProviderRuntimePlan>,
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
        if let Some(plan) = self.runtime_plan.clone() {
            return plan.into_transport();
        }
        AgentTransport {
            kind: self.transport_kind.clone(),
            command: self.command.clone(),
            args: self.args.clone(),
            arg_specs: Vec::new(),
            env: self.transport_env.clone(),
            model: self.default_model.clone(),
            ..Default::default()
        }
    }
}

fn detect_agent_cli_providers_in_path_with_config_dir(
    path: OsString,
    config_dir: &Path,
) -> Vec<DetectedAgentProvider> {
    let registry = ProviderRegistry::load(config_dir).ok();
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
            let runtime_plan = registry.as_ref().and_then(|registry| {
                registry
                    .resolve_runtime_plan(&AgentProviderRef {
                        id: manifest_id,
                        mode: Some("print".into()),
                        model: default_model.clone(),
                        reasoning_effort: None,
                    })
                    .ok()
            });
            DetectedAgentProvider {
                id,
                display_name,
                command,
                transport_kind,
                args,
                transport_env: env,
                runtime_plan,
                default_model,
                model_choices,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proto::methods::{CommandOutputFormat, CommandSessionIdSource, ProviderArgSpec};
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
        let claude_session = claude_transport.session.as_ref().expect("claude session");
        let resume_args = claude_session.resume_args.as_ref().expect("resume args");
        assert!(resume_args.contains(&"--resume".into()));
        assert!(resume_args.contains(&"{session_id}".into()));
        assert!(resume_args.contains(&"{prompt.user}".into()));
        assert!(claude_session.resume_arg_specs.iter().any(|arg| matches!(
            arg,
            ProviderArgSpec::Conditional { when, .. } if when == "model"
        )));
        assert!(claude_transport.model_args.is_empty());
        let qoder = providers
            .iter()
            .find(|provider| provider.id == "qoder")
            .expect("qoder provider");
        assert!(qoder.args.contains(&"--append-system-prompt".into()));
        let qoder_plan = qoder.runtime_plan.as_ref().expect("qoder runtime plan");
        assert_eq!(qoder_plan.provider_id, "qoder");
        assert_eq!(qoder_plan.mode, "print");
        assert_eq!(qoder_plan.transport_kind, "command");
        assert_eq!(
            qoder.transport().output_format,
            Some(CommandOutputFormat::ClaudeStreamJson)
        );
        let serialized = serde_json::to_value(qoder).expect("serialize provider");
        assert!(
            serialized.get("runtimePlan").is_none(),
            "daemon inventory should publish provider summary, not runtime plan internals"
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
