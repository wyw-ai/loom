//! Local agent CLI discovery and provider-spec synthesis.
//!
//! This module is intentionally small and side-effect free except for reading
//! `PATH`. The GUI uses it to present supported local CLIs, while `loom-daemon`
//! uses the same provider profiles to build runtime `AgentSpec`s from machine
//! config without requiring on-disk provider JSON specs.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use proto::methods::{
    AgentActorDefaults, AgentActorSpec, AgentModelChoice, AgentModelSpec, AgentProviderInfo,
    AgentProviderRef, AgentProviderSpec, AgentSpec, AgentTransport, CommandOutputFormat,
    CommandSession, CommandSessionIdSource, IdentityFiles, IdentityScaffoldSpec, IdentitySpec,
    PromptVia,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

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
    ProviderDef {
        id: "opencode",
        display_name: "OpenCode",
        candidates: &["opencode"],
        args: &["run"],
    },
];

pub fn detect_agent_cli_providers() -> Vec<DetectedAgentProvider> {
    detect_agent_cli_providers_in_path_with_config_dir(
        std::env::var_os("PATH").unwrap_or_default(),
        &loom_config_dir(),
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
        }
        if let Some(args) = &override_config.args {
            provider.args = args.clone();
        }
        provider.transport_env.extend(
            override_config
                .env
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
    }
    providers
}

pub fn resolve_provider_ref_in_spec(spec: &mut AgentSpec) -> Result<(), String> {
    let Some(provider_ref) = spec.provider_ref.clone() else {
        return Ok(());
    };
    let mode = provider_ref.mode.as_deref().unwrap_or("print");
    if mode != "print" && mode != "command" {
        return Err(format!(
            "providerRef for {} requested unsupported mode `{mode}`",
            spec.actor.id
        ));
    }
    let provider = detect_agent_cli_providers()
        .into_iter()
        .find(|provider| provider.id == provider_ref.id)
        .ok_or_else(|| {
            format!(
                "provider `{}` referenced by {} was not detected on PATH",
                provider_ref.id, spec.actor.id
            )
        })?;
    let mut transport = provider.transport();
    if provider_ref
        .model
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .is_none()
    {
        transport.model = None;
    }
    apply_provider_ref_options(&mut transport, &provider_ref);
    spec.transport = transport;
    Ok(())
}

fn apply_provider_ref_options(transport: &mut AgentTransport, provider_ref: &AgentProviderRef) {
    if let Some(model) = provider_ref
        .model
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty())
    {
        transport.model = Some(model.to_string());
    }
    if let Some(reasoning_effort) = provider_ref
        .reasoning_effort
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        match provider_ref.id.as_str() {
            "opencode" => {
                transport.args.push("--variant".into());
                transport.args.push(reasoning_effort.to_string());
            }
            "qoder" => {
                transport.args.push("--reasoning-effort".into());
                transport.args.push(reasoning_effort.to_string());
            }
            "copilot" => {
                transport.args.push("--effort".into());
                transport.args.push(reasoning_effort.to_string());
            }
            _ => {}
        }
    }
}

impl DetectedAgentProvider {
    pub fn transport(&self) -> AgentTransport {
        let mut env = self.transport_env.clone();
        if self.id == "codex" {
            // Codex runs model-generated shell commands inside its own
            // sandbox. The Loom daemon socket is outside the actor workspace
            // and macOS Seatbelt denies AF_UNIX access there, so have `loom`
            // CLI calls use LOOM_SERVER directly.
            env.entry("LOOM_NO_DAEMON".into())
                .or_insert_with(|| "1".into());
        }
        if self.id == "opencode" {
            // OpenCode keeps a SQLite store under XDG_DATA_HOME. Use an
            // actor-local store so a user's interactive opencode process does
            // not lock the daemon-run provider.
            env.entry("XDG_DATA_HOME".into())
                .or_insert_with(|| "{agent.profile}/opencode/data".into());
            env.entry("XDG_STATE_HOME".into())
                .or_insert_with(|| "{agent.profile}/opencode/state".into());
            env.entry("XDG_CACHE_HOME".into())
                .or_insert_with(|| "{agent.profile}/opencode/cache".into());
        }
        AgentTransport {
            kind: self.transport_kind.clone(),
            command: self.command.clone(),
            args: self.args.clone(),
            env,
            auth_method: None,
            model: self.default_model.clone(),
            model_args: model_args_for_provider(&self.id),
            session: command_session_for_provider(&self.id, &self.args),
            output_format: Some(command_output_format_for_provider(&self.id)),
            prompt_via: PromptVia::Args,
            timeout_ms: None,
            idle_timeout_ms: None,
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
        identity: Some(IdentitySpec {
            files: IdentityFiles::default(),
            description: definition.description.clone(),
            scaffold: definition
                .description
                .as_ref()
                .map(|description| IdentityScaffoldSpec {
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
            let (default_model, model_choices) = model_choices_for_provider(def.id);
            Some(DetectedAgentProvider {
                id: def.id.into(),
                display_name: def.display_name.into(),
                command: command.display().to_string(),
                transport_kind: "command".into(),
                args: provider_args(def, config_dir),
                transport_env: BTreeMap::new(),
                default_model,
                model_choices,
            })
        })
        .collect()
}

fn model_args_for_provider(provider_id: &str) -> Vec<String> {
    match provider_id {
        "claude" | "qoder" | "copilot" | "codex" | "opencode" => {
            vec!["--model".into(), "{model}".into()]
        }
        _ => Vec::new(),
    }
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

fn model_choices_for_provider(provider_id: &str) -> (Option<String>, Vec<AgentModelChoice>) {
    match provider_id {
        "codex" => model_choices_from_codex_cache(),
        "qoder" => model_choices_from_qoder_registry(),
        "copilot" => static_model_choices(COPILOT_MODELS),
        "claude" => static_model_choices(CLAUDE_MODELS),
        "opencode" => static_model_choices(OPENCODE_MODELS),
        _ => (None, Vec::new()),
    }
}

const CLAUDE_MODELS: &[(&str, &str)] = &[
    ("sonnet", "Sonnet"),
    ("opus", "Opus"),
    ("claude-sonnet-4.6", "Claude Sonnet 4.6"),
    ("claude-opus-4.7", "Claude Opus 4.7"),
    ("claude-haiku-4.5", "Claude Haiku 4.5"),
];

const COPILOT_MODELS: &[(&str, &str)] = &[
    ("gpt-5.5", "GPT-5.5"),
    ("gpt-5.4", "GPT-5.4"),
    ("gpt-5.3-codex", "GPT-5.3 Codex"),
    ("gpt-5.2-codex", "GPT-5.2 Codex"),
    ("gpt-5.2", "GPT-5.2"),
    ("gpt-5.1", "GPT-5.1"),
    ("gpt-5.4-mini", "GPT-5.4 Mini"),
    ("gpt-5-mini", "GPT-5 Mini"),
    ("gpt-4.1", "GPT-4.1"),
    ("claude-sonnet-4.6", "Claude Sonnet 4.6"),
    ("claude-sonnet-4.5", "Claude Sonnet 4.5"),
    ("claude-haiku-4.5", "Claude Haiku 4.5"),
    ("claude-opus-4.7", "Claude Opus 4.7"),
    ("claude-opus-4.6", "Claude Opus 4.6"),
    ("claude-opus-4.6-fast", "Claude Opus 4.6 Fast"),
    ("claude-opus-4.5", "Claude Opus 4.5"),
    ("claude-sonnet-4", "Claude Sonnet 4"),
];

const OPENCODE_MODELS: &[(&str, &str)] = &[
    ("openai/gpt-5.5", "OpenAI GPT-5.5"),
    ("openai/gpt-5.4", "OpenAI GPT-5.4"),
    ("openai/gpt-5.4-mini", "OpenAI GPT-5.4 Mini"),
    ("openai/gpt-5.3-codex", "OpenAI GPT-5.3 Codex"),
    ("openai/gpt-5.3-codex-spark", "OpenAI GPT-5.3 Codex Spark"),
    ("openai/gpt-5.2", "OpenAI GPT-5.2"),
    ("opencode/big-pickle", "OpenCode Big Pickle"),
    (
        "opencode/deepseek-v4-flash-free",
        "OpenCode DeepSeek V4 Flash Free",
    ),
    ("opencode/minimax-m2.5-free", "OpenCode MiniMax M2.5 Free"),
];

fn static_model_choices(models: &[(&str, &str)]) -> (Option<String>, Vec<AgentModelChoice>) {
    let choices = models
        .iter()
        .map(|(id, label)| model_choice(id, label))
        .collect::<Vec<_>>();
    (models.first().map(|(id, _)| (*id).to_string()), choices)
}

fn model_choices_from_codex_cache() -> (Option<String>, Vec<AgentModelChoice>) {
    let Some(path) = home_relative_path(".codex/models_cache.json") else {
        return static_model_choices(CODEX_MODELS);
    };
    let Some(root) = read_json_file(&path) else {
        return static_model_choices(CODEX_MODELS);
    };
    let choices = root
        .get("models")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|model| string_field(model, "visibility") == Some("list"))
        .filter_map(|model| {
            let id = string_field(model, "slug")?.trim();
            if id.is_empty() {
                return None;
            }
            let label = string_field(model, "display_name")
                .filter(|label| !label.trim().is_empty())
                .unwrap_or(id);
            Some(model_choice(id, label))
        })
        .collect::<Vec<_>>();
    if choices.is_empty() {
        return static_model_choices(CODEX_MODELS);
    }
    (choices.first().map(|choice| choice.id.clone()), choices)
}

const CODEX_MODELS: &[(&str, &str)] = &[
    ("gpt-5.5", "GPT-5.5"),
    ("gpt-5.4", "GPT-5.4"),
    ("gpt-5.4-mini", "GPT-5.4 Mini"),
    ("gpt-5.3-codex", "GPT-5.3 Codex"),
    ("gpt-5.3-codex-spark", "GPT-5.3 Codex Spark"),
    ("gpt-5.2", "GPT-5.2"),
];

fn model_choices_from_qoder_registry() -> (Option<String>, Vec<AgentModelChoice>) {
    let Some(path) = home_relative_path(".qoder/.auth/models") else {
        return static_model_choices(QODER_MODELS);
    };
    let Some(root) = read_json_file(&path) else {
        return static_model_choices(QODER_MODELS);
    };
    let choices = root
        .get("assistant")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|model| model.get("enable").and_then(Value::as_bool).unwrap_or(true))
        .filter_map(|model| {
            let id = string_field(model, "key")?.trim();
            if id.is_empty() {
                return None;
            }
            let label = string_field(model, "display_name")
                .filter(|label| !label.trim().is_empty())
                .unwrap_or(id);
            Some(model_choice(id, label))
        })
        .collect::<Vec<_>>();
    if choices.is_empty() {
        return static_model_choices(QODER_MODELS);
    }
    let default = root
        .get("assistant")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|model| model.get("is_default").and_then(Value::as_bool) == Some(true))
        .and_then(|model| string_field(model, "key"))
        .map(ToOwned::to_owned)
        .or_else(|| choices.first().map(|choice| choice.id.clone()));
    (default, choices)
}

const QODER_MODELS: &[(&str, &str)] = &[
    ("auto", "Auto"),
    ("ultimate", "Ultimate"),
    ("performance", "Performance"),
    ("efficient", "Efficient"),
    ("lite", "Lite"),
];

fn model_choice(id: &str, label: &str) -> AgentModelChoice {
    AgentModelChoice {
        id: id.to_string(),
        label: label.to_string(),
        description: None,
    }
}

fn read_json_file(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn string_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn home_relative_path(path: &str) -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|home| home.join(path))
}

fn provider_args(def: &ProviderDef, config_dir: &Path) -> Vec<String> {
    let loom_config_dir = absolute_path(config_dir);
    let mut args = Vec::new();
    match def.id {
        "claude" => {
            append_add_dir_arg(&mut args, &loom_config_dir);
            args.push("--permission-mode".into());
            args.push("bypassPermissions".into());
            args.push("--output-format".into());
            args.push("stream-json".into());
            args.push("--verbose".into());
            args.push("--session-id".into());
            args.push("{session_id}".into());
            args.extend(def.args.iter().map(|arg| (*arg).to_string()));
        }
        "qoder" => {
            append_add_dir_arg(&mut args, &loom_config_dir);
            args.push("--yolo".into());
            args.push("--output-format".into());
            args.push("stream-json".into());
            args.extend(def.args.iter().map(|arg| (*arg).to_string()));
        }
        "copilot" => {
            append_add_dir_arg(&mut args, &loom_config_dir);
            args.push("--yolo".into());
            args.push("--output-format".into());
            args.push("json".into());
            args.push("--stream".into());
            args.push("off".into());
            args.extend(def.args.iter().map(|arg| (*arg).to_string()));
        }
        "codex" => {
            args.extend(def.args.iter().map(|arg| (*arg).to_string()));
            args.push("--json".into());
            args.push("--sandbox".into());
            args.push("danger-full-access".into());
            args.push("-c".into());
            args.push("sandbox_workspace_write.network_access=true".into());
            append_add_dir_arg(&mut args, &loom_config_dir);
        }
        "opencode" => {
            args.extend(def.args.iter().map(|arg| (*arg).to_string()));
            args.push("--dangerously-skip-permissions".into());
        }
        _ => args.extend(def.args.iter().map(|arg| (*arg).to_string())),
    }
    args
}

fn command_output_format_for_provider(provider_id: &str) -> CommandOutputFormat {
    match provider_id {
        "claude" | "qoder" => CommandOutputFormat::ClaudeStreamJson,
        "copilot" => CommandOutputFormat::CopilotJson,
        "codex" => CommandOutputFormat::CodexStreamJson,
        _ => CommandOutputFormat::Text,
    }
}

fn command_session_for_provider(
    provider_id: &str,
    first_run_args: &[String],
) -> Option<CommandSession> {
    match provider_id {
        "claude" => Some(CommandSession {
            id_source: Some(CommandSessionIdSource::LoomUuid),
            first_run_capture: None,
            resume_args: Some(claude_resume_args(first_run_args)),
        }),
        _ => None,
    }
}

fn claude_resume_args(first_run_args: &[String]) -> Vec<String> {
    let mut args = Vec::with_capacity(first_run_args.len());
    let mut i = 0;
    while i < first_run_args.len() {
        if first_run_args[i] == "--session-id"
            && first_run_args
                .get(i + 1)
                .is_some_and(|arg| arg == "{session_id}")
        {
            args.push("--resume".into());
            args.push("{session_id}".into());
            i += 2;
            continue;
        }
        args.push(first_run_args[i].clone());
        i += 1;
    }
    args
}

fn append_add_dir_arg(args: &mut Vec<String>, dir: &Path) {
    args.push("--add-dir".into());
    args.push(dir.display().to_string());
}

fn loom_config_dir() -> PathBuf {
    if let Some(value) = std::env::var_os("LOOM_CONFIG_DIR").filter(|value| !value.is_empty()) {
        return PathBuf::from(value);
    }
    if let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(home).join(".loom");
    }
    PathBuf::from(".loom")
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
    let mut path_dirs = std::env::split_paths(path).collect::<Vec<_>>();
    path_dirs.extend(fallback_command_dirs());
    path_dirs.sort();
    path_dirs.dedup();
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

fn fallback_command_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/opt/local/bin"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
    ];
    if let Some(home) = std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    {
        dirs.push(home.join(".local").join("bin"));
        dirs.push(home.join(".cargo").join("bin"));
        dirs.push(home.join(".bun").join("bin"));
        let nvm_node_root = home.join(".nvm").join("versions").join("node");
        if let Ok(entries) = std::fs::read_dir(nvm_node_root) {
            dirs.extend(
                entries
                    .flatten()
                    .map(|entry| entry.path().join("bin"))
                    .filter(|path| path.is_dir()),
            );
        }
    }
    dirs
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
        let config_dir_arg = config_dir.display().to_string();
        let add_dir = vec!["--add-dir", config_dir_arg.as_str()];

        assert_eq!(ids, vec!["claude", "qoder", "copilot", "codex", "opencode"]);
        let claude = providers
            .iter()
            .find(|provider| provider.id == "claude")
            .expect("claude provider");
        let mut expected = add_dir.clone();
        expected.extend([
            "--permission-mode",
            "bypassPermissions",
            "--output-format",
            "stream-json",
            "--verbose",
            "--session-id",
            "{session_id}",
        ]);
        expected.push("-p");
        assert_eq!(claude.args, expected);
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
                config_dir_arg.as_str(),
                "--permission-mode",
                "bypassPermissions",
                "--output-format",
                "stream-json",
                "--verbose",
                "--resume",
                "{session_id}",
                "-p",
            ])
        );
        let qoder = providers
            .iter()
            .find(|provider| provider.id == "qoder")
            .expect("qoder provider");
        let mut expected = add_dir.clone();
        expected.push("--yolo");
        expected.push("--output-format");
        expected.push("stream-json");
        expected.push("-p");
        assert_eq!(qoder.args, expected);
        assert_eq!(
            qoder.transport().output_format,
            Some(CommandOutputFormat::ClaudeStreamJson)
        );
        let copilot = providers
            .iter()
            .find(|provider| provider.id == "copilot")
            .expect("copilot provider");
        let mut expected = add_dir.clone();
        expected.push("--yolo");
        expected.push("--output-format");
        expected.push("json");
        expected.push("--stream");
        expected.push("off");
        expected.push("-p");
        assert_eq!(copilot.args, expected);
        assert_eq!(
            copilot.transport().output_format,
            Some(CommandOutputFormat::CopilotJson)
        );
        let codex = providers
            .iter()
            .find(|provider| provider.id == "codex")
            .expect("codex provider");
        let mut expected = vec![
            "exec",
            "--skip-git-repo-check",
            "--json",
            "--sandbox",
            "danger-full-access",
            "-c",
            "sandbox_workspace_write.network_access=true",
        ];
        expected.extend(add_dir);
        assert_eq!(codex.args, expected);
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
        assert_eq!(opencode.args, vec!["run", "--dangerously-skip-permissions"]);
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
        assert_eq!(
            transport.env.get("LOOM_NO_DAEMON").map(String::as_str),
            Some("1")
        );
    }
}
