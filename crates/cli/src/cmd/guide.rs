use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use loom_platform::process::Command;
use serde::Serialize;

use crate::{config, render};

const GUIDE_REPO: &str = "git@github.com:wyw-ai/loom-guide.git";

const EMBEDDED_TOPICS: &[EmbeddedTopic] = &[
    EmbeddedTopic {
        id: "runtime-awareness",
        title: "Runtime Awareness",
        content: include_str!("../../assets/loom-guide/guides/runtime-awareness.md"),
    },
    EmbeddedTopic {
        id: "messaging-routing",
        title: "Messaging And Routing",
        content: include_str!("../../assets/loom-guide/guides/messaging-routing.md"),
    },
    EmbeddedTopic {
        id: "tasks-and-coordination",
        title: "Tasks And Coordination",
        content: include_str!("../../assets/loom-guide/guides/tasks-and-coordination.md"),
    },
    EmbeddedTopic {
        id: "provider-integration",
        title: "Provider Integration",
        content: include_str!("../../assets/loom-guide/guides/provider-integration.md"),
    },
];

#[derive(Debug, Clone)]
struct EmbeddedTopic {
    id: &'static str,
    title: &'static str,
    content: &'static str,
}

#[derive(Debug, Clone)]
struct Topic {
    id: String,
    title: String,
    content: String,
    source: String,
}

#[derive(Debug, Serialize)]
struct TopicSummary<'a> {
    id: &'a str,
    title: &'a str,
    source: &'a str,
}

#[derive(Debug, Serialize)]
struct TopicOutput<'a> {
    id: &'a str,
    title: &'a str,
    source: &'a str,
    content: &'a str,
}

#[derive(Debug, Serialize)]
struct SearchHit<'a> {
    id: &'a str,
    title: &'a str,
    source: &'a str,
    matches: Vec<String>,
}

#[derive(Debug, Serialize)]
struct UpdateOutput {
    repo: &'static str,
    path: String,
    command: String,
    success: bool,
    stdout: String,
    stderr: String,
}

pub fn list() -> Result<()> {
    let topics = load_topics()?;
    if render::is_json() {
        let summaries = topics
            .iter()
            .map(|topic| TopicSummary {
                id: &topic.id,
                title: &topic.title,
                source: &topic.source,
            })
            .collect::<Vec<_>>();
        render::print_json(&summaries);
        return Ok(());
    }

    for topic in topics {
        println!("{} - {} ({})", topic.id, topic.title, topic.source);
    }
    Ok(())
}

pub fn show(topic_id: &str) -> Result<()> {
    let topics = load_topics()?;
    let topic = topics
        .iter()
        .find(|topic| topic.id == topic_id)
        .ok_or_else(|| anyhow!("unknown guide topic `{topic_id}`"))?;

    if render::is_json() {
        render::print_json(&TopicOutput {
            id: &topic.id,
            title: &topic.title,
            source: &topic.source,
            content: &topic.content,
        });
    } else {
        println!("{}", topic.content.trim_end());
    }
    Ok(())
}

pub fn search(query: &str) -> Result<()> {
    let query = query.trim();
    if query.is_empty() {
        bail!("guide search query cannot be empty");
    }
    let query_lower = query.to_ascii_lowercase();
    let topics = load_topics()?;
    let hits = topics
        .iter()
        .filter_map(|topic| {
            let mut matches = Vec::new();
            if topic.id.to_ascii_lowercase().contains(&query_lower)
                || topic.title.to_ascii_lowercase().contains(&query_lower)
            {
                matches.push(topic.title.clone());
            }
            matches.extend(
                topic
                    .content
                    .lines()
                    .filter(|line| line.to_ascii_lowercase().contains(&query_lower))
                    .take(5)
                    .map(|line| line.trim().to_string()),
            );
            if matches.is_empty() {
                None
            } else {
                Some(SearchHit {
                    id: &topic.id,
                    title: &topic.title,
                    source: &topic.source,
                    matches,
                })
            }
        })
        .collect::<Vec<_>>();

    if render::is_json() {
        render::print_json(&hits);
        return Ok(());
    }

    for hit in hits {
        println!("{} - {} ({})", hit.id, hit.title, hit.source);
        for line in hit.matches {
            println!("  {}", line);
        }
    }
    Ok(())
}

pub fn update() -> Result<()> {
    let cache = guide_cache_dir();
    let parent = cache
        .parent()
        .ok_or_else(|| anyhow!("invalid guide cache path {}", cache.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;

    let mut command = Command::new("git");
    let command_text;
    if cache.join(".git").is_dir() {
        command.arg("-C").arg(&cache).arg("pull").arg("--ff-only");
        command_text = format!("git -C {} pull --ff-only", cache.display());
    } else if cache.exists() && cache.read_dir()?.next().is_some() {
        bail!(
            "guide cache exists but is not a git repo: {}",
            cache.display()
        );
    } else {
        command.arg("clone").arg(GUIDE_REPO).arg(&cache);
        command_text = format!("git clone {GUIDE_REPO} {}", cache.display());
    }

    let output = command
        .output()
        .with_context(|| format!("run `{command_text}`"))?;
    let result = UpdateOutput {
        repo: GUIDE_REPO,
        path: cache.display().to_string(),
        command: command_text,
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    };

    if render::is_json() {
        render::print_json(&result);
    } else {
        println!(
            "{} guide cache at {}",
            if result.success {
                "updated"
            } else {
                "failed to update"
            },
            result.path
        );
        if !result.stdout.trim().is_empty() {
            println!("{}", result.stdout.trim_end());
        }
        if !result.stderr.trim().is_empty() {
            eprintln!("{}", result.stderr.trim_end());
        }
    }

    if result.success {
        Ok(())
    } else {
        bail!("guide update failed")
    }
}

fn load_topics() -> Result<Vec<Topic>> {
    let cached = load_cached_topics(&guide_cache_dir())?;
    if !cached.is_empty() {
        return Ok(cached);
    }
    Ok(embedded_topics())
}

fn embedded_topics() -> Vec<Topic> {
    EMBEDDED_TOPICS
        .iter()
        .map(|topic| Topic {
            id: topic.id.to_string(),
            title: topic.title.to_string(),
            content: topic.content.to_string(),
            source: "embedded".into(),
        })
        .collect()
}

fn load_cached_topics(cache: &Path) -> Result<Vec<Topic>> {
    let guides = cache.join("guides");
    let entries = match fs::read_dir(&guides) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err).with_context(|| format!("read {}", guides.display())),
    };

    let mut topics = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let content =
            fs::read_to_string(&path).with_context(|| format!("read guide {}", path.display()))?;
        topics.push(Topic {
            id: id.to_string(),
            title: markdown_title(&content).unwrap_or_else(|| title_from_id(id)),
            content,
            source: "cache".into(),
        });
    }
    topics.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(topics)
}

fn markdown_title(content: &str) -> Option<String> {
    content
        .lines()
        .find_map(|line| line.strip_prefix("# ").map(str::trim))
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn title_from_id(id: &str) -> String {
    id.split('-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => {
                    let mut out = first.to_uppercase().collect::<String>();
                    out.push_str(chars.as_str());
                    out
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn guide_cache_dir() -> PathBuf {
    config::config_dir().join("guides").join("loom-guide")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_topics_include_runtime_awareness() {
        let topics = embedded_topics();
        assert!(topics.iter().any(|topic| topic.id == "runtime-awareness"));
        assert!(topics
            .iter()
            .any(|topic| topic.content.contains("AGENTS.md")));
    }

    #[test]
    fn markdown_title_reads_h1() {
        assert_eq!(markdown_title("# Hello\n\nbody").as_deref(), Some("Hello"));
    }
}
