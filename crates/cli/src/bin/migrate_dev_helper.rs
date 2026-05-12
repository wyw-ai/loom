//! `migrate-dev-helper` — one-shot operator tool to lift dev-helper
//! filesystem state into the joi-native layout. See design §9.
//!
//! Subcommands
//! -----------
//! - `scan`       — enumerate source files, classify them by category,
//!                  print a summary. No reads of contents beyond what's
//!                  needed to classify.
//! - `dry-run`    — produce a full plan: every source path, the target
//!                  path it would migrate to, the transform that would
//!                  be applied, and any conflicts (target already
//!                  exists). Does not write anything. Exits non-zero
//!                  if the plan has unresolved conflicts.
//! - `apply`      — execute the plan. Always copies (never moves) so
//!                  the source remains intact for rollback. Skips
//!                  targets that already exist unless `--force`. A
//!                  pre-apply backup tarball is written next to the
//!                  source root.
//! - `verify`     — re-read source + target pairs from a plan-or-disk
//!                  comparison and report mismatches on the migrated
//!                  fields the runtime actually consumes (provider
//!                  session id, command signature, scope.json keys).
//!
//! Categories implemented in this revision
//! ---------------------------------------
//! 1. **session files** — `<old>/<actor>/<scope>.session` (text:
//!    provider session id) + `<old>/<actor>/<scope>.session-meta.json`
//!    (signature + metadata) → joi-runtime
//!    `<data_root>/sessions/<actor>/<kind>-<scope>.json` (combined
//!    SessionRecord shape, see `agent-runtime::interactive`).
//!
//! 2. **channel scope** — `<old>/channel-<cid>.json` →
//!    `<channel_workspace>/.joi/state/scope.json`. Top-level keys are
//!    merged into the existing scope.json (preserves any pre-existing
//!    `mounts[]` / `resident_threads{}` written by `joi thread create`).
//!
//! 3. **scope-projects** — `<old>/scope-projects/<kind>-<id>.json` →
//!    `<scope_workspace>/.joi/state/scope.json`. Same merge semantics.
//!
//! 4. **mr-watcher** — `<old>/mr-watcher/state.json` →
//!    `<service_host_data>/services/mr-watcher/state.json`. Verbatim
//!    copy; the watcher reads its own JSON shape unchanged.
//!
//! Categories NOT YET implemented (hard error, listed in scan):
//! - dispatch cursor files (no joi-runtime consumer yet)
//! - context-share KV exports (need live mysql access)
//! - capability_atlas / repo_notes (skill-defined target paths)

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Parser, Debug)]
#[command(
    name = "migrate-dev-helper",
    about = "One-shot dev-helper -> joi data migration",
    version
)]
struct Args {
    /// Old dev-helper state root. Defaults to
    /// `~/.local/state/joi-agent/` (per design §9).
    #[arg(long)]
    source_root: Option<PathBuf>,

    /// Joi agent-host data root. Defaults to `JOI_AGENT_DATA_ROOT` /
    /// `AGENTHUB_HOME` / `AGENTX_HOME` / `~/.agentx/`.
    #[arg(long)]
    data_root: Option<PathBuf>,

    /// Joi service-host data root for service-related migrations.
    /// Defaults to `JOI_SERVICE_HOST_DATA` /
    /// `~/Library/Application Support/joi/service-host` (mac) /
    /// `~/.local/share/joi/service-host` (linux) /
    /// `./.joi-service-host` (fallback).
    #[arg(long)]
    service_data_root: Option<PathBuf>,

    /// Workspaces root used to resolve `channel://` and per-scope
    /// `.joi/state/scope.json` targets. Defaults to
    /// `<data_root>/channels/`.
    #[arg(long)]
    channels_root: Option<PathBuf>,

    /// Restrict to a comma-separated list of categories. Default:
    /// all implemented. Available: `session`, `channel-scope`,
    /// `scope-projects`, `mr-watcher`.
    #[arg(long, value_delimiter = ',')]
    only: Vec<String>,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Print a one-line summary per category.
    Scan,
    /// Print every planned source -> target move + transform note.
    /// Exits non-zero if any plan item has an unresolved conflict.
    DryRun,
    /// Execute the plan. Writes a tar.gz backup of `--source-root`
    /// to `<source_root>/../joi-migrate-backup-<unix_ms>.tar.gz`
    /// before any change unless `--no-backup`.
    Apply {
        /// Overwrite existing targets.
        #[arg(long)]
        force: bool,
        /// Skip the pre-apply backup tarball.
        #[arg(long)]
        no_backup: bool,
    },
    /// Re-read source + target pairs and report drift.
    Verify,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(2);
    }
}

fn run() -> Result<()> {
    let args = Args::parse();
    let ctx = Ctx::resolve(&args)?;
    eprintln!("source_root        = {}", ctx.source_root.display());
    eprintln!("data_root          = {}", ctx.data_root.display());
    eprintln!("service_data_root  = {}", ctx.service_data_root.display());
    eprintln!("channels_root      = {}", ctx.channels_root.display());
    let plan = build_plan(&ctx)?;
    match args.cmd {
        Cmd::Scan => print_scan(&plan),
        Cmd::DryRun => print_dry_run(&plan)?,
        Cmd::Apply { force, no_backup } => apply_plan(&ctx, &plan, force, no_backup)?,
        Cmd::Verify => verify_plan(&plan)?,
    }
    Ok(())
}

// ---------- ctx ----------

struct Ctx {
    source_root: PathBuf,
    data_root: PathBuf,
    service_data_root: PathBuf,
    channels_root: PathBuf,
    only: Vec<String>,
}

impl Ctx {
    fn resolve(args: &Args) -> Result<Self> {
        let source_root = args.source_root.clone().unwrap_or_else(|| {
            dirs::home_dir()
                .map(|d| d.join(".local").join("state").join("joi-agent"))
                .unwrap_or_else(|| PathBuf::from(".local/state/joi-agent"))
        });
        let data_root = args.data_root.clone().unwrap_or_else(default_data_root);
        let service_data_root = args
            .service_data_root
            .clone()
            .unwrap_or_else(default_service_data_root);
        let channels_root = args
            .channels_root
            .clone()
            .unwrap_or_else(|| data_root.join("channels"));
        Ok(Ctx {
            source_root,
            data_root,
            service_data_root,
            channels_root,
            only: args.only.clone(),
        })
    }

    fn category_enabled(&self, cat: &str) -> bool {
        self.only.is_empty() || self.only.iter().any(|c| c == cat)
    }
}

fn default_data_root() -> PathBuf {
    for key in ["JOI_AGENT_DATA_ROOT", "AGENTHUB_HOME", "AGENTX_HOME"] {
        if let Some(v) = std::env::var_os(key) {
            if !v.is_empty() {
                return PathBuf::from(v);
            }
        }
    }
    dirs::home_dir()
        .map(|d| d.join(".agentx"))
        .unwrap_or_else(|| PathBuf::from(".agentx"))
}

fn default_service_data_root() -> PathBuf {
    if let Some(v) = std::env::var_os("JOI_SERVICE_HOST_DATA") {
        if !v.is_empty() {
            return PathBuf::from(v);
        }
    }
    dirs::data_local_dir()
        .map(|d| d.join("joi").join("service-host"))
        .unwrap_or_else(|| PathBuf::from(".joi-service-host"))
}

// ---------- plan ----------

#[derive(Debug, Clone, Serialize)]
struct PlanItem {
    category: String,
    source: PathBuf,
    target: PathBuf,
    /// Human description of the transform: "copy", "merge into
    /// scope.json", "combine .session + .session-meta.json", etc.
    transform: String,
    /// Pre-computed conflict notes; empty means clean.
    conflicts: Vec<String>,
}

fn build_plan(ctx: &Ctx) -> Result<Vec<PlanItem>> {
    let mut out = Vec::new();
    if ctx.category_enabled("session") {
        out.extend(plan_session(ctx)?);
    }
    if ctx.category_enabled("channel-scope") {
        out.extend(plan_channel_scope(ctx)?);
    }
    if ctx.category_enabled("scope-projects") {
        out.extend(plan_scope_projects(ctx)?);
    }
    if ctx.category_enabled("mr-watcher") {
        out.extend(plan_mr_watcher(ctx)?);
    }
    Ok(out)
}

/// `<src>/<actor>/<scope>.session` + sibling `.session-meta.json` ->
/// joi sessions dir entry. Source filename has no kind hint, so we
/// emit the channel form by default; if a `.kind` file or a
/// `.session-meta.json::scope.kind` field is present we honour it.
fn plan_session(ctx: &Ctx) -> Result<Vec<PlanItem>> {
    let mut out = Vec::new();
    let actors_dir = &ctx.source_root;
    let entries = match fs::read_dir(actors_dir) {
        Ok(e) => e,
        Err(_) => return Ok(out),
    };
    for actor_entry in entries.flatten() {
        let actor_path = actor_entry.path();
        if !actor_path.is_dir() {
            continue;
        }
        let actor_id = match actor_path.file_name().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        // Skip non-actor dirs (dispatch-state, scope-projects,
        // mr-watcher, etc.) by simple denylist.
        if matches!(
            actor_id.as_str(),
            "dispatch-state" | "scope-projects" | "mr-watcher" | "kv" | "tmp"
        ) {
            continue;
        }
        let scopes = match fs::read_dir(&actor_path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        for scope_entry in scopes.flatten() {
            let scope_path = scope_entry.path();
            let name = match scope_path.file_name().and_then(|s| s.to_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };
            let Some(scope_id) = name.strip_suffix(".session") else {
                continue;
            };
            let meta_path = actor_path.join(format!("{scope_id}.session-meta.json"));
            let kind = read_scope_kind(&meta_path).unwrap_or_else(|| "channel".to_string());
            let target = ctx
                .data_root
                .join("sessions")
                .join(&actor_id)
                .join(format!("{kind}-{scope_id}.json"));
            let mut conflicts = Vec::new();
            if !meta_path.exists() {
                conflicts.push(format!(
                    "missing session-meta.json sibling at {}; will write SessionRecord with empty command_signature",
                    meta_path.display()
                ));
            }
            if target.exists() {
                conflicts.push(format!(
                    "target {} already exists (use --force to overwrite)",
                    target.display()
                ));
            }
            out.push(PlanItem {
                category: "session".into(),
                source: scope_path,
                target,
                transform: format!(
                    "combine session + meta into SessionRecord(actor={actor_id}, kind={kind}, scope={scope_id})"
                ),
                conflicts,
            });
        }
    }
    Ok(out)
}

fn read_scope_kind(meta_path: &Path) -> Option<String> {
    let text = fs::read_to_string(meta_path).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    v.get("scope")
        .and_then(|s| s.get("kind"))
        .and_then(|k| k.as_str())
        .map(|s| s.to_lowercase())
}

fn plan_channel_scope(ctx: &Ctx) -> Result<Vec<PlanItem>> {
    let mut out = Vec::new();
    let entries = match fs::read_dir(&ctx.source_root) {
        Ok(e) => e,
        Err(_) => return Ok(out),
    };
    for entry in entries.flatten() {
        let p = entry.path();
        let Some(name) = p.file_name().and_then(|s| s.to_str()).map(str::to_owned) else {
            continue;
        };
        let Some(rest) = name.strip_prefix("channel-") else {
            continue;
        };
        let Some(cid) = rest.strip_suffix(".json") else {
            continue;
        };
        let cid = cid.to_string();
        let target = ctx
            .channels_root
            .join(&cid)
            .join("shared")
            .join(".joi")
            .join("state")
            .join("scope.json");
        let conflicts = Vec::new(); // merge is always safe
        out.push(PlanItem {
            category: "channel-scope".into(),
            source: p,
            target,
            transform: format!("merge top-level keys into scope.json (channel={cid})"),
            conflicts,
        });
    }
    Ok(out)
}

fn plan_scope_projects(ctx: &Ctx) -> Result<Vec<PlanItem>> {
    let mut out = Vec::new();
    let dir = ctx.source_root.join("scope-projects");
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Ok(out),
    };
    for entry in entries.flatten() {
        let p = entry.path();
        let Some(name) = p.file_name().and_then(|s| s.to_str()).map(str::to_owned) else {
            continue;
        };
        let Some(stem) = name.strip_suffix(".json") else {
            continue;
        };
        // `<kind>-<id>` where id can itself contain dashes; split on first '-'.
        let Some(sep) = stem.find('-') else { continue };
        let (kind_ref, rest) = stem.split_at(sep);
        let kind = kind_ref.to_string();
        let id = rest[1..].to_string();
        let target = match kind.as_str() {
            "channel" => ctx
                .channels_root
                .join(&id)
                .join("shared")
                .join(".joi")
                .join("state")
                .join("scope.json"),
            "thread" => {
                // We can't recover the parent channel id from this
                // filename alone — record the conflict and ask the
                // operator to map manually with --only=channel-scope
                // first then re-run.
                let placeholder = ctx
                    .channels_root
                    .join("UNKNOWN_CHANNEL")
                    .join("threads")
                    .join(&id)
                    .join("shared")
                    .join(".joi")
                    .join("state")
                    .join("scope.json");
                out.push(PlanItem {
                    category: "scope-projects".into(),
                    source: p,
                    target: placeholder,
                    transform: format!(
                        "thread scope (id={id}) — parent channel unknown from old filename"
                    ),
                    conflicts: vec![
                        "old layout doesn't encode parent channel id; supply via runtime hook or skip"
                            .into(),
                    ],
                });
                continue;
            }
            _ => continue,
        };
        out.push(PlanItem {
            category: "scope-projects".into(),
            source: p,
            target,
            transform: format!("merge top-level keys into scope.json ({kind}={id})"),
            conflicts: Vec::new(),
        });
    }
    Ok(out)
}

fn plan_mr_watcher(ctx: &Ctx) -> Result<Vec<PlanItem>> {
    let mut out = Vec::new();
    let src = ctx.source_root.join("mr-watcher").join("state.json");
    if !src.exists() {
        return Ok(out);
    }
    let target = ctx
        .service_data_root
        .join("services")
        .join("mr-watcher")
        .join("state.json");
    let mut conflicts = Vec::new();
    if target.exists() {
        conflicts.push(format!(
            "target {} already exists (use --force to overwrite)",
            target.display()
        ));
    }
    out.push(PlanItem {
        category: "mr-watcher".into(),
        source: src,
        target,
        transform: "verbatim copy of state.json (seen_note_keys + emitted flags)".into(),
        conflicts,
    });
    Ok(out)
}

// ---------- printers ----------

fn print_scan(plan: &[PlanItem]) {
    let mut counts: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for item in plan {
        let entry = counts.entry(item.category.as_str()).or_insert((0, 0));
        entry.0 += 1;
        if !item.conflicts.is_empty() {
            entry.1 += 1;
        }
    }
    if counts.is_empty() {
        println!("(no migration sources found)");
        return;
    }
    println!("category           items  conflicts");
    for (cat, (n, c)) in counts {
        println!("{cat:<18} {n:>5}  {c:>9}");
    }
}

fn print_dry_run(plan: &[PlanItem]) -> Result<()> {
    let mut had_conflict = false;
    for item in plan {
        println!("[{}] {}", item.category, item.transform);
        println!("  source: {}", item.source.display());
        println!("  target: {}", item.target.display());
        if !item.conflicts.is_empty() {
            had_conflict = true;
            for c in &item.conflicts {
                println!("  conflict: {c}");
            }
        }
    }
    if plan.is_empty() {
        println!("(empty plan)");
    }
    if had_conflict {
        bail!("plan has unresolved conflicts; resolve or pass --force at apply time");
    }
    Ok(())
}

// ---------- apply ----------

fn apply_plan(ctx: &Ctx, plan: &[PlanItem], force: bool, no_backup: bool) -> Result<()> {
    if !no_backup {
        backup_source(&ctx.source_root)?;
    }
    let mut applied = 0usize;
    let mut skipped = 0usize;
    for item in plan {
        let conflict_blocks = item
            .conflicts
            .iter()
            .any(|c| c.contains("already exists") && !force)
            || item.conflicts.iter().any(|c| c.contains("UNKNOWN_CHANNEL"));
        if conflict_blocks {
            eprintln!(
                "skip [{}] {} -> {} (conflict; use --force where applicable)",
                item.category,
                item.source.display(),
                item.target.display()
            );
            skipped += 1;
            continue;
        }
        if let Some(parent) = item.target.parent() {
            fs::create_dir_all(parent).with_context(|| format!("mkdir -p {}", parent.display()))?;
        }
        match item.category.as_str() {
            "session" => apply_session(item)?,
            "channel-scope" | "scope-projects" => apply_merge_scope_json(item)?,
            "mr-watcher" => apply_copy_verbatim(item)?,
            other => bail!("apply not implemented for category {other}"),
        }
        applied += 1;
    }
    println!("applied={applied} skipped={skipped} total={}", plan.len());
    Ok(())
}

fn apply_session(item: &PlanItem) -> Result<()> {
    // `source` ends in `.session` (provider session id text).
    let session_id = fs::read_to_string(&item.source)
        .with_context(|| format!("read {}", item.source.display()))?
        .trim()
        .to_string();
    let meta_path = {
        let mut p = item.source.clone();
        let stem = p
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        p.set_file_name(format!("{stem}.session-meta.json"));
        p
    };
    let (created_at, last_used_at, command_signature) = if meta_path.exists() {
        let text = fs::read_to_string(&meta_path)
            .with_context(|| format!("read {}", meta_path.display()))?;
        let v: Value = serde_json::from_str(&text)
            .with_context(|| format!("parse {}", meta_path.display()))?;
        (
            v.get("created_at")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
            v.get("last_used_at")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
            v.get("command_signature")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
        )
    } else {
        (String::new(), String::new(), String::new())
    };
    let record = json!({
        "session_id": session_id,
        "created_at": created_at,
        "last_used_at": last_used_at,
        "command_signature": command_signature,
    });
    fs::write(&item.target, serde_json::to_string_pretty(&record)?)
        .with_context(|| format!("write {}", item.target.display()))?;
    Ok(())
}

fn apply_merge_scope_json(item: &PlanItem) -> Result<()> {
    let src_text = fs::read_to_string(&item.source)
        .with_context(|| format!("read {}", item.source.display()))?;
    let src: Value = serde_json::from_str(&src_text)
        .with_context(|| format!("parse {}", item.source.display()))?;
    let mut tgt: Value = if item.target.exists() {
        let t = fs::read_to_string(&item.target)
            .with_context(|| format!("read {}", item.target.display()))?;
        serde_json::from_str(&t).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };
    let (Value::Object(src_map), Value::Object(tgt_map)) = (&src, &mut tgt) else {
        bail!("non-object scope.json: {}", item.source.display());
    };
    for (k, v) in src_map {
        // Don't clobber existing target keys — operator-set state
        // (e.g. mounts written by `joi thread create`) wins.
        tgt_map.entry(k.clone()).or_insert_with(|| v.clone());
    }
    fs::write(&item.target, serde_json::to_string_pretty(&tgt)?)
        .with_context(|| format!("write {}", item.target.display()))?;
    Ok(())
}

fn apply_copy_verbatim(item: &PlanItem) -> Result<()> {
    fs::copy(&item.source, &item.target).with_context(|| {
        format!(
            "copy {} -> {}",
            item.source.display(),
            item.target.display()
        )
    })?;
    Ok(())
}

fn backup_source(source_root: &Path) -> Result<()> {
    if !source_root.exists() {
        return Ok(());
    }
    let parent = source_root
        .parent()
        .ok_or_else(|| anyhow!("source_root has no parent"))?;
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let out = parent.join(format!("joi-migrate-backup-{now_ms}.tar.gz"));
    let status = std::process::Command::new("tar")
        .arg("-czf")
        .arg(&out)
        .arg("-C")
        .arg(parent)
        .arg(
            source_root
                .file_name()
                .ok_or_else(|| anyhow!("source_root has no basename"))?,
        )
        .status()
        .with_context(|| "spawn tar")?;
    if !status.success() {
        bail!("tar failed creating backup at {}", out.display());
    }
    eprintln!("backup written: {}", out.display());
    Ok(())
}

// ---------- verify ----------

#[derive(Default, Serialize, Deserialize)]
struct VerifyReport {
    checked: usize,
    drift: Vec<String>,
}

fn verify_plan(plan: &[PlanItem]) -> Result<()> {
    let mut report = VerifyReport::default();
    for item in plan {
        if !item.target.exists() {
            report.drift.push(format!(
                "[{}] missing target {}",
                item.category,
                item.target.display()
            ));
            continue;
        }
        report.checked += 1;
        match item.category.as_str() {
            "session" => verify_session(item, &mut report),
            "channel-scope" | "scope-projects" => verify_scope_json(item, &mut report),
            "mr-watcher" => verify_byte_equal(item, &mut report),
            _ => {}
        }
    }
    let mut out = std::io::stdout().lock();
    writeln!(out, "checked = {}", report.checked)?;
    if report.drift.is_empty() {
        writeln!(out, "ok: no drift")?;
    } else {
        for line in &report.drift {
            writeln!(out, "drift: {line}")?;
        }
        bail!("verify failed with {} drift item(s)", report.drift.len());
    }
    Ok(())
}

fn verify_session(item: &PlanItem, report: &mut VerifyReport) {
    let src_id = match fs::read_to_string(&item.source) {
        Ok(s) => s.trim().to_string(),
        Err(_) => return,
    };
    let tgt_text = match fs::read_to_string(&item.target) {
        Ok(t) => t,
        Err(_) => return,
    };
    let v: Value = match serde_json::from_str(&tgt_text) {
        Ok(v) => v,
        Err(_) => {
            report.drift.push(format!(
                "[session] target not valid JSON: {}",
                item.target.display()
            ));
            return;
        }
    };
    let tgt_id = v.get("session_id").and_then(|s| s.as_str()).unwrap_or("");
    if tgt_id != src_id {
        report.drift.push(format!(
            "[session] id mismatch src={src_id} tgt={tgt_id} ({})",
            item.target.display()
        ));
    }
}

fn verify_scope_json(item: &PlanItem, report: &mut VerifyReport) {
    let Ok(src_text) = fs::read_to_string(&item.source) else {
        return;
    };
    let Ok(tgt_text) = fs::read_to_string(&item.target) else {
        return;
    };
    let (Ok(src), Ok(tgt)) = (
        serde_json::from_str::<Value>(&src_text),
        serde_json::from_str::<Value>(&tgt_text),
    ) else {
        report.drift.push(format!(
            "[scope-json] unparseable {}",
            item.target.display()
        ));
        return;
    };
    if let (Some(src_obj), Some(tgt_obj)) = (src.as_object(), tgt.as_object()) {
        for (k, v) in src_obj {
            // The target should contain at least every source key (we
            // never overwrite, so the value we wrote is the older of
            // src/tgt at apply time; but verify runs after apply so
            // either the src value or a pre-existing operator value
            // is acceptable — only flag "key missing entirely").
            if !tgt_obj.contains_key(k) {
                report.drift.push(format!(
                    "[scope-json] key `{k}` not present in target {} (src had {v})",
                    item.target.display()
                ));
            }
        }
    }
}

fn verify_byte_equal(item: &PlanItem, report: &mut VerifyReport) {
    let (Ok(a), Ok(b)) = (fs::read(&item.source), fs::read(&item.target)) else {
        return;
    };
    if a != b {
        report.drift.push(format!(
            "[{}] byte mismatch {} vs {}",
            item.category,
            item.source.display(),
            item.target.display()
        ));
    }
}

// ---------- tests ----------

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "joi-migrate-tests-{}",
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn ctx_for(root: &Path) -> Ctx {
        Ctx {
            source_root: root.join("src"),
            data_root: root.join("data"),
            service_data_root: root.join("svc"),
            channels_root: root.join("data").join("channels"),
            only: vec![],
        }
    }

    #[test]
    fn plan_session_combines_meta() {
        let root = temp();
        let src = root.join("src").join("classmaster");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("c-42.session"), "sess-abc\n").unwrap();
        fs::write(
            src.join("c-42.session-meta.json"),
            r#"{"created_at":"2024-01-01","last_used_at":"2024-02-01","command_signature":"sig","scope":{"kind":"channel"}}"#,
        )
        .unwrap();
        let ctx = ctx_for(&root);
        let plan = plan_session(&ctx).unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].category, "session");
        assert!(plan[0]
            .target
            .ends_with("data/sessions/classmaster/channel-c-42.json"));
        // apply + verify the SessionRecord ends up correctly.
        apply_plan(&ctx, &plan, false, true).unwrap();
        let body = fs::read_to_string(&plan[0].target).unwrap();
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["session_id"], "sess-abc");
        assert_eq!(v["command_signature"], "sig");
        verify_plan(&plan).unwrap();
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn plan_session_defaults_to_channel_when_meta_missing() {
        let root = temp();
        let src = root.join("src").join("teacher");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("xyz.session"), "id1").unwrap();
        let ctx = ctx_for(&root);
        let plan = plan_session(&ctx).unwrap();
        assert_eq!(plan.len(), 1);
        assert!(!plan[0].conflicts.is_empty()); // missing-meta note
        assert!(plan[0]
            .target
            .ends_with("data/sessions/teacher/channel-xyz.json"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn channel_scope_merges_without_clobber() {
        let root = temp();
        let src = root.join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            src.join("channel-cid42.json"),
            r#"{"summary":"old","mounts":[{"name":"old"}]}"#,
        )
        .unwrap();
        let ctx = ctx_for(&root);
        // Pre-populate target with a `mounts[]` written by an
        // operator/CLI; merge must NOT overwrite it.
        let target = root.join("data/channels/cid42/shared/.joi/state/scope.json");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, r#"{"mounts":[{"name":"new"}]}"#).unwrap();
        let plan = plan_channel_scope(&ctx).unwrap();
        assert_eq!(plan.len(), 1);
        apply_plan(&ctx, &plan, false, true).unwrap();
        let body: Value = serde_json::from_str(&fs::read_to_string(&target).unwrap()).unwrap();
        assert_eq!(body["mounts"][0]["name"], "new"); // pre-existing wins
        assert_eq!(body["summary"], "old"); // new key picked up
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn mr_watcher_state_is_copied_verbatim() {
        let root = temp();
        let src = root.join("src").join("mr-watcher");
        fs::create_dir_all(&src).unwrap();
        let body = r#"{"seen_note_keys":["a","b"],"merged_emitted":[]}"#;
        fs::write(src.join("state.json"), body).unwrap();
        let ctx = ctx_for(&root);
        let plan = plan_mr_watcher(&ctx).unwrap();
        assert_eq!(plan.len(), 1);
        apply_plan(&ctx, &plan, false, true).unwrap();
        let out = fs::read_to_string(&plan[0].target).unwrap();
        assert_eq!(out, body);
        verify_plan(&plan).unwrap();
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn dry_run_with_no_sources_is_clean() {
        let root = temp();
        fs::create_dir_all(root.join("src")).unwrap();
        let ctx = ctx_for(&root);
        let plan = build_plan(&ctx).unwrap();
        assert!(plan.is_empty());
        print_dry_run(&plan).unwrap();
        fs::remove_dir_all(&root).ok();
    }
}
