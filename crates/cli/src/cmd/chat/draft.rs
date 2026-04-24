use unicode_width::UnicodeWidthChar;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DraftInput {
    segments: Vec<DraftSegment>,
    cursor: usize,
    preferred_column: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DraftSegment {
    Text(String),
    Pasted {
        display: String,
        text: String,
    },
    SavedWorkspace {
        display: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnitRef<'a> {
    Char {
        segment_idx: usize,
        char_idx: usize,
        ch: char,
    },
    Token {
        segment_idx: usize,
        display: &'a str,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CursorPosition {
    line: usize,
    column: usize,
}

impl DraftInput {
    pub fn is_empty(&self) -> bool {
        self.len_units() == 0
    }

    pub fn clear(&mut self) {
        self.segments.clear();
        self.cursor = 0;
        self.preferred_column = None;
    }

    pub fn pop(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let target = self.cursor - 1;
        self.remove_unit_at(target);
        self.cursor = target;
        self.preferred_column = None;
    }

    pub fn delete_word_left(&mut self) {
        let units = self.units();
        let mut start = self.cursor;
        while start > 0 && !is_word_unit(units[start - 1]) {
            start -= 1;
        }
        while start > 0 && is_word_unit(units[start - 1]) {
            start -= 1;
        }
        self.remove_units_in_range(start, self.cursor);
        self.cursor = start;
        self.preferred_column = None;
    }

    pub fn delete_to_line_start(&mut self) {
        let units = self.units();
        let mut start = self.cursor;
        while start > 0 {
            if matches!(units[start - 1], UnitRef::Char { ch: '\n', .. }) {
                break;
            }
            start -= 1;
        }
        self.remove_units_in_range(start, self.cursor);
        self.cursor = start;
        self.preferred_column = None;
    }

    pub fn push_char(&mut self, c: char) {
        self.insert_text(&c.to_string());
    }

    pub fn push_str(&mut self, text: &str) {
        self.insert_text(text);
    }

    pub fn push_pasted_chunk(&mut self, id: u64, text: String) {
        let line_count = text.lines().count().max(1);
        let extra_lines = line_count.saturating_sub(1);
        let display = if extra_lines == 0 {
            format!("[Pasted text #{id}]")
        } else {
            format!("[Pasted text #{id} +{extra_lines} lines]")
        };
        self.insert_segment(DraftSegment::Pasted { display, text });
    }

    pub fn push_saved_workspace_ref(&mut self, display: String) {
        self.insert_segment(DraftSegment::SavedWorkspace { display });
    }

    pub fn move_left(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        self.cursor -= 1;
        self.preferred_column = None;
        true
    }

    pub fn move_right(&mut self) -> bool {
        if self.cursor >= self.len_units() {
            return false;
        }
        self.cursor += 1;
        self.preferred_column = None;
        true
    }

    pub fn move_to_start(&mut self) {
        self.cursor = 0;
        self.preferred_column = None;
    }

    pub fn move_to_end(&mut self) {
        self.cursor = self.len_units();
        self.preferred_column = None;
    }

    pub fn move_word_left(&mut self) {
        let units = self.units();
        let mut cursor = self.cursor;
        while cursor > 0 && !is_word_unit(units[cursor - 1]) {
            cursor -= 1;
        }
        while cursor > 0 && is_word_unit(units[cursor - 1]) {
            cursor -= 1;
        }
        self.cursor = cursor;
        self.preferred_column = None;
    }

    pub fn move_word_right(&mut self) {
        let units = self.units();
        let mut cursor = self.cursor;
        while cursor < units.len() && !is_word_unit(units[cursor]) {
            cursor += 1;
        }
        while cursor < units.len() && is_word_unit(units[cursor]) {
            cursor += 1;
        }
        self.cursor = cursor;
        self.preferred_column = None;
    }

    pub fn move_up(&mut self) -> bool {
        let units = self.units();
        let current = self.cursor_position(&units);
        if current.line == 0 {
            return false;
        }
        let target_column = self.preferred_column.unwrap_or(current.column);
        self.cursor = self.cursor_for_line_column(&units, current.line - 1, target_column);
        self.preferred_column = Some(target_column);
        true
    }

    pub fn move_down(&mut self) -> bool {
        let units = self.units();
        let current = self.cursor_position(&units);
        let max_line = self.max_line_index(&units);
        if current.line >= max_line {
            return false;
        }
        let target_column = self.preferred_column.unwrap_or(current.column);
        self.cursor = self.cursor_for_line_column(&units, current.line + 1, target_column);
        self.preferred_column = Some(target_column);
        true
    }

    pub fn display_text(&self) -> String {
        self.segments
            .iter()
            .map(display_for_segment)
            .collect::<Vec<_>>()
            .concat()
    }

    pub fn display_cursor_char_index(&self) -> usize {
        let units = self.units();
        units
            .iter()
            .take(self.cursor)
            .map(display_len)
            .sum::<usize>()
    }

    pub fn submission_text(&self) -> String {
        self.segments
            .iter()
            .map(submission_for_segment)
            .collect::<Vec<_>>()
            .concat()
    }

    pub fn take_submission_text(&mut self) -> String {
        std::mem::take(self).submission_text()
    }

    fn insert_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let insert_at = self.segment_insert_index();
        let insert_pos = self.unit_position_in_segment(insert_at);
        let append_prev = insert_at > 0 && self.cursor_is_at_segment_end(insert_at - 1);
        match self.segments.get(insert_at) {
            Some(DraftSegment::Text(_)) if insert_pos == Some(0) => {
                if let Some(DraftSegment::Text(buf)) = self.segments.get_mut(insert_at) {
                    buf.insert_str(0, text);
                }
            }
            Some(DraftSegment::Text(_)) if insert_pos.is_some() => {
                if let Some(DraftSegment::Text(buf)) = self.segments.get_mut(insert_at) {
                    let char_idx = insert_pos.unwrap();
                    let byte_idx = char_to_byte_idx(buf, char_idx);
                    buf.insert_str(byte_idx, text);
                }
            }
            _ if append_prev => {
                if let Some(DraftSegment::Text(buf)) = self.segments.get_mut(insert_at - 1) {
                    buf.push_str(text);
                } else {
                    self.segments.insert(insert_at, DraftSegment::Text(text.to_string()));
                }
            }
            _ => {
                self.segments.insert(insert_at, DraftSegment::Text(text.to_string()));
            }
        }
        self.cursor += text.chars().count();
        self.preferred_column = None;
        self.coalesce_text_segments();
    }

    fn insert_segment(&mut self, segment: DraftSegment) {
        let insert_at = self.segment_insert_index();
        self.segments.insert(insert_at, segment);
        self.cursor += 1;
        self.preferred_column = None;
        self.coalesce_text_segments();
    }

    fn remove_unit_at(&mut self, unit_index: usize) {
        let Some(unit) = self.units().get(unit_index).copied() else {
            return;
        };
        match unit {
            UnitRef::Char {
                segment_idx,
                char_idx,
                ..
            } => {
                if let Some(DraftSegment::Text(text)) = self.segments.get_mut(segment_idx) {
                    let start = char_to_byte_idx(text, char_idx);
                    let end = char_to_byte_idx(text, char_idx + 1);
                    text.replace_range(start..end, "");
                    if text.is_empty() {
                        self.segments.remove(segment_idx);
                    }
                }
            }
            UnitRef::Token { segment_idx, .. } => {
                self.segments.remove(segment_idx);
            }
        }
        self.coalesce_text_segments();
    }

    fn remove_units_in_range(&mut self, start: usize, end: usize) {
        if start >= end {
            return;
        }
        for unit_index in (start..end).rev() {
            self.remove_unit_at(unit_index);
        }
    }

    fn len_units(&self) -> usize {
        self.units().len()
    }

    fn units(&self) -> Vec<UnitRef<'_>> {
        let mut units = Vec::new();
        for (segment_idx, segment) in self.segments.iter().enumerate() {
            match segment {
                DraftSegment::Text(text) => {
                    for (char_idx, ch) in text.chars().enumerate() {
                        units.push(UnitRef::Char {
                            segment_idx,
                            char_idx,
                            ch,
                        });
                    }
                }
                DraftSegment::Pasted { display, .. } => units.push(UnitRef::Token {
                    segment_idx,
                    display,
                }),
                DraftSegment::SavedWorkspace { display } => units.push(UnitRef::Token {
                    segment_idx,
                    display,
                }),
            }
        }
        units
    }

    fn segment_insert_index(&self) -> usize {
        let units = self.units();
        if self.cursor == units.len() {
            return self.segments.len();
        }
        match units[self.cursor] {
            UnitRef::Char { segment_idx, .. } | UnitRef::Token { segment_idx, .. } => segment_idx,
        }
    }

    fn unit_position_in_segment(&self, segment_idx: usize) -> Option<usize> {
        let mut count = 0usize;
        for unit in self.units().iter().take(self.cursor) {
            match unit {
                UnitRef::Char {
                    segment_idx: idx, ..
                } if *idx == segment_idx => count += 1,
                UnitRef::Char { segment_idx: idx, .. } | UnitRef::Token { segment_idx: idx, .. }
                    if *idx > segment_idx =>
                {
                    break;
                }
                _ => {}
            }
        }
        match self.segments.get(segment_idx) {
            Some(DraftSegment::Text(_)) => Some(count),
            _ => None,
        }
    }

    fn cursor_is_at_segment_end(&self, segment_idx: usize) -> bool {
        let Some(DraftSegment::Text(text)) = self.segments.get(segment_idx) else {
            return false;
        };
        self.unit_position_in_segment(segment_idx) == Some(text.chars().count())
    }

    fn coalesce_text_segments(&mut self) {
        let mut merged = Vec::with_capacity(self.segments.len());
        for segment in std::mem::take(&mut self.segments) {
            match segment {
                DraftSegment::Text(text) if text.is_empty() => {}
                DraftSegment::Text(text) => {
                    if let Some(DraftSegment::Text(existing)) = merged.last_mut() {
                        existing.push_str(&text);
                    } else {
                        merged.push(DraftSegment::Text(text));
                    }
                }
                other => merged.push(other),
            }
        }
        self.segments = merged;
    }

    fn cursor_position(&self, units: &[UnitRef<'_>]) -> CursorPosition {
        let mut line = 0usize;
        let mut column = 0usize;
        for unit in units.iter().take(self.cursor) {
            match unit {
                UnitRef::Char { ch: '\n', .. } => {
                    line += 1;
                    column = 0;
                }
                UnitRef::Char { ch, .. } => {
                    column += UnicodeWidthChar::width(*ch).unwrap_or(0).max(1);
                }
                UnitRef::Token { display, .. } => {
                    column += display.chars().count().max(1);
                }
            }
        }
        CursorPosition { line, column }
    }

    fn max_line_index(&self, units: &[UnitRef<'_>]) -> usize {
        units.iter().fold(0usize, |lines, unit| match unit {
            UnitRef::Char { ch: '\n', .. } => lines + 1,
            _ => lines,
        })
    }

    fn cursor_for_line_column(
        &self,
        units: &[UnitRef<'_>],
        target_line: usize,
        target_column: usize,
    ) -> usize {
        let mut line = 0usize;
        let mut cursor = 0usize;
        let mut line_start = 0usize;

        while cursor < units.len() && line < target_line {
            if matches!(units[cursor], UnitRef::Char { ch: '\n', .. }) {
                line += 1;
                line_start = cursor + 1;
            }
            cursor += 1;
        }

        cursor = line_start;
        let mut column = 0usize;
        while cursor < units.len() {
            if matches!(units[cursor], UnitRef::Char { ch: '\n', .. }) {
                break;
            }
            let next_width = match units[cursor] {
                UnitRef::Char { ch, .. } => UnicodeWidthChar::width(ch).unwrap_or(0).max(1),
                UnitRef::Token { display, .. } => display.chars().count().max(1),
            };
            if column + next_width > target_column {
                break;
            }
            column += next_width;
            cursor += 1;
        }
        cursor
    }
}

fn display_for_segment(segment: &DraftSegment) -> String {
    match segment {
        DraftSegment::Text(text) => text.clone(),
        DraftSegment::Pasted { display, .. } => display.clone(),
        DraftSegment::SavedWorkspace { display } => display.clone(),
    }
}

fn submission_for_segment(segment: &DraftSegment) -> String {
    match segment {
        DraftSegment::Text(text) => text.clone(),
        DraftSegment::Pasted { text, .. } => text.clone(),
        DraftSegment::SavedWorkspace { display } => display.clone(),
    }
}

fn display_len(unit: &UnitRef<'_>) -> usize {
    match unit {
        UnitRef::Char { .. } => 1,
        UnitRef::Token { display, .. } => display.chars().count(),
    }
}

fn is_word_unit(unit: UnitRef<'_>) -> bool {
    match unit {
        UnitRef::Char { ch, .. } => ch.is_alphanumeric() || ch == '_',
        UnitRef::Token { .. } => false,
    }
}

fn char_to_byte_idx(text: &str, char_idx: usize) -> usize {
    text.char_indices()
        .nth(char_idx)
        .map(|(idx, _)| idx)
        .unwrap_or(text.len())
}

impl From<String> for DraftInput {
    fn from(value: String) -> Self {
        if value.is_empty() {
            Self::default()
        } else {
            let len = value.chars().count();
            Self {
                segments: vec![DraftSegment::Text(value)],
                cursor: len,
                preferred_column: None,
            }
        }
    }
}

impl From<&str> for DraftInput {
    fn from(value: &str) -> Self {
        Self::from(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::DraftInput;

    #[test]
    fn pop_removes_entire_pasted_token() {
        let mut input = DraftInput::default();
        input.push_str("before ");
        input.push_pasted_chunk(1, "line 1\nline 2".into());
        assert_eq!(input.display_text(), "before [Pasted text #1 +1 lines]");
        input.pop();
        assert_eq!(input.display_text(), "before ");
        assert_eq!(input.submission_text(), "before ");
    }

    #[test]
    fn submission_text_expands_pasted_chunk() {
        let mut input = DraftInput::default();
        input.push_pasted_chunk(2, "hello\nworld".into());
        input.push_saved_workspace_ref("[Saved pasted content to workspace (5.0 KB) id=3]".into());
        assert_eq!(
            input.submission_text(),
            "hello\nworld[Saved pasted content to workspace (5.0 KB) id=3]"
        );
    }

    #[test]
    fn inserts_text_at_cursor() {
        let mut input = DraftInput::from("hello");
        input.move_left();
        input.move_left();
        input.push_str("X");
        assert_eq!(input.display_text(), "helXlo");
    }

    #[test]
    fn vertical_navigation_moves_between_lines() {
        let mut input = DraftInput::from("abc\ndef\nghi");
        input.move_to_end();
        assert!(input.move_up());
        assert!(input.move_up());
        assert_eq!(input.display_cursor_char_index(), 3);
        assert!(!input.move_up());
    }

    #[test]
    fn option_word_navigation_skips_word_boundaries() {
        let mut input = DraftInput::from("hello brave world");
        input.move_word_left();
        assert_eq!(input.display_cursor_char_index(), 12);
        input.move_word_left();
        assert_eq!(input.display_cursor_char_index(), 6);
        input.move_word_right();
        assert_eq!(input.display_cursor_char_index(), 11);
    }

    #[test]
    fn delete_word_left_removes_previous_word() {
        let mut input = DraftInput::from("hello brave world");
        input.delete_word_left();
        assert_eq!(input.display_text(), "hello brave ");
        input.delete_word_left();
        assert_eq!(input.display_text(), "hello ");
    }

    #[test]
    fn delete_to_line_start_clears_back_to_previous_newline() {
        let mut input = DraftInput::from("abc\ndef");
        input.delete_to_line_start();
        assert_eq!(input.display_text(), "abc\n");
    }
}
