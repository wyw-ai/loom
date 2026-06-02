use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use chrono::Utc;
use uuid::Uuid;

use agent_runtime::memory::{
    JsonlMemoryStore, MemoryQuery, MemoryRecord, MemorySource, MemoryStore,
};

use crate::render;

pub fn query(
    actor_id: String,
    profile_dir: Option<PathBuf>,
    channel: Option<String>,
    text: Option<String>,
    limit: usize,
    include_non_accepted: bool,
    json: bool,
) -> Result<()> {
    let store = store_for(&actor_id, profile_dir)?;
    let records = store
        .query(&MemoryQuery {
            text,
            tags: Vec::new(),
            types: Vec::new(),
            limit,
            channel_scope: channel,
            include_non_accepted,
        })
        .map_err(anyhow::Error::msg)?;
    if json {
        render::print_json(&records);
    } else {
        for record in records {
            println!(
                "{} [{}] {} ({})",
                record.id, record.status, record.summary, record.source.channel_id
            );
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn append(
    actor_id: String,
    profile_dir: Option<PathBuf>,
    summary: String,
    source_channel: String,
    source_message: Option<String>,
    status: String,
    record_type: String,
    confidence: String,
    detail: Option<String>,
    tags: Vec<String>,
    json: bool,
) -> Result<()> {
    validate_status(&status)?;
    if status == "accepted" && source_message.as_deref().unwrap_or_default().is_empty() {
        bail!("accepted memory requires --source-message; append defaults to pending");
    }
    let store = store_for(&actor_id, profile_dir)?;
    let record = MemoryRecord {
        schema_version: 1,
        id: format!("mem_{}", Uuid::new_v4().simple()),
        actor_id,
        ts: Utc::now().to_rfc3339(),
        record_type,
        status,
        summary,
        detail: detail.unwrap_or_default(),
        confidence,
        source: MemorySource {
            channel_id: source_channel,
            thread_id: String::new(),
            message_ids: source_message.into_iter().collect(),
        },
        tags,
    };
    store.append(&record).map_err(anyhow::Error::msg)?;
    if json {
        render::print_json(&record);
    } else {
        println!("memory {} [{}]", record.id, record.status);
    }
    Ok(())
}

pub fn get(
    actor_id: String,
    profile_dir: Option<PathBuf>,
    memory_id: String,
    json: bool,
) -> Result<()> {
    let store = store_for(&actor_id, profile_dir)?;
    let record = store
        .get(&memory_id)
        .map_err(anyhow::Error::msg)?
        .with_context(|| format!("memory {memory_id} not found"))?;
    if json {
        render::print_json(&record);
    } else {
        println!("{} [{}] {}", record.id, record.status, record.summary);
        if !record.detail.trim().is_empty() {
            println!("{}", record.detail);
        }
    }
    Ok(())
}

pub fn update(
    actor_id: String,
    profile_dir: Option<PathBuf>,
    memory_id: String,
    status: String,
    reason: Option<String>,
    source_message: Option<String>,
    json: bool,
) -> Result<()> {
    validate_status(&status)?;
    let store = store_for(&actor_id, profile_dir)?;
    let mut record = store
        .get(&memory_id)
        .map_err(anyhow::Error::msg)?
        .with_context(|| format!("memory {memory_id} not found"))?;
    if status == "accepted"
        && (record.source.channel_id.trim().is_empty()
            || (record.source.message_ids.is_empty()
                && source_message.as_deref().unwrap_or_default().is_empty()))
    {
        bail!("accepted memory requires source channel and source refs");
    }
    record.status = status;
    record.ts = Utc::now().to_rfc3339();
    if let Some(message_id) = source_message {
        if !record.source.message_ids.iter().any(|id| id == &message_id) {
            record.source.message_ids.push(message_id);
        }
    }
    if let Some(reason) = reason {
        if !reason.trim().is_empty() {
            if !record.detail.trim().is_empty() {
                record.detail.push_str("\n\n");
            }
            record.detail.push_str("status update: ");
            record.detail.push_str(&reason);
        }
    }
    store.append(&record).map_err(anyhow::Error::msg)?;
    if json {
        render::print_json(&record);
    } else {
        println!("memory {} [{}]", record.id, record.status);
    }
    Ok(())
}

fn store_for(actor_id: &str, profile_dir: Option<PathBuf>) -> Result<JsonlMemoryStore> {
    let profile_dir = profile_dir.unwrap_or_else(|| resolve_profile_dir(actor_id));
    Ok(JsonlMemoryStore::new(
        profile_dir.join("memory").join("records"),
    ))
}

fn resolve_profile_dir(actor_id: &str) -> PathBuf {
    if let Ok(path) = std::env::var("LOOM_MEMORY_PROFILE_DIR") {
        return PathBuf::from(path);
    }
    let mut candidates = vec![actor_id.to_string()];
    if let Some(stripped) = actor_id.strip_prefix("actor_") {
        candidates.push(stripped.to_string());
    }
    if let Some(stripped) = actor_id.strip_prefix("actor_agent_") {
        candidates.push(stripped.to_string());
    }
    for candidate in candidates {
        let path = PathBuf::from("data")
            .join("agents")
            .join(candidate)
            .join("profile");
        if path.exists() {
            return path;
        }
    }
    PathBuf::from("data")
        .join("agents")
        .join(actor_id)
        .join("profile")
}

fn validate_status(status: &str) -> Result<()> {
    match status {
        "pending" | "accepted" | "rejected" | "archived" => Ok(()),
        other => bail!("unknown memory status `{other}`"),
    }
}
