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
use std::path::{Component, Path, PathBuf};

use proto::methods::{
    AgentModelChoice, AgentModelSpec, AgentProviderRef, AgentTransport, ClaudeSettingsMode,
    ClaudeSettingsSpec, CommandOutputFormat, CommandSession, CommandSessionIdSource,
    InteractiveCommandSpec, InteractiveCompletionContractSpec, InteractiveCompletionSpec,
    InteractiveKillAction, InteractiveKillKind, InteractiveKillSpec, InteractiveOutputSpec,
    InteractivePromptSpec, InteractiveProviderSpec, InteractiveSessionSpec, PromptVia,
    ProviderArgSpec, ProviderConditionalArgSpec, ProviderDecoderCaptureSpec,
    ProviderDecoderEmitSpec, ProviderDecoderEventSpec, ProviderDecoderSpec, ProviderDetectSpec,
    ProviderJsonConditionSpec, ProviderJsonlReduceSpec, ProviderJsonlTextReducerSpec,
    ProviderManifest, ProviderModeSpec, ProviderPromptOutputSpec, ProviderPromptRoleHint,
    ProviderPromptSpec, ProviderRenderTitle, ProviderSessionIdSource, ProviderSessionSpec,
    ProviderWorkspaceFileSpec,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::adapter::{PromptPart, PromptRoleHint};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRuntimePlan {
    pub provider_id: String,
    pub mode: String,
    pub transport_kind: String,
    pub command: String,
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arg_specs: Vec<ProviderArgSpec>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "modelArgs")]
    pub model_args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<CommandSession>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decoder: Option<ProviderDecoderSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "stderr")]
    pub stderr_decoder: Option<ProviderDecoderSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<ProviderPromptSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_timeout_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interactive: Option<InteractiveCommandSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<InteractiveProviderSpec>,
}

/// Transport-neutral events produced by provider output decoders before Loom
/// maps them into adapter-visible events or runtime bookkeeping.
#[derive(Debug, Clone)]
pub enum ProviderRuntimeEvent {
    Text { content: String, is_partial: bool },
    ToolUse { tool_name: String, input: Value },
    Status { status: String },
    Error { message: String },
    Finished { success: bool, summary: String },
    Session { session_id: String },
}

impl ProviderRuntimeEvent {
    pub fn into_session_id(self) -> Option<String> {
        match self {
            ProviderRuntimeEvent::Session { session_id } => Some(session_id),
            _ => None,
        }
    }
}

impl ProviderRuntimePlan {
    pub fn into_transport(self) -> AgentTransport {
        let output_format = self
            .decoder
            .as_ref()
            .and_then(|decoder| output_format(decoder).ok())
            .unwrap_or_default();
        AgentTransport {
            kind: self.transport_kind,
            command: self.command,
            args: self.args,
            arg_specs: self.arg_specs,
            env: self.env,
            auth_method: None,
            model: self.model,
            model_args: self.model_args,
            session: self.session,
            output_format: Some(output_format),
            decoder: self.decoder,
            stderr_decoder: self.stderr_decoder,
            prompt_via: PromptVia::Args,
            prompt: self.prompt,
            stdin: self.stdin,
            timeout_ms: self.timeout_ms,
            idle_timeout_ms: self.idle_timeout_ms,
            interactive: self.interactive,
            provider: self.provider,
        }
    }
}

impl ProviderRegistry {
    pub fn load(config_dir: &Path) -> Result<Self, String> {
        let mut manifests = BTreeMap::new();
        for manifest in builtin_provider_manifests() {
            validate_manifest(&manifest)?;
            manifests.insert(manifest.id.clone(), manifest);
        }
        load_local_manifests(config_dir, &mut manifests)?;
        Ok(Self { manifests })
    }

    pub fn manifests(&self) -> impl Iterator<Item = &ProviderManifest> {
        self.manifests.values()
    }

    pub fn get(&self, id: &str) -> Option<&ProviderManifest> {
        self.manifests.get(id)
    }

    pub fn resolve_manifest_value(&self, value: Value) -> Result<ProviderManifest, String> {
        resolve_manifest_value(&self.manifests, value)
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
        self.resolve_runtime_plan(provider_ref)
            .map(ProviderRuntimePlan::into_transport)
    }

    pub fn resolve_runtime_plan(
        &self,
        provider_ref: &AgentProviderRef,
    ) -> Result<ProviderRuntimePlan, String> {
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
        runtime_plan_from_manifest(manifest, mode_name, mode, &bin, provider_ref)
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
    validate_provider_id(&manifest.id)?;
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
        validate_mode_command(manifest, mode_name, mode)?;
        validate_decoder_spec(manifest, mode_name, "stdout", &mode.stdout)?;
        if let Some(stderr) = mode.stderr.as_ref() {
            validate_decoder_spec(manifest, mode_name, "stderr", stderr)?;
        }
        validate_prompt_references(manifest, mode_name, mode)?;
        validate_interactive_mode(manifest, mode_name, mode)?;
    }
    Ok(())
}

fn validate_provider_id(id: &str) -> Result<(), String> {
    let mut chars = id.chars();
    let Some(first) = chars.next() else {
        return Err("provider id is required".into());
    };
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return Err(format!(
            "provider id `{id}` must start with a lowercase ascii letter or digit"
        ));
    }
    if !chars
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '_' | '-' | '.'))
    {
        return Err(format!(
            "provider id `{id}` may only contain lowercase ascii letters, digits, `_`, `-`, or `.`"
        ));
    }
    Ok(())
}

fn validate_mode_command(
    manifest: &ProviderManifest,
    mode_name: &str,
    mode: &ProviderModeSpec,
) -> Result<(), String> {
    let command = mode.command.trim();
    if command == "{bin}" {
        if manifest.detect.candidates.is_empty() {
            return Err(format!(
                "provider `{}` mode `{mode_name}` uses `{{bin}}` but detect.candidates is empty",
                manifest.id
            ));
        }
        return Ok(());
    }
    if command.contains('{') || command.contains('}') {
        return Err(format!(
            "provider `{}` mode `{mode_name}` command may only use the `{{bin}}` template",
            manifest.id
        ));
    }
    if Path::new(command).components().count() > 1 {
        return Ok(());
    }
    if manifest
        .detect
        .candidates
        .iter()
        .any(|candidate| candidate == command)
    {
        return Ok(());
    }
    Err(format!(
        "provider `{}` mode `{mode_name}` command `{command}` must be `{{bin}}`, an explicit path, or one of detect.candidates",
        manifest.id
    ))
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
    let prompt_spec = spec
        .filter(|spec| !spec.outputs.is_empty())
        .unwrap_or(&default);
    for (name, output) in &prompt_spec.outputs {
        let mut rendered = render_prompt_output(output, parts, full_prompt)?;
        if name == "full" && rendered.trim().is_empty() && !full_prompt.trim().is_empty() {
            rendered = full_prompt.to_string();
        }
        outputs.insert(name.clone(), rendered);
    }
    Ok(outputs)
}

pub fn workspace_prompt_parts(
    spec: Option<&ProviderPromptSpec>,
    workspace: &Path,
) -> Result<Vec<PromptPart>, String> {
    let Some(spec) = spec else {
        return Ok(Vec::new());
    };
    let mut parts = Vec::new();
    for file in &spec.workspace_files {
        validate_workspace_file_spec(file).map_err(|err| {
            format!(
                "workspace prompt file `{}` is invalid: {err}",
                file.key.trim()
            )
        })?;
        let loom_root = workspace.join(".loom");
        let path = loom_root.join(Path::new(&file.path));
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound && file.optional => continue,
            Err(err) => {
                return Err(format!(
                    "read workspace prompt file `{}` at {}: {err}",
                    file.key,
                    path.display()
                ));
            }
        };
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "workspace prompt file `{}` at {} must not be a symlink",
                file.key,
                path.display()
            ));
        }
        let root_metadata = std::fs::symlink_metadata(&loom_root).map_err(|err| {
            format!(
                "read workspace prompt root {} for `{}`: {err}",
                loom_root.display(),
                file.key
            )
        })?;
        if root_metadata.file_type().is_symlink() {
            return Err(format!(
                "workspace prompt root {} for `{}` must not be a symlink",
                loom_root.display(),
                file.key
            ));
        }
        if !root_metadata.is_dir() {
            return Err(format!(
                "workspace prompt root {} for `{}` is not a directory",
                loom_root.display(),
                file.key
            ));
        }
        let canonical_root = std::fs::canonicalize(&loom_root).map_err(|err| {
            format!(
                "resolve workspace prompt root {} for `{}`: {err}",
                loom_root.display(),
                file.key
            )
        })?;
        let canonical_path = std::fs::canonicalize(&path).map_err(|err| {
            format!(
                "resolve workspace prompt file `{}` at {}: {err}",
                file.key,
                path.display()
            )
        })?;
        if !canonical_path.starts_with(&canonical_root) {
            return Err(format!(
                "workspace prompt file `{}` at {} resolves outside {}",
                file.key,
                path.display(),
                loom_root.display()
            ));
        }
        if !metadata.is_file() {
            return Err(format!(
                "workspace prompt file `{}` at {} is not a regular file",
                file.key,
                path.display()
            ));
        }
        if metadata.len() > file.max_bytes {
            return Err(format!(
                "workspace prompt file `{}` at {} is {} bytes, above maxBytes {}",
                file.key,
                path.display(),
                metadata.len(),
                file.max_bytes
            ));
        }
        let content = std::fs::read_to_string(&path).map_err(|err| {
            format!(
                "read workspace prompt file `{}` at {} as utf-8: {err}",
                file.key,
                path.display()
            )
        })?;
        if content.trim().is_empty() {
            continue;
        }
        let title = file
            .title
            .as_deref()
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("Workspace file: .loom/{}", file.path));
        parts.push(PromptPart {
            key: workspace_file_prompt_part_key(&file.key),
            title: title.clone(),
            rendered_content: format!("=== {title} ===\n{content}"),
            content,
            role_hint: provider_role_hint(file.role_hint),
        });
    }
    Ok(parts)
}

fn default_prompt_outputs() -> ProviderPromptSpec {
    ProviderPromptSpec {
        workspace_files: Vec::new(),
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
        let render_title = output.render_title.unwrap_or(ProviderRenderTitle::Never);
        render_prompt_template(template, &part_map, render_title)
    } else {
        let render_title = output.render_title.unwrap_or(ProviderRenderTitle::Auto);
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
                    .filter_map(|key| part_map.get(key.as_str()))
                    .map(|part| render_prompt_part(part, render_title)),
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

fn render_prompt_part(part: &PromptPart, render_title: ProviderRenderTitle) -> String {
    match render_title {
        ProviderRenderTitle::Always => {
            let title = part.title.trim();
            if title.is_empty() {
                return part.content.clone();
            }
            let heading = format!("=== {title} ===");
            if part.content.trim_start().starts_with(&heading) {
                part.content.clone()
            } else {
                format!("{heading}\n{}", part.content)
            }
        }
        ProviderRenderTitle::Never => part.content.clone(),
        ProviderRenderTitle::Auto => part.rendered_content.clone(),
    }
}

fn render_prompt_template(
    template: &str,
    parts: &BTreeMap<&str, &PromptPart>,
    render_title: ProviderRenderTitle,
) -> String {
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
                out.push_str(&render_prompt_part(part, render_title));
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
    is_builtin_prompt_part_placeholder(key) || key.starts_with("workspace_file.")
}

fn is_builtin_prompt_part_placeholder(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn preset_parts(preset: &str) -> Result<Vec<&'static str>, String> {
    match preset {
        "loom_system" => Ok(vec![
            "actor_context",
            "agent_instructions",
            "bootstrap_memory",
            "scope_bootstrap",
        ]),
        "loom_turn" => Ok(vec!["turn_memory", "runtime_context", "user_message"]),
        "loom_full" => Ok(vec![
            "actor_context",
            "agent_instructions",
            "bootstrap_memory",
            "scope_bootstrap",
            "turn_memory",
            "runtime_context",
            "user_message",
        ]),
        other => Err(format!("unknown prompt preset `{other}`")),
    }
}

fn join_parts(parts: impl IntoIterator<Item = String>, join: &str) -> String {
    parts
        .into_iter()
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join(join)
}

fn load_local_manifests(
    config_dir: &Path,
    manifests: &mut BTreeMap<String, ProviderManifest>,
) -> Result<(), String> {
    let dir = providers_dir(config_dir);
    if !dir.exists() {
        return Ok(());
    }
    let mut pending = Vec::new();
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
        let value = serde_json::from_str::<Value>(&text)
            .map_err(|e| format!("parse provider manifest `{}`: {e}", path.display()))?;
        pending.push((path, value));
    }
    pending.sort_by(|(a, _), (b, _)| a.cmp(b));
    while !pending.is_empty() {
        let mut unresolved = Vec::new();
        let mut made_progress = false;
        for (path, value) in pending {
            let header = manifest_header(&value)
                .map_err(|e| format!("parse provider manifest `{}`: {e}", path.display()))?;
            if header
                .extends
                .as_ref()
                .is_some_and(|base| !manifests.contains_key(base))
            {
                unresolved.push((path, value));
                continue;
            }
            if manifests.contains_key(&header.id) {
                return Err(format!(
                    "provider `{}` from `{}` conflicts with an existing provider id",
                    header.id,
                    path.display()
                ));
            }
            let manifest = resolve_manifest_value(manifests, value)
                .map_err(|e| format!("resolve provider manifest `{}`: {e}", path.display()))?;
            validate_manifest(&manifest)?;
            manifests.insert(manifest.id.clone(), manifest);
            made_progress = true;
        }
        if !made_progress {
            let missing = unresolved
                .iter()
                .filter_map(|(path, value)| {
                    manifest_header(value).ok().and_then(|header| {
                        header
                            .extends
                            .map(|base| format!("{} extends `{base}`", path.display()))
                    })
                })
                .collect::<Vec<_>>()
                .join(", ");
            return Err(format!("unresolved provider extends chain: {missing}"));
        }
        pending = unresolved;
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderManifestHeader {
    id: String,
    #[serde(default)]
    extends: Option<String>,
}

fn manifest_header(value: &Value) -> Result<ProviderManifestHeader, String> {
    serde_json::from_value(value.clone()).map_err(|e| e.to_string())
}

fn resolve_manifest_value(
    manifests: &BTreeMap<String, ProviderManifest>,
    value: Value,
) -> Result<ProviderManifest, String> {
    let header = manifest_header(&value)?;
    let resolved = if let Some(base_id) = header.extends.as_deref() {
        let base = manifests
            .get(base_id)
            .ok_or_else(|| format!("provider `{}` extends unknown `{base_id}`", header.id))?;
        merge_manifest_value(
            serde_json::to_value(base).map_err(|e| e.to_string())?,
            value,
        )?
    } else {
        value
    };
    let manifest: ProviderManifest = serde_json::from_value(resolved).map_err(|e| e.to_string())?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

fn merge_manifest_value(mut base: Value, patch: Value) -> Result<Value, String> {
    let base_obj = base
        .as_object_mut()
        .ok_or_else(|| "base provider manifest must be an object".to_string())?;
    let patch_obj = patch
        .as_object()
        .ok_or_else(|| "provider manifest patch must be an object".to_string())?;

    reject_unknown_keys(
        patch_obj,
        &[
            "schemaVersion",
            "id",
            "extends",
            "displayName",
            "detect",
            "modes",
            "models",
        ],
        "provider manifest patch",
    )?;

    for key in ["schemaVersion", "id", "extends", "displayName", "models"] {
        if let Some(value) = patch_obj.get(key) {
            base_obj.insert(key.to_string(), value.clone());
        }
    }
    if let Some(detect_value) = patch_obj.get("detect") {
        let detect = detect_value
            .as_object()
            .ok_or_else(|| "detect patch must be an object".to_string())?;
        reject_unknown_keys(detect, &["candidates"], "detect patch")?;
        base_obj.insert("detect".into(), Value::Object(detect.clone()));
    }
    if let Some(modes_value) = patch_obj.get("modes") {
        let patch_modes = modes_value
            .as_object()
            .ok_or_else(|| "modes patch must be an object".to_string())?;
        let base_modes = base_obj
            .entry("modes")
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| "base provider modes must be an object".to_string())?;
        for (name, patch_mode) in patch_modes {
            if let Some(base_mode) = base_modes.get_mut(name) {
                apply_mode_patch(base_mode, patch_mode)?;
            } else {
                base_modes.insert(name.clone(), patch_mode.clone());
            }
        }
    }
    Ok(base)
}

fn apply_mode_patch(base_mode: &mut Value, patch_mode: &Value) -> Result<(), String> {
    let base = base_mode
        .as_object_mut()
        .ok_or_else(|| "base provider mode must be an object".to_string())?;
    let patch = patch_mode
        .as_object()
        .ok_or_else(|| "provider mode patch must be an object".to_string())?;

    reject_unknown_keys(
        patch,
        &[
            "transport",
            "command",
            "args",
            "env",
            "stdin",
            "prompt",
            "stdout",
            "stderr",
            "session",
            "timeoutMs",
            "idleTimeoutMs",
        ],
        "provider mode patch",
    )?;

    for key in [
        "transport",
        "command",
        "stdin",
        "stdout",
        "stderr",
        "session",
        "timeoutMs",
        "idleTimeoutMs",
    ] {
        if let Some(value) = patch.get(key) {
            base.insert(key.to_string(), value.clone());
        }
    }
    if let Some(args_patch) = patch.get("args") {
        apply_args_patch(base, args_patch)?;
    }
    if let Some(env_patch) = patch.get("env") {
        apply_env_patch(base, env_patch)?;
    }
    if let Some(prompt_patch) = patch.get("prompt") {
        apply_prompt_patch(base, prompt_patch)?;
    }
    Ok(())
}

fn apply_args_patch(base: &mut Map<String, Value>, patch: &Value) -> Result<(), String> {
    if patch.is_array() {
        return Err("args patch must be an object; use args.replace to replace args".into());
    }
    let Some(obj) = patch.as_object() else {
        return Err("args patch must be an array or object".into());
    };
    reject_unknown_keys(obj, &["replace", "prepend", "append"], "args patch")?;
    if let Some(replace) = obj.get("replace") {
        ensure_array_field(replace, "args.replace")?;
        base.insert("args".into(), replace.clone());
        return Ok(());
    }
    let mut args = base
        .get("args")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if let Some(prepend) = obj.get("prepend") {
        let mut next = prepend
            .as_array()
            .ok_or_else(|| "args.prepend must be an array".to_string())?
            .clone();
        next.extend(args);
        args = next;
    }
    if let Some(append) = obj.get("append") {
        args.extend(
            append
                .as_array()
                .ok_or_else(|| "args.append must be an array".to_string())?
                .iter()
                .cloned(),
        );
    }
    base.insert("args".into(), Value::Array(args));
    Ok(())
}

fn apply_env_patch(base: &mut Map<String, Value>, patch: &Value) -> Result<(), String> {
    let obj = patch
        .as_object()
        .ok_or_else(|| "env patch must be an object".to_string())?;
    let is_patch = obj.contains_key("merge") || obj.contains_key("unset");
    if !is_patch {
        return if obj.is_empty() {
            Ok(())
        } else {
            Err("env patch must use merge and/or unset".into())
        };
    }
    reject_unknown_keys(obj, &["merge", "unset"], "env patch")?;
    let mut env = base
        .get("env")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if let Some(unset) = obj.get("unset") {
        for key in unset
            .as_array()
            .ok_or_else(|| "env.unset must be an array".to_string())?
        {
            let key = key
                .as_str()
                .ok_or_else(|| "env.unset entries must be strings".to_string())?;
            env.remove(key);
        }
    }
    if let Some(merge) = obj.get("merge") {
        for (key, value) in merge
            .as_object()
            .ok_or_else(|| "env.merge must be an object".to_string())?
        {
            env.insert(key.clone(), value.clone());
        }
    }
    base.insert("env".into(), Value::Object(env));
    Ok(())
}

fn apply_prompt_patch(base: &mut Map<String, Value>, patch: &Value) -> Result<(), String> {
    let obj = patch
        .as_object()
        .ok_or_else(|| "prompt patch must be an object".to_string())?;
    if obj.is_empty() {
        return Ok(());
    }
    reject_unknown_keys(obj, &["outputs", "workspaceFiles"], "prompt patch")?;
    let prompt = base
        .entry("prompt")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| "base prompt must be an object".to_string())?;
    if let Some(workspace_files) = obj.get("workspaceFiles") {
        if !workspace_files.is_array() {
            return Err("prompt.workspaceFiles must be an array".into());
        }
        prompt.insert("workspaceFiles".into(), workspace_files.clone());
    }
    if let Some(outputs_patch) = obj.get("outputs") {
        let outputs = prompt
            .entry("outputs")
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| "base prompt.outputs must be an object".to_string())?;
        for (name, output) in outputs_patch
            .as_object()
            .ok_or_else(|| "prompt.outputs must be an object".to_string())?
        {
            outputs.insert(name.clone(), output.clone());
        }
    }
    Ok(())
}

fn reject_unknown_keys(
    obj: &Map<String, Value>,
    allowed: &[&str],
    where_: &str,
) -> Result<(), String> {
    for key in obj.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(format!("{where_} contains unknown field `{key}`"));
        }
    }
    Ok(())
}

fn ensure_array_field(value: &Value, name: &str) -> Result<(), String> {
    if value.is_array() {
        Ok(())
    } else {
        Err(format!("{name} must be an array"))
    }
}

fn validate_prompt_references(
    manifest: &ProviderManifest,
    mode_name: &str,
    mode: &ProviderModeSpec,
) -> Result<(), String> {
    let outputs = prompt_output_names(mode.prompt.as_ref());
    validate_prompt_outputs(manifest, mode_name, mode.prompt.as_ref())?;
    validate_template_variables(manifest, mode_name, mode, &outputs)?;
    validate_session_spec(manifest, mode_name, mode)?;
    let references = prompt_references_in_mode(mode);
    for name in &references {
        if !outputs.contains(name) {
            return Err(format!(
                "provider `{}` mode `{mode_name}` references unknown prompt output `{name}`",
                manifest.id
            ));
        }
    }
    if mode.transport == "command" {
        if !mode_has_prompt_reference(mode, false) {
            return Err(format!(
                "provider `{}` mode `{mode_name}` must pass one prompt output through args, env, or stdin",
                manifest.id
            ));
        }
        if mode
            .session
            .as_ref()
            .is_some_and(|session| !session.resume_args.is_empty())
            && !mode_has_prompt_reference(mode, true)
        {
            return Err(format!(
                "provider `{}` mode `{mode_name}` session resumeArgs must pass one prompt output",
                manifest.id
            ));
        }
    }
    Ok(())
}

fn validate_interactive_mode(
    manifest: &ProviderManifest,
    mode_name: &str,
    mode: &ProviderModeSpec,
) -> Result<(), String> {
    if mode.transport != "interactive_command" {
        return Ok(());
    }
    let Some(interactive) = mode.interactive.as_ref() else {
        return Err(format!(
            "provider `{}` mode `{mode_name}` uses interactive_command but missing interactive config",
            manifest.id
        ));
    };
    if interactive.session.new_args.is_empty() {
        return Err(format!(
            "provider `{}` mode `{mode_name}` interactive.session.newArgs is required",
            manifest.id
        ));
    }
    if !string_args_have_prompt_reference(&interactive.session.new_args) {
        return Err(format!(
            "provider `{}` mode `{mode_name}` interactive.session.newArgs must pass the prompt",
            manifest.id
        ));
    }
    if !interactive.session.resume_args.is_empty()
        && !string_args_have_prompt_reference(&interactive.session.resume_args)
    {
        return Err(format!(
            "provider `{}` mode `{mode_name}` interactive.session.resumeArgs must pass the prompt",
            manifest.id
        ));
    }
    Ok(())
}

fn string_args_have_prompt_reference(args: &[String]) -> bool {
    args.iter()
        .any(|value| template_placeholders(value).any(|name| is_prompt_delivery_var(&name)))
}

fn prompt_output_names(prompt: Option<&ProviderPromptSpec>) -> HashSet<String> {
    prompt
        .filter(|prompt| !prompt.outputs.is_empty())
        .map(|prompt| prompt.outputs.keys().cloned().collect())
        .unwrap_or_else(|| HashSet::from(["full".into()]))
}

fn validate_session_spec(
    manifest: &ProviderManifest,
    mode_name: &str,
    mode: &ProviderModeSpec,
) -> Result<(), String> {
    let Some(session) = mode.session.as_ref() else {
        return Ok(());
    };
    if let Some(scope) = session.scope.as_deref() {
        if !matches!(scope, "actor_scope" | "actor" | "turn") {
            return Err(format!(
                "provider `{}` mode `{mode_name}` uses unsupported session scope `{scope}`",
                manifest.id
            ));
        }
    }
    if matches!(
        session.id_source,
        Some(ProviderSessionIdSource::ProviderCapture)
    ) && decoder_session_capture(mode).is_none()
    {
        return Err(format!(
            "provider `{}` mode `{mode_name}` uses provider_capture but stdout/stderr capture.session is missing",
            manifest.id
        ));
    }
    Ok(())
}

fn decoder_session_capture(mode: &ProviderModeSpec) -> Option<&ProviderJsonlTextReducerSpec> {
    mode.stdout
        .capture
        .as_ref()
        .and_then(|capture| capture.session.as_ref())
        .or_else(|| {
            mode.stderr
                .as_ref()
                .and_then(|stderr| stderr.capture.as_ref())
                .and_then(|capture| capture.session.as_ref())
        })
}

fn validate_decoder_spec(
    manifest: &ProviderManifest,
    mode_name: &str,
    stream: &str,
    decoder: &ProviderDecoderSpec,
) -> Result<(), String> {
    output_format(decoder).map_err(|err| {
        format!(
            "provider `{}` mode `{mode_name}` {stream} decoder: {err}",
            manifest.id
        )
    })?;
    for (idx, event) in decoder.events.iter().enumerate() {
        if let Some(condition) = event.when.as_ref() {
            validate_json_condition(
                manifest,
                mode_name,
                &format!("{stream}.events[{idx}].when"),
                condition,
            )?;
        }
        validate_decoder_emit(
            manifest,
            mode_name,
            &format!("{stream}.events[{idx}].emit"),
            &event.emit,
        )?;
    }
    if let Some(reduce) = decoder.reduce.as_ref() {
        if let Some(final_text) = reduce.final_text.as_ref() {
            validate_jsonl_text_reducer(
                manifest,
                mode_name,
                &format!("{stream}.reduce.finalText"),
                final_text,
            )?;
        }
    }
    if let Some(capture) = decoder.capture.as_ref() {
        if let Some(session) = capture.session.as_ref() {
            validate_jsonl_text_reducer(
                manifest,
                mode_name,
                &format!("{stream}.capture.session"),
                session,
            )?;
        }
    }
    Ok(())
}

fn validate_decoder_emit(
    manifest: &ProviderManifest,
    mode_name: &str,
    where_: &str,
    emit: &ProviderDecoderEmitSpec,
) -> Result<(), String> {
    let emit_type = emit.emit_type.trim();
    match emit_type {
        "text" => {
            require_decoder_template(manifest, mode_name, where_, "text", emit.text.as_deref())?;
            validate_optional_decoder_template(manifest, mode_name, where_, "text", &emit.text)?;
        }
        "tool_use" | "toolUse" | "tool" => {
            require_decoder_template(
                manifest,
                mode_name,
                where_,
                "toolName",
                emit.tool_name.as_deref(),
            )?;
            validate_optional_decoder_template(
                manifest,
                mode_name,
                where_,
                "toolName",
                &emit.tool_name,
            )?;
            validate_optional_decoder_template(manifest, mode_name, where_, "input", &emit.input)?;
        }
        "status" => {
            if emit.status.as_ref().or(emit.text.as_ref()).is_none() {
                return Err(format!(
                    "provider `{}` mode `{mode_name}` {where_} status event requires `status` or `text`",
                    manifest.id
                ));
            }
            validate_optional_decoder_template(
                manifest,
                mode_name,
                where_,
                "status",
                &emit.status,
            )?;
            validate_optional_decoder_template(manifest, mode_name, where_, "text", &emit.text)?;
        }
        "error" => {
            if emit.message.as_ref().or(emit.text.as_ref()).is_none() {
                return Err(format!(
                    "provider `{}` mode `{mode_name}` {where_} error event requires `message` or `text`",
                    manifest.id
                ));
            }
            validate_optional_decoder_template(
                manifest,
                mode_name,
                where_,
                "message",
                &emit.message,
            )?;
            validate_optional_decoder_template(manifest, mode_name, where_, "text", &emit.text)?;
        }
        "finish" | "finished" => {
            validate_optional_decoder_template(
                manifest,
                mode_name,
                where_,
                "summary",
                &emit.summary,
            )?;
            validate_optional_decoder_template(
                manifest,
                mode_name,
                where_,
                "message",
                &emit.message,
            )?;
        }
        "" => {
            return Err(format!(
                "provider `{}` mode `{mode_name}` {where_} type is required",
                manifest.id
            ));
        }
        other => {
            return Err(format!(
                "provider `{}` mode `{mode_name}` {where_} uses unsupported event type `{other}`",
                manifest.id
            ));
        }
    }
    Ok(())
}

fn require_decoder_template(
    manifest: &ProviderManifest,
    mode_name: &str,
    where_: &str,
    field: &str,
    value: Option<&str>,
) -> Result<(), String> {
    if value.is_some_and(|value| !value.trim().is_empty()) {
        return Ok(());
    }
    Err(format!(
        "provider `{}` mode `{mode_name}` {where_} {field} is required",
        manifest.id
    ))
}

fn validate_optional_decoder_template(
    manifest: &ProviderManifest,
    mode_name: &str,
    where_: &str,
    field: &str,
    value: &Option<String>,
) -> Result<(), String> {
    let Some(value) = value.as_deref() else {
        return Ok(());
    };
    let value = value.trim();
    if value.starts_with('$') || value.starts_with('.') {
        validate_json_path_syntax(value).map_err(|err| {
            format!(
                "provider `{}` mode `{mode_name}` {where_}.{field}: {err}",
                manifest.id
            )
        })?;
    }
    Ok(())
}

fn validate_jsonl_text_reducer(
    manifest: &ProviderManifest,
    mode_name: &str,
    where_: &str,
    reducer: &ProviderJsonlTextReducerSpec,
) -> Result<(), String> {
    let mode = reducer.mode.trim();
    if !matches!(
        mode,
        "lastNonEmpty"
            | "last_non_empty"
            | "firstNonEmpty"
            | "first_non_empty"
            | "concat"
            | "joinText"
            | "join_text"
    ) {
        return Err(format!(
            "provider `{}` mode `{mode_name}` {where_} uses unsupported reducer mode `{mode}`",
            manifest.id
        ));
    }
    validate_json_path_syntax(&reducer.path).map_err(|err| {
        format!(
            "provider `{}` mode `{mode_name}` {where_}.path: {err}",
            manifest.id
        )
    })?;
    if let Some(condition) = reducer.when.as_ref() {
        validate_json_condition(manifest, mode_name, &format!("{where_}.when"), condition)?;
    }
    if let Some(fallback) = reducer.fallback.as_deref() {
        validate_jsonl_text_reducer(manifest, mode_name, &format!("{where_}.fallback"), fallback)?;
    }
    Ok(())
}

fn validate_json_condition(
    manifest: &ProviderManifest,
    mode_name: &str,
    where_: &str,
    condition: &ProviderJsonConditionSpec,
) -> Result<(), String> {
    if let Some(path) = condition.path.as_deref() {
        validate_json_path_syntax(path).map_err(|err| {
            format!(
                "provider `{}` mode `{mode_name}` {where_}.path: {err}",
                manifest.id
            )
        })?;
    }
    for (idx, item) in condition.all.iter().enumerate() {
        validate_json_condition(manifest, mode_name, &format!("{where_}.all[{idx}]"), item)?;
    }
    for (idx, item) in condition.any.iter().enumerate() {
        validate_json_condition(manifest, mode_name, &format!("{where_}.any[{idx}]"), item)?;
    }
    if let Some(item) = condition.not.as_deref() {
        validate_json_condition(manifest, mode_name, &format!("{where_}.not"), item)?;
    }
    Ok(())
}

fn validate_json_path_syntax(path: &str) -> Result<(), String> {
    let raw = path.trim();
    if raw.is_empty() {
        return Err("json path is required".into());
    }
    let path = raw
        .strip_prefix("$.")
        .or_else(|| raw.strip_prefix('.'))
        .unwrap_or(raw);
    if path.is_empty() || path == "$" {
        return Err(format!("unsupported json path `{raw}`"));
    }
    for segment in path.split('.') {
        if segment.is_empty() {
            return Err(format!("unsupported empty json path segment in `{raw}`"));
        }
        validate_json_path_segment(raw, segment)?;
    }
    Ok(())
}

fn validate_json_path_segment(path: &str, segment: &str) -> Result<(), String> {
    let mut rest = segment;
    if let Some(open) = rest.find('[') {
        let key = &rest[..open];
        if !key.is_empty() && !valid_json_path_key(key) {
            return Err(format!(
                "unsupported json path segment `{segment}` in `{path}`"
            ));
        }
        rest = &rest[open..];
    } else {
        if !valid_json_path_key(rest) {
            return Err(format!(
                "unsupported json path segment `{segment}` in `{path}`"
            ));
        }
        return Ok(());
    }
    while !rest.is_empty() {
        if !rest.starts_with('[') {
            return Err(format!(
                "unsupported json path segment `{segment}` in `{path}`"
            ));
        }
        let Some(close) = rest.find(']') else {
            return Err(format!("unterminated json path index in `{path}`"));
        };
        let index = &rest[1..close];
        if index != "*" && (index.is_empty() || !index.chars().all(|ch| ch.is_ascii_digit())) {
            return Err(format!("unsupported json path index `{index}` in `{path}`"));
        }
        rest = &rest[close + 1..];
    }
    Ok(())
}

fn valid_json_path_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
}

fn validate_workspace_file_spec(file: &ProviderWorkspaceFileSpec) -> Result<(), String> {
    validate_workspace_file_key(&file.key)?;
    validate_workspace_file_path(&file.path)?;
    if file.max_bytes == 0 {
        return Err("maxBytes must be greater than zero".into());
    }
    if let Some(title) = file.title.as_deref() {
        if title.contains('\0') {
            return Err("title must not contain NUL bytes".into());
        }
    }
    Ok(())
}

fn validate_workspace_file_key(key: &str) -> Result<(), String> {
    if !is_workspace_file_key(key) {
        return Err(format!(
            "key `{key}` must start with a lowercase letter or digit and contain only lowercase letters, digits, `_`, or `-`"
        ));
    }
    Ok(())
}

fn is_workspace_file_key(key: &str) -> bool {
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_lowercase() || first.is_ascii_digit())
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-')
}

fn validate_workspace_file_path(path: &str) -> Result<(), String> {
    if path.trim().is_empty() {
        return Err("path must not be empty".into());
    }
    if path.contains('\0') {
        return Err("path must not contain NUL bytes".into());
    }
    if path.contains('\\') {
        return Err("path must use `/` as the separator".into());
    }
    let path = Path::new(path);
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir => {
                return Err("path must not contain `.` components".into());
            }
            Component::ParentDir => {
                return Err("path must not contain `..` components".into());
            }
            Component::Prefix(_) | Component::RootDir => {
                return Err("path must be relative under .loom".into());
            }
        }
    }
    Ok(())
}

fn workspace_file_prompt_part_key(key: &str) -> String {
    format!("workspace_file.{key}")
}

fn provider_role_hint(role_hint: Option<ProviderPromptRoleHint>) -> PromptRoleHint {
    match role_hint {
        Some(ProviderPromptRoleHint::User) => PromptRoleHint::User,
        Some(ProviderPromptRoleHint::System) | None => PromptRoleHint::System,
    }
}

fn validate_prompt_outputs(
    manifest: &ProviderManifest,
    mode_name: &str,
    prompt: Option<&ProviderPromptSpec>,
) -> Result<(), String> {
    let Some(prompt) = prompt else {
        return Ok(());
    };
    let mut workspace_parts = HashSet::new();
    for file in &prompt.workspace_files {
        validate_workspace_file_spec(file).map_err(|err| {
            format!(
                "provider `{}` mode `{mode_name}` workspace file `{}`: {err}",
                manifest.id, file.key
            )
        })?;
        let part_key = workspace_file_prompt_part_key(&file.key);
        if !workspace_parts.insert(part_key.clone()) {
            return Err(format!(
                "provider `{}` mode `{mode_name}` declares duplicate workspace prompt part `{part_key}`",
                manifest.id
            ));
        }
    }
    for (output_name, output) in &prompt.outputs {
        if let Some(preset) = output.preset.as_deref() {
            preset_parts(preset).map_err(|err| {
                format!(
                    "provider `{}` mode `{mode_name}` prompt output `{output_name}`: {err}",
                    manifest.id
                )
            })?;
        }
        for key in output.include.iter().chain(output.required.iter()) {
            if key == "full" {
                continue;
            }
            if !is_known_prompt_part(key) && !workspace_parts.contains(key) {
                return Err(format!(
                    "provider `{}` mode `{mode_name}` prompt output `{output_name}` references unknown prompt part `{key}`",
                    manifest.id
                ));
            }
        }
        if let Some(template) = output.template.as_deref() {
            for key in prompt_part_placeholders(template) {
                if !is_known_prompt_part(&key) && !workspace_parts.contains(&key) {
                    return Err(format!(
                        "provider `{}` mode `{mode_name}` prompt output `{output_name}` references unknown prompt part `{key}`",
                        manifest.id
                    ));
                }
            }
        }
    }
    Ok(())
}

fn validate_template_variables(
    manifest: &ProviderManifest,
    mode_name: &str,
    mode: &ProviderModeSpec,
    outputs: &HashSet<String>,
) -> Result<(), String> {
    for (where_, value) in mode_templates(mode, false) {
        for name in template_placeholders(value) {
            validate_template_variable(manifest, mode_name, where_, &name, outputs)?;
        }
    }
    for value in &mode.model_args {
        validate_string_template(manifest, mode_name, "modelArgs", value, outputs)?;
    }
    if let Some(session) = mode.session.as_ref() {
        for arg in &session.resume_args {
            for (where_, value) in arg_templates(arg, "session.resumeArgs") {
                for name in template_placeholders(value) {
                    validate_template_variable(manifest, mode_name, where_, &name, outputs)?;
                }
            }
        }
    }
    if let Some(interactive) = mode.interactive.as_ref() {
        for value in &interactive.session.new_args {
            validate_string_template(
                manifest,
                mode_name,
                "interactive.session.newArgs",
                value,
                outputs,
            )?;
        }
        for value in &interactive.session.resume_args {
            validate_string_template(
                manifest,
                mode_name,
                "interactive.session.resumeArgs",
                value,
                outputs,
            )?;
        }
        validate_string_template(
            manifest,
            mode_name,
            "interactive.prompt.template",
            &interactive.prompt.template,
            outputs,
        )?;
    }
    if let Some(path) = mode
        .provider
        .as_ref()
        .and_then(|provider| provider.settings.as_ref())
        .filter(|settings| settings.mode == ClaudeSettingsMode::Custom)
        .and_then(|settings| settings.path.as_deref())
    {
        validate_string_template(manifest, mode_name, "provider.settings.path", path, outputs)?;
    }
    validate_condition_names(manifest, mode_name, &mode.args)?;
    if let Some(session) = mode.session.as_ref() {
        validate_condition_names(manifest, mode_name, &session.resume_args)?;
    }
    Ok(())
}

fn validate_string_template(
    manifest: &ProviderManifest,
    mode_name: &str,
    where_: &str,
    value: &str,
    outputs: &HashSet<String>,
) -> Result<(), String> {
    for name in template_placeholders(value) {
        validate_template_variable(manifest, mode_name, where_, &name, outputs)?;
    }
    Ok(())
}

fn validate_template_variable(
    manifest: &ProviderManifest,
    mode_name: &str,
    where_: &str,
    name: &str,
    outputs: &HashSet<String>,
) -> Result<(), String> {
    if let Some(output) = name.strip_prefix("prompt.") {
        if outputs.contains(output) {
            return Ok(());
        }
        return Err(format!(
            "provider `{}` mode `{mode_name}` {where_} references unknown prompt output `{output}`",
            manifest.id
        ));
    }
    if is_supported_runtime_template_var(name) {
        return Ok(());
    }
    Err(format!(
        "provider `{}` mode `{mode_name}` {where_} references unknown template variable `{name}`",
        manifest.id
    ))
}

fn validate_condition_names(
    manifest: &ProviderManifest,
    mode_name: &str,
    args: &[ProviderArgSpec],
) -> Result<(), String> {
    for arg in args {
        match arg {
            ProviderArgSpec::Literal(_) => {}
            ProviderArgSpec::Conditional(spec) => {
                let when = spec.when.trim();
                if !matches!(when, "model" | "reasoningEffort" | "reasoning_effort") {
                    return Err(format!(
                        "provider `{}` mode `{mode_name}` uses unsupported args condition `{when}`",
                        manifest.id
                    ));
                }
                validate_condition_names(manifest, mode_name, &spec.args)?;
            }
        }
    }
    Ok(())
}

fn mode_has_prompt_reference(mode: &ProviderModeSpec, resume: bool) -> bool {
    mode_templates(mode, resume)
        .any(|(_, value)| template_placeholders(value).any(|name| is_prompt_delivery_var(&name)))
}

fn mode_templates<'a>(
    mode: &'a ProviderModeSpec,
    resume: bool,
) -> impl Iterator<Item = (&'static str, &'a str)> + 'a {
    let command = std::iter::once(("command", mode.command.as_str()));
    let args: Box<dyn Iterator<Item = (&'static str, &'a str)> + 'a> = if resume {
        Box::new(
            mode.session
                .as_ref()
                .into_iter()
                .flat_map(|session| session.resume_args.iter())
                .flat_map(|arg| arg_templates(arg, "session.resumeArgs")),
        )
    } else {
        Box::new(mode.args.iter().flat_map(|arg| arg_templates(arg, "args")))
    };
    let env = mode.env.values().map(|value| ("env", value.as_str()));
    let stdin = mode
        .stdin
        .as_deref()
        .map(|value| ("stdin", value))
        .into_iter();
    command.chain(args).chain(env).chain(stdin)
}

fn arg_templates<'a>(
    arg: &'a ProviderArgSpec,
    where_: &'static str,
) -> Box<dyn Iterator<Item = (&'static str, &'a str)> + 'a> {
    match arg {
        ProviderArgSpec::Literal(value) => Box::new(std::iter::once((where_, value.as_str()))),
        ProviderArgSpec::Conditional(spec) => Box::new(
            spec.args
                .iter()
                .flat_map(move |arg| arg_templates(arg, where_)),
        ),
    }
}

fn template_placeholders(value: &str) -> impl Iterator<Item = String> + '_ {
    let mut rest = value;
    std::iter::from_fn(move || loop {
        let start = rest.find('{')?;
        let after_open = &rest[start + 1..];
        let Some(end) = after_open.find('}') else {
            rest = "";
            return None;
        };
        let key = &after_open[..end];
        rest = &after_open[end + 1..];
        if is_template_placeholder(key) {
            return Some(key.to_string());
        }
    })
}

fn prompt_part_placeholders(value: &str) -> impl Iterator<Item = String> + '_ {
    template_placeholders(value).filter(|key| is_prompt_part_placeholder(key))
}

fn is_template_placeholder(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
}

fn is_prompt_delivery_var(name: &str) -> bool {
    name == "prompt" || name == "loom_envelope" || name.starts_with("prompt.")
}

fn is_supported_runtime_template_var(name: &str) -> bool {
    matches!(
        name,
        "bin"
            | "model"
            | "reasoningEffort"
            | "session.id"
            | "session_id"
            | "prompt"
            | "loom_envelope"
            | "actor.id"
            | "scope.id"
            | "scope.kind"
            | "loom.configDir"
            | "loom.server"
            | "loom.actor"
            | "loom.scope.id"
            | "loom.scope.kind"
            | "loom.run.id"
            | "loom.trigger.id"
            | "loom.trigger.actor"
            | "paths.cwd"
            | "workspace.dir"
            | "agent.root"
            | "agent.configDir"
            | "agent.specPath"
            | "agent.profile"
            | "agent.workspace"
            | "agent.logs"
            | "agent.skills"
            | "agent.bundle_root"
            | "agent.bundle"
            | "agent.skillBody"
            | "scope.skills"
            | "channel.id"
            | "channel.root"
            | "channel.shared"
            | "channel.sharedArtifacts"
            | "thread.id"
            | "trigger.id"
            | "trigger.actor_id"
            | "reply.target"
            | "prompt.activeSkill"
    ) || name.starts_with("vars.")
}

fn is_known_prompt_part(key: &str) -> bool {
    matches!(
        key,
        "trigger_prefix"
            | "actor_context"
            | "agent_instructions"
            | "bootstrap_memory"
            | "scope_bootstrap"
            | "turn_memory"
            | "runtime_context"
            | "latest_message"
            | "assignment_context"
            | "turn_input"
            | "user_message"
    )
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
        ProviderArgSpec::Conditional(spec) => {
            for arg in &spec.args {
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

#[cfg(test)]
fn transport_from_manifest(
    manifest: &ProviderManifest,
    mode: &ProviderModeSpec,
    bin: &Path,
    provider_ref: &AgentProviderRef,
) -> Result<AgentTransport, String> {
    runtime_plan_from_manifest(
        manifest,
        provider_ref
            .mode
            .as_deref()
            .unwrap_or(default_mode_name(manifest)),
        mode,
        bin,
        provider_ref,
    )
    .map(ProviderRuntimePlan::into_transport)
}

fn runtime_plan_from_manifest(
    manifest: &ProviderManifest,
    mode_name: &str,
    mode: &ProviderModeSpec,
    bin: &Path,
    provider_ref: &AgentProviderRef,
) -> Result<ProviderRuntimePlan, String> {
    let arg_specs = expand_static_arg_specs(&mode.args, bin);
    let args = flatten_args_for_inventory(&arg_specs);
    let resume_arg_specs = mode
        .session
        .as_ref()
        .map(|session| expand_static_arg_specs(&session.resume_args, bin))
        .unwrap_or_default();
    let resume_args = if resume_arg_specs.is_empty() {
        Vec::new()
    } else {
        flatten_args_for_inventory(&resume_arg_specs)
    };
    let env = mode
        .env
        .iter()
        .map(|(key, value)| (key.clone(), expand_static_template(value, bin)))
        .collect::<BTreeMap<_, _>>();
    let model_args = mode
        .model_args
        .iter()
        .map(|value| expand_static_template(value, bin))
        .collect::<Vec<_>>();
    let stdin = mode
        .stdin
        .as_ref()
        .map(|value| expand_static_template(value, bin));

    let plan = ProviderRuntimePlan {
        provider_id: manifest.id.clone(),
        mode: mode_name.to_string(),
        transport_kind: if mode.transport.trim().is_empty() {
            "command".into()
        } else {
            mode.transport.clone()
        },
        command: expand_static_command(&mode.command, bin),
        args: normalize_session_tokens(args),
        arg_specs,
        env,
        model: provider_ref
            .model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        model_args,
        session: mode.session.as_ref().map(|session| CommandSession {
            id_source: session.id_source.map(|source| match source {
                ProviderSessionIdSource::LoomUuid => CommandSessionIdSource::LoomUuid,
                ProviderSessionIdSource::ProviderCapture => CommandSessionIdSource::ProviderCapture,
            }),
            scope: session.scope.clone(),
            first_run_capture: None,
            resume_args: if resume_args.is_empty() {
                None
            } else {
                Some(normalize_session_tokens(resume_args))
            },
            resume_arg_specs,
        }),
        decoder: Some(mode.stdout.clone()),
        stderr_decoder: mode.stderr.clone(),
        prompt: mode.prompt.clone(),
        stdin,
        timeout_ms: mode.timeout_ms,
        idle_timeout_ms: mode.idle_timeout_ms,
        interactive: mode.interactive.clone(),
        provider: mode.provider.clone(),
    };
    output_format(&mode.stdout)?;
    validate_manifest(manifest)?;
    Ok(plan)
}

fn expand_static_arg_specs(specs: &[ProviderArgSpec], bin: &Path) -> Vec<ProviderArgSpec> {
    specs
        .iter()
        .map(|spec| match spec {
            ProviderArgSpec::Literal(value) => {
                ProviderArgSpec::Literal(expand_static_template(value, bin))
            }
            ProviderArgSpec::Conditional(spec) => {
                ProviderArgSpec::Conditional(ProviderConditionalArgSpec {
                    when: spec.when.clone(),
                    args: expand_static_arg_specs(&spec.args, bin),
                })
            }
        })
        .collect()
}

fn expand_static_template(value: &str, bin: &Path) -> String {
    value.replace("{bin}", &bin.display().to_string())
}

fn append_literals(specs: &[ProviderArgSpec], out: &mut Vec<String>) {
    for spec in specs {
        match spec {
            ProviderArgSpec::Literal(value) => out.push(value.clone()),
            ProviderArgSpec::Conditional(spec) => append_literals(&spec.args, out),
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
        ("json", _) => Ok(CommandOutputFormat::Text),
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
        workspace_files: Vec::new(),
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
        workspace_files: Vec::new(),
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
        model_args: Vec::new(),
        env: BTreeMap::new(),
        stdin: None,
        prompt: Some(prompt),
        stdout: ProviderDecoderSpec {
            format: "builtin".into(),
            name: Some(stdout_name.into()),
            events: Vec::new(),
            reduce: None,
            capture: None,
        },
        stderr: None,
        session,
        timeout_ms: None,
        idle_timeout_ms: None,
        interactive: None,
        provider: None,
    }
}

fn copilot_jsonl_decoder() -> ProviderDecoderSpec {
    ProviderDecoderSpec {
        format: "jsonl".into(),
        name: None,
        events: Vec::new(),
        reduce: Some(ProviderJsonlReduceSpec {
            final_text: Some(ProviderJsonlTextReducerSpec {
                mode: "lastNonEmpty".into(),
                path: "$.data.content".into(),
                when: Some(ProviderJsonConditionSpec {
                    all: vec![
                        json_condition_equals("$.type", "assistant.message"),
                        json_condition_absent_or_null("$.agentId"),
                        json_condition_absent_or_null("$.data.parentToolCallId"),
                        ProviderJsonConditionSpec {
                            path: Some("$.data.phase".into()),
                            not_in: Some(vec![json!("thinking"), json!("reasoning")]),
                            ..Default::default()
                        },
                    ],
                    ..Default::default()
                }),
                fallback: Some(Box::new(ProviderJsonlTextReducerSpec {
                    mode: "concat".into(),
                    path: "$.data.deltaContent".into(),
                    when: Some(ProviderJsonConditionSpec {
                        all: vec![
                            json_condition_equals("$.type", "assistant.message_delta"),
                            json_condition_absent_or_null("$.agentId"),
                            json_condition_absent_or_null("$.data.parentToolCallId"),
                        ],
                        ..Default::default()
                    }),
                    fallback: None,
                })),
            }),
        }),
        capture: None,
    }
}

fn opencode_jsonl_decoder() -> ProviderDecoderSpec {
    ProviderDecoderSpec {
        format: "jsonl".into(),
        name: None,
        events: vec![
            ProviderDecoderEventSpec {
                when: Some(json_condition_equals("$.type", "tool_use")),
                emit: ProviderDecoderEmitSpec {
                    emit_type: "tool_use".into(),
                    tool_name: Some("$.part.tool".into()),
                    input: Some("$.part.state.input".into()),
                    ..Default::default()
                },
            },
            ProviderDecoderEventSpec {
                when: Some(json_condition_equals("$.type", "error")),
                emit: ProviderDecoderEmitSpec {
                    emit_type: "error".into(),
                    message: Some("$.error.data.message".into()),
                    ..Default::default()
                },
            },
        ],
        reduce: Some(ProviderJsonlReduceSpec {
            final_text: Some(ProviderJsonlTextReducerSpec {
                mode: "lastNonEmpty".into(),
                path: "$.part.text".into(),
                when: Some(json_condition_equals("$.type", "text")),
                fallback: None,
            }),
        }),
        capture: Some(session_capture("$.sessionID")),
    }
}

fn session_capture(path: &str) -> ProviderDecoderCaptureSpec {
    ProviderDecoderCaptureSpec {
        session: Some(ProviderJsonlTextReducerSpec {
            mode: "lastNonEmpty".into(),
            path: path.into(),
            when: None,
            fallback: None,
        }),
    }
}

fn json_condition_equals(path: &str, value: &str) -> ProviderJsonConditionSpec {
    ProviderJsonConditionSpec {
        path: Some(path.into()),
        equals: Some(json!(value)),
        ..Default::default()
    }
}

fn json_condition_absent_or_null(path: &str) -> ProviderJsonConditionSpec {
    ProviderJsonConditionSpec {
        path: Some(path.into()),
        absent_or_null: Some(true),
        ..Default::default()
    }
}

fn lit(value: &str) -> ProviderArgSpec {
    ProviderArgSpec::Literal(value.into())
}

fn when(when: &str, args: Vec<ProviderArgSpec>) -> ProviderArgSpec {
    ProviderArgSpec::Conditional(ProviderConditionalArgSpec {
        when: when.into(),
        args,
    })
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
        lit("{agent.configDir}"),
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
        lit("{agent.configDir}"),
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
        resume_args,
        scope: Some("actor_scope".into()),
    };
    let nonprint_mode = ProviderModeSpec {
        transport: "interactive_command".into(),
        command: "{bin}".into(),
        args: vec![lit("--add-dir"), lit("{agent.configDir}")],
        model_args: vec!["--model".into(), "{model}".into()],
        env: BTreeMap::new(),
        stdin: None,
        prompt: None,
        stdout: ProviderDecoderSpec {
            format: "text".into(),
            ..Default::default()
        },
        stderr: None,
        session: None,
        timeout_ms: None,
        idle_timeout_ms: None,
        interactive: Some(InteractiveCommandSpec {
            session: InteractiveSessionSpec {
                new_args: vec![
                    "--permission-mode".into(),
                    "bypassPermissions".into(),
                    "{prompt}".into(),
                    "--session-id".into(),
                    "{session_id}".into(),
                ],
                resume_args: vec![
                    "--permission-mode".into(),
                    "bypassPermissions".into(),
                    "{prompt}".into(),
                    "--resume".into(),
                    "{session_id}".into(),
                ],
                ..Default::default()
            },
            prompt: InteractivePromptSpec {
                completion_contract: InteractiveCompletionContractSpec {
                    sentinel: "__LOOM_DONE__".into(),
                    instruction: "When your final user-visible answer is complete, output __LOOM_DONE__ on a line by itself. Do not output anything after it.".into(),
                },
                ..Default::default()
            },
            completion: InteractiveCompletionSpec {
                idle_timeout_ms: Some(60_000),
                max_turn_ms: 3_600_000,
                ..Default::default()
            },
            output: InteractiveOutputSpec::default(),
            kill: InteractiveKillSpec {
                on_complete: InteractiveKillAction {
                    action: InteractiveKillKind::Sigterm,
                    grace_ms: Some(3000),
                    fallback: Some(InteractiveKillKind::Sigkill),
                },
                on_cancel: InteractiveKillAction {
                    action: InteractiveKillKind::Sigterm,
                    grace_ms: Some(1000),
                    fallback: Some(InteractiveKillKind::Sigkill),
                },
                on_timeout: InteractiveKillAction {
                    action: InteractiveKillKind::Sigkill,
                    grace_ms: None,
                    fallback: None,
                },
            },
        }),
        provider: Some(InteractiveProviderSpec {
            kind: "claude".into(),
            settings: Some(ClaudeSettingsSpec {
                mode: ClaudeSettingsMode::Global,
                path: None,
            }),
        }),
    };
    manifest(
        "claude",
        "Claude Code",
        &["claude"],
        BTreeMap::from([
            (
                "print".into(),
                mode(
                    "{bin}",
                    first_args,
                    base_prompt(),
                    "claude_stream_json",
                    Some(session),
                ),
            ),
            ("nonprint".into(), nonprint_mode),
        ]),
        &[
            ("sonnet", "Sonnet"),
            ("opus", "Opus"),
            ("claude-sonnet-4-6", "Claude Sonnet 4.6"),
            ("claude-opus-4-7", "Claude Opus 4.7"),
            ("claude-haiku-4-5", "Claude Haiku 4.5"),
        ],
    )
}

fn qoder_manifest() -> ProviderManifest {
    let first_args = vec![
        lit("--add-dir"),
        lit("{agent.configDir}"),
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
        lit("{agent.configDir}"),
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
        resume_args,
        scope: Some("actor_scope".into()),
    };
    manifest(
        "qoder",
        "Qoder CLI",
        &["qodercli"],
        BTreeMap::from([("print".into(), {
            let mut mode = mode(
                "{bin}",
                first_args,
                base_prompt(),
                "claude_stream_json",
                Some(session),
            );
            mode.stdout.capture = Some(session_capture("$.session_id"));
            mode
        })]),
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
        lit("{agent.configDir}"),
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
        resume_args: args.clone(),
        scope: Some("actor_scope".into()),
    };
    manifest(
        "copilot",
        "GitHub Copilot CLI",
        &["copilot", "copilotcli"],
        BTreeMap::from([("print".into(), {
            let mut mode = mode(
                "{bin}",
                args,
                full_prompt(),
                "copilot_jsonl_final_text",
                Some(session),
            );
            mode.stdout = copilot_jsonl_decoder();
            mode
        })]),
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
        lit("{agent.configDir}"),
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
        lit("{agent.configDir}"),
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
            resume_args,
            scope: Some("actor_scope".into()),
        }),
    );
    mode.stdout.capture = Some(session_capture("$.session_id"));
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
    let first_args = vec![
        lit("run"),
        lit("--dangerously-skip-permissions"),
        lit("--format"),
        lit("json"),
        when("model", vec![lit("--model"), lit("{model}")]),
        when(
            "reasoningEffort",
            vec![lit("--variant"), lit("{reasoningEffort}")],
        ),
        lit("{prompt.full}"),
    ];
    let resume_args = vec![
        lit("run"),
        lit("--dangerously-skip-permissions"),
        lit("--format"),
        lit("json"),
        lit("--session"),
        lit("{session.id}"),
        when("model", vec![lit("--model"), lit("{model}")]),
        when(
            "reasoningEffort",
            vec![lit("--variant"), lit("{reasoningEffort}")],
        ),
        lit("{prompt.full}"),
    ];
    let mut mode = mode(
        "{bin}",
        first_args,
        full_prompt(),
        "text",
        Some(ProviderSessionSpec {
            id_source: Some(ProviderSessionIdSource::ProviderCapture),
            resume_args,
            scope: Some("actor_scope".into()),
        }),
    );
    mode.stdout = opencode_jsonl_decoder();
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
            ("opencode/big-pickle", "OpenCode Big Pickle"),
            (
                "opencode/deepseek-v4-flash-free",
                "OpenCode DeepSeek V4 Flash Free",
            ),
            ("opencode/mimo-v2.5-free", "OpenCode Mimo V2.5 Free"),
            (
                "opencode/nemotron-3-super-free",
                "OpenCode Nemotron 3 Super Free",
            ),
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
    use proto::methods::ProviderDecoderEventSpec;

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
            rendered_content: content.into(),
            role_hint: crate::adapter::PromptRoleHint::User,
        }
    }

    fn provider_manifest_parse_error(text: &str) -> String {
        serde_json::from_str::<ProviderManifest>(text)
            .expect_err("provider manifest should fail schema parsing")
            .to_string()
    }

    fn provider_registry_load_error(name: &str, text: &str) -> String {
        let config = temp_dir(name);
        let providers = providers_dir(&config);
        std::fs::create_dir_all(&providers).expect("providers dir");
        std::fs::write(providers.join("provider.json"), text).expect("write provider");
        ProviderRegistry::load(&config)
            .expect_err("provider registry should reject manifest")
            .to_string()
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
    fn empty_prompt_spec_renders_only_default_full_output() {
        let parts = vec![
            prompt_part("actor_context", "actor context"),
            prompt_part("runtime_context", "runtime context"),
            prompt_part("user_message", "user message"),
        ];
        let outputs =
            render_prompt_outputs(Some(&ProviderPromptSpec::default()), &parts, "full prompt")
                .expect("outputs");

        assert_eq!(
            outputs.get("full").map(String::as_str),
            Some("actor context\n\nruntime context\n\nuser message")
        );
        assert!(!outputs.contains_key("system"));
        assert!(!outputs.contains_key("user"));
    }

    #[test]
    fn declared_prompt_outputs_do_not_create_implicit_full_system_or_user() {
        let parts = vec![prompt_part("actor_context", "actor context")];
        let outputs = render_prompt_outputs(
            Some(&ProviderPromptSpec {
                workspace_files: Vec::new(),
                outputs: BTreeMap::from([(
                    "system".into(),
                    ProviderPromptOutputSpec {
                        include: vec!["actor_context".into()],
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
            Some("actor context")
        );
        assert!(!outputs.contains_key("full"));
        assert!(!outputs.contains_key("user"));
    }

    #[test]
    fn prompt_template_missing_part_expands_to_empty_string() {
        let parts = vec![prompt_part("actor_context", "actor context")];
        let outputs = render_prompt_outputs(
            Some(&ProviderPromptSpec {
                workspace_files: Vec::new(),
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
    fn render_title_controls_prompt_part_headings() {
        let parts = vec![PromptPart {
            key: "actor_context".into(),
            title: "System: Loom actor context".into(),
            content: "You are @demo.".into(),
            rendered_content: "=== System: Loom actor context ===\nYou are @demo.".into(),
            role_hint: crate::adapter::PromptRoleHint::System,
        }];
        let outputs = render_prompt_outputs(
            Some(&ProviderPromptSpec {
                workspace_files: Vec::new(),
                outputs: BTreeMap::from([
                    (
                        "with_title".into(),
                        ProviderPromptOutputSpec {
                            include: vec!["actor_context".into()],
                            render_title: Some(ProviderRenderTitle::Always),
                            ..Default::default()
                        },
                    ),
                    (
                        "raw".into(),
                        ProviderPromptOutputSpec {
                            include: vec!["actor_context".into()],
                            render_title: Some(ProviderRenderTitle::Never),
                            ..Default::default()
                        },
                    ),
                ]),
            }),
            &parts,
            "full prompt",
        )
        .expect("outputs");

        assert_eq!(
            outputs.get("with_title").map(String::as_str),
            Some("=== System: Loom actor context ===\nYou are @demo.")
        );
        assert_eq!(
            outputs.get("raw").map(String::as_str),
            Some("You are @demo.")
        );
    }

    #[test]
    fn render_title_always_does_not_duplicate_existing_heading() {
        let parts = vec![PromptPart {
            key: "actor_context".into(),
            title: "System: Loom actor context".into(),
            content: "You are @demo.".into(),
            rendered_content: "=== System: Loom actor context ===\nYou are @demo.".into(),
            role_hint: crate::adapter::PromptRoleHint::System,
        }];
        let outputs = render_prompt_outputs(
            Some(&ProviderPromptSpec {
                workspace_files: Vec::new(),
                outputs: BTreeMap::from([(
                    "system".into(),
                    ProviderPromptOutputSpec {
                        template: Some("{actor_context}".into()),
                        render_title: Some(ProviderRenderTitle::Always),
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
            Some("=== System: Loom actor context ===\nYou are @demo.")
        );
    }

    #[test]
    fn manifest_validation_rejects_command_mode_without_prompt_delivery() {
        let manifest = manifest(
            "no_prompt",
            "No Prompt",
            &["no-prompt"],
            BTreeMap::from([(
                "print".into(),
                mode("{bin}", vec![lit("run")], full_prompt(), "text", None),
            )]),
            &[],
        );

        let err = validate_manifest(&manifest).expect_err("missing prompt should fail");
        assert!(err.contains("must pass one prompt output"), "{err}");
    }

    #[test]
    fn manifest_validation_rejects_unstable_provider_id() {
        let manifest = manifest(
            "Bad Provider",
            "Bad Provider",
            &["bad-provider"],
            BTreeMap::from([(
                "print".into(),
                mode(
                    "{bin}",
                    vec![lit("{prompt.full}")],
                    full_prompt(),
                    "text",
                    None,
                ),
            )]),
            &[],
        );

        let err = validate_manifest(&manifest).expect_err("bad provider id should fail");
        assert!(err.contains("provider id `Bad Provider`"), "{err}");
    }

    #[test]
    fn manifest_validation_rejects_command_not_bound_to_detect_or_path() {
        let manifest = manifest(
            "bad_command",
            "Bad Command",
            &["safe-agent"],
            BTreeMap::from([(
                "print".into(),
                mode(
                    "other-agent",
                    vec![lit("{prompt.full}")],
                    full_prompt(),
                    "text",
                    None,
                ),
            )]),
            &[],
        );

        let err = validate_manifest(&manifest).expect_err("unbound command should fail");
        assert!(
            err.contains("command `other-agent` must be `{bin}`, an explicit path, or one of detect.candidates"),
            "{err}"
        );
    }

    #[test]
    fn manifest_validation_allows_candidate_command_and_explicit_path() {
        let candidate_command = manifest(
            "candidate_command",
            "Candidate Command",
            &["candidate-agent"],
            BTreeMap::from([(
                "print".into(),
                mode(
                    "candidate-agent",
                    vec![lit("{prompt.full}")],
                    full_prompt(),
                    "text",
                    None,
                ),
            )]),
            &[],
        );
        validate_manifest(&candidate_command).expect("candidate command should pass");

        let explicit_path = manifest(
            "explicit_path",
            "Explicit Path",
            &[],
            BTreeMap::from([(
                "print".into(),
                mode(
                    "/opt/loom/providers/agent",
                    vec![lit("{prompt.full}")],
                    full_prompt(),
                    "text",
                    None,
                ),
            )]),
            &[],
        );
        validate_manifest(&explicit_path).expect("explicit path should pass");
    }

    #[test]
    fn manifest_validation_rejects_non_bin_command_template() {
        let manifest = manifest(
            "bad_command_template",
            "Bad Command Template",
            &["bad-command-template"],
            BTreeMap::from([(
                "print".into(),
                mode(
                    "{loom.configDir}/provider",
                    vec![lit("{prompt.full}")],
                    full_prompt(),
                    "text",
                    None,
                ),
            )]),
            &[],
        );

        let err = validate_manifest(&manifest).expect_err("command template should fail");
        assert!(
            err.contains("command may only use the `{bin}` template"),
            "{err}"
        );
    }

    #[test]
    fn manifest_validation_rejects_unknown_template_variable() {
        let manifest = manifest(
            "bad_var",
            "Bad Var",
            &["bad-var"],
            BTreeMap::from([(
                "print".into(),
                mode(
                    "{bin}",
                    vec![lit("{prompt.full}"), lit("{unknown.var}")],
                    full_prompt(),
                    "text",
                    None,
                ),
            )]),
            &[],
        );

        let err = validate_manifest(&manifest).expect_err("unknown variable should fail");
        assert!(
            err.contains("unknown template variable `unknown.var`"),
            "{err}"
        );
    }

    #[test]
    fn manifest_validation_rejects_undeclared_prompt_output_reference() {
        let manifest = manifest(
            "bad_prompt_output",
            "Bad Prompt Output",
            &["bad-prompt-output"],
            BTreeMap::from([(
                "print".into(),
                mode(
                    "{bin}",
                    vec![lit("--system"), lit("{prompt.system}")],
                    ProviderPromptSpec::default(),
                    "text",
                    None,
                ),
            )]),
            &[],
        );

        let err = validate_manifest(&manifest).expect_err("unknown prompt output should fail");
        assert!(
            err.contains("references unknown prompt output `system`"),
            "{err}"
        );
    }

    #[test]
    fn manifest_validation_rejects_unknown_prompt_part_reference() {
        let manifest = manifest(
            "bad_part",
            "Bad Part",
            &["bad-part"],
            BTreeMap::from([(
                "print".into(),
                mode(
                    "{bin}",
                    vec![lit("{prompt.full}")],
                    ProviderPromptSpec {
                        workspace_files: Vec::new(),
                        outputs: BTreeMap::from([(
                            "full".into(),
                            ProviderPromptOutputSpec {
                                include: vec!["actor_context".into(), "identity".into()],
                                ..Default::default()
                            },
                        )]),
                    },
                    "text",
                    None,
                ),
            )]),
            &[],
        );

        let err = validate_manifest(&manifest).expect_err("unknown part should fail");
        assert!(err.contains("unknown prompt part `identity`"), "{err}");
    }

    #[test]
    fn manifest_validation_accepts_declared_workspace_prompt_part() {
        let manifest = manifest(
            "workspace_part",
            "Workspace Part",
            &["workspace-part"],
            BTreeMap::from([(
                "print".into(),
                mode(
                    "{bin}",
                    vec![lit("{prompt.full}")],
                    ProviderPromptSpec {
                        workspace_files: vec![ProviderWorkspaceFileSpec {
                            key: "persona".into(),
                            path: "persona.md".into(),
                            title: Some("System: Persona".into()),
                            role_hint: Some(ProviderPromptRoleHint::System),
                            optional: true,
                            max_bytes: 32768,
                        }],
                        outputs: BTreeMap::from([(
                            "full".into(),
                            ProviderPromptOutputSpec {
                                include: vec![
                                    "actor_context".into(),
                                    "workspace_file.persona".into(),
                                    "user_message".into(),
                                ],
                                ..Default::default()
                            },
                        )]),
                    },
                    "text",
                    None,
                ),
            )]),
            &[],
        );

        validate_manifest(&manifest).expect("declared workspace prompt part should pass");
    }

    #[test]
    fn manifest_validation_rejects_undeclared_workspace_prompt_part_reference() {
        let manifest = manifest(
            "undeclared_workspace_part",
            "Undeclared Workspace Part",
            &["undeclared-workspace-part"],
            BTreeMap::from([(
                "print".into(),
                mode(
                    "{bin}",
                    vec![lit("{prompt.full}")],
                    ProviderPromptSpec {
                        workspace_files: Vec::new(),
                        outputs: BTreeMap::from([(
                            "full".into(),
                            ProviderPromptOutputSpec {
                                include: vec!["workspace_file.persona".into()],
                                ..Default::default()
                            },
                        )]),
                    },
                    "text",
                    None,
                ),
            )]),
            &[],
        );

        let err = validate_manifest(&manifest).expect_err("undeclared workspace part should fail");
        assert!(
            err.contains("unknown prompt part `workspace_file.persona`"),
            "{err}"
        );
    }

    #[test]
    fn manifest_validation_rejects_workspace_prompt_file_path_escape() {
        let manifest = manifest(
            "bad_workspace_file",
            "Bad Workspace File",
            &["bad-workspace-file"],
            BTreeMap::from([(
                "print".into(),
                mode(
                    "{bin}",
                    vec![lit("{prompt.full}")],
                    ProviderPromptSpec {
                        workspace_files: vec![ProviderWorkspaceFileSpec {
                            key: "persona".into(),
                            path: "../persona.md".into(),
                            title: None,
                            role_hint: None,
                            optional: true,
                            max_bytes: 32768,
                        }],
                        outputs: BTreeMap::from([(
                            "full".into(),
                            ProviderPromptOutputSpec {
                                include: vec!["workspace_file.persona".into()],
                                ..Default::default()
                            },
                        )]),
                    },
                    "text",
                    None,
                ),
            )]),
            &[],
        );

        let err = validate_manifest(&manifest).expect_err("path escape should fail");
        assert!(err.contains("path must not contain `..`"), "{err}");
    }

    #[test]
    fn workspace_prompt_parts_read_declared_files() {
        let workspace = temp_dir("workspace-prompt-parts");
        let loom_dir = workspace.join(".loom");
        std::fs::create_dir_all(&loom_dir).expect("loom dir");
        std::fs::write(loom_dir.join("persona.md"), "Be concise.").expect("persona");
        let spec = ProviderPromptSpec {
            workspace_files: vec![ProviderWorkspaceFileSpec {
                key: "persona".into(),
                path: "persona.md".into(),
                title: Some("System: Persona".into()),
                role_hint: Some(ProviderPromptRoleHint::System),
                optional: true,
                max_bytes: 32768,
            }],
            outputs: BTreeMap::new(),
        };

        let parts = workspace_prompt_parts(Some(&spec), &workspace).expect("workspace parts");

        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].key, "workspace_file.persona");
        assert_eq!(parts[0].title, "System: Persona");
        assert_eq!(parts[0].content, "Be concise.");
        assert_eq!(
            parts[0].rendered_content,
            "=== System: Persona ===\nBe concise."
        );
        assert_eq!(parts[0].role_hint, PromptRoleHint::System);
        std::fs::remove_dir_all(workspace).ok();
    }

    #[test]
    fn workspace_prompt_parts_skip_optional_missing_and_fail_required_missing() {
        let workspace = temp_dir("workspace-prompt-missing");
        let mut spec = ProviderPromptSpec {
            workspace_files: vec![ProviderWorkspaceFileSpec {
                key: "persona".into(),
                path: "persona.md".into(),
                title: None,
                role_hint: None,
                optional: true,
                max_bytes: 32768,
            }],
            outputs: BTreeMap::new(),
        };

        let parts = workspace_prompt_parts(Some(&spec), &workspace).expect("optional missing");
        assert!(parts.is_empty());

        spec.workspace_files[0].optional = false;
        let err = workspace_prompt_parts(Some(&spec), &workspace)
            .expect_err("required missing should fail");
        assert!(
            err.contains("read workspace prompt file `persona`"),
            "{err}"
        );
        std::fs::remove_dir_all(workspace).ok();
    }

    #[cfg(unix)]
    #[test]
    fn workspace_prompt_parts_reject_symlink() {
        let workspace = temp_dir("workspace-prompt-symlink");
        let loom_dir = workspace.join(".loom");
        std::fs::create_dir_all(&loom_dir).expect("loom dir");
        let target = workspace.join("outside.md");
        std::fs::write(&target, "outside").expect("target");
        std::os::unix::fs::symlink(&target, loom_dir.join("persona.md")).expect("symlink");
        let spec = ProviderPromptSpec {
            workspace_files: vec![ProviderWorkspaceFileSpec {
                key: "persona".into(),
                path: "persona.md".into(),
                title: None,
                role_hint: None,
                optional: true,
                max_bytes: 32768,
            }],
            outputs: BTreeMap::new(),
        };

        let err = workspace_prompt_parts(Some(&spec), &workspace).expect_err("symlink fails");
        assert!(err.contains("must not be a symlink"), "{err}");
        std::fs::remove_dir_all(workspace).ok();
    }

    #[cfg(unix)]
    #[test]
    fn workspace_prompt_parts_reject_parent_symlink_escape() {
        let workspace = temp_dir("workspace-prompt-parent-symlink");
        let loom_dir = workspace.join(".loom");
        let outside_dir = workspace.join("outside");
        std::fs::create_dir_all(&loom_dir).expect("loom dir");
        std::fs::create_dir_all(&outside_dir).expect("outside dir");
        std::fs::write(outside_dir.join("persona.md"), "outside").expect("outside file");
        std::os::unix::fs::symlink(&outside_dir, loom_dir.join("rules")).expect("symlink dir");
        let spec = ProviderPromptSpec {
            workspace_files: vec![ProviderWorkspaceFileSpec {
                key: "persona".into(),
                path: "rules/persona.md".into(),
                title: None,
                role_hint: None,
                optional: true,
                max_bytes: 32768,
            }],
            outputs: BTreeMap::new(),
        };

        let err =
            workspace_prompt_parts(Some(&spec), &workspace).expect_err("parent symlink fails");
        assert!(err.contains("resolves outside"), "{err}");
        std::fs::remove_dir_all(workspace).ok();
    }

    #[test]
    fn manifest_validation_rejects_legacy_session_capture() {
        let err = serde_json::from_str::<ProviderManifest>(
            r#"{
              "schemaVersion": 1,
              "id": "legacy_capture",
              "detect": { "candidates": ["legacy-capture"] },
              "modes": {
                "print": {
                  "transport": "command",
                  "command": "{bin}",
                  "args": ["{prompt.full}"],
                  "stdout": { "format": "text" },
                  "session": {
                    "idSource": "provider_capture",
                    "capture": "stdout_json:.session_id",
                    "resumeArgs": ["--resume", "{session.id}", "{prompt.full}"]
                  }
                }
              }
            }"#,
        )
        .expect_err("legacy capture should fail schema parsing");
        assert!(err.to_string().contains("unknown field `capture`"), "{err}");
    }

    #[test]
    fn provider_manifest_schema_rejects_unknown_top_level_field() {
        let err = provider_manifest_parse_error(
            r#"{
              "schemaVersion": 1,
              "id": "unknown_top_level",
              "displayName": "Unknown Top Level",
              "detect": { "candidates": ["unknown-top-level"] },
              "owner": "daemon",
              "modes": {
                "print": {
                  "transport": "command",
                  "command": "{bin}",
                  "args": ["{prompt.full}"],
                  "stdout": { "format": "text" }
                }
              }
            }"#,
        );
        assert!(err.contains("unknown field `owner`"), "{err}");
    }

    #[test]
    fn provider_manifest_schema_rejects_unknown_mode_field() {
        let err = provider_manifest_parse_error(
            r#"{
              "schemaVersion": 1,
              "id": "unknown_mode",
              "detect": { "candidates": ["unknown-mode"] },
              "modes": {
                "print": {
                  "transport": "command",
                  "command": "{bin}",
                  "argz": ["{prompt.full}"],
                  "args": ["{prompt.full}"],
                  "stdout": { "format": "text" }
                }
              }
            }"#,
        );
        assert!(err.contains("unknown field `argz`"), "{err}");
    }

    #[test]
    fn provider_manifest_schema_rejects_unknown_conditional_arg_field() {
        let err = provider_manifest_parse_error(
            r#"{
              "schemaVersion": 1,
              "id": "unknown_conditional_arg",
              "detect": { "candidates": ["unknown-conditional-arg"] },
              "modes": {
                "print": {
                  "transport": "command",
                  "command": "{bin}",
                  "args": [
                    { "when": "model", "argz": ["--model"], "args": ["--model", "{model}"] },
                    "{prompt.full}"
                  ],
                  "stdout": { "format": "text" }
                }
              }
            }"#,
        );
        assert!(err.contains("unknown field `argz`"), "{err}");
    }

    #[test]
    fn provider_manifest_schema_rejects_unknown_prompt_output_field() {
        let err = provider_manifest_parse_error(
            r#"{
              "schemaVersion": 1,
              "id": "unknown_prompt_output",
              "detect": { "candidates": ["unknown-prompt-output"] },
              "modes": {
                "print": {
                  "transport": "command",
                  "command": "{bin}",
                  "prompt": {
                    "outputs": {
                      "full": { "template": "{user_message}", "joim": "\n\n" }
                    }
                  },
                  "args": ["{prompt.full}"],
                  "stdout": { "format": "text" }
                }
              }
            }"#,
        );
        assert!(err.contains("unknown field `joim`"), "{err}");
    }

    #[test]
    fn provider_manifest_schema_rejects_unknown_decoder_event_field() {
        let err = provider_manifest_parse_error(
            r#"{
              "schemaVersion": 1,
              "id": "unknown_decoder_event",
              "detect": { "candidates": ["unknown-decoder-event"] },
              "modes": {
                "print": {
                  "transport": "command",
                  "command": "{bin}",
                  "args": ["{prompt.full}"],
                  "stdout": {
                    "format": "jsonl",
                    "events": [
                      {
                        "when": { "path": "$.type", "equals": "message" },
                        "emit": { "type": "text", "text": "$.text" },
                        "extra": true
                      }
                    ]
                  }
                }
              }
            }"#,
        );
        assert!(err.contains("unknown field `extra`"), "{err}");
    }

    #[test]
    fn provider_capture_requires_decoder_capture_session() {
        let manifest = manifest(
            "missing_capture",
            "Missing Capture",
            &["missing-capture"],
            BTreeMap::from([(
                "print".into(),
                mode(
                    "{bin}",
                    vec![lit("{prompt.full}")],
                    full_prompt(),
                    "text",
                    Some(ProviderSessionSpec {
                        id_source: Some(ProviderSessionIdSource::ProviderCapture),
                        resume_args: vec![
                            lit("--resume"),
                            lit("{session.id}"),
                            lit("{prompt.full}"),
                        ],
                        scope: Some("actor_scope".into()),
                    }),
                ),
            )]),
            &[],
        );

        let err = validate_manifest(&manifest).expect_err("missing capture should fail");
        assert!(
            err.contains("stdout/stderr capture.session is missing"),
            "{err}"
        );
    }

    #[test]
    fn provider_capture_allows_stderr_capture_session() {
        let mut provider_mode = mode(
            "{bin}",
            vec![lit("{prompt.full}")],
            full_prompt(),
            "text",
            Some(ProviderSessionSpec {
                id_source: Some(ProviderSessionIdSource::ProviderCapture),
                resume_args: vec![lit("--resume"), lit("{session.id}"), lit("{prompt.full}")],
                scope: Some("actor_scope".into()),
            }),
        );
        provider_mode.stderr = Some(ProviderDecoderSpec {
            format: "jsonl".into(),
            name: None,
            events: Vec::new(),
            reduce: None,
            capture: Some(session_capture("$.session_id")),
        });
        let manifest = manifest(
            "stderr_capture",
            "Stderr Capture",
            &["stderr-capture"],
            BTreeMap::from([("print".into(), provider_mode)]),
            &[],
        );

        validate_manifest(&manifest).expect("stderr capture should satisfy provider_capture");
    }

    #[test]
    fn manifest_validation_rejects_unknown_session_scope() {
        let manifest = manifest(
            "bad_session_scope",
            "Bad Session Scope",
            &["bad-session-scope"],
            BTreeMap::from([(
                "print".into(),
                mode(
                    "{bin}",
                    vec![lit("{prompt.full}")],
                    full_prompt(),
                    "text",
                    Some(ProviderSessionSpec {
                        id_source: Some(ProviderSessionIdSource::LoomUuid),
                        resume_args: vec![
                            lit("--session-id"),
                            lit("{session.id}"),
                            lit("{prompt.full}"),
                        ],
                        scope: Some("workspace".into()),
                    }),
                ),
            )]),
            &[],
        );

        let err = validate_manifest(&manifest).expect_err("bad session scope should fail");
        assert!(
            err.contains("unsupported session scope `workspace`"),
            "{err}"
        );
    }

    #[test]
    fn manifest_validation_rejects_unknown_decoder_format() {
        let mut provider_mode = mode(
            "{bin}",
            vec![lit("{prompt.full}")],
            full_prompt(),
            "text",
            None,
        );
        provider_mode.stdout.format = "xml".into();
        provider_mode.stdout.name = None;
        let manifest = manifest(
            "bad_decoder_format",
            "Bad Decoder Format",
            &["bad-decoder-format"],
            BTreeMap::from([("print".into(), provider_mode)]),
            &[],
        );

        let err = validate_manifest(&manifest).expect_err("bad decoder should fail");
        assert!(err.contains("unknown decoder format `xml`"), "{err}");
    }

    #[test]
    fn manifest_validation_rejects_unknown_decoder_event_type() {
        let mut provider_mode = mode(
            "{bin}",
            vec![lit("{prompt.full}")],
            full_prompt(),
            "text",
            None,
        );
        provider_mode.stdout.format = "jsonl".into();
        provider_mode.stdout.name = None;
        provider_mode.stdout.events = vec![ProviderDecoderEventSpec {
            when: None,
            emit: ProviderDecoderEmitSpec {
                emit_type: "metric".into(),
                text: Some("$.value".into()),
                ..Default::default()
            },
        }];
        let manifest = manifest(
            "bad_decoder_event",
            "Bad Decoder Event",
            &["bad-decoder-event"],
            BTreeMap::from([("print".into(), provider_mode)]),
            &[],
        );

        let err = validate_manifest(&manifest).expect_err("bad event should fail");
        assert!(err.contains("unsupported event type `metric`"), "{err}");
    }

    #[test]
    fn manifest_validation_rejects_bad_decoder_reducer() {
        let mut provider_mode = mode(
            "{bin}",
            vec![lit("{prompt.full}")],
            full_prompt(),
            "text",
            None,
        );
        provider_mode.stdout.format = "jsonl".into();
        provider_mode.stdout.name = None;
        provider_mode.stdout.reduce = Some(ProviderJsonlReduceSpec {
            final_text: Some(ProviderJsonlTextReducerSpec {
                mode: "last".into(),
                path: "$.data.content".into(),
                when: Some(json_condition_equals("$.type", "assistant.message")),
                fallback: None,
            }),
        });
        let manifest = manifest(
            "bad_decoder_reducer",
            "Bad Decoder Reducer",
            &["bad-decoder-reducer"],
            BTreeMap::from([("print".into(), provider_mode)]),
            &[],
        );

        let err = validate_manifest(&manifest).expect_err("bad reducer should fail");
        assert!(err.contains("unsupported reducer mode `last`"), "{err}");
    }

    #[test]
    fn manifest_validation_allows_decoder_wildcard_path() {
        let mut provider_mode = mode(
            "{bin}",
            vec![lit("{prompt.full}")],
            full_prompt(),
            "text",
            None,
        );
        provider_mode.stdout.format = "jsonl".into();
        provider_mode.stdout.name = None;
        provider_mode.stdout.reduce = Some(ProviderJsonlReduceSpec {
            final_text: Some(ProviderJsonlTextReducerSpec {
                mode: "concat".into(),
                path: "$.items[*].text".into(),
                when: None,
                fallback: None,
            }),
        });
        let manifest = manifest(
            "wildcard_decoder",
            "Wildcard Decoder",
            &["wildcard-decoder"],
            BTreeMap::from([("print".into(), provider_mode)]),
            &[],
        );

        validate_manifest(&manifest).expect("wildcard decoder path should pass");
    }

    #[test]
    fn manifest_validation_allows_json_decoder() {
        let mut provider_mode = mode(
            "{bin}",
            vec![lit("{prompt.full}")],
            full_prompt(),
            "text",
            None,
        );
        provider_mode.stdout.format = "json".into();
        provider_mode.stdout.name = None;
        provider_mode.stdout.reduce = Some(ProviderJsonlReduceSpec {
            final_text: Some(ProviderJsonlTextReducerSpec {
                mode: "lastNonEmpty".into(),
                path: "$.result.text".into(),
                when: None,
                fallback: None,
            }),
        });
        let manifest = manifest(
            "json_decoder",
            "JSON Decoder",
            &["json-decoder"],
            BTreeMap::from([("print".into(), provider_mode)]),
            &[],
        );

        validate_manifest(&manifest).expect("json decoder should pass");
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
        assert!(transport.args.contains(&"{agent.configDir}".into()));
        assert!(!transport.args.contains(&"{loom.configDir}".into()));
        assert_eq!(
            transport.session.as_ref().and_then(|s| s.id_source),
            Some(CommandSessionIdSource::LoomUuid)
        );
        assert_eq!(
            transport.session.as_ref().and_then(|s| s.scope.as_deref()),
            Some("actor_scope")
        );
    }

    #[test]
    fn builtin_claude_nonprint_resolves_interactive_transport() {
        let dir = temp_dir("claude-nonprint-path");
        make_executable(&dir.join("claude"));
        let registry =
            ProviderRegistry::load(&temp_dir("claude-nonprint-config")).expect("registry");
        let provider = registry
            .detect_with_path(dir.into_os_string())
            .expect("detect")
            .into_iter()
            .find(|provider| provider.id == "claude")
            .expect("claude");
        let transport = transport_from_manifest(
            &provider.manifest,
            provider.manifest.modes.get("nonprint").unwrap(),
            Path::new(&provider.command),
            &AgentProviderRef {
                id: "claude".into(),
                mode: Some("nonprint".into()),
                model: Some("sonnet".into()),
                reasoning_effort: None,
            },
        )
        .expect("transport");

        assert_eq!(transport.kind, "interactive_command");
        assert_eq!(transport.args, vec!["--add-dir", "{agent.configDir}"]);
        assert_eq!(transport.model_args, vec!["--model", "{model}"]);
        let interactive = transport.interactive.as_ref().expect("interactive config");
        assert!(interactive.session.new_args.contains(&"{prompt}".into()));
        assert!(interactive
            .session
            .new_args
            .contains(&"--session-id".into()));
        assert!(interactive.session.resume_args.contains(&"{prompt}".into()));
        assert!(interactive.session.resume_args.contains(&"--resume".into()));
        assert_eq!(
            transport
                .provider
                .as_ref()
                .map(|provider| provider.kind.as_str()),
            Some("claude")
        );
    }

    #[test]
    fn builtins_default_add_dir_to_agent_config_dir() {
        for provider_id in ["claude", "qoder", "copilot", "codex"] {
            let manifest = builtin_provider_manifests()
                .into_iter()
                .find(|manifest| manifest.id == provider_id)
                .expect("provider");
            let rendered = serde_json::to_string(&manifest).expect("manifest json");
            assert!(
                rendered.contains("{agent.configDir}"),
                "{provider_id} should expose only the current agent config dir by default"
            );
            assert!(
                !rendered.contains("{loom.configDir}"),
                "{provider_id} should not expose the daemon config dir by default"
            );
        }
    }

    #[test]
    fn builtin_opencode_declares_json_session_capture_and_resume() {
        let dir = temp_dir("opencode-path");
        make_executable(&dir.join("opencode"));
        let registry = ProviderRegistry::load(&temp_dir("opencode-config")).expect("registry");
        let provider = registry
            .detect_with_path(dir.into_os_string())
            .expect("detect")
            .into_iter()
            .find(|provider| provider.id == "opencode")
            .expect("opencode");
        let provider_ref = AgentProviderRef {
            id: "opencode".into(),
            mode: Some("print".into()),
            model: Some("opencode/big-pickle".into()),
            reasoning_effort: None,
        };
        let plan = runtime_plan_from_manifest(
            &provider.manifest,
            "print",
            provider.manifest.modes.get("print").unwrap(),
            Path::new(&provider.command),
            &provider_ref,
        )
        .expect("runtime plan");

        assert_eq!(plan.provider_id, "opencode");
        assert!(plan.args.contains(&"--format".into()));
        assert!(plan.args.contains(&"json".into()));
        assert!(plan.args.contains(&"{prompt.full}".into()));
        assert_eq!(
            plan.session.as_ref().and_then(|session| session.id_source),
            Some(CommandSessionIdSource::ProviderCapture)
        );
        assert!(plan
            .session
            .as_ref()
            .and_then(|session| session.resume_args.as_ref())
            .is_some_and(|args| args.contains(&"--session".into())
                && args.contains(&"{session_id}".into())
                && args.contains(&"{prompt.full}".into())));
        assert_eq!(
            plan.decoder.as_ref().map(|decoder| decoder.format.as_str()),
            Some("jsonl")
        );
        assert!(plan
            .decoder
            .as_ref()
            .and_then(|decoder| decoder.capture.as_ref())
            .and_then(|capture| capture.session.as_ref())
            .is_some_and(|session| session.path == "$.sessionID"));
    }

    #[test]
    fn provider_runtime_plan_resolves_before_adapter_transport() {
        let dir = temp_dir("runtime-plan-path");
        make_executable(&dir.join("qodercli"));
        let registry = ProviderRegistry::load(&temp_dir("runtime-plan-config")).expect("registry");
        let provider = registry
            .detect_with_path(dir.into_os_string())
            .expect("detect")
            .into_iter()
            .find(|provider| provider.id == "qoder")
            .expect("qoder");
        let provider_ref = AgentProviderRef {
            id: "qoder".into(),
            mode: Some("print".into()),
            model: Some("auto".into()),
            reasoning_effort: Some("high".into()),
        };
        let plan = runtime_plan_from_manifest(
            &provider.manifest,
            "print",
            provider.manifest.modes.get("print").unwrap(),
            Path::new(&provider.command),
            &provider_ref,
        )
        .expect("runtime plan");

        assert_eq!(plan.provider_id, "qoder");
        assert_eq!(plan.mode, "print");
        assert_eq!(plan.transport_kind, "command");
        assert_eq!(plan.model.as_deref(), Some("auto"));
        let serialized_plan = serde_json::to_value(&plan).expect("serialize runtime plan");
        assert!(serialized_plan.get("outputFormat").is_none());
        assert!(serialized_plan.get("promptVia").is_none());
        assert!(serialized_plan.get("modelArgs").is_none());
        assert!(plan.arg_specs.iter().any(|arg| matches!(
            arg,
            ProviderArgSpec::Conditional(spec) if spec.when == "model"
        )));
        assert!(plan
            .decoder
            .as_ref()
            .and_then(|decoder| decoder.capture.as_ref())
            .and_then(|capture| capture.session.as_ref())
            .is_some());

        let arg_specs_len = plan.arg_specs.len();
        let transport = plan.into_transport();
        assert_eq!(transport.kind, "command");
        assert_eq!(transport.command, provider.command);
        assert_eq!(transport.arg_specs.len(), arg_specs_len);
        assert_eq!(
            transport.output_format,
            Some(CommandOutputFormat::ClaudeStreamJson)
        );
        assert_eq!(transport.prompt_via, PromptVia::Args);
        assert!(
            transport.model_args.is_empty(),
            "provider conditionals should stay in arg_specs instead of legacy model_args"
        );
        assert_eq!(
            transport
                .session
                .as_ref()
                .and_then(|session| session.id_source),
            Some(CommandSessionIdSource::ProviderCapture)
        );
        assert!(transport
            .session
            .as_ref()
            .is_some_and(|session| !session.resume_arg_specs.is_empty()));
    }

    #[test]
    fn provider_runtime_plan_expands_bin_in_static_templates() {
        let bin = Path::new("/tmp/demo-agent");
        let mut provider_mode = mode(
            "{bin}",
            vec![
                lit("--bin"),
                lit("{bin}"),
                when("model", vec![lit("--model-bin"), lit("{bin}")]),
                lit("{prompt.full}"),
            ],
            full_prompt(),
            "text",
            Some(ProviderSessionSpec {
                id_source: Some(ProviderSessionIdSource::LoomUuid),
                resume_args: vec![
                    lit("--resume"),
                    lit("{session.id}"),
                    lit("{bin}"),
                    lit("{prompt.full}"),
                ],
                scope: Some("actor_scope".into()),
            }),
        );
        provider_mode
            .env
            .insert("PROVIDER_BIN".into(), "{bin}".into());
        provider_mode.stdin = Some("stdin:{bin}:{prompt.full}".into());
        let manifest = manifest(
            "bin_templates",
            "Bin Templates",
            &["demo-agent"],
            BTreeMap::from([("print".into(), provider_mode)]),
            &[],
        );

        let plan = runtime_plan_from_manifest(
            &manifest,
            "print",
            manifest.modes.get("print").unwrap(),
            bin,
            &AgentProviderRef {
                id: "bin_templates".into(),
                mode: Some("print".into()),
                model: Some("demo-model".into()),
                reasoning_effort: None,
            },
        )
        .expect("runtime plan");
        let bin_text = bin.display().to_string();
        let expected_stdin = format!("stdin:{bin_text}:{{prompt.full}}");

        assert_eq!(plan.command, bin_text);
        assert!(plan.args.contains(&bin_text));
        assert_eq!(
            plan.env.get("PROVIDER_BIN").map(String::as_str),
            Some(bin_text.as_str())
        );
        assert_eq!(plan.stdin.as_deref(), Some(expected_stdin.as_str()));
        assert!(plan.arg_specs.iter().any(|arg| matches!(
            arg,
            ProviderArgSpec::Conditional(spec)
                if spec.args.iter().any(|item| matches!(item, ProviderArgSpec::Literal(value) if value == &bin_text))
        )));
        let session = plan.session.as_ref().expect("session");
        assert!(
            session
                .resume_args
                .as_ref()
                .is_some_and(|args| args.contains(&bin_text)),
            "resume_args should expand bin"
        );
        assert!(
            session
                .resume_arg_specs
                .iter()
                .any(|arg| matches!(arg, ProviderArgSpec::Literal(value) if value == &bin_text)),
            "resume_arg_specs should expand bin"
        );
    }

    #[test]
    fn builtin_provider_capture_sessions_are_declared_on_stdout_decoder() {
        for provider_id in ["qoder", "codex", "opencode"] {
            let manifest = builtin_provider_manifests()
                .into_iter()
                .find(|manifest| manifest.id == provider_id)
                .expect("provider");
            let mode = manifest.modes.get("print").expect("print mode");
            let session = mode.session.as_ref().expect("session");
            assert_eq!(
                session.id_source,
                Some(ProviderSessionIdSource::ProviderCapture)
            );
            assert!(
                mode.stdout
                    .capture
                    .as_ref()
                    .and_then(|capture| capture.session.as_ref())
                    .is_some(),
                "{provider_id} should capture session through stdout decoder"
            );
        }
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

    #[test]
    fn extended_provider_mode_patch_merges_args_env_and_prompt_outputs() {
        let config = temp_dir("extends");
        let providers = providers_dir(&config);
        std::fs::create_dir_all(&providers).expect("providers dir");
        std::fs::write(
            providers.join("codex_budgeted.json"),
            r#"{
              "schemaVersion": 1,
              "id": "codex_budgeted",
              "displayName": "Codex Budgeted",
              "extends": "codex",
              "modes": {
                "print": {
                  "timeoutMs": 12345,
                  "env": {
                    "unset": ["LOOM_NO_DAEMON"],
                    "merge": { "EXTRA_FLAG": "1" }
                  },
                  "args": {
                    "prepend": ["--prepended"],
                    "append": ["--max-budget-usd", "5"]
                  },
                  "prompt": {
                    "outputs": {
                      "diagnostic": { "template": "{actor_context}" }
                    }
                  }
                }
              }
            }"#,
        )
        .expect("write provider");

        let registry = ProviderRegistry::load(&config).expect("registry");
        let manifest = registry.get("codex_budgeted").expect("provider");
        let mode = manifest.modes.get("print").expect("print mode");
        assert_eq!(mode.timeout_ms, Some(12345));
        assert!(matches!(
            mode.args.first(),
            Some(ProviderArgSpec::Literal(value)) if value == "--prepended"
        ));
        assert!(mode
            .args
            .windows(2)
            .any(|items| matches!(&items[0], ProviderArgSpec::Literal(value) if value == "--max-budget-usd")
                && matches!(&items[1], ProviderArgSpec::Literal(value) if value == "5")));
        assert_eq!(mode.env.get("EXTRA_FLAG").map(String::as_str), Some("1"));
        assert!(!mode.env.contains_key("LOOM_NO_DAEMON"));
        let outputs = &mode.prompt.as_ref().expect("prompt").outputs;
        assert!(outputs.contains_key("full"));
        assert!(outputs.contains_key("diagnostic"));
        assert_eq!(
            mode.stdout.name.as_deref(),
            Some("codex_stream_json"),
            "patch must preserve base parser"
        );
    }

    #[test]
    fn extended_provider_patch_rejects_unknown_fields_before_merge() {
        let top_level_err = provider_registry_load_error(
            "extends-unknown-top-level",
            r#"{
              "schemaVersion": 1,
              "id": "codex_unknown_top",
              "extends": "codex",
              "detcet": { "candidates": ["codex"] }
            }"#,
        );
        assert!(
            top_level_err.contains("provider manifest patch contains unknown field `detcet`"),
            "{top_level_err}"
        );

        let mode_err = provider_registry_load_error(
            "extends-unknown-mode",
            r#"{
              "schemaVersion": 1,
              "id": "codex_unknown_mode",
              "extends": "codex",
              "modes": { "print": { "argz": ["--bad"] } }
            }"#,
        );
        assert!(
            mode_err.contains("provider mode patch contains unknown field `argz`"),
            "{mode_err}"
        );

        let args_err = provider_registry_load_error(
            "extends-unknown-args",
            r#"{
              "schemaVersion": 1,
              "id": "codex_unknown_args",
              "extends": "codex",
              "modes": { "print": { "args": { "appned": ["--bad"] } } }
            }"#,
        );
        assert!(
            args_err.contains("args patch contains unknown field `appned`"),
            "{args_err}"
        );

        let prompt_err = provider_registry_load_error(
            "extends-unknown-prompt",
            r#"{
              "schemaVersion": 1,
              "id": "codex_unknown_prompt",
              "extends": "codex",
              "modes": {
                "print": {
                  "prompt": {
                    "outputs": { "diagnostic": { "template": "{actor_context}" } },
                    "join": "\n\n"
                  }
                }
              }
            }"#,
        );
        assert!(
            prompt_err.contains("prompt patch contains unknown field `join`"),
            "{prompt_err}"
        );

        let env_err = provider_registry_load_error(
            "extends-unknown-env",
            r#"{
              "schemaVersion": 1,
              "id": "codex_unknown_env",
              "extends": "codex",
              "modes": { "print": { "env": { "merge": {}, "drop": ["X"] } } }
            }"#,
        );
        assert!(
            env_err.contains("env patch contains unknown field `drop`"),
            "{env_err}"
        );
    }

    #[test]
    fn extended_provider_patch_requires_explicit_patch_shapes_for_existing_modes() {
        let args_err = provider_registry_load_error(
            "extends-args-shorthand",
            r#"{
              "schemaVersion": 1,
              "id": "codex_args_shorthand",
              "extends": "codex",
              "modes": { "print": { "args": ["--bad"] } }
            }"#,
        );
        assert!(
            args_err.contains("args patch must be an object; use args.replace"),
            "{args_err}"
        );

        let env_err = provider_registry_load_error(
            "extends-env-shorthand",
            r#"{
              "schemaVersion": 1,
              "id": "codex_env_shorthand",
              "extends": "codex",
              "modes": { "print": { "env": { "EXTRA_FLAG": "1" } } }
            }"#,
        );
        assert!(
            env_err.contains("env patch must use merge and/or unset"),
            "{env_err}"
        );

        let prompt_err = provider_registry_load_error(
            "extends-prompt-shorthand",
            r#"{
              "schemaVersion": 1,
              "id": "codex_prompt_shorthand",
              "extends": "codex",
              "modes": { "print": { "prompt": { "outputsWrong": {} } } }
            }"#,
        );
        assert!(
            prompt_err.contains("prompt patch contains unknown field `outputsWrong`"),
            "{prompt_err}"
        );
    }

    #[test]
    fn local_provider_cannot_shadow_existing_provider_id() {
        let config = temp_dir("shadow");
        let providers = providers_dir(&config);
        std::fs::create_dir_all(&providers).expect("providers dir");
        std::fs::write(
            providers.join("claude.json"),
            r#"{
              "schemaVersion": 1,
              "id": "claude",
              "displayName": "Shadow Claude",
              "detect": { "candidates": ["shadow-claude"] },
              "modes": {
                "print": {
                  "transport": "command",
                  "command": "{bin}",
                  "args": ["{prompt.full}"],
                  "stdout": { "format": "text" }
                }
              }
            }"#,
        )
        .expect("write provider");

        let err = ProviderRegistry::load(&config).expect_err("shadow should fail");
        assert!(err.contains("conflicts with an existing provider id"));
    }
}
