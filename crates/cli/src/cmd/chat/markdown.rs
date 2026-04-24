//! Markdown → ratatui spans for chat bubble bodies.
//!
//! Chat messages are typically short and arrive incrementally during streaming,
//! so we keep the renderer cheap: parse with `pulldown-cmark`, walk the event
//! stream, accumulate styled spans into rows, and emit one row per visual line.
//!
//! The renderer is line-based — callers wrap the returned rows themselves
//! (matching the existing `Paragraph::wrap` machinery in the chat history pane).
//! Trailing blank rows are stripped so a message ending in `\n\n` does not
//! inflate the bubble's apparent height.

use pulldown_cmark::{Alignment, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use unicode_width::UnicodeWidthStr;

/// Render the markdown `text` into rows of styled spans suitable for assembling
/// `Line`s. The base color is applied to plain text; markdown styling layers
/// on top (bold/italic/underline/code-bg).
pub fn render_to_rows(text: &str, base: Style) -> Vec<Vec<Span<'static>>> {
    let mut renderer = Renderer::new(base);
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);
    let parser = Parser::new_ext(text, opts);
    for ev in parser {
        renderer.handle(ev);
    }
    renderer.finish()
}

struct Renderer {
    base: Style,
    rows: Vec<Vec<Span<'static>>>,
    current: Vec<Span<'static>>,
    style_stack: Vec<Style>,
    /// Top of the stack as the next text-write style. Cached so we don't
    /// allocate for every Text event.
    active: Style,
    /// `Some(prefix)` means the next non-blank row should be prefixed with
    /// this string (list bullet, blockquote `>`, etc.). Cleared once consumed.
    pending_prefix: Option<String>,
    /// Stack of list contexts: `Some(start)` = ordered list with running counter,
    /// `None` = bullet list. Used to emit the right prefix per `<li>`.
    list_stack: Vec<Option<u64>>,
    /// Open link destinations awaiting their `End(Link)` to decide whether to
    /// append the URL as a dim trailing fragment.
    link_url_stack: Vec<String>,
    /// True while we're inside a fenced/indented code block. Code-block lines
    /// use a single dim-bg span per row instead of per-token styling.
    in_code_block: bool,
    /// True after we've emitted at least one row of content; controls whether
    /// a paragraph break should insert a blank separator row.
    seen_content: bool,
    /// Buffered table being assembled. Cells are captured into
    /// `current_cell` rather than `current` while this is `Some`, so the
    /// emit-on-`End(Table)` path can size columns and draw box borders
    /// instead of letting raw cell text leak into the surrounding flow.
    table: Option<TableState>,
}

#[derive(Default)]
struct TableState {
    alignments: Vec<Alignment>,
    headers: Vec<Vec<Span<'static>>>,
    rows: Vec<Vec<Vec<Span<'static>>>>,
    in_header: bool,
    current_row: Vec<Vec<Span<'static>>>,
    current_cell: Vec<Span<'static>>,
}

impl Renderer {
    fn new(base: Style) -> Self {
        Self {
            base,
            rows: Vec::new(),
            current: Vec::new(),
            style_stack: Vec::new(),
            active: base,
            pending_prefix: None,
            list_stack: Vec::new(),
            link_url_stack: Vec::new(),
            in_code_block: false,
            seen_content: false,
            table: None,
        }
    }

    fn handle(&mut self, ev: Event<'_>) {
        match ev {
            Event::Start(tag) => self.start(tag),
            Event::End(end) => self.end(end),
            Event::Text(t) => self.write_text(&t),
            Event::Code(c) => {
                let style = self.active.bg(Color::DarkGray).fg(Color::White);
                self.push_span(c.into_string(), style);
            }
            // Chat messages aren't prose — a literal `\n` is meant as a
            // visible line break, not a typographic soft break that should
            // reflow into a space. Treat both break kinds the same.
            // Inside a table cell, collapse to a single space so the row
            // stays one visual line.
            Event::SoftBreak | Event::HardBreak => {
                if self.table.is_some() {
                    self.push_span(" ".to_string(), self.active);
                } else {
                    self.flush_row();
                }
            }
            Event::Rule => {
                self.flush_row();
                self.rows.push(vec![Span::styled(
                    "─".repeat(40),
                    Style::default().fg(Color::DarkGray),
                )]);
                self.seen_content = true;
            }
            Event::Html(h) | Event::InlineHtml(h) => {
                // We don't attempt to render HTML — emit the source verbatim so
                // it isn't silently lost.
                self.write_text(&h);
            }
            Event::FootnoteReference(_) | Event::TaskListMarker(_) => {}
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {
                self.maybe_block_break();
            }
            Tag::Heading { level, .. } => {
                self.maybe_block_break();
                let weight = match level {
                    HeadingLevel::H1 | HeadingLevel::H2 => Modifier::BOLD,
                    _ => Modifier::BOLD,
                };
                self.push_style(self.active.fg(Color::White).add_modifier(weight));
            }
            Tag::BlockQuote(_) => {
                self.maybe_block_break();
                self.pending_prefix = Some("│ ".into());
                self.push_style(self.active.fg(Color::DarkGray));
            }
            Tag::CodeBlock(_) => {
                self.maybe_block_break();
                self.in_code_block = true;
            }
            Tag::List(start) => {
                self.maybe_block_break();
                self.list_stack.push(start);
            }
            Tag::Item => {
                self.flush_row();
                let prefix = match self.list_stack.last_mut() {
                    Some(Some(n)) => {
                        let s = format!("{}. ", n);
                        *n += 1;
                        s
                    }
                    Some(None) => "• ".into(),
                    None => "• ".into(),
                };
                self.pending_prefix = Some(prefix);
            }
            Tag::Emphasis => {
                self.push_style(self.active.add_modifier(Modifier::ITALIC));
            }
            Tag::Strong => {
                self.push_style(self.active.add_modifier(Modifier::BOLD));
            }
            Tag::Strikethrough => {
                self.push_style(self.active.add_modifier(Modifier::CROSSED_OUT));
            }
            Tag::Link { dest_url, .. } => {
                self.push_style(
                    self.active
                        .fg(Color::Blue)
                        .add_modifier(Modifier::UNDERLINED),
                );
                self.link_url_stack.push(dest_url.into_string());
            }
            Tag::Image { dest_url, .. } => {
                self.write_text(&format!("[image: {}]", dest_url));
            }
            Tag::Table(alignments) => {
                self.maybe_block_break();
                self.table = Some(TableState {
                    alignments,
                    ..TableState::default()
                });
            }
            Tag::TableHead => {
                if let Some(t) = self.table.as_mut() {
                    t.in_header = true;
                    t.current_row.clear();
                }
            }
            Tag::TableRow => {
                if let Some(t) = self.table.as_mut() {
                    t.in_header = false;
                    t.current_row.clear();
                }
            }
            Tag::TableCell => {
                if let Some(t) = self.table.as_mut() {
                    t.current_cell.clear();
                }
            }
            _ => {}
        }
    }

    fn end(&mut self, end: TagEnd) {
        match end {
            TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::BlockQuote(_) | TagEnd::Item => {
                self.flush_row();
            }
            TagEnd::CodeBlock => {
                self.in_code_block = false;
                self.flush_row();
            }
            TagEnd::List(_) => {
                self.list_stack.pop();
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                self.pop_style();
            }
            TagEnd::Link => {
                if let Some(url) = self.link_url_stack.pop() {
                    // Only append the URL if it's distinct from the link text;
                    // bare-URL links like `<https://x>` already show the URL.
                    let prev = self
                        .current
                        .last()
                        .map(|s| s.content.as_ref())
                        .unwrap_or("");
                    if prev != url {
                        let style = Style::default().fg(Color::DarkGray);
                        self.push_span(format!(" ({})", url), style);
                    }
                }
                self.pop_style();
            }
            TagEnd::TableCell => {
                if let Some(t) = self.table.as_mut() {
                    let cell = std::mem::take(&mut t.current_cell);
                    t.current_row.push(cell);
                }
            }
            TagEnd::TableHead => {
                if let Some(t) = self.table.as_mut() {
                    t.headers = std::mem::take(&mut t.current_row);
                    t.in_header = false;
                }
            }
            TagEnd::TableRow => {
                if let Some(t) = self.table.as_mut() {
                    let row = std::mem::take(&mut t.current_row);
                    t.rows.push(row);
                }
            }
            TagEnd::Table => {
                if let Some(t) = self.table.take() {
                    self.emit_table(t);
                }
            }
            _ => {}
        }
    }

    fn write_text(&mut self, t: &str) {
        if self.in_code_block {
            for (idx, line) in t.split('\n').enumerate() {
                if idx > 0 {
                    self.flush_row();
                }
                if !line.is_empty() {
                    let style = Style::default().bg(Color::DarkGray).fg(Color::White);
                    self.push_span(line.to_string(), style);
                }
            }
            return;
        }
        // Markdown text events arrive as logical chunks; soft/hard breaks come
        // separately, so we don't need to split on `\n` here.
        if !t.is_empty() {
            let style = self.active;
            self.push_span(t.to_string(), style);
        }
    }

    fn push_span(&mut self, text: String, style: Style) {
        if let Some(t) = self.table.as_mut() {
            t.current_cell.push(Span::styled(text, style));
            return;
        }
        self.consume_pending_prefix();
        self.current.push(Span::styled(text, style));
    }

    fn consume_pending_prefix(&mut self) {
        if self.current.is_empty() {
            if let Some(p) = self.pending_prefix.take() {
                self.current
                    .push(Span::styled(p, Style::default().fg(Color::DarkGray)));
            }
        }
    }

    fn flush_row(&mut self) {
        if self.current.is_empty() && self.pending_prefix.is_none() {
            return;
        }
        self.consume_pending_prefix();
        if self.current.is_empty() {
            return;
        }
        let row = std::mem::take(&mut self.current);
        self.rows.push(row);
        self.seen_content = true;
    }

    fn maybe_block_break(&mut self) {
        // Insert a blank separator row between successive blocks. Skip when no
        // content exists yet (avoids a leading blank line) or when the last
        // row is already blank.
        self.flush_row();
        if self.seen_content && !matches!(self.rows.last(), Some(r) if r.is_empty()) {
            self.rows.push(Vec::new());
        }
    }

    fn push_style(&mut self, s: Style) {
        self.style_stack.push(self.active);
        self.active = s;
    }

    fn pop_style(&mut self) {
        if let Some(prev) = self.style_stack.pop() {
            self.active = prev;
        } else {
            self.active = self.base;
        }
    }

    /// Emit a buffered markdown table as a sequence of rows with box-drawing
    /// borders. Columns are sized to the widest cell in the column (header
    /// included) and aligned per the markdown header divider (`:--`, `:-:`,
    /// `--:`). Cells are styled spans, so any inline bold/italic/code inside
    /// a cell survives.
    fn emit_table(&mut self, t: TableState) {
        let cols = t
            .headers
            .len()
            .max(t.rows.iter().map(|r| r.len()).max().unwrap_or(0));
        if cols == 0 {
            return;
        }
        let widths: Vec<usize> = (0..cols)
            .map(|c| {
                let mut w = t.headers.get(c).map(|cell| cell_width(cell)).unwrap_or(0);
                for r in &t.rows {
                    if let Some(cell) = r.get(c) {
                        w = w.max(cell_width(cell));
                    }
                }
                w.max(1)
            })
            .collect();
        let alignments: Vec<Alignment> = (0..cols)
            .map(|c| t.alignments.get(c).copied().unwrap_or(Alignment::None))
            .collect();
        let border_style = Style::default().fg(Color::DarkGray);

        if !t.headers.is_empty() {
            let row = render_table_row(&t.headers, &widths, &alignments, border_style);
            self.rows.push(row);
            self.rows
                .push(render_table_separator(&widths, &alignments, border_style));
        }
        for r in &t.rows {
            let row = render_table_row(r, &widths, &alignments, border_style);
            self.rows.push(row);
        }
        self.seen_content = true;
    }

    fn finish(mut self) -> Vec<Vec<Span<'static>>> {
        self.flush_row();
        // Strip trailing blank rows — `\n\n` at the end of a streamed bubble
        // would otherwise inflate its visible height.
        while matches!(self.rows.last(), Some(r) if r.is_empty()) {
            self.rows.pop();
        }
        if self.rows.is_empty() {
            self.rows.push(Vec::new());
        }
        self.rows
    }
}

fn cell_width(cell: &[Span<'_>]) -> usize {
    cell.iter()
        .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
        .sum()
}

fn render_table_row(
    cells: &[Vec<Span<'static>>],
    widths: &[usize],
    alignments: &[Alignment],
    border_style: Style,
) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    for (i, &w) in widths.iter().enumerate() {
        spans.push(Span::styled("│ ", border_style));
        let empty: Vec<Span<'static>> = Vec::new();
        let cell = cells.get(i).unwrap_or(&empty);
        let cw = cell_width(cell);
        let pad = w.saturating_sub(cw);
        let (left, right) = match alignments.get(i).copied().unwrap_or(Alignment::None) {
            Alignment::Right => (pad, 0),
            Alignment::Center => (pad / 2, pad - pad / 2),
            _ => (0, pad),
        };
        if left > 0 {
            spans.push(Span::raw(" ".repeat(left)));
        }
        for s in cell {
            spans.push(s.clone());
        }
        if right > 0 {
            spans.push(Span::raw(" ".repeat(right)));
        }
        spans.push(Span::raw(" "));
    }
    spans.push(Span::styled("│", border_style));
    spans
}

fn render_table_separator(
    widths: &[usize],
    alignments: &[Alignment],
    border_style: Style,
) -> Vec<Span<'static>> {
    // Header divider, mirroring markdown's own `:---:` form so column
    // alignment is visible in the rendered table.
    let mut s = String::from("├");
    for (i, &w) in widths.iter().enumerate() {
        if i > 0 {
            s.push('┼');
        }
        let interior = w + 2; // matches the `│ {content} ` cell padding
        let dashes = "─".repeat(interior);
        match alignments.get(i).copied().unwrap_or(Alignment::None) {
            Alignment::Left => {
                s.push(':');
                s.push_str(&"─".repeat(interior.saturating_sub(1)));
            }
            Alignment::Right => {
                s.push_str(&"─".repeat(interior.saturating_sub(1)));
                s.push(':');
            }
            Alignment::Center => {
                s.push(':');
                s.push_str(&"─".repeat(interior.saturating_sub(2)));
                s.push(':');
            }
            Alignment::None => s.push_str(&dashes),
        }
    }
    s.push('┤');
    vec![Span::styled(s, border_style)]
}

#[cfg(test)]
mod tests {
    use super::render_to_rows;
    use ratatui::style::{Modifier, Style};

    fn texts(rows: &[Vec<ratatui::text::Span<'static>>]) -> Vec<String> {
        rows.iter()
            .map(|r| r.iter().map(|s| s.content.as_ref()).collect::<String>())
            .collect()
    }

    #[test]
    fn plain_text_passes_through() {
        let rows = render_to_rows("hello world", Style::default());
        assert_eq!(texts(&rows), vec!["hello world".to_string()]);
    }

    #[test]
    fn trailing_blank_lines_are_stripped() {
        let rows = render_to_rows("hello\n\n\n\n", Style::default());
        assert_eq!(texts(&rows), vec!["hello".to_string()]);
    }

    #[test]
    fn paragraphs_separated_by_blank_row() {
        let rows = render_to_rows("first paragraph\n\nsecond paragraph", Style::default());
        assert_eq!(
            texts(&rows),
            vec![
                "first paragraph".to_string(),
                "".to_string(),
                "second paragraph".to_string(),
            ]
        );
    }

    #[test]
    fn bullet_list_renders_with_bullets() {
        let rows = render_to_rows("- one\n- two", Style::default());
        let t = texts(&rows);
        assert_eq!(t, vec!["• one".to_string(), "• two".to_string()]);
    }

    #[test]
    fn ordered_list_uses_running_counter() {
        let rows = render_to_rows("1. first\n2. second", Style::default());
        let t = texts(&rows);
        assert_eq!(t, vec!["1. first".to_string(), "2. second".to_string()]);
    }

    #[test]
    fn bold_marks_modifier_on_span() {
        let rows = render_to_rows("hello **bold** world", Style::default());
        let row = &rows[0];
        let bold_span = row
            .iter()
            .find(|s| s.content.as_ref() == "bold")
            .expect("bold span");
        assert!(bold_span.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn inline_code_uses_distinct_style() {
        let rows = render_to_rows("call `foo()` directly", Style::default());
        let row = &rows[0];
        let code_span = row
            .iter()
            .find(|s| s.content.as_ref() == "foo()")
            .expect("code span");
        assert!(code_span.style.bg.is_some());
    }

    #[test]
    fn fenced_code_block_renders_each_line() {
        let rows = render_to_rows("```\nfn main() {}\nlet x = 1;\n```", Style::default());
        let t = texts(&rows);
        assert!(
            t.iter().any(|line| line.contains("fn main()")),
            "got: {t:?}"
        );
        assert!(
            t.iter().any(|line| line.contains("let x = 1;")),
            "got: {t:?}"
        );
    }

    #[test]
    fn link_appends_url_when_distinct_from_text() {
        let rows = render_to_rows("see [docs](https://example.com)", Style::default());
        let row = &rows[0];
        let combined: String = row.iter().map(|s| s.content.as_ref()).collect();
        assert!(combined.contains("docs"), "got: {combined}");
        assert!(combined.contains("https://example.com"), "got: {combined}");
    }

    #[test]
    fn heading_emits_one_row_with_bold() {
        let rows = render_to_rows("# Title\n\nbody", Style::default());
        let t = texts(&rows);
        assert_eq!(t[0], "Title".to_string());
        assert!(rows[0]
            .iter()
            .any(|s| s.style.add_modifier.contains(Modifier::BOLD)));
    }

    #[test]
    fn table_renders_with_box_borders_and_header_divider() {
        let md = "| a | bb |\n|---|---:|\n| 1 | 22 |\n| 33 | 4 |";
        let rows = render_to_rows(md, Style::default());
        let t = texts(&rows);
        // 4 rows: header, separator, two body rows.
        assert_eq!(t.len(), 4, "got: {t:?}");
        // Header / body rows use `│ … │`; the separator uses `├ … ┤`.
        for (i, line) in t.iter().enumerate() {
            let (l, r) = if i == 1 {
                ('├', '┤')
            } else {
                ('│', '│')
            };
            assert!(line.starts_with(l), "row {i} missing left border: {line:?}");
            assert!(line.ends_with(r), "row {i} missing right border: {line:?}");
        }
        // Header divider uses `─` and the right-alignment marker `:` for col 2.
        assert!(t[1].contains('─'), "no dashes in separator: {:?}", t[1]);
        assert!(
            t[1].contains(":┤"),
            "right-align marker missing: {:?}",
            t[1]
        );
        // Body cells appear in their rows.
        assert!(t[2].contains('1') && t[2].contains("22"), "got: {:?}", t[2]);
        assert!(t[3].contains("33") && t[3].contains('4'), "got: {:?}", t[3]);
    }

    #[test]
    fn empty_input_yields_one_blank_row() {
        let rows = render_to_rows("", Style::default());
        assert_eq!(rows.len(), 1);
        assert!(rows[0].is_empty());
    }
}
