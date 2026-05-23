// First user is `loom service am-handler` reply path; allow until S2-5.
#![allow(dead_code)]

//! Markdown → plain-text flattening for `am` reply sends. Mirrors the
//! Python `plain_am_text` reference. Hand-rolled string ops (no `regex`
//! crate) — patterns are simple prefix-strips + a single character-class
//! pass, so a regex dep would be overkill.
//!
//! Pipeline (per line, then join):
//!
//! 1. `^#{1,6}\s+` → drop heading marker
//! 2. `^[-*+]\s+` → drop bullet marker
//! 3. `^\d+[.)]\s+` → drop ordered-list marker
//! 4. Strip `**`, `__`, backticks (no-context inline marks)
//! 5. Join non-empty lines with `；` (full-width semicolon)
//! 6. Collapse runs of whitespace to a single space
//! 7. Drop `；+` immediately after `:：,，;；.。!！?？`
//! 8. Cap at `max_chars` (when > 1) and append `…`

pub fn plain_am_text(input: &str, max_chars: usize) -> String {
    let mut lines: Vec<String> = Vec::new();
    for raw_line in input.trim().lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        let line = strip_md_heading(line);
        let line = strip_bullet(line);
        let line = strip_ordered_list(line);
        let line = line.replace("**", "").replace("__", "").replace('`', "");
        lines.push(line);
    }
    let joined = lines.join("；");
    let collapsed = collapse_whitespace(&joined);
    let mut flattened = drop_extra_separator_after_punct(&collapsed);
    if max_chars > 1 {
        let chars: Vec<char> = flattened.chars().collect();
        if chars.len() > max_chars {
            let kept: String = chars[..max_chars.saturating_sub(1)].iter().collect();
            let kept = kept.trim_end().to_string();
            flattened = format!("{kept}…");
        }
    }
    if flattened.is_empty() {
        return input.trim().to_string();
    }
    flattened
}

fn strip_md_heading(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut hashes = 0;
    while hashes < bytes.len() && hashes < 6 && bytes[hashes] == b'#' {
        hashes += 1;
    }
    if hashes == 0 {
        return line;
    }
    if hashes >= bytes.len() || !bytes[hashes].is_ascii_whitespace() {
        return line;
    }
    let mut j = hashes;
    while j < bytes.len() && bytes[j].is_ascii_whitespace() {
        j += 1;
    }
    &line[j..]
}

fn strip_bullet(line: &str) -> &str {
    let mut iter = line.char_indices();
    let Some((_, first)) = iter.next() else {
        return line;
    };
    if !matches!(first, '-' | '*' | '+') {
        return line;
    }
    let after_marker_idx = match iter.next() {
        Some((idx, _)) => idx,
        None => return line,
    };
    let rest = &line[after_marker_idx..];
    let trimmed = rest.trim_start();
    if trimmed.len() == rest.len() {
        // No whitespace after the marker — not a real list item, leave alone.
        return line;
    }
    trimmed
}

fn strip_ordered_list(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 || i >= bytes.len() {
        return line;
    }
    if bytes[i] != b'.' && bytes[i] != b')' {
        return line;
    }
    let after_punct = i + 1;
    if after_punct >= bytes.len() {
        return line;
    }
    let rest = &line[after_punct..];
    let trimmed = rest.trim_start();
    if trimmed.len() == rest.len() {
        return line;
    }
    trimmed
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

const PUNCT_THAT_OWNS_NEXT_SEP: &[char] = &[
    ':', '：', ',', '，', ';', '；', '.', '。', '!', '！', '?', '？',
];

/// After a sentence-ending punctuation, an immediately following run of
/// `；` (the line-join separator from step 5) reads as a typo to humans
/// — drop it. Mirrors Python's `r"([...])；+" → \1`.
fn drop_extra_separator_after_punct(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if PUNCT_THAT_OWNS_NEXT_SEP.contains(&c) {
            while matches!(chars.peek(), Some('；')) {
                chars.next();
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_returns_empty() {
        assert_eq!(plain_am_text("", 1800), "");
        assert_eq!(plain_am_text("   ", 1800), "");
    }

    #[test]
    fn strips_heading_marker() {
        assert_eq!(plain_am_text("# h1", 1800), "h1");
        assert_eq!(plain_am_text("## Title", 1800), "Title");
        assert_eq!(plain_am_text("###### deepest", 1800), "deepest");
    }

    #[test]
    fn leaves_seven_or_more_hashes_alone() {
        // Per CommonMark + the Python regex (#{1,6}), 7+ hashes is not
        // a heading marker.
        let out = plain_am_text("####### too many", 1800);
        assert_eq!(out, "####### too many");
    }

    #[test]
    fn strips_bullet_marker() {
        assert_eq!(plain_am_text("- a", 1800), "a");
        assert_eq!(plain_am_text("* b", 1800), "b");
        assert_eq!(plain_am_text("+ c", 1800), "c");
    }

    #[test]
    fn leaves_bullet_marker_without_following_space() {
        // `*important*` is not a bullet — has no whitespace after the `*`.
        // The inline-mark stripper later removes the `*` pair on its own.
        let out = plain_am_text("*important*", 1800);
        // strip_bullet bails (no ws after *); inline ** strip is no-op
        // (we only strip `**`, not single `*`). End result keeps the *.
        assert_eq!(out, "*important*");
    }

    #[test]
    fn strips_ordered_list_marker() {
        assert_eq!(plain_am_text("1. first", 1800), "first");
        assert_eq!(plain_am_text("12. dozen", 1800), "dozen");
        assert_eq!(plain_am_text("3) paren", 1800), "paren");
    }

    #[test]
    fn strips_inline_marks() {
        assert_eq!(
            plain_am_text("**bold** plus `code` plus __italic__", 1800),
            "bold plus code plus italic"
        );
    }

    #[test]
    fn joins_multi_line_with_full_width_semicolon() {
        assert_eq!(
            plain_am_text("first\nsecond\nthird", 1800),
            "first；second；third"
        );
    }

    #[test]
    fn skips_blank_lines_when_joining() {
        // Empty/whitespace-only lines must not produce empty join slots
        // (would emit `；；`). Matches Python's `if not line: continue`.
        assert_eq!(plain_am_text("a\n\n   \nb", 1800), "a；b");
    }

    #[test]
    fn collapses_internal_whitespace() {
        assert_eq!(plain_am_text("hello    world", 1800), "hello world");
        assert_eq!(plain_am_text("a\tb\nc", 1800), "a b；c");
    }

    #[test]
    fn drops_separator_after_punct() {
        // After "：" the join introduced "；" which reads as a typo.
        assert_eq!(plain_am_text("结论：\n详情", 1800), "结论：详情");
        assert_eq!(plain_am_text("done.\nnext", 1800), "done.next");
    }

    #[test]
    fn truncates_to_exact_char_count_with_ellipsis() {
        let long = "x".repeat(2000);
        let out = plain_am_text(&long, 100);
        assert_eq!(out.chars().count(), 100);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn truncation_disabled_when_max_chars_le_1() {
        // Python's `if max_chars > 1` — 0 / 1 disables truncation.
        let long = "x".repeat(50);
        assert_eq!(plain_am_text(&long, 1), long);
        assert_eq!(plain_am_text(&long, 0), long);
    }

    #[test]
    fn realistic_markdown_reply() {
        let input = "## 总结\n\n- 第一点：完成\n- 第二点：等待评审\n\n**下一步**：等用户回复";
        let out = plain_am_text(input, 1800);
        // Heading stripped; bullets stripped; full-width ;; the bold
        // inline marker stripped; "：" + "；" collapsed where adjacent.
        assert_eq!(
            out,
            "总结；第一点：完成；第二点：等待评审；下一步：等用户回复"
        );
    }
}
