use serde_json::Value;

use crate::adapter::TokenUsage;

pub fn estimate_tokens(text: &str) -> u64 {
    let mut total = 0u64;
    let mut ascii_run = 0u64;

    for ch in text.chars() {
        if ch.is_ascii() && !ch.is_ascii_whitespace() {
            ascii_run += 1;
            continue;
        }

        if ascii_run > 0 {
            total += ascii_run.div_ceil(4);
            ascii_run = 0;
        }

        if !ch.is_whitespace() {
            total += 1;
        }
    }

    if ascii_run > 0 {
        total += ascii_run.div_ceil(4);
    }

    total
}

pub fn estimated_usage(input_tokens: u64, output_text: &str) -> TokenUsage {
    let output_tokens = estimate_tokens(output_text);
    TokenUsage {
        input_tokens: Some(input_tokens),
        output_tokens: Some(output_tokens),
        total_tokens: Some(input_tokens + output_tokens),
        estimated: true,
        ..TokenUsage::default()
    }
}

pub fn add_usage(total: &mut TokenUsage, increment: &TokenUsage) {
    total.input_tokens = add_opt(total.input_tokens, increment.input_tokens);
    total.output_tokens = add_opt(total.output_tokens, increment.output_tokens);
    total.total_tokens = add_opt(total.total_tokens, normalized_total(increment));
    total.cache_creation_input_tokens = add_opt(
        total.cache_creation_input_tokens,
        increment.cache_creation_input_tokens,
    );
    total.cache_read_input_tokens = add_opt(
        total.cache_read_input_tokens,
        increment.cache_read_input_tokens,
    );
    total.reasoning_tokens = add_opt(total.reasoning_tokens, increment.reasoning_tokens);
    total.total_cost_usd = add_opt_f64(total.total_cost_usd, increment.total_cost_usd);
    total.estimated = total.estimated || increment.estimated;
}

pub fn normalized_usage(mut usage: TokenUsage) -> TokenUsage {
    if usage.total_tokens.is_none() {
        usage.total_tokens = normalized_total(&usage);
    }
    usage
}

pub fn normalized_total(usage: &TokenUsage) -> Option<u64> {
    usage.total_tokens.or_else(|| {
        let mut total = 0u64;
        let mut seen = false;
        for value in [
            usage.input_tokens,
            usage.output_tokens,
            usage.cache_creation_input_tokens,
            usage.cache_read_input_tokens,
            usage.reasoning_tokens,
        ] {
            if let Some(value) = value {
                total += value;
                seen = true;
            }
        }
        seen.then_some(total)
    })
}

pub fn extract_token_usage_from_text(text: &str) -> Option<TokenUsage> {
    let mut last = None;
    for line in text.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(usage) = extract_token_usage(&value) {
            last = Some(usage);
        }
    }
    last.map(normalized_usage)
}

pub fn extract_token_usage(value: &Value) -> Option<TokenUsage> {
    for candidate in usage_candidates(value) {
        if let Some(usage) = token_usage_from_object(candidate) {
            return Some(normalized_usage(usage));
        }
    }
    None
}

fn usage_candidates(value: &Value) -> Vec<&Value> {
    let mut out = Vec::new();
    out.push(value);
    for path in [
        "/usage",
        "/tokenUsage",
        "/token_usage",
        "/result/usage",
        "/result/tokenUsage",
        "/result/token_usage",
        "/data/usage",
        "/data/tokenUsage",
        "/data/token_usage",
        "/message/usage",
        "/message/tokenUsage",
        "/message/token_usage",
        "/response/usage",
        "/response/tokenUsage",
        "/response/token_usage",
    ] {
        if let Some(candidate) = value.pointer(path) {
            out.push(candidate);
        }
    }
    out
}

fn token_usage_from_object(value: &Value) -> Option<TokenUsage> {
    value.as_object()?;
    let usage = TokenUsage {
        input_tokens: first_u64(
            value,
            &[
                "input_tokens",
                "inputTokens",
                "prompt_tokens",
                "promptTokens",
                "input",
                "prompt",
            ],
        ),
        output_tokens: first_u64(
            value,
            &[
                "output_tokens",
                "outputTokens",
                "completion_tokens",
                "completionTokens",
                "output",
                "completion",
            ],
        ),
        total_tokens: first_u64(
            value,
            &[
                "total_tokens",
                "totalTokens",
                "tokens",
                "token_count",
                "tokenCount",
            ],
        ),
        cache_creation_input_tokens: first_u64(
            value,
            &[
                "cache_creation_input_tokens",
                "cacheCreationInputTokens",
                "cache_write_input_tokens",
                "cacheWriteInputTokens",
            ],
        ),
        cache_read_input_tokens: first_u64(
            value,
            &[
                "cache_read_input_tokens",
                "cacheReadInputTokens",
                "cached_input_tokens",
                "cachedInputTokens",
            ],
        ),
        reasoning_tokens: first_u64(
            value,
            &[
                "reasoning_tokens",
                "reasoningTokens",
                "reasoning_output_tokens",
                "reasoningOutputTokens",
            ],
        ),
        total_cost_usd: first_f64(
            value,
            &[
                "total_cost_usd",
                "totalCostUsd",
                "cost_usd",
                "costUsd",
                "cost",
            ],
        ),
        estimated: false,
    };

    let has_usage = usage.input_tokens.is_some()
        || usage.output_tokens.is_some()
        || usage.total_tokens.is_some()
        || usage.cache_creation_input_tokens.is_some()
        || usage.cache_read_input_tokens.is_some()
        || usage.reasoning_tokens.is_some()
        || usage.total_cost_usd.is_some();

    if has_usage {
        Some(usage)
    } else {
        None
    }
}

fn first_u64(value: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|key| {
        value.get(*key).and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_f64().filter(|n| *n >= 0.0).map(|n| n as u64))
                .or_else(|| v.as_str()?.parse::<u64>().ok())
        })
    })
}

fn first_f64(value: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|key| {
        value.get(*key).and_then(|v| {
            v.as_f64()
                .or_else(|| v.as_u64().map(|n| n as f64))
                .or_else(|| v.as_str()?.parse::<f64>().ok())
        })
    })
}

fn add_opt(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left + right),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

fn add_opt_f64(left: Option<f64>, right: Option<f64>) -> Option<f64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left + right),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_common_usage_shapes() {
        let usage = extract_token_usage(&json!({
            "type": "result",
            "usage": {
                "input_tokens": 100,
                "outputTokens": 25,
                "cache_read_input_tokens": 10
            }
        }))
        .unwrap();

        assert_eq!(usage.input_tokens, Some(100));
        assert_eq!(usage.output_tokens, Some(25));
        assert_eq!(usage.cache_read_input_tokens, Some(10));
        assert_eq!(usage.total_tokens, Some(135));
    }

    #[test]
    fn estimates_mixed_ascii_and_cjk() {
        assert!(estimate_tokens("hello world 你好") >= 4);
    }
}
