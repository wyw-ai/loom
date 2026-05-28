//! Provider manifest registry and resolution.
//!
//! The public surface here is intentionally data-driven: built-in providers are
//! represented as the same `ProviderManifest` shape that users can add under
//! `{LOOM_CONFIG_DIR}/providers/`. Runtime code resolves a manifest into the
//! existing `AgentTransport` adapter contract so the rest of Loom can migrate
//! incrementally without keeping provider-specific argv/parser logic scattered
//! across the daemon.

use std::collections::{BTreeMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use proto::methods::{
    AgentModelChoice, AgentModelSpec, AgentProviderRef, AgentTransport, CommandOutputFormat,
    CommandSession, CommandSessionIdSource, PromptVia, ProviderArgSpec, ProviderDecoderSpec,
    ProviderDetectSpec, ProviderManifest, ProviderModeSpec, ProviderPromptOutputSpec,
    ProviderPromptSpec, ProviderSessionIdSource, ProviderSessionSpec,
};

use crate::adapter::PromptPart;

#[derive(Debug, Clone)]
pub struct DetectedProvider {
    pub id: String,
    pub display_name: String,
    pub command: String,
    pub transport_kind: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub default_model: Option<String>,
    pub model_choices: Vec<AgentModelChoice>,
    pub manifest: ProviderManifest,
}

#[derive(Debug, Clone)]
pub struct ProviderRegistry {
    manifests: BTreeMap<String, ProviderManifest>,
}

impl ProviderRegistry {
    pub fn load(config_dir: &Path) -> Result<Self, String> {
        let mut manifests = BTreeMap::new();
        for manifest in builtin_provider_manifests() {
            validate_manifest(&manifest)?;
            manifests.insert(manifest.id.clone(), manifest);
        }
        for manifest in load_local_manifests(config_dir)? {
            validate_manifest(&manifest)?;
            let manifest = if let Some(base_id) = manifest.extends.as_deref() {
                let base = manifests.get(base_id).cloned().ok_or_else(|| {
                    format!("provider `{}` extends unknown `{base_id}`", manifest.id)
                })?;
                merge_manifest(base, manifest)
            } else {
                manifest
            };
            validate_manifest(&manifest)?;
            manifests.insert(manifest.id.clone(), manifest);
        }
        Ok(Self { manifests })
    }

    pub fn manifests(&self) -> impl Iterator<Item = &ProviderManifest> {
        self.manifests.values()
    }

    pub fn get(&self, id: &str) -> Option<&ProviderManifest> {
        self.manifests.get(id)
    }

    pub fn detect_with_path(&self, path: OsString) -> Result<Vec<DetectedProvider>, String> {
        let mut detected = Vec::new();
        for manifest in self.manifests.values() {
            let mode_name = default_mode_name(manifest);
            let mode = manifest
                .modes
                .get(mode_name)
                .ok_or_else(|| format!("provider `{}` has no `{mode_name}` mode", manifest.id))?;
            let bin = find_command_in_path(&manifest.detect.candidates, &path)
                .or_else(|| command_candidate_from_mode(mode))
                .filter(|candidate| command_exists(candidate));
            let Some(bin) = bin else {
                continue;
            };
            let args = flatten_args_for_inventory(&mode.args);
            let transport_kind = if mode.transport.trim().is_empty() {
                "command".to_string()
            } else {
                mode.transport.clone()
            };
            let (default_model, model_choices) = model_inventory(manifest);
            detected.push(DetectedProvider {
                id: manifest.id.clone(),
                display_name: display_name(manifest),
                command: expand_static_command(&mode.command, &bin),
                transport_kind,
                args,
                env: mode.env.clone(),
                default_model,
                model_choices,
                manifest: manifest.clone(),
            });
        }
        Ok(detected)
    }

    pub fn resolve_transport(
        &self,
        provider_ref: &AgentProviderRef,
    ) -> Result<AgentTransport, String> {
        let manifest = self
            .get(&provider_ref.id)
            .ok_or_else(|| format!("provider `{}` not found", provider_ref.id))?;
        let mode_name = provider_ref
            .mode
            .as_deref()
            .unwrap_or(default_mode_name(manifest));
        let mode = manifest.modes.get(mode_name).ok_or_else(|| {
            format!(
                "provider `{}` does not define mode `{mode_name}`",
                provider_ref.id
            )
        })?;
        let path = std::env::var_os("PATH").unwrap_or_default();
        let bin = find_command_in_path(&manifest.detect.candidates, &path)
            .or_else(|| command_candidate_from_mode(mode))
            .ok_or_else(|| {
                format!(
                    "provider `{}` was not detected; candidates: {}",
                    provider_ref.id,
                    manifest.detect.candidates.join(", ")
                )
            })?;
        transport_from_manifest(manifest, mode, &bin, provider_ref)
    }
}

pub fn loom_config_dir() -> PathBuf {
    if let Some(value) = std::env::var_os("LOOM_CONFIG_DIR").filter(|value| !value.is_empty()) {
        return PathBuf::from(value);
    }
    if let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(home).join(".loom");
    }
    PathBuf::from(".loom")
}

pub fn providers_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("providers")
}

pub fn default_registry() -> Result<ProviderRegistry, String> {
    ProviderRegistry::load(&loom_config_dir())
}

pub fn detect_agent_cli_providers() -> Result<Vec<DetectedProvider>, String> {
    let registry = default_registry()?;
    registry.detect_with_path(std::env::var_os("PATH").unwrap_or_default())
}

pub fn detect_agent_cli_providers_with_config_dir_and_path(
    config_dir: &Path,
    path: OsString,
) -> Result<Vec<DetectedProvider>, String> {
    ProviderRegistry::load(config_dir)?.detect_with_path(path)
}

pub fn validate_manifest(manifest: &ProviderManifest) -> Result<(), String> {
    if manifest.schema_version != 1 {
        return Err(format!(
            "provider `{}` uses unsupported schemaVersion {}",
            manifest.id, manifest.schema_version
        ));
    }
    if manifest.id.trim().is_empty() {
        return Err("provider id is required".into());
    }
    if manifest.modes.is_empty() {
        return Err(format!(
            "provider `{}` must define at least one mode",
            manifest.id
        ));
    }
    for (mode_name, mode) in &manifest.modes {
        if mode.transport.trim().is_empty() {
            return Err(format!(
                "provider `{}` mode `{mode_name}` has empty transport",
                manifest.id
            ));
        }
        if mode.command.trim().is_empty() {
            return Err(format!(
                "provider `{}` mode `{mode_name}` has empty command",
                manifest.id
            ));
        }
        validate_prompt_references(manifest, mode_name, mode)?;
    }
    Ok(())
}

pub fn builtin_provider_manifests() -> Vec<ProviderManifest> {
    vec![
        claude_manifest(),
        qoder_manifest(),
        copilot_manifest(),
        codex_manifest(),
        opencode_manifest(),
    ]
}

pub fn render_prompt_outputs(
    spec: Option<&ProviderPromptSpec>,
    parts: &[PromptPart],
    full_prompt: &str,
) -> Result<BTreeMap<String, String>, String> {
    let mut outputs = BTreeMap::new();
    let default = default_prompt_outputs();
    let prompt_spec = spec.unwrap_or(&default);
    if prompt_spec.outputs.is_empty() {
        outputs.insert("full".into(), full_prompt.to_string());
    } else {
        for (name, output) in &prompt_spec.outputs {
            outputs.insert(
                name.clone(),
                render_prompt_output(output, parts, full_prompt)?,
            );
        }
    }
    outputs
        .entry("full".into())
        .or_insert_with(|| full_prompt.to_string());
    if !outputs.contains_key("system") {
        outputs.insert(
            "system".into(),
            render_prompt_output(
                &ProviderPromptOutputSpec {
                    preset: Some("loom_system".into()),
                    ..Default::default()
                },
                parts,
                full_prompt,
            )?,
        );
    }
    if !outputs.contains_key("user") {
        outputs.insert(
            "user".into(),
            render_prompt_output(
                &ProviderPromptOutputSpec {
                    preset: Some("loom_turn".into()),
                    ..Default::default()
                },
                parts,
                full_prompt,
            )?,
        );
    }
    Ok(outputs)
}

fn default_prompt_outputs() -> ProviderPromptSpec {
    ProviderPromptSpec {
        outputs: BTreeMap::from([(
            "full".into(),
            ProviderPromptOutputSpec {
                preset: Some("loom_full".into()),
                ..Default::default()
            },
        )]),
    }
}

fn render_prompt_output(
    output: &ProviderPromptOutputSpec,
    parts: &[PromptPart],
    full_prompt: &str,
) -> Result<String, String> {
    let part_map = parts
        .iter()
        .map(|part| (part.key.as_str(), part))
        .collect::<BTreeMap<_, _>>();
    for required in &output.required {
        if !part_map
            .get(required.as_str())
            .is_some_and(|part| !part.content.trim().is_empty())
        {
            return Err(format!("required prompt part `{required}` is missing"));
        }
    }

    let mut rendered = if let Some(template) = output.template.as_ref() {
        render_prompt_template(template, &part_map)
    } else {
        let include = if !output.include.is_empty() {
            output.include.clone()
        } else if let Some(preset) = output.preset.as_deref() {
            preset_parts(preset)?
                .into_iter()
                .map(str::to_string)
                .collect()
        } else {
            vec!["full".into()]
        };
        if include.len() == 1 && include[0] == "full" {
            full_prompt.to_string()
        } else {
            let join = output.join.as_deref().unwrap_or("\n\n");
            join_parts(
                include
                    .iter()
                    .filter_map(|key| part_map.get(key.as_str()).map(|part| part.content.as_str())),
                join,
            )
        }
    };
    if let Some(prefix) = output.prefix.as_ref() {
        rendered = format!("{prefix}{rendered}");
    }
    if let Some(suffix) = output.suffix.as_ref() {
        rendered.push_str(suffix);
    }
    Ok(rendered)
}

fn render_prompt_template(template: &str, parts: &BTreeMap<&str, &PromptPart>) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after_open = &rest[start + 1..];
        let Some(end) = after_open.find('}') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let key = &after_open[..end];
        if is_prompt_part_placeholder(key) {
            if let Some(part) = parts.get(key) {
                out.push_str(&part.content);
            }
        } else {
            out.push('{');
            out.push_str(key);
            out.push('}');
        }
        rest = &after_open[end + 1..];
    }
    out.push_str(rest);
    out
}

fn is_prompt_part_placeholder(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn preset_parts(preset: &str) -> Result<Vec<&'static str>, String> {
    match preset {
        "loom_system" => Ok(vec!["actor_context", "bootstrap_memory", "scope_bootstrap"]),
        "loom_turn" => Ok(vec!["turn_memory", "runtime_context", "user_message"]),
        "loom_full" => Ok(vec![
            "actor_context",
            "bootstrap_memory",
            "scope_bootstrap",
            "turn_memory",
            "runtime_context",
            "user_message",
        ]),
        other => Err(format!("unknown prompt preset `{other}`")),
    }
}

fn join_parts<'a>(parts: impl IntoIterator<Item = &'a str>, join: &str) -> String {
    parts
        .into_iter()
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join(join)
}

fn load_local_manifests(config_dir: &Path) -> Result<Vec<ProviderManifest>, String> {
    let dir = providers_dir(config_dir);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let entries = std::fs::read_dir(&dir)
        .map_err(|e| format!("read provider dir `{}`: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("read provider dir entry: {e}"))?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("read provider manifest `{}`: {e}", path.display()))?;
        let manifest = serde_json::from_str::<ProviderManifest>(&text)
            .map_err(|e| format!("parse provider manifest `{}`: {e}", path.display()))?;
        out.push(manifest);
    }
    Ok(out)
}

fn merge_manifest(mut base: ProviderManifest, patch: ProviderManifest) -> ProviderManifest {
    base.id = patch.id;
    base.extends = patch.extends;
    if !patch.display_name.trim().is_empty() {
        base.display_name = patch.display_name;
    }
    if !patch.detect.candidates.is_empty() {
        base.detect = patch.detect;
    }
    for (name, mode) in patch.modes {
        base.modes.insert(name, mode);
    }
    if patch.models.is_some() {
        base.models = patch.models;
    }
    base
}

fn validate_prompt_references(
    manifest: &ProviderManifest,
    mode_name: &str,
    mode: &ProviderModeSpec,
) -> Result<(), String> {
    let mut outputs = mode
        .prompt
        .as_ref()
        .map(|prompt| prompt.outputs.keys().cloned().collect::<HashSet<_>>())
        .unwrap_or_default();
    outputs.insert("full".into());
    outputs.insert("system".into());
    outputs.insert("user".into());
    let references = prompt_references_in_mode(mode);
    for name in references {
        if !outputs.contains(&name) {
            return Err(format!(
                "provider `{}` mode `{mode_name}` references unknown prompt output `{name}`",
                manifest.id
            ));
        }
    }
    Ok(())
}

fn prompt_references_in_mode(mode: &ProviderModeSpec) -> HashSet<String> {
    let mut refs = HashSet::new();
    for arg in &mode.args {
        collect_prompt_refs_from_arg(arg, &mut refs);
    }
    for value in mode.env.values() {
        collect_prompt_refs(value, &mut refs);
    }
    if let Some(stdin) = &mode.stdin {
        collect_prompt_refs(stdin, &mut refs);
    }
    if let Some(session) = &mode.session {
        for arg in &session.resume_args {
            collect_prompt_refs_from_arg(arg, &mut refs);
        }
    }
    refs
}

fn collect_prompt_refs_from_arg(arg: &ProviderArgSpec, refs: &mut HashSet<String>) {
    match arg {
        ProviderArgSpec::Literal(value) => collect_prompt_refs(value, refs),
        ProviderArgSpec::Conditional { args, .. } => {
            for arg in args {
                collect_prompt_refs_from_arg(arg, refs);
            }
        }
    }
}

fn collect_prompt_refs(value: &str, refs: &mut HashSet<String>) {
    let mut rest = value;
    while let Some(idx) = rest.find("{prompt.") {
        rest = &rest[idx + "{prompt.".len()..];
        let Some(end) = rest.find('}') else {
            break;
        };
        let name = &rest[..end];
        if !name.is_empty() {
            refs.insert(name.to_string());
        }
        rest = &rest[end + 1..];
    }
}

fn transport_from_manifest(
    manifest: &ProviderManifest,
    mode: &ProviderModeSpec,
    bin: &Path,
    provider_ref: &AgentProviderRef,
) -> Result<AgentTransport, String> {
    let mut args = Vec::new();
    let mut model_args = Vec::new();
    append_provider_args(
        &mode.args,
        &mut args,
        &mut model_args,
        provider_ref.reasoning_effort.as_deref(),
    );
    let mut resume_args = Vec::new();
    let mut resume_model_args = Vec::new();
    if let Some(session) = mode.session.as_ref() {
        append_provider_args(
            &session.resume_args,
            &mut resume_args,
            &mut resume_model_args,
            provider_ref.reasoning_effort.as_deref(),
        );
        if model_args.is_empty() && !resume_model_args.is_empty() {
            model_args = resume_model_args;
        }
    }

    let transport = AgentTransport {
        kind: if mode.transport.trim().is_empty() {
            "command".into()
        } else {
            mode.transport.clone()
        },
        command: expand_static_command(&mode.command, bin),
        args: normalize_session_tokens(args),
        env: mode.env.clone(),
        auth_method: None,
        model: provider_ref
            .model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        model_args: normalize_session_tokens(model_args),
        session: mode.session.as_ref().map(|session| CommandSession {
            id_source: session.id_source.map(|source| match source {
                ProviderSessionIdSource::LoomUuid => CommandSessionIdSource::LoomUuid,
                ProviderSessionIdSource::ProviderCapture => CommandSessionIdSource::ProviderCapture,
            }),
            first_run_capture: session.capture.clone(),
            resume_args: if resume_args.is_empty() {
                None
            } else {
                Some(normalize_session_tokens(resume_args))
            },
        }),
        output_format: Some(output_format(&mode.stdout)?),
        prompt_via: PromptVia::Args,
        prompt: mode.prompt.clone(),
        stdin: mode.stdin.clone(),
        timeout_ms: mode.timeout_ms,
        idle_timeout_ms: mode.idle_timeout_ms,
        interactive: None,
        provider: None,
    };
    validate_manifest(manifest)?;
    Ok(transport)
}

fn append_provider_args(
    specs: &[ProviderArgSpec],
    args: &mut Vec<String>,
    model_args: &mut Vec<String>,
    reasoning_effort: Option<&str>,
) {
    for spec in specs {
        match spec {
            ProviderArgSpec::Literal(value) => args.push(value.clone()),
            ProviderArgSpec::Conditional { when, args: nested } => {
                let when = when.trim();
                if when == "model" {
                    append_literals(nested, model_args);
                } else if matches!(when, "reasoningEffort" | "reasoning_effort")
                    && reasoning_effort.is_some_and(|value| !value.trim().is_empty())
                {
                    append_literals(nested, args);
                }
            }
        }
    }
}

fn append_literals(specs: &[ProviderArgSpec], out: &mut Vec<String>) {
    for spec in specs {
        match spec {
            ProviderArgSpec::Literal(value) => out.push(value.clone()),
            ProviderArgSpec::Conditional { args, .. } => append_literals(args, out),
        }
    }
}

fn output_format(decoder: &ProviderDecoderSpec) -> Result<CommandOutputFormat, String> {
    match (decoder.format.as_str(), decoder.name.as_deref()) {
        ("text", _) => Ok(CommandOutputFormat::Text),
        ("builtin", Some("text")) => Ok(CommandOutputFormat::Text),
        ("builtin", Some("claude_stream_json")) => Ok(CommandOutputFormat::ClaudeStreamJson),
        ("builtin", Some("qoder_stream_json")) => Ok(CommandOutputFormat::ClaudeStreamJson),
        ("builtin", Some("copilot_jsonl_final_text")) => Ok(CommandOutputFormat::CopilotJson),
        ("builtin", Some("copilot_json")) => Ok(CommandOutputFormat::CopilotJson),
        ("builtin", Some("codex_stream_json")) => Ok(CommandOutputFormat::CodexStreamJson),
        ("builtin", Some("ndjson_lines")) => Ok(CommandOutputFormat::NdjsonLines),
        ("jsonl", _) => Ok(CommandOutputFormat::NdjsonLines),
        ("builtin", Some(other)) => Err(format!("unknown builtin decoder `{other}`")),
        (other, _) => Err(format!("unknown decoder format `{other}`")),
    }
}

fn normalize_session_tokens(args: Vec<String>) -> Vec<String> {
    args.into_iter()
        .map(|arg| arg.replace("{session.id}", "{session_id}"))
        .collect()
}

fn flatten_args_for_inventory(args: &[ProviderArgSpec]) -> Vec<String> {
    let mut out = Vec::new();
    append_literals(args, &mut out);
    out
}

fn default_mode_name(manifest: &ProviderManifest) -> &str {
    if manifest.modes.contains_key("print") {
        "print"
    } else {
        manifest
            .modes
            .keys()
            .next()
            .map(String::as_str)
            .unwrap_or("print")
    }
}

fn display_name(manifest: &ProviderManifest) -> String {
    if manifest.display_name.trim().is_empty() {
        manifest.id.clone()
    } else {
        manifest.display_name.clone()
    }
}

fn model_inventory(manifest: &ProviderManifest) -> (Option<String>, Vec<AgentModelChoice>) {
    manifest
        .models
        .as_ref()
        .map(|models| (models.default.clone(), models.choices.clone()))
        .unwrap_or((None, Vec::new()))
}

fn expand_static_command(command: &str, bin: &Path) -> String {
    command.replace("{bin}", &bin.display().to_string())
}

fn command_candidate_from_mode(mode: &ProviderModeSpec) -> Option<PathBuf> {
    let command = mode.command.trim();
    if command.is_empty() || command.contains('{') {
        return None;
    }
    Some(PathBuf::from(command))
}

fn command_exists(path: &Path) -> bool {
    if path.components().count() > 1 {
        is_executable(path)
    } else {
        true
    }
}

fn base_prompt() -> ProviderPromptSpec {
    ProviderPromptSpec {
        outputs: BTreeMap::from([
            (
                "system".into(),
                ProviderPromptOutputSpec {
                    preset: Some("loom_system".into()),
                    ..Default::default()
                },
            ),
            (
                "user".into(),
                ProviderPromptOutputSpec {
                    preset: Some("loom_turn".into()),
                    ..Default::default()
                },
            ),
            (
                "full".into(),
                ProviderPromptOutputSpec {
                    preset: Some("loom_full".into()),
                    ..Default::default()
                },
            ),
        ]),
    }
}

fn full_prompt() -> ProviderPromptSpec {
    ProviderPromptSpec {
        outputs: BTreeMap::from([(
            "full".into(),
            ProviderPromptOutputSpec {
                preset: Some("loom_full".into()),
                ..Default::default()
            },
        )]),
    }
}

fn mode(
    command: &str,
    args: Vec<ProviderArgSpec>,
    prompt: ProviderPromptSpec,
    stdout_name: &str,
    session: Option<ProviderSessionSpec>,
) -> ProviderModeSpec {
    ProviderModeSpec {
        transport: "command".into(),
        command: command.into(),
        args,
        env: BTreeMap::new(),
        stdin: None,
        prompt: Some(prompt),
        stdout: ProviderDecoderSpec {
            format: "builtin".into(),
            name: Some(stdout_name.into()),
        },
        session,
        timeout_ms: None,
        idle_timeout_ms: None,
    }
}

fn lit(value: &str) -> ProviderArgSpec {
    ProviderArgSpec::Literal(value.into())
}

fn when(when: &str, args: Vec<ProviderArgSpec>) -> ProviderArgSpec {
    ProviderArgSpec::Conditional {
        when: when.into(),
        args,
    }
}

fn manifest(
    id: &str,
    display_name: &str,
    candidates: &[&str],
    modes: BTreeMap<String, ProviderModeSpec>,
    models: &[(&str, &str)],
) -> ProviderManifest {
    ProviderManifest {
        schema_version: 1,
        id: id.into(),
        extends: None,
        display_name: display_name.into(),
        detect: ProviderDetectSpec {
            candidates: candidates
                .iter()
                .map(|candidate| (*candidate).into())
                .collect(),
        },
        modes,
        models: Some(AgentModelSpec {
            default: models.first().map(|(id, _)| (*id).into()),
            choices: models
                .iter()
                .map(|(id, label)| AgentModelChoice {
                    id: (*id).into(),
                    label: (*label).into(),
                    description: None,
                })
                .collect(),
        }),
    }
}

fn claude_manifest() -> ProviderManifest {
    let first_args = vec![
        lit("--add-dir"),
        lit("{loom.configDir}"),
        lit("--permission-mode"),
        lit("bypassPermissions"),
        lit("--output-format"),
        lit("stream-json"),
        lit("--verbose"),
        lit("--session-id"),
        lit("{session.id}"),
        when("model", vec![lit("--model"), lit("{model}")]),
        lit("--append-system-prompt"),
        lit("{prompt.system}"),
        lit("-p"),
        lit("{prompt.user}"),
    ];
    let resume_args = vec![
        lit("--add-dir"),
        lit("{loom.configDir}"),
        lit("--permission-mode"),
        lit("bypassPermissions"),
        lit("--output-format"),
        lit("stream-json"),
        lit("--verbose"),
        lit("--resume"),
        lit("{session.id}"),
        when("model", vec![lit("--model"), lit("{model}")]),
        lit("--append-system-prompt"),
        lit("{prompt.system}"),
        lit("-p"),
        lit("{prompt.user}"),
    ];
    let session = ProviderSessionSpec {
        id_source: Some(ProviderSessionIdSource::LoomUuid),
        capture: None,
        resume_args,
        scope: Some("actor_scope".into()),
    };
    manifest(
        "claude",
        "Claude Code",
        &["claude"],
        BTreeMap::from([(
            "print".into(),
            mode(
                "{bin}",
                first_args,
                base_prompt(),
                "claude_stream_json",
                Some(session),
            ),
        )]),
        &[
            ("sonnet", "Sonnet"),
            ("opus", "Opus"),
            ("claude-sonnet-4.6", "Claude Sonnet 4.6"),
            ("claude-opus-4.7", "Claude Opus 4.7"),
            ("claude-haiku-4.5", "Claude Haiku 4.5"),
        ],
    )
}

fn qoder_manifest() -> ProviderManifest {
    let first_args = vec![
        lit("--add-dir"),
        lit("{loom.configDir}"),
        lit("--yolo"),
        lit("--output-format"),
        lit("stream-json"),
        when("model", vec![lit("--model"), lit("{model}")]),
        when(
            "reasoningEffort",
            vec![lit("--reasoning-effort"), lit("{reasoningEffort}")],
        ),
        lit("--append-system-prompt"),
        lit("{prompt.system}"),
        lit("-p"),
        lit("{prompt.user}"),
    ];
    let resume_args = vec![
        lit("--add-dir"),
        lit("{loom.configDir}"),
        lit("--yolo"),
        lit("--output-format"),
        lit("stream-json"),
        when("model", vec![lit("--model"), lit("{model}")]),
        when(
            "reasoningEffort",
            vec![lit("--reasoning-effort"), lit("{reasoningEffort}")],
        ),
        lit("--append-system-prompt"),
        lit("{prompt.system}"),
        lit("--resume"),
        lit("{session.id}"),
        lit("-p"),
        lit("{prompt.user}"),
    ];
    let session = ProviderSessionSpec {
        id_source: Some(ProviderSessionIdSource::ProviderCapture),
        capture: Some("stdout_json:.session_id".into()),
        resume_args,
        scope: Some("actor_scope".into()),
    };
    manifest(
        "qoder",
        "Qoder CLI",
        &["qodercli"],
        BTreeMap::from([(
            "print".into(),
            mode(
                "{bin}",
                first_args,
                base_prompt(),
                "claude_stream_json",
                Some(session),
            ),
        )]),
        &[
            ("auto", "Auto"),
            ("ultimate", "Ultimate"),
            ("performance", "Performance"),
            ("efficient", "Efficient"),
            ("lite", "Lite"),
        ],
    )
}

fn copilot_manifest() -> ProviderManifest {
    let args = vec![
        lit("--add-dir"),
        lit("{loom.configDir}"),
        lit("--yolo"),
        lit("--output-format"),
        lit("json"),
        lit("--stream"),
        lit("off"),
        lit("--resume"),
        lit("{session.id}"),
        when("model", vec![lit("--model"), lit("{model}")]),
        when(
            "reasoningEffort",
            vec![lit("--effort"), lit("{reasoningEffort}")],
        ),
        lit("-p"),
        lit("{prompt.full}"),
    ];
    let session = ProviderSessionSpec {
        id_source: Some(ProviderSessionIdSource::LoomUuid),
        capture: None,
        resume_args: args.clone(),
        scope: Some("actor_scope".into()),
    };
    manifest(
        "copilot",
        "GitHub Copilot CLI",
        &["copilot", "copilotcli"],
        BTreeMap::from([(
            "print".into(),
            mode(
                "{bin}",
                args,
                full_prompt(),
                "copilot_jsonl_final_text",
                Some(session),
            ),
        )]),
        &[
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
        ],
    )
}

fn codex_manifest() -> ProviderManifest {
    let first_args = vec![
        lit("exec"),
        lit("--skip-git-repo-check"),
        lit("--json"),
        lit("--sandbox"),
        lit("danger-full-access"),
        lit("-c"),
        lit("sandbox_workspace_write.network_access=true"),
        lit("--add-dir"),
        lit("{loom.configDir}"),
        when("model", vec![lit("--model"), lit("{model}")]),
        lit("{prompt.full}"),
    ];
    let resume_args = vec![
        lit("exec"),
        lit("resume"),
        lit("{session.id}"),
        lit("--skip-git-repo-check"),
        lit("--json"),
        lit("--sandbox"),
        lit("danger-full-access"),
        lit("-c"),
        lit("sandbox_workspace_write.network_access=true"),
        lit("--add-dir"),
        lit("{loom.configDir}"),
        when("model", vec![lit("--model"), lit("{model}")]),
        lit("{prompt.full}"),
    ];
    let mut mode = mode(
        "{bin}",
        first_args,
        full_prompt(),
        "codex_stream_json",
        Some(ProviderSessionSpec {
            id_source: Some(ProviderSessionIdSource::ProviderCapture),
            capture: Some("stdout_json:.session_id".into()),
            resume_args,
            scope: Some("actor_scope".into()),
        }),
    );
    mode.env.insert("LOOM_NO_DAEMON".into(), "1".into());
    manifest(
        "codex",
        "Codex CLI",
        &["codex", "codexcli"],
        BTreeMap::from([("print".into(), mode)]),
        &[
            ("gpt-5.5", "GPT-5.5"),
            ("gpt-5.4", "GPT-5.4"),
            ("gpt-5.4-mini", "GPT-5.4 Mini"),
            ("gpt-5.3-codex", "GPT-5.3 Codex"),
            ("gpt-5.3-codex-spark", "GPT-5.3 Codex Spark"),
            ("gpt-5.2", "GPT-5.2"),
        ],
    )
}

fn opencode_manifest() -> ProviderManifest {
    let args = vec![
        lit("run"),
        lit("--dangerously-skip-permissions"),
        when("model", vec![lit("--model"), lit("{model}")]),
        when(
            "reasoningEffort",
            vec![lit("--variant"), lit("{reasoningEffort}")],
        ),
        lit("{prompt.full}"),
    ];
    let mut mode = mode("{bin}", args, full_prompt(), "text", None);
    mode.env.insert(
        "XDG_DATA_HOME".into(),
        "{agent.profile}/opencode/data".into(),
    );
    mode.env.insert(
        "XDG_STATE_HOME".into(),
        "{agent.profile}/opencode/state".into(),
    );
    mode.env.insert(
        "XDG_CACHE_HOME".into(),
        "{agent.profile}/opencode/cache".into(),
    );
    manifest(
        "opencode",
        "OpenCode",
        &["opencode"],
        BTreeMap::from([("print".into(), mode)]),
        &[
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
        ],
    )
}

fn find_command_in_path(candidates: &[String], path: &OsString) -> Option<PathBuf> {
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
            "loom-provider-{name}-{}",
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

    fn prompt_part(key: &'static str, content: &'static str) -> PromptPart {
        PromptPart {
            key: key.into(),
            title: key.into(),
            content: content.into(),
            role_hint: crate::adapter::PromptRoleHint::User,
        }
    }

    #[test]
    fn base_prompt_keeps_dynamic_turn_context_out_of_system_output() {
        let parts = vec![
            prompt_part("actor_context", "actor context"),
            prompt_part("bootstrap_memory", "bootstrap memory"),
            prompt_part("scope_bootstrap", "scope bootstrap"),
            prompt_part("turn_memory", "turn memory"),
            prompt_part("runtime_context", "runtime context"),
            prompt_part("user_message", "user message"),
        ];
        let outputs =
            render_prompt_outputs(Some(&base_prompt()), &parts, "full prompt").expect("outputs");

        let system = outputs.get("system").expect("system");
        assert!(system.contains("actor context"));
        assert!(system.contains("scope bootstrap"));
        assert!(!system.contains("turn memory"));
        assert!(!system.contains("runtime context"));
        assert!(!system.contains("user message"));

        let user = outputs.get("user").expect("user");
        assert!(user.contains("turn memory"));
        assert!(user.contains("runtime context"));
        assert!(user.contains("user message"));
    }

    #[test]
    fn empty_prompt_spec_still_renders_default_system_and_user_outputs() {
        let parts = vec![
            prompt_part("actor_context", "actor context"),
            prompt_part("runtime_context", "runtime context"),
            prompt_part("user_message", "user message"),
        ];
        let outputs =
            render_prompt_outputs(Some(&ProviderPromptSpec::default()), &parts, "full prompt")
                .expect("outputs");

        assert_eq!(outputs.get("full").map(String::as_str), Some("full prompt"));
        assert_eq!(
            outputs.get("system").map(String::as_str),
            Some("actor context")
        );
        assert_eq!(
            outputs.get("user").map(String::as_str),
            Some("runtime context\n\nuser message")
        );
    }

    #[test]
    fn prompt_template_missing_part_expands_to_empty_string() {
        let parts = vec![prompt_part("actor_context", "actor context")];
        let outputs = render_prompt_outputs(
            Some(&ProviderPromptSpec {
                outputs: BTreeMap::from([(
                    "system".into(),
                    ProviderPromptOutputSpec {
                        template: Some("{actor_context}\n{assignment_context}\n{json:keep}".into()),
                        ..Default::default()
                    },
                )]),
            }),
            &parts,
            "full prompt",
        )
        .expect("outputs");

        assert_eq!(
            outputs.get("system").map(String::as_str),
            Some("actor context\n\n{json:keep}")
        );
    }

    #[test]
    fn builtin_claude_uses_system_and_user_prompt_outputs() {
        let dir = temp_dir("path");
        make_executable(&dir.join("claude"));
        let registry = ProviderRegistry::load(&temp_dir("config")).expect("registry");
        let provider = registry
            .detect_with_path(dir.into_os_string())
            .expect("detect")
            .into_iter()
            .find(|provider| provider.id == "claude")
            .expect("claude");
        let transport = transport_from_manifest(
            &provider.manifest,
            provider.manifest.modes.get("print").unwrap(),
            Path::new(&provider.command),
            &AgentProviderRef {
                id: "claude".into(),
                mode: Some("print".into()),
                model: Some("sonnet".into()),
                reasoning_effort: None,
            },
        )
        .expect("transport");
        assert!(transport.args.contains(&"--append-system-prompt".into()));
        assert!(transport.args.contains(&"{prompt.system}".into()));
        assert!(transport.args.contains(&"{prompt.user}".into()));
        assert_eq!(
            transport.session.as_ref().and_then(|s| s.id_source),
            Some(CommandSessionIdSource::LoomUuid)
        );
    }

    #[test]
    fn custom_provider_manifest_loads_from_config_dir() {
        let config = temp_dir("custom");
        let providers = providers_dir(&config);
        std::fs::create_dir_all(&providers).expect("providers dir");
        std::fs::write(
            providers.join("demo.json"),
            r#"{
              "schemaVersion": 1,
              "id": "demo",
              "displayName": "Demo",
              "detect": { "candidates": ["demo-agent"] },
              "modes": {
                "print": {
                  "transport": "command",
                  "command": "{bin}",
                  "prompt": { "outputs": { "full": { "preset": "loom_full" } } },
                  "args": ["run", "{prompt.full}"],
                  "stdout": { "format": "text" }
                }
              }
            }"#,
        )
        .expect("write provider");
        let registry = ProviderRegistry::load(&config).expect("registry");
        assert!(registry.get("demo").is_some());
    }
}
