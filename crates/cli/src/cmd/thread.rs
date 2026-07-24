use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use proto::methods::*;
use serde_json::{json, Map, Value};

use crate::client::Client;
use crate::render;

/// `loom thread create --root-message <message_id> [--resident-as <role>] [--bootstrap-artifact <uri>]`.
///
/// After the server creates the thread, the CLI writes two channel-local
/// metadata files when the optional flags are passed (see design §4.7.1
/// + §4.7.2):
///
/// * `--resident-as` records `resident_threads.<role> = <tid>` in the
///   channel-shared `scope.json` so a router can address the thread by
///   role without scraping messages.
/// * `--bootstrap-artifact` reads the artifact, derives a `mounts` array
///   (either taken verbatim or generated from a `repos[]` clone-manifest
///   shape) and writes it to the thread-shared `scope.json`. Per-actor
///   `agent serve` workspaces seed mounts from this file on first
///   ensure_scope.
pub async fn create(
    client: Arc<Client>,
    channel_id: String,
    root_message_id: String,
    title: String,
    resident_as: Option<String>,
    bootstrap_artifact: Option<String>,
) -> Result<()> {
    let res: ThreadCreateResult = client
        .call(
            method::THREAD_CREATE,
            json!({ "channelId": channel_id, "rootMessageId": root_message_id, "title": title }),
        )
        .await?;
    let thread_id = res.thread.id.clone();

    let data_root = super::paths::agent_data_root();
    if let Some(role) = resident_as.as_deref() {
        write_resident_thread(&data_root, &channel_id, role, &thread_id)?;
    }
    let mut bootstrap_summary: Option<(String, usize)> = None;
    if let Some(uri_or_id) = bootstrap_artifact.as_deref() {
        let mounts = fetch_bootstrap_mounts(client.clone(), uri_or_id).await?;
        let count = mounts.len();
        write_thread_mounts(&data_root, &channel_id, &thread_id, &mounts)?;
        bootstrap_summary = Some((uri_or_id.to_string(), count));
    }

    if render::is_json() {
        let mut out = serde_json::to_value(&res)?;
        if let Some(role) = resident_as.as_deref() {
            out["resident_as"] = Value::String(role.to_string());
        }
        if let Some((uri, count)) = bootstrap_summary.as_ref() {
            out["bootstrap_artifact"] = Value::String(uri.clone());
            out["bootstrap_mounts"] = Value::Number((*count as u64).into());
        }
        render::print_json(&out);
    } else {
        println!("thread {}\t{}", res.thread.id, res.thread.title);
        if let Some(role) = resident_as.as_deref() {
            println!("  resident_as={role}  channel={channel_id}");
        }
        if let Some((uri, count)) = bootstrap_summary {
            println!("  bootstrap_artifact={uri}  mounts={count}");
        }
    }
    Ok(())
}

pub async fn list(client: Arc<Client>, channel_id: Option<String>, archived: bool) -> Result<()> {
    let res: ThreadListResult = client
        .call(
            method::THREAD_LIST,
            json!({ "channelId": channel_id, "archived": archived }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
        return Ok(());
    }
    if res.threads.is_empty() {
        println!(
            "{}",
            if archived {
                "(no archived threads)"
            } else {
                "(no threads)"
            }
        );
    }
    for t in res.threads {
        if archived {
            println!(
                "{}\t{}\t{}\t{}",
                t.id,
                t.channel_id,
                t.archived_at
                    .map(|ts| ts.to_rfc3339())
                    .unwrap_or_else(|| "-".into()),
                t.title
            );
        } else {
            println!("{}\t{}\t{}", t.id, t.channel_id, t.title);
        }
    }
    Ok(())
}

/// `loom thread archive <thread_id>` / `loom thread unarchive <thread_id>`.
pub async fn archive(client: Arc<Client>, thread_id: String, archived: bool) -> Result<()> {
    let res: ThreadArchiveResult = client
        .call(
            method::THREAD_ARCHIVE,
            json!({ "threadId": thread_id, "archived": archived }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else if archived {
        println!("archived thread {}", res.thread.id);
    } else {
        println!("restored thread {}", res.thread.id);
    }
    Ok(())
}

/// `loom thread delete <thread_id>`.
///
/// Calls `thread/delete` on the server, which removes the thread plus
/// its scope skills. Thread-bound services watching that thread (per
/// `bind.auto_stop_on=["thread.closed"]`, §4.7.3) reap their
/// instances on the next watcher tick. The CLI does not rewrite the
/// channel-local `resident_threads.<role>` map — callers who used
/// `--resident-as` must update it themselves.
pub async fn delete(client: Arc<Client>, thread_id: String) -> Result<()> {
    let res: ThreadDeleteResult = client
        .call(method::THREAD_DELETE, json!({ "threadId": thread_id }))
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else if res.deleted {
        println!("deleted thread {thread_id}");
    } else {
        println!("thread {thread_id} was already gone");
    }
    Ok(())
}

pub async fn follow(client: Arc<Client>, thread_id: String, muted: bool) -> Result<()> {
    let res: ThreadFollowResult = client
        .call(
            method::THREAD_FOLLOW,
            json!({ "threadId": thread_id, "muted": muted }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else if res.presence.muted {
        println!(
            "followed thread {} (muted)",
            res.presence.thread_id.unwrap_or_default()
        );
    } else {
        println!(
            "followed thread {}",
            res.presence.thread_id.unwrap_or_default()
        );
    }
    Ok(())
}

pub async fn unfollow(client: Arc<Client>, thread_id: String) -> Result<()> {
    let res: ThreadUnfollowResult = client
        .call(method::THREAD_UNFOLLOW, json!({ "threadId": thread_id }))
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!(
            "unfollowed thread {}",
            res.presence.thread_id.unwrap_or_default()
        );
    }
    Ok(())
}

pub async fn set_instruction(
    client: Arc<Client>,
    thread_id: String,
    instructions: String,
) -> Result<()> {
    warn_thread_instructions_deprecated("set");
    let res: ThreadSetInstructionResult = client
        .call(
            method::THREAD_SET_INSTRUCTION,
            json!({ "threadId": thread_id, "instructions": instructions }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else {
        println!("instructions set for thread {}", res.thread.id);
    }
    Ok(())
}

pub async fn get_instruction(client: Arc<Client>, thread_id: String) -> Result<()> {
    warn_thread_instructions_deprecated("get");
    let res: ThreadGetInstructionResult = client
        .call(
            method::THREAD_GET_INSTRUCTION,
            json!({ "threadId": thread_id }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
        return Ok(());
    }
    match res.instructions {
        Some(instructions) => println!("{}", instructions),
        None => println!("(none)"),
    }
    Ok(())
}

pub async fn clear_instruction(client: Arc<Client>, thread_id: String) -> Result<()> {
    warn_thread_instructions_deprecated("clear");
    let res: ThreadClearInstructionResult = client
        .call(
            method::THREAD_CLEAR_INSTRUCTION,
            json!({ "threadId": thread_id }),
        )
        .await?;
    if render::is_json() {
        render::print_json(&res);
    } else if res.cleared {
        println!("thread instructions cleared");
    } else {
        println!("thread instructions were already empty");
    }
    Ok(())
}

/// Emits a deprecation warning to stderr for the thread instruction CLI
/// subcommands. Thread-scoped instructions are shelved at the runtime
/// projection layer (issue #1+#9): the data model (Thread.instructions,
/// journal Mutation::ThreadInstructionSet) is preserved and the CLI
/// commands remain functional so existing scripts keep working and the
/// feature can be re-enabled in a future per-Run provider-scope
/// implementation, but values written here are NOT projected into the
/// shared AGENTS.md block consumed by agent runs. The warning is sent to
/// stderr (not stdout) so JSON output is unaffected.
fn warn_thread_instructions_deprecated(action: &str) {
    eprintln!(
        "warning: thread instructions are not projected to agent runtime in this version \
         (issue #1+#9, shelved). `loom thread instruction {action}` still reads/writes the \
         stored value, but it will not affect agent behavior. The data model is preserved \
         for a future per-Run provider-scope implementation."
    );
}

// -----------------------------------------------------------------
// Thread skill management (file-based registry, agent data root)
// -----------------------------------------------------------------

/// Resolve the channel_id for a thread via THREAD_LIST.
async fn resolve_channel_id(client: &Arc<Client>, thread_id: &str) -> Result<String> {
    let res: ThreadListResult = client
        .call(method::THREAD_LIST, json!({}))
        .await
        .context("thread.list")?;
    let thread = res
        .threads
        .into_iter()
        .find(|t| t.id == thread_id)
        .ok_or_else(|| anyhow!("thread {thread_id} not found"))?;
    Ok(thread.channel_id)
}

pub async fn skill_add(
    client: Arc<Client>,
    thread_id: String,
    source: String,
    skill_id: Option<String>,
) -> Result<()> {
    let channel_id = resolve_channel_id(&client, &thread_id).await?;
    let data_root = crate::cmd::agent_serve::default_data_root_pub();
    let id = skill_id.unwrap_or_else(|| {
        std::path::Path::new(&source)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("skill")
            .to_string()
    });
    let registry = super::skill_registry::add_thread_skill(
        &data_root,
        &channel_id,
        &thread_id,
        id.clone(),
        source.clone(),
    )
    .map_err(|err| anyhow!("write thread skill registry: {err}"))?;
    if render::is_json() {
        render::print_json(&registry);
    } else {
        println!("skill '{id}' added to thread {thread_id}");
    }
    Ok(())
}

pub async fn skill_remove(client: Arc<Client>, thread_id: String, skill_id: String) -> Result<()> {
    let channel_id = resolve_channel_id(&client, &thread_id).await?;
    let data_root = crate::cmd::agent_serve::default_data_root_pub();
    let (registry, removed) =
        super::skill_registry::remove_thread_skill(&data_root, &channel_id, &thread_id, &skill_id)
            .map_err(|err| anyhow!("read/remove thread skill registry: {err}"))?;
    if render::is_json() {
        render::print_json(&registry);
    } else if removed {
        println!("skill '{skill_id}' removed from thread {thread_id}");
    } else {
        println!("skill '{skill_id}' was not registered in thread {thread_id}");
    }
    Ok(())
}

pub async fn skill_list(client: Arc<Client>, thread_id: String) -> Result<()> {
    let channel_id = resolve_channel_id(&client, &thread_id).await?;
    let data_root = crate::cmd::agent_serve::default_data_root_pub();
    let registry = super::skill_registry::read_thread_skills(&data_root, &channel_id, &thread_id)
        .map_err(|err| anyhow!("read thread skill registry: {err}"))?;
    if render::is_json() {
        render::print_json(&registry);
    } else if registry.skills.is_empty() {
        println!("(no skills registered for thread {thread_id})");
    } else {
        for entry in &registry.skills {
            println!("{:<20} {}", entry.id, entry.source);
        }
    }
    Ok(())
}

/// `loom thread bootstrap --in <thread_id> --channel <chan> --bootstrap-artifact <uri>`.
///
/// Bootstrap an *existing* thread from a clone-manifest / mounts artifact.
/// Used when a thread already exists (e.g. a bug-fix loop reuses its bugfix
/// thread as the delivery thread) and the caller wants to add target /
/// reference repo worktrees without creating a fresh thread. The CLI:
///
/// 1. Verifies the thread exists and belongs to the given channel via
///    `thread/get`. This catches typos and prevents writing scope.json
///    for an unrelated thread.
/// 2. Fetches the artifact, derives `mounts[]` (clone-manifest or explicit).
/// 3. Writes `scope.json.mounts` on the thread-shared scope.
///
/// Per-actor `agent serve` instances pick up the new mounts on the next
/// ensure_scope (i.e. next directed message into the thread).
pub async fn bootstrap(
    client: Arc<Client>,
    channel_id: String,
    thread_id: String,
    bootstrap_artifact: String,
) -> Result<()> {
    let res: ThreadListResult = client
        .call(method::THREAD_LIST, json!({ "channelId": channel_id }))
        .await
        .with_context(|| format!("thread/list channel={channel_id}"))?;
    if !res.threads.iter().any(|t| t.id == thread_id) {
        anyhow::bail!("thread {} not found in channel {}", thread_id, channel_id);
    }
    let mounts = fetch_bootstrap_mounts(client.clone(), &bootstrap_artifact).await?;
    let count = mounts.len();
    let data_root = super::paths::agent_data_root();
    write_thread_mounts(&data_root, &channel_id, &thread_id, &mounts)?;
    if render::is_json() {
        render::print_json(&json!({
            "threadId": thread_id,
            "channelId": channel_id,
            "bootstrap_artifact": bootstrap_artifact,
            "bootstrap_mounts": count,
        }));
    } else {
        println!("bootstrapped thread {thread_id}  channel={channel_id}  mounts={count}  artifact={bootstrap_artifact}");
    }
    Ok(())
}

fn channel_shared_scope_json(data_root: &Path, channel_id: &str) -> PathBuf {
    data_root
        .join("channels")
        .join(channel_id)
        .join("shared")
        .join(".loom")
        .join("state")
        .join("scope.json")
}

fn thread_shared_scope_json(data_root: &Path, channel_id: &str, thread_id: &str) -> PathBuf {
    data_root
        .join("channels")
        .join(channel_id)
        .join("threads")
        .join(thread_id)
        .join("shared")
        .join(".loom")
        .join("state")
        .join("scope.json")
}

fn read_or_init_scope_json(path: &Path) -> Result<Value> {
    match std::fs::read_to_string(path) {
        Ok(body) => {
            serde_json::from_str(&body).with_context(|| format!("parse {}", path.display()))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Value::Object(Map::new())),
        Err(err) => Err(err).with_context(|| format!("read {}", path.display())),
    }
}

fn write_pretty(path: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("mkdir -p {}", parent.display()))?;
    }
    let body = serde_json::to_string_pretty(value)?;
    std::fs::write(path, body).with_context(|| format!("write {}", path.display()))
}

/// Record `resident_threads.<role> = <thread_id>` on the channel-shared
/// scope.json. Idempotent and preserves any other keys.
pub(crate) fn write_resident_thread(
    data_root: &Path,
    channel_id: &str,
    role: &str,
    thread_id: &str,
) -> Result<()> {
    let path = channel_shared_scope_json(data_root, channel_id);
    let mut value = read_or_init_scope_json(&path)?;
    let obj = value
        .as_object_mut()
        .ok_or_else(|| anyhow!("{} is not a JSON object", path.display()))?;
    obj.entry("scope_kind")
        .or_insert_with(|| Value::String("channel".into()));
    obj.entry("channel_id")
        .or_insert_with(|| Value::String(channel_id.to_string()));
    let entry = obj
        .entry("resident_threads")
        .or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    entry
        .as_object_mut()
        .unwrap()
        .insert(role.to_string(), Value::String(thread_id.to_string()));
    write_pretty(&path, &value)
}

/// Write the bootstrap-derived `mounts` array onto the thread-shared
/// scope.json so per-actor `agent serve` workspaces can seed from it.
pub(crate) fn write_thread_mounts(
    data_root: &Path,
    channel_id: &str,
    thread_id: &str,
    mounts: &[Value],
) -> Result<()> {
    let path = thread_shared_scope_json(data_root, channel_id, thread_id);
    let mut value = read_or_init_scope_json(&path)?;
    let obj = value
        .as_object_mut()
        .ok_or_else(|| anyhow!("{} is not a JSON object", path.display()))?;
    obj.entry("scope_kind")
        .or_insert_with(|| Value::String("thread".into()));
    obj.entry("channel_id")
        .or_insert_with(|| Value::String(channel_id.to_string()));
    obj.entry("thread_id")
        .or_insert_with(|| Value::String(thread_id.to_string()));
    obj.insert("mounts".into(), Value::Array(mounts.to_vec()));
    write_pretty(&path, &value)
}

async fn fetch_bootstrap_mounts(client: Arc<Client>, uri_or_id: &str) -> Result<Vec<Value>> {
    // Resolve to a concrete artifact id (ARTIFACT_READ takes ids, not URIs).
    let get_params = if uri_or_id.starts_with("artifact://") {
        json!({ "artifactUri": uri_or_id })
    } else {
        json!({ "artifactId": uri_or_id })
    };
    let got: ArtifactGetResult = client.call(method::ARTIFACT_GET, get_params).await?;
    let artifact_id = got.artifact.id.clone();
    let read: ArtifactReadResult = client
        .call(
            method::ARTIFACT_READ,
            json!({ "artifactId": artifact_id, "maxBytes": 1_048_576u64 }),
        )
        .await?;
    if read.truncated {
        anyhow::bail!(
            "bootstrap artifact {} was truncated at 1MiB; refusing to derive mounts from a partial body",
            artifact_id
        );
    }
    parse_bootstrap_mounts(&read.content)
        .with_context(|| format!("parse bootstrap artifact {} as clone manifest", artifact_id))
}

/// Accept either an explicit `{ "mounts": [...] }` payload or a
/// clone-manifest shape `{ "repos": [{repo_id, ...}] }`. The latter is
/// projected into mounts that point at the shared `repo-cache` service
/// host data dir, matching design §4.7.2.
pub(crate) fn parse_bootstrap_mounts(body: &str) -> Result<Vec<Value>> {
    let parsed: Value = serde_json::from_str(body).context("artifact body is not JSON")?;
    if let Some(arr) = parsed.get("mounts").and_then(|v| v.as_array()) {
        return Ok(arr.clone());
    }
    if let Some(repos) = parsed.get("repos").and_then(|v| v.as_array()) {
        let mut mounts = Vec::with_capacity(repos.len());
        for repo in repos {
            let obj = repo
                .as_object()
                .ok_or_else(|| anyhow!("repos[] entry must be an object"))?;
            let repo_id = obj
                .get("repo_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("repos[].repo_id missing"))?;
            let to = obj
                .get("to")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    let leaf = repo_id.rsplit('/').next().unwrap_or(repo_id);
                    format!("repos/{leaf}")
                });
            let readonly = obj
                .get("readonly")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let from = obj
                .get("from")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    let encoded = repo_id.replace('/', "%2F");
                    format!("service://repo-cache/cache/{encoded}")
                });
            mounts.push(json!({
                "name": format!("target-repo:{repo_id}"),
                "from": from,
                "to": to,
                "readonly": readonly,
                "ref": obj.get("ref").and_then(|v| v.as_str()).unwrap_or("HEAD"),
                "repo_id": repo_id,
            }));
        }
        return Ok(mounts);
    }
    anyhow::bail!("bootstrap artifact must have either `mounts[]` or `repos[]`");
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            let mut p = std::env::temp_dir();
            p.push(format!(
                "loom-thread-tests-{tag}-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("clock drift")
                    .as_nanos()
            ));
            std::fs::create_dir_all(&p).unwrap();
            TempDir(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn tempdir() -> TempDir {
        TempDir::new("scope")
    }

    #[test]
    fn parse_mounts_passthrough() {
        let body = r#"{"mounts":[{"name":"a","from":"channel://x","to":"x","readonly":true}]}"#;
        let mounts = parse_bootstrap_mounts(body).expect("parse");
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0]["name"], "a");
        assert_eq!(mounts[0]["readonly"], true);
    }

    #[test]
    fn parse_mounts_from_clone_manifest() {
        let body = r#"{"repos":[{"repo_id":"aone/loom-apps"},{"repo_id":"aone/other","to":"repos/custom","readonly":true}]}"#;
        let mounts = parse_bootstrap_mounts(body).expect("parse");
        assert_eq!(mounts.len(), 2);
        assert_eq!(mounts[0]["name"], "target-repo:aone/loom-apps");
        assert_eq!(
            mounts[0]["from"],
            "service://repo-cache/cache/aone%2Floom-apps"
        );
        assert_eq!(mounts[0]["to"], "repos/loom-apps");
        assert_eq!(mounts[0]["readonly"], false);
        assert_eq!(mounts[1]["to"], "repos/custom");
        assert_eq!(mounts[1]["readonly"], true);
    }

    #[test]
    fn parse_mounts_rejects_unknown_shape() {
        assert!(parse_bootstrap_mounts(r#"{"hello":"world"}"#).is_err());
        assert!(parse_bootstrap_mounts("not json").is_err());
    }

    #[test]
    fn write_resident_thread_is_idempotent_and_merging() {
        let tmp = tempdir();
        write_resident_thread(tmp.path(), "chan_a", "discovery", "thr_1").unwrap();
        write_resident_thread(tmp.path(), "chan_a", "ops", "thr_2").unwrap();
        // Replace existing role.
        write_resident_thread(tmp.path(), "chan_a", "discovery", "thr_3").unwrap();
        let path = channel_shared_scope_json(tmp.path(), "chan_a");
        let body = std::fs::read_to_string(&path).unwrap();
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["scope_kind"], "channel");
        assert_eq!(v["channel_id"], "chan_a");
        assert_eq!(v["resident_threads"]["discovery"], "thr_3");
        assert_eq!(v["resident_threads"]["ops"], "thr_2");
    }

    #[test]
    fn write_thread_mounts_overwrites_array() {
        let tmp = tempdir();
        let mounts1 = vec![json!({"name":"a","from":"channel://x","to":"x","readonly":false})];
        let mounts2 = vec![
            json!({"name":"b","from":"channel://y","to":"y","readonly":true}),
            json!({"name":"c","from":"service://s/z","to":"z","readonly":false}),
        ];
        write_thread_mounts(tmp.path(), "chan_a", "thr_1", &mounts1).unwrap();
        write_thread_mounts(tmp.path(), "chan_a", "thr_1", &mounts2).unwrap();
        let path = thread_shared_scope_json(tmp.path(), "chan_a", "thr_1");
        let body = std::fs::read_to_string(&path).unwrap();
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["scope_kind"], "thread");
        assert_eq!(v["channel_id"], "chan_a");
        assert_eq!(v["thread_id"], "thr_1");
        let arr = v["mounts"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["name"], "b");
    }
}
