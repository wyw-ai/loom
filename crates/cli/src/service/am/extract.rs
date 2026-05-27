//! Field extraction from `am listen` event JSON. The legacy Python
//! bridge is gone; these tests encode the observed message shapes it
//! handled so the Rust handler keeps that coverage.

use serde_json::Value;
use std::collections::HashSet;

/// Keys that may carry the user-visible message body. Order does not
/// matter — `find` returns the first scalar hit during a depth-first
/// walk.
pub const TEXT_KEYS: &[&str] = &[
    "text",
    "content",
    "message",
    "msg",
    "msgcontent",
    "messagecontent",
    "markdown",
    "body",
];

pub const CONVERSATION_KEYS: &[&str] = &[
    "conversationid",
    "conversation_id",
    "openconversationid",
    "open_conversation_id",
    "cid",
];

pub const SENDER_KEYS: &[&str] = &[
    "staffid",
    "staff_id",
    "senderstaffid",
    "sender_staff_id",
    "senderid",
    "sender_id",
    "fromstaffid",
    "from_staff_id",
    "accountid",
    "account_id",
];

pub const MESSAGE_ID_KEYS: &[&str] = &[
    "messageid",
    "message_id",
    "msgid",
    "msg_id",
    "eventid",
    "event_id",
];

/// If `value` is a JSON-encoded string starting with `{` or `[`, return
/// the parsed structure. Otherwise return a clone unchanged. Mirrors the
/// Python `maybe_json` helper — DingTalk wraps message content as a
/// JSON-string-inside-JSON often enough that this is load-bearing.
fn maybe_json(value: &Value) -> Value {
    if let Value::String(s) = value {
        let trimmed = s.trim();
        if !trimmed.is_empty() && (trimmed.starts_with('{') || trimmed.starts_with('[')) {
            if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
                return parsed;
            }
        }
    }
    value.clone()
}

fn normalize_key(k: &str) -> String {
    k.replace('-', "_").to_lowercase()
}

fn walk_items<F: FnMut(&str, &Value)>(value: &Value, f: &mut F) {
    let unwrapped = maybe_json(value);
    match &unwrapped {
        Value::Object(map) => {
            for (k, child) in map {
                let nk = normalize_key(k);
                f(&nk, child);
                walk_items(child, f);
            }
        }
        Value::Array(arr) => {
            for child in arr {
                walk_items(child, f);
            }
        }
        _ => {}
    }
}

fn matches_any(key: &str, keys: &HashSet<&str>) -> bool {
    if keys.contains(key) {
        return true;
    }
    let stripped = key.replace('_', "");
    keys.contains(stripped.as_str())
}

/// Find the first scalar value (string or number) whose normalized key
/// matches `keys` or `keys-with-underscores-stripped`. Returns "" when
/// nothing matched. Equivalent to the Python `find_field`.
pub fn find_field(event: &Value, keys: &[&str]) -> String {
    let key_set: HashSet<&str> = keys.iter().copied().collect();
    let mut found = String::new();
    walk_items(event, &mut |key, value| {
        if !found.is_empty() {
            return;
        }
        if !matches_any(key, &key_set) {
            return;
        }
        let v = maybe_json(value);
        match v {
            Value::String(s) => {
                let s = s.trim();
                if !s.is_empty() {
                    found = s.to_string();
                }
            }
            Value::Number(n) => {
                found = n.to_string();
            }
            _ => {}
        }
    });
    found
}

/// Extract user-visible text. Same walk as `find_field` but also
/// recurses into nested object values (DingTalk sometimes wraps the
/// real text under `data.msg.content`-style sub-objects). Falls back to
/// `raw` (trimmed) when nothing matched — matches the Python contract
/// the listener depended on.
pub fn extract_text(event: &Value, raw: &str) -> String {
    let key_set: HashSet<&str> = TEXT_KEYS.iter().copied().collect();
    let mut found = String::new();
    walk_items(event, &mut |key, value| {
        if !found.is_empty() {
            return;
        }
        if !matches_any(key, &key_set) {
            return;
        }
        let v = maybe_json(value);
        match v {
            Value::String(s) => {
                let s = s.trim();
                if !s.is_empty() {
                    found = s.to_string();
                }
            }
            Value::Object(_) => {
                let nested = extract_text(&v, "");
                if !nested.is_empty() {
                    found = nested;
                }
            }
            _ => {}
        }
    });
    if found.is_empty() {
        raw.trim().to_string()
    } else {
        found
    }
}

pub fn conversation(event: &Value) -> String {
    find_field(event, CONVERSATION_KEYS)
}

pub fn sender(event: &Value) -> String {
    find_field(event, SENDER_KEYS)
}

pub fn message_id(event: &Value) -> String {
    find_field(event, MESSAGE_ID_KEYS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_text_finds_top_level_text() {
        let v = json!({ "text": "hello" });
        assert_eq!(extract_text(&v, ""), "hello");
    }

    #[test]
    fn extract_text_falls_back_to_raw_when_nothing_matches() {
        let v = json!({ "irrelevant": 1 });
        assert_eq!(extract_text(&v, "  fallback line  "), "fallback line");
    }

    #[test]
    fn extract_text_drills_into_camelcase_keys() {
        // DingTalk bot envelopes carry the body under `data.msgContent`
        // (camelCase). The normalizer lowercases + strips underscores so
        // both `msgcontent` and `msg_content` patterns match the
        // CONVERSATION-style key set.
        let v = json!({
            "data": {
                "msgContent": "actual question text"
            }
        });
        assert_eq!(extract_text(&v, ""), "actual question text");
    }

    #[test]
    fn extract_text_unwraps_json_string_payloads() {
        // DingTalk sometimes serializes msgContent as a JSON-encoded
        // string ({"text": "..."}) rather than a real object. maybe_json
        // must transparently parse it.
        let v = json!({
            "msgContent": r#"{"text": "stringified body"}"#
        });
        assert_eq!(extract_text(&v, ""), "stringified body");
    }

    #[test]
    fn extract_text_recurses_into_nested_object() {
        let v = json!({
            "body": { "content": "nested body content" }
        });
        assert_eq!(extract_text(&v, ""), "nested body content");
    }

    #[test]
    fn extract_text_takes_first_non_empty_match() {
        // First top-level text key that is non-empty wins. If `text` is
        // empty, we fall through to `content`.
        let v = json!({
            "text": "  ",
            "content": "actual"
        });
        assert_eq!(extract_text(&v, ""), "actual");
    }

    #[test]
    fn find_field_matches_underscore_variants() {
        // SENDER_KEYS lists both `staffid` and `staff_id` — the
        // normalizer + strip lets either input shape match either entry.
        let v1 = json!({ "staffId": "12345" });
        assert_eq!(find_field(&v1, SENDER_KEYS), "12345");
        let v2 = json!({ "sender_staff_id": "67890" });
        assert_eq!(find_field(&v2, SENDER_KEYS), "67890");
        let v3 = json!({ "fromStaffId": "abcd" });
        assert_eq!(find_field(&v3, SENDER_KEYS), "abcd");
    }

    #[test]
    fn find_field_accepts_numeric_values() {
        // accountId frequently arrives as a JSON number, not string.
        let v = json!({ "accountId": 9876 });
        assert_eq!(find_field(&v, SENDER_KEYS), "9876");
    }

    #[test]
    fn find_field_returns_empty_when_value_is_object_only() {
        // A key match whose value is itself an object (no scalar leaf
        // here) does not count — find_field is for scalar lookup.
        let v = json!({ "staffId": { "nested": "x" } });
        assert_eq!(find_field(&v, SENDER_KEYS), "");
    }

    #[test]
    fn conversation_sender_message_id_helpers() {
        let v = json!({
            "openConversationId": "conv_abc",
            "senderStaffId": "user_1",
            "messageId": "msg_42"
        });
        assert_eq!(conversation(&v), "conv_abc");
        assert_eq!(sender(&v), "user_1");
        assert_eq!(message_id(&v), "msg_42");
    }

    #[test]
    fn full_envelope_round_trip() {
        // A more realistic top-level envelope: outer DingTalk metadata
        // plus an inner body. Each helper should pull its field out
        // even though they're scattered across the depth.
        let v = json!({
            "topic": "/v1.0/im/bot/messages/get",
            "data": {
                "conversationId": "conv_x",
                "senderStaffId": "user_y",
                "messageId": "msg_z",
                "msgContent": r#"{"text": "请帮我看一下 CR"}"#
            }
        });
        assert_eq!(extract_text(&v, ""), "请帮我看一下 CR");
        assert_eq!(conversation(&v), "conv_x");
        assert_eq!(sender(&v), "user_y");
        assert_eq!(message_id(&v), "msg_z");
    }
}
