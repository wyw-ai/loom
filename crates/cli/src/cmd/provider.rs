use std::path::{Path, PathBuf};

use agent_runtime::provider::{
    builtin_provider_manifests, providers_dir, DetectedProvider, ProviderRegistry,
};
use anyhow::{anyhow, Context, Result};
use proto::methods::{AgentProviderRef, ProviderManifest, ProviderModeSpec};
use serde_json::{json, Map, Value};

use crate::{config, render};

pub fn validate(path: PathBuf) -> Result<()> {
    let manifest = resolve_manifest_file(&path)?;
    if render::is_json() {
        render::print_json(&json!({
            "ok": true,
            "provider": provider_summary(&manifest, false, None, None),
        }));
    } else {
        println!("provider {} OK", manifest.id);
    }
    Ok(())
}

pub fn add(path: PathBuf, replace: bool) -> Result<()> {
    let (manifest, raw) = read_and_resolve_manifest_file(&path)?;
    let registry = ProviderRegistry::load(&config::config_dir()).map_err(|err| anyhow!(err))?;
    let dir = providers_dir(&config::config_dir());
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create provider dir {}", dir.display()))?;
    let target = dir.join(format!("{}.json", manifest.id));
    let target_exists = target.exists();
    if registry.get(&manifest.id).is_some() && !(replace && target_exists) {
        return Err(anyhow!(
            "provider `{}` already exists; use --replace only for existing local provider manifests, or use a new id with extends for local variants",
            manifest.id
        ));
    }
    if target_exists && !replace {
        return Err(anyhow!(
            "local provider `{}` already exists at {}; pass --replace to overwrite",
            manifest.id,
            target.display()
        ));
    }
    let text = serde_json::to_string_pretty(&raw).context("serialize provider manifest")?;
    std::fs::write(&target, text)
        .with_context(|| format!("write provider manifest {}", target.display()))?;
    if render::is_json() {
        render::print_json(&json!({
            "ok": true,
            "replaced": target_exists,
            "path": target.display().to_string(),
            "provider": provider_summary(&manifest, false, None, None),
        }));
    } else {
        let verb = if target_exists { "replaced" } else { "added" };
        println!("{verb} provider {} -> {}", manifest.id, target.display());
    }
    Ok(())
}

pub fn list() -> Result<()> {
    let config_dir = config::config_dir();
    let registry = ProviderRegistry::load(&config_dir).map_err(|err| anyhow!(err))?;
    let detected = registry
        .detect_with_path(std::env::var_os("PATH").unwrap_or_default())
        .map_err(|err| anyhow!(err))?;
    let rows = registry
        .manifests()
        .map(|manifest| {
            let detected_provider = detected.iter().find(|provider| provider.id == manifest.id);
            provider_summary(
                manifest,
                detected_provider.is_some(),
                detected_provider.map(|provider| provider.command.as_str()),
                Some(&config_dir),
            )
        })
        .collect::<Vec<_>>();
    if render::is_json() {
        render::print_json(&json!({ "providers": rows }));
    } else if rows.is_empty() {
        println!("No providers registered.");
    } else {
        for row in rows {
            let detected = if row["detected"].as_bool() == Some(true) {
                "detected"
            } else {
                "missing"
            };
            println!(
                "{}\t{}\t{}\t{}",
                row["id"].as_str().unwrap_or_default(),
                row["displayName"].as_str().unwrap_or_default(),
                detected,
                row["command"].as_str().unwrap_or_default()
            );
        }
    }
    Ok(())
}

pub fn show(provider_id: String) -> Result<()> {
    let config_dir = config::config_dir();
    let registry = ProviderRegistry::load(&config_dir).map_err(|err| anyhow!(err))?;
    let manifest = registry
        .get(&provider_id)
        .ok_or_else(|| anyhow!("provider `{provider_id}` not found"))?;
    let detected = registry
        .detect_with_path(std::env::var_os("PATH").unwrap_or_default())
        .map_err(|err| anyhow!(err))?
        .into_iter()
        .find(|provider| provider.id == provider_id);
    let detail = provider_detail(&registry, &config_dir, manifest, detected.as_ref())?;
    if render::is_json() {
        render::print_json(&detail);
    } else {
        print_provider_detail(&detail);
    }
    Ok(())
}

pub fn remove(provider_id: String) -> Result<()> {
    validate_provider_id_for_path(&provider_id)?;
    let path = providers_dir(&config::config_dir()).join(format!("{provider_id}.json"));
    if !path.exists() {
        return Err(anyhow!(
            "local provider `{provider_id}` not found at {}",
            path.display()
        ));
    }
    std::fs::remove_file(&path)
        .with_context(|| format!("remove provider manifest {}", path.display()))?;
    if render::is_json() {
        render::print_json(&json!({ "ok": true, "removed": provider_id }));
    } else {
        println!("removed provider {provider_id}");
    }
    Ok(())
}

fn validate_provider_id_for_path(provider_id: &str) -> Result<()> {
    proto::path_component::validate_path_component(provider_id, "provider_id")
        .map_err(|err| anyhow!(err))?;
    let mut chars = provider_id.chars();
    let Some(first) = chars.next() else {
        return Err(anyhow!("provider id is required"));
    };
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return Err(anyhow!(
            "provider id `{provider_id}` must start with a lowercase ascii letter or digit"
        ));
    }
    if chars.any(|ch| {
        !(ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '_' | '-' | '.'))
    }) {
        return Err(anyhow!(
            "provider id `{provider_id}` may only contain lowercase ascii letters, digits, `_`, `-`, or `.`"
        ));
    }
    Ok(())
}

pub fn doctor(provider_id: String) -> Result<()> {
    let config_dir = config::config_dir();
    let registry = ProviderRegistry::load(&config_dir).map_err(|err| anyhow!(err))?;
    let manifest = registry
        .get(&provider_id)
        .ok_or_else(|| anyhow!("provider `{provider_id}` not found"))?;
    let detected = registry
        .detect_with_path(std::env::var_os("PATH").unwrap_or_default())
        .map_err(|err| anyhow!(err))?
        .into_iter()
        .find(|provider| provider.id == provider_id);
    if render::is_json() {
        let detail = provider_detail(&registry, &config_dir, manifest, detected.as_ref())?;
        render::print_json(&json!({
            "ok": detected.is_some(),
            "checks": provider_doctor_checks(manifest, detected.as_ref()),
            "provider": detail,
        }));
    } else {
        let checks = provider_doctor_checks(manifest, detected.as_ref());
        if detected.is_some() {
            println!("provider {} OK", manifest.id);
        } else {
            println!("provider {} has issues", manifest.id);
        }
        for check in checks {
            let status = if check["ok"].as_bool() == Some(true) {
                "ok"
            } else {
                "fail"
            };
            println!(
                "[{status}] {}",
                check["message"].as_str().unwrap_or_default()
            );
        }
        let detail = provider_detail(&registry, &config_dir, manifest, detected.as_ref())?;
        print_provider_detail(&detail);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ExampleSelection {
    pub claude: bool,
    pub qoder: bool,
    pub copilot: bool,
    pub codex: bool,
    pub opencode: bool,
    pub kimi: bool,
    pub zcode: bool,
}

impl ExampleSelection {
    fn selected_ids(self) -> Vec<&'static str> {
        let mut ids = Vec::new();
        if self.claude {
            ids.push("claude");
        }
        if self.qoder {
            ids.push("qoder");
        }
        if self.copilot {
            ids.push("copilot");
        }
        if self.codex {
            ids.push("codex");
        }
        if self.opencode {
            ids.push("opencode");
        }
        if self.kimi {
            ids.push("kimi");
        }
        if self.zcode {
            ids.push("zcode");
        }
        ids
    }
}

pub fn example(selection: ExampleSelection) -> Result<()> {
    if selection.selected_ids().is_empty() {
        let value = standard_provider_example();
        if render::is_json() {
            let text =
                serde_json::to_string_pretty(&value).context("serialize provider example")?;
            println!("{text}");
        } else {
            println!("{}", standard_provider_example_text(&value)?);
        }
        return Ok(());
    }
    let mut examples = official_provider_examples(selection)?;
    let value = if examples.len() == 1 {
        examples.remove(0)
    } else {
        Value::Array(examples)
    };
    let text = serde_json::to_string_pretty(&value).context("serialize provider example")?;
    println!("{text}");
    Ok(())
}

fn standard_provider_example_text(value: &Value) -> Result<String> {
    let json_text = serde_json::to_string_pretty(value).context("serialize provider example")?;
    Ok(format!(
        r#"Provider manifest example

This is a complete generic ProviderManifest template. It is meant as a starting point for adding a command-line agent provider to Loom.

Suggested workflow:
  loom --json provider example > /tmp/my_provider.json
  loom provider validate /tmp/my_provider.json
  loom provider add /tmp/my_provider.json
  loom provider doctor my_provider

Where to install it:
  $LOOM_CONFIG_DIR/providers/my_provider.json

What to edit first:
  1. id: stable lowercase provider id. Use a new id for local variants.
  2. displayName: human-friendly name shown in Loom tools.
  3. detect.candidates: command names Loom may find on the daemon host.
  4. modes.print.command: normally "{{bin}}", meaning the detected command path.
  5. modes.print.args: the exact argv order for the provider CLI.
  6. models.default / models.choices: values shown in GUI and substituted into "{{model}}".
  7. stdout: how Loom reads agent output. Use "text" for plain stdout, or a provider decoder for JSON/JSONL streams.
  8. timeoutMs / idleTimeoutMs: -1 means unlimited; a positive integer enables that millisecond limit.

Prompt flow in this template:
  - Loom writes stable actor/channel context and AgentSpec.instructions to {{agent.workspace}}/AGENTS.md.
  - Loom no longer injects a Loom-owned system prompt or scope bootstrap manifest.
  - AgentSpec.promptAssembly decides how memory, profile prompt files, runtime context, assignment context, and user_message become prompt.system, prompt.user, and prompt.full.
  - Loom creates provider-native skill directories under {{agent.workspace}} and projects the default "loom" skill plus scope / actor-local skills into each directory.
  - Providers launched with {{agent.workspace}} as cwd can discover skills/, .agents/skills, .claude/skills, .qoder/skills, or .opencode/skills directly. Extra workspace dirs are only needed for provider-specific file access.
  - ProviderManifest only declares how the provider CLI receives those rendered prompt outputs.
  - Simple providers should pass "{{prompt.full}}" directly. Providers that need an explicit instruction directory should point it at {{agent.workspace}}.

Common runtime path variables:
  - {{agent.workspace}} is the provider cwd and the authoritative AGENTS.md location for the current actor in the current channel.
  - {{loom_agent_home}} points at the current agent's runtime home for profile, bundle, and session data; it is not the default AGENTS.md location.
  - {{agent.skillWorkspace}} is a compatibility alias for {{agent.workspace}}.
  - {{agent.skills}} and {{scope.skills}} point at the shared scope skill registry that is projected into {{agent.workspace}}.
  - {{agent.configDir}} points at the current agent's config directory, normally $LOOM_CONFIG_DIR/agents/<actor_id>.
  - {{agent.specPath}} points at that agent's spec.json.
  - {{loom.configDir}} remains available for advanced providers that intentionally need daemon-level config, but built-in providers avoid exposing it by default.

Prompt delivery variables:
  - {{prompt.system}}, {{prompt.user}}, and {{prompt.full}} are rendered by AgentSpec.promptAssembly.
  - Built-in providers no longer pass {{prompt.system}} through provider system-prompt flags.
  - ProviderManifest should not declare agent profile or workspace prompt files.
  - Keep provider fields focused on command, args, env, stdin, stdout, session, model, and detection.

Provider-specific examples:
  loom provider example --claude
  loom provider example --qoder
  loom provider example --copilot
  loom provider example --codex
  loom provider example --opencode

JSON template:

```json
{json_text}
```
"#
    ))
}

fn read_and_resolve_manifest_file(path: &Path) -> Result<(ProviderManifest, serde_json::Value)> {
    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let raw = serde_json::from_str::<serde_json::Value>(&text)
        .with_context(|| format!("parse provider manifest {}", path.display()))?;
    let registry = ProviderRegistry::load(&config::config_dir()).map_err(|err| anyhow!(err))?;
    let manifest = registry
        .resolve_manifest_value(raw.clone())
        .map_err(|err| anyhow!(err))?;
    Ok((manifest, raw))
}

fn resolve_manifest_file(path: &Path) -> Result<ProviderManifest> {
    read_and_resolve_manifest_file(path).map(|(manifest, _)| manifest)
}

fn standard_provider_example() -> Value {
    json!({
        "schemaVersion": 1,
        "id": "my_provider",
        "displayName": "My Provider",
        "detect": {
            "candidates": ["my-agent"]
        },
        "modes": {
            "print": {
                "transport": "command",
                "command": "{bin}",
                "args": [
                    "run",
                    {
                        "when": "model",
                        "args": ["--model", "{model}"]
                    },
                    "{prompt.full}"
                ],
                "stdout": {
                    "format": "text"
                },
                "timeoutMs": -1,
                "idleTimeoutMs": -1
            }
        },
        "models": {
            "default": "default",
            "choices": [
                {
                    "id": "default",
                    "label": "Default"
                }
            ]
        }
    })
}

fn official_provider_examples(selection: ExampleSelection) -> Result<Vec<Value>> {
    let selected = selection.selected_ids();
    let builtins = builtin_provider_manifests();
    selected
        .into_iter()
        .map(|id| {
            builtins
                .iter()
                .find(|manifest| manifest.id == id)
                .ok_or_else(|| anyhow!("official provider `{id}` not found"))
                .and_then(|manifest| {
                    serde_json::to_value(manifest)
                        .with_context(|| format!("serialize provider `{id}`"))
                })
        })
        .collect()
}

fn provider_summary(
    manifest: &ProviderManifest,
    detected: bool,
    command: Option<&str>,
    config_dir: Option<&Path>,
) -> serde_json::Value {
    json!({
        "id": manifest.id.clone(),
        "displayName": if manifest.display_name.trim().is_empty() {
            manifest.id.as_str()
        } else {
            manifest.display_name.as_str()
        },
        "source": provider_source_value(config_dir, manifest),
        "detected": detected,
        "command": command.unwrap_or(""),
        "defaultMode": default_mode_name(manifest),
        "modes": manifest.modes.keys().cloned().collect::<Vec<_>>(),
        "models": manifest.models.clone(),
    })
}

fn provider_detail(
    registry: &ProviderRegistry,
    config_dir: &Path,
    manifest: &ProviderManifest,
    detected: Option<&DetectedProvider>,
) -> Result<Value> {
    let mut modes = Map::new();
    let default_mode = default_mode_name(manifest).to_string();
    for (name, mode) in &manifest.modes {
        modes.insert(
            name.clone(),
            provider_mode_detail(registry, manifest, name, mode, detected.is_some())?,
        );
    }
    Ok(json!({
        "id": manifest.id,
        "displayName": display_name(manifest),
        "source": provider_source_value(Some(config_dir), manifest),
        "detected": detected.map(|provider| {
            json!({
                "available": true,
                "command": provider.command,
                "transportKind": provider.transport_kind,
                "args": provider.args,
                "env": provider.env,
            })
        }).unwrap_or_else(|| {
            json!({
                "available": false,
                "candidates": manifest.detect.candidates,
            })
        }),
        "detect": manifest.detect,
        "defaultMode": default_mode,
        "models": manifest.models,
        "modes": modes,
    }))
}

fn provider_mode_detail(
    registry: &ProviderRegistry,
    manifest: &ProviderManifest,
    mode_name: &str,
    mode: &ProviderModeSpec,
    detected: bool,
) -> Result<Value> {
    let runtime_plan = if detected {
        let provider_ref = AgentProviderRef {
            id: manifest.id.clone(),
            mode: Some(mode_name.to_string()),
            model: manifest
                .models
                .as_ref()
                .and_then(|models| models.default.clone()),
            reasoning_effort: None,
            ..Default::default()
        };
        match registry.resolve_runtime_plan(&provider_ref) {
            Ok(plan) => serde_json::to_value(plan).context("serialize runtime plan")?,
            Err(err) => json!({ "error": err }),
        }
    } else {
        Value::Null
    };
    Ok(json!({
        "transport": mode.transport,
        "command": mode.command,
        "args": mode.args,
        "env": mode.env,
        "stdin": mode.stdin,
        "prompt": mode.prompt,
        "stdout": mode.stdout,
        "stderr": mode.stderr,
        "session": mode.session,
        "timeoutMs": mode.timeout_ms,
        "idleTimeoutMs": mode.idle_timeout_ms,
        "runtimePlan": runtime_plan,
    }))
}

fn provider_doctor_checks(
    manifest: &ProviderManifest,
    detected: Option<&DetectedProvider>,
) -> Vec<Value> {
    let mut checks = vec![json!({
        "id": "manifest.valid",
        "ok": true,
        "message": "manifest loaded and validated",
    })];
    checks.push(json!({
        "id": "command.detected",
        "ok": detected.is_some(),
        "message": detected
            .map(|provider| format!("command detected: {}", provider.command))
            .unwrap_or_else(|| format!(
                "no command detected; candidates: {}",
                manifest.detect.candidates.join(", ")
            )),
    }));
    checks.push(json!({
        "id": "mode.default",
        "ok": manifest.modes.contains_key(default_mode_name(manifest)),
        "message": format!("default mode: {}", default_mode_name(manifest)),
    }));
    checks
}

fn print_provider_detail(detail: &Value) {
    println!("provider  = {}", detail["id"].as_str().unwrap_or_default());
    println!(
        "name      = {}",
        detail["displayName"].as_str().unwrap_or_default()
    );
    println!(
        "source    = {}",
        detail["source"]["kind"].as_str().unwrap_or_default()
    );
    if let Some(path) = detail["source"]["path"].as_str() {
        println!("path      = {path}");
    }
    println!(
        "detected  = {}",
        if detail["detected"]["available"].as_bool() == Some(true) {
            "yes"
        } else {
            "no"
        }
    );
    if let Some(command) = detail["detected"]["command"].as_str() {
        println!("command   = {command}");
    } else if let Some(candidates) = detail["detected"]["candidates"].as_array() {
        let candidates = candidates
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        println!("candidates = {candidates}");
    }
    println!(
        "default   = {}",
        detail["defaultMode"].as_str().unwrap_or_default()
    );
    if !detail["models"].is_null() {
        println!("models    = {}", compact_json(&detail["models"]));
    }
    if let Some(modes) = detail["modes"].as_object() {
        for (name, mode) in modes {
            println!();
            println!("[mode {name}]");
            println!(
                "transport = {}",
                mode["transport"].as_str().unwrap_or_default()
            );
            println!(
                "command   = {}",
                mode["command"].as_str().unwrap_or_default()
            );
            println!("args      = {}", compact_json(&mode["args"]));
            if !mode["env"].as_object().is_none_or(Map::is_empty) {
                println!("env       = {}", compact_json(&mode["env"]));
            }
            if !mode["stdin"].is_null() {
                println!("stdin     = {}", compact_json(&mode["stdin"]));
            }
            println!("prompt    = {}", compact_json(&mode["prompt"]));
            println!("stdout    = {}", compact_json(&mode["stdout"]));
            if !mode["stderr"].is_null() {
                println!("stderr    = {}", compact_json(&mode["stderr"]));
            }
            println!("session   = {}", compact_json(&mode["session"]));
            if !mode["runtimePlan"].is_null() {
                println!("runtime   = {}", compact_json(&mode["runtimePlan"]));
            }
        }
    }
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "<json error>".into())
}

fn display_name(manifest: &ProviderManifest) -> &str {
    if manifest.display_name.trim().is_empty() {
        manifest.id.as_str()
    } else {
        manifest.display_name.as_str()
    }
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

fn provider_source_value(config_dir: Option<&Path>, manifest: &ProviderManifest) -> Value {
    let Some(config_dir) = config_dir else {
        return json!({ "kind": "file" });
    };
    let path = providers_dir(config_dir).join(format!("{}.json", manifest.id));
    if path.exists() {
        json!({ "kind": "local", "path": path.display().to_string() })
    } else {
        json!({ "kind": "builtin" })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_config_dir(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "loom-cli-provider-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock drift")
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).expect("create temp config dir");
        path
    }

    #[test]
    fn provider_detail_exposes_source_default_mode_and_decoder_capture() {
        let config_dir = temp_config_dir("detail");
        let registry = ProviderRegistry::load(&config_dir).expect("registry");
        let manifest = registry.get("qoder").expect("qoder manifest");

        let detail = provider_detail(&registry, &config_dir, manifest, None).expect("detail");

        assert_eq!(detail["source"]["kind"], "builtin");
        assert_eq!(detail["defaultMode"], "print");
        assert_eq!(
            detail["modes"]["print"]["stdout"]["capture"]["session"]["path"],
            "$.session_id"
        );
        assert!(detail["modes"]["print"]["runtimePlan"].is_null());
        std::fs::remove_dir_all(config_dir).ok();
    }

    #[test]
    fn doctor_checks_report_missing_command_candidates() {
        let config_dir = temp_config_dir("doctor");
        let registry = ProviderRegistry::load(&config_dir).expect("registry");
        let manifest = registry.get("claude").expect("claude manifest");

        let checks = provider_doctor_checks(manifest, None);

        assert!(checks.iter().any(|check| {
            check["id"] == "manifest.valid" && check["ok"].as_bool() == Some(true)
        }));
        assert!(checks.iter().any(|check| {
            check["id"] == "command.detected"
                && check["ok"].as_bool() == Some(false)
                && check["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("candidates: claude"))
        }));
        std::fs::remove_dir_all(config_dir).ok();
    }

    #[test]
    fn standard_provider_example_is_valid_manifest() {
        let config_dir = temp_config_dir("example");
        let registry = ProviderRegistry::load(&config_dir).expect("registry");
        let manifest = registry
            .resolve_manifest_value(standard_provider_example())
            .expect("example manifest");

        assert_eq!(manifest.id, "my_provider");
        assert!(manifest.modes["print"].prompt.is_none());
        assert!(manifest.modes["print"]
            .args
            .iter()
            .any(|arg| matches!(arg, proto::methods::ProviderArgSpec::Literal(value) if value == "{prompt.full}")));
        assert_eq!(manifest.modes["print"].timeout_ms, Some(-1));
        assert_eq!(manifest.modes["print"].idle_timeout_ms, Some(-1));
        std::fs::remove_dir_all(config_dir).ok();
    }

    #[test]
    fn standard_provider_example_text_explains_agent_prompt_assembly_and_contains_json() {
        let text =
            standard_provider_example_text(&standard_provider_example()).expect("example text");

        assert!(text.contains("Provider manifest example"));
        assert!(text.contains("loom --json provider example"));
        assert!(text.contains("agent.configDir"));
        assert!(text.contains("loom.configDir"));
        assert!(text.contains("AgentSpec.promptAssembly"));
        assert!(text.contains("prompt.full"));
        assert!(text.contains("timeoutMs / idleTimeoutMs: -1 means unlimited"));
        assert!(text.contains("```json"));
        assert!(!text.contains("\"workspaceFiles\""));
    }

    #[test]
    fn provider_remove_rejects_unsafe_provider_ids() {
        for provider_id in [
            "../daemon",
            "provider/slash",
            ".hidden",
            "foo..bar",
            "Claude",
        ] {
            assert!(
                validate_provider_id_for_path(provider_id).is_err(),
                "{provider_id}"
            );
        }

        assert!(validate_provider_id_for_path("claude.local").is_ok());
        assert!(validate_provider_id_for_path("my-provider_1").is_ok());
    }

    #[test]
    fn official_provider_examples_use_builtin_manifests() {
        let examples = official_provider_examples(ExampleSelection {
            claude: true,
            opencode: true,
            ..Default::default()
        })
        .expect("official examples");

        assert_eq!(examples.len(), 2);
        assert_eq!(examples[0]["id"], "claude");
        assert_eq!(examples[1]["id"], "opencode");
        assert_eq!(examples[1]["models"]["default"], "opencode/big-pickle");
    }
}
