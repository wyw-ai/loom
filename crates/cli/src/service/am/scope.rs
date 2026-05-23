// First user is `loom service am-handler` (S2-5).
#![allow(dead_code)]

//! Thread-map persistence + scope-mode resolution.
//!
//! Per-channel mapping `{ channelId: { thread_key: ThreadEntry } }`, mirrors
//! the JSON shape the Python reference wrote. Canonical path is
//! `<state_dir>/thread-map.json` (§10).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::extract;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeMode {
    /// All messages write to the channel's common area.
    Channel,
    /// All messages write to a single fixed thread (`AmConfig.thread_id`).
    Thread,
    /// One thread per `thread_key(event)`. Default.
    AutoThread,
}

impl Default for ScopeMode {
    fn default() -> Self {
        Self::AutoThread
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadEntry {
    pub thread_id: String,
    #[serde(default)]
    pub title: String,
    /// Unix epoch seconds when the entry was first created. Stored for
    /// future cleanup heuristics; not load-bearing for routing.
    #[serde(default)]
    pub created_at: i64,
}

pub type ChannelMap = BTreeMap<String, ThreadEntry>;

/// On-disk shape: `{ channelId: ChannelMap }`. `transparent` so the
/// JSON has no extra `channels` envelope — matches the Python file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ThreadMap {
    pub channels: BTreeMap<String, ChannelMap>,
}

/// Routing key for `auto_thread` mode. Prefers conversation id over
/// sender; falls back to literal `"default"` when neither is present
/// (so untagged messages share one bucket instead of becoming a thread
/// per parse-failure).
pub fn thread_key(event: &Value) -> String {
    let conv = extract::conversation(event);
    if !conv.is_empty() {
        return format!("conversation:{conv}");
    }
    let sender = extract::sender(event);
    if !sender.is_empty() {
        return format!("sender:{sender}");
    }
    "default".to_string()
}

/// Display title for a freshly-created auto-thread. Same format as
/// the Python script so post-migration threads are visually
/// indistinguishable from pre-migration ones.
pub fn thread_title(event: &Value, key: &str) -> String {
    let sender = extract::sender(event);
    let conversation = extract::conversation(event);
    let title = if !conversation.is_empty() {
        let label = if !sender.is_empty() {
            sender
        } else {
            conversation
        };
        format!("钉钉答疑 · {label}")
    } else if !sender.is_empty() {
        format!("钉钉答疑 · {sender}")
    } else {
        format!("钉钉答疑 · {key}")
    };
    let collapsed = collapse_whitespace(&title);
    truncate_chars(&collapsed, 64)
}

fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_was_space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !last_was_space {
                out.push(' ');
                last_was_space = true;
            }
        } else {
            out.push(c);
            last_was_space = false;
        }
    }
    out.trim().to_string()
}

fn truncate_chars(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else {
        chars[..max].iter().collect()
    }
}

pub fn load(path: &Path) -> Result<ThreadMap> {
    if !path.exists() {
        return Ok(ThreadMap::default());
    }
    let raw =
        fs::read_to_string(path).with_context(|| format!("read thread map {}", path.display()))?;
    if raw.trim().is_empty() {
        return Ok(ThreadMap::default());
    }
    serde_json::from_str(&raw).with_context(|| format!("parse thread map {}", path.display()))
}

/// Atomic write: `<path>.tmp` then rename. Crash mid-write leaves the
/// previous map intact. Pretty-prints for human inspection (the file is
/// expected to stay small — one entry per conversation).
pub fn save(path: &Path, map: &ThreadMap) -> Result<()> {
    let parent = path.parent().expect("thread map path always has a parent");
    fs::create_dir_all(parent)?;
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_string_pretty(map).context("serialize thread map")?;
    fs::write(&tmp, body).with_context(|| format!("write tmp thread map {}", tmp.display()))?;
    fs::rename(&tmp, path)
        .with_context(|| format!("rename {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}

pub fn thread_map_path(state_dir: &Path) -> PathBuf {
    state_dir.join("thread-map.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_dir() -> PathBuf {
        let p =
            std::env::temp_dir().join(format!("loom-am-scope-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn thread_key_prefers_conversation_then_sender_then_default() {
        assert_eq!(
            thread_key(&json!({"conversationId": "c1", "senderStaffId": "s1"})),
            "conversation:c1"
        );
        assert_eq!(thread_key(&json!({"staffId": "u1"})), "sender:u1");
        assert_eq!(thread_key(&json!({})), "default");
    }

    #[test]
    fn thread_title_uses_sender_when_conversation_present() {
        let v = json!({"conversationId": "conv_x", "senderStaffId": "user_a"});
        assert_eq!(thread_title(&v, "key"), "钉钉答疑 · user_a");
    }

    #[test]
    fn thread_title_falls_back_to_conversation() {
        let v = json!({"conversationId": "conv_x"});
        assert_eq!(thread_title(&v, "key"), "钉钉答疑 · conv_x");
    }

    #[test]
    fn thread_title_falls_back_to_sender_only() {
        let v = json!({"staffId": "user_b"});
        assert_eq!(thread_title(&v, "key"), "钉钉答疑 · user_b");
    }

    #[test]
    fn thread_title_falls_back_to_key() {
        assert_eq!(thread_title(&json!({}), "default"), "钉钉答疑 · default");
    }

    #[test]
    fn thread_title_truncates_to_64_chars() {
        let long: String = std::iter::repeat('啊').take(200).collect();
        let v = json!({ "staffId": long });
        let title = thread_title(&v, "k");
        assert_eq!(title.chars().count(), 64);
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = temp_dir();
        let path = thread_map_path(&dir);
        let mut map = ThreadMap::default();
        let mut chan = ChannelMap::new();
        chan.insert(
            "conversation:c1".into(),
            ThreadEntry {
                thread_id: "t1".into(),
                title: "title1".into(),
                created_at: 1234,
            },
        );
        map.channels.insert("ch1".into(), chan);
        save(&path, &map).expect("save");

        let loaded = load(&path).expect("load");
        let entry = loaded
            .channels
            .get("ch1")
            .and_then(|c| c.get("conversation:c1"))
            .expect("entry survives round-trip");
        assert_eq!(entry.thread_id, "t1");
        assert_eq!(entry.title, "title1");
        assert_eq!(entry.created_at, 1234);
    }

    #[test]
    fn load_returns_default_for_missing_or_empty_file() {
        let dir = temp_dir();
        let missing = dir.join("nope.json");
        let m = load(&missing).expect("missing -> default");
        assert!(m.channels.is_empty());

        let empty = dir.join("empty.json");
        fs::write(&empty, "").unwrap();
        let m2 = load(&empty).expect("empty -> default");
        assert!(m2.channels.is_empty());
    }

    #[test]
    fn save_atomic_leaves_no_tmp() {
        let dir = temp_dir();
        let path = dir.join("thread-map.json");
        save(&path, &ThreadMap::default()).expect("save");
        let tmp = path.with_extension("json.tmp");
        assert!(!tmp.exists());
    }
}
