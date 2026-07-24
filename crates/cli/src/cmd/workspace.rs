//! `loom workspace` — read/write/list files inside a scope workspace.
//!
//! Mirrors the layout that `crate::cmd::agent_serve` provisions:
//!
//! - per-actor scope workspace: `<data_root>/channels/<cid>/agents/<aid>/workspace/`
//! - shared per-channel area:   `<data_root>/channels/<cid>/shared/`
//! - shared per-thread area:    `<data_root>/channels/<cid>/threads/<tid>/shared/`
//!   (the thread variant is provisioned by Phase 1 changes to agent_serve.)
//!
//! These commands are pure local filesystem operations — they do not
//! contact the loom-server. They exist so skill processes (running under a
//! served agent) and operators can read/write workspace files without
//! re-implementing the path-resolution dance.

use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};

use crate::render;

#[derive(Debug, Clone, Copy)]
pub enum WsKind {
    /// Per-actor workspace under `channels/<cid>/agents/<aid>/workspace/`.
    /// Requires both --channel/--in *and* --actor (or env LOOM_ACTOR).
    Actor,
    /// Shared channel area under `channels/<cid>/shared/`.
    Channel,
    /// Shared thread area under `channels/<cid>/threads/<tid>/shared/`.
    Thread,
}

#[derive(Debug, Clone)]
pub struct WsRef {
    pub kind: WsKind,
    pub channel_id: String,
    pub thread_id: Option<String>,
    pub actor_id: Option<String>,
}

impl WsRef {
    pub fn resolve(&self) -> Result<PathBuf> {
        let base = super::paths::agent_data_root();
        let channel_root = base.join("channels").join(&self.channel_id);
        let path = match self.kind {
            WsKind::Actor => {
                let actor = self
                    .actor_id
                    .as_deref()
                    .ok_or_else(|| anyhow!("actor workspace requires --actor or LOOM_ACTOR"))?;
                channel_root.join("agents").join(actor).join("workspace")
            }
            WsKind::Channel => channel_root.join("shared"),
            WsKind::Thread => {
                let tid = self
                    .thread_id
                    .as_deref()
                    .ok_or_else(|| anyhow!("thread workspace requires --in <tid>"))?;
                channel_root.join("threads").join(tid).join("shared")
            }
        };
        Ok(path)
    }
}

fn sanitize_relative(rel: &str) -> Result<PathBuf> {
    let p = PathBuf::from(rel);
    if p.is_absolute() {
        bail!("expected relative path, got absolute: {rel}");
    }
    for comp in p.components() {
        match comp {
            Component::ParentDir => bail!("`..` not allowed in workspace paths"),
            Component::Prefix(_) | Component::RootDir => {
                bail!("absolute components not allowed: {rel}")
            }
            _ => {}
        }
    }
    Ok(p)
}

fn join_safe(base: &Path, rel: &str) -> Result<PathBuf> {
    let safe = sanitize_relative(rel)?;
    let out = base.join(&safe);
    if !out.starts_with(base) {
        bail!("path {rel} escapes workspace");
    }
    Ok(out)
}

pub fn path(ws: WsRef, sub: Option<String>) -> Result<()> {
    let base = ws.resolve()?;
    let target = match sub {
        Some(rel) => join_safe(&base, &rel)?,
        None => base,
    };
    if render::is_json() {
        render::print_json(&serde_json::json!({"path": target.display().to_string()}));
    } else {
        println!("{}", target.display());
    }
    Ok(())
}

pub fn info(ws: WsRef) -> Result<()> {
    let base = ws.resolve()?;
    let exists = base.exists();
    let kind_str = match ws.kind {
        WsKind::Actor => "actor",
        WsKind::Channel => "channel",
        WsKind::Thread => "thread",
    };
    if render::is_json() {
        render::print_json(&serde_json::json!({
            "kind": kind_str,
            "channel_id": ws.channel_id,
            "thread_id": ws.thread_id,
            "actor_id": ws.actor_id,
            "path": base.display().to_string(),
            "exists": exists,
        }));
    } else {
        println!("kind     {kind_str}");
        println!("channel  {}", ws.channel_id);
        if let Some(t) = &ws.thread_id {
            println!("thread   {t}");
        }
        if let Some(a) = &ws.actor_id {
            println!("actor    {a}");
        }
        println!("path     {}", base.display());
        println!("exists   {exists}");
    }
    Ok(())
}

pub fn list(ws: WsRef, sub: Option<String>, recursive: bool) -> Result<()> {
    let base = ws.resolve()?;
    let target = match &sub {
        Some(rel) => join_safe(&base, rel)?,
        None => base.clone(),
    };
    if !target.exists() {
        bail!("not found: {}", target.display());
    }
    let mut entries: Vec<String> = if recursive {
        let mut out = Vec::new();
        walk(&target, &target, &mut out)?;
        out
    } else {
        let mut out = Vec::new();
        for entry in
            std::fs::read_dir(&target).with_context(|| format!("read {}", target.display()))?
        {
            let entry = entry?;
            let mut name = entry.file_name().to_string_lossy().to_string();
            if entry.file_type()?.is_dir() {
                name.push('/');
            }
            out.push(name);
        }
        out
    };
    entries.sort();
    if render::is_json() {
        render::print_json(&serde_json::json!({
            "path": target.display().to_string(),
            "entries": entries,
        }));
    } else {
        for e in entries {
            println!("{e}");
        }
    }
    Ok(())
}

fn walk(root: &Path, base: &Path, out: &mut Vec<String>) -> Result<()> {
    for entry in std::fs::read_dir(root).with_context(|| format!("read {}", root.display()))? {
        let entry = entry?;
        let path = entry.path();
        let ft = entry.file_type()?;
        if ft.is_dir() {
            walk(&path, base, out)?;
        } else if let Ok(rel) = path.strip_prefix(base) {
            out.push(rel.display().to_string());
        }
    }
    Ok(())
}

pub fn read(ws: WsRef, rel: String, max_bytes: u64) -> Result<()> {
    let base = ws.resolve()?;
    let target = join_safe(&base, &rel)?;
    if !target.exists() {
        bail!("not found: {}", target.display());
    }
    let mut f =
        std::fs::File::open(&target).with_context(|| format!("open {}", target.display()))?;
    let mut buf = Vec::with_capacity(max_bytes.min(64 * 1024) as usize);
    let mut limited = (&mut f).take(max_bytes);
    limited.read_to_end(&mut buf)?;
    std::io::stdout().write_all(&buf)?;
    if !buf.ends_with(b"\n") {
        println!();
    }
    Ok(())
}

pub fn write(
    ws: WsRef,
    rel: String,
    body_text: Option<String>,
    body_file: Option<PathBuf>,
    append: bool,
) -> Result<()> {
    let base = ws.resolve()?;
    let target = join_safe(&base, &rel)?;
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
    }
    let body: Vec<u8> = match (body_text, body_file) {
        (Some(_), Some(_)) => bail!("--text and --file are mutually exclusive"),
        (Some(t), None) => t.into_bytes(),
        (None, Some(p)) => std::fs::read(&p).with_context(|| format!("read {}", p.display()))?,
        (None, None) => {
            let mut buf = Vec::new();
            std::io::stdin().read_to_end(&mut buf)?;
            buf
        }
    };
    if append {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&target)
            .with_context(|| format!("open {}", target.display()))?;
        f.write_all(&body)?;
    } else {
        std::fs::write(&target, &body).with_context(|| format!("write {}", target.display()))?;
    }
    if render::is_json() {
        render::print_json(&serde_json::json!({
            "path": target.display().to_string(),
            "bytes": body.len(),
            "append": append,
        }));
    } else {
        println!("{} ({} bytes)", target.display(), body.len());
    }
    Ok(())
}

pub fn rm(ws: WsRef, rel: String, recursive: bool) -> Result<()> {
    let base = ws.resolve()?;
    let target = join_safe(&base, &rel)?;
    if !target.exists() {
        return Ok(());
    }
    if target.is_dir() {
        if !recursive {
            bail!(
                "{} is a directory (pass --recursive to remove)",
                target.display()
            );
        }
        std::fs::remove_dir_all(&target).with_context(|| format!("rm -r {}", target.display()))?;
    } else {
        std::fs::remove_file(&target).with_context(|| format!("rm {}", target.display()))?;
    }
    if render::is_json() {
        render::print_json(&serde_json::json!({"removed": target.display().to_string()}));
    } else {
        println!("removed {}", target.display());
    }
    Ok(())
}
