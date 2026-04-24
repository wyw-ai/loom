#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DraftInput {
    segments: Vec<DraftSegment>,
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

impl DraftInput {
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    pub fn clear(&mut self) {
        self.segments.clear();
    }

    pub fn pop(&mut self) {
        let Some(last) = self.segments.last_mut() else {
            return;
        };
        match last {
            DraftSegment::Text(text) => {
                text.pop();
                if text.is_empty() {
                    self.segments.pop();
                }
            }
            DraftSegment::Pasted { .. } | DraftSegment::SavedWorkspace { .. } => {
                self.segments.pop();
            }
        }
    }

    pub fn push_char(&mut self, c: char) {
        self.push_str(&c.to_string());
    }

    pub fn push_str(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        match self.segments.last_mut() {
            Some(DraftSegment::Text(buf)) => buf.push_str(text),
            _ => self.segments.push(DraftSegment::Text(text.to_string())),
        }
    }

    pub fn push_pasted_chunk(&mut self, id: u64, text: String) {
        let line_count = text.lines().count().max(1);
        let extra_lines = line_count.saturating_sub(1);
        let display = if extra_lines == 0 {
            format!("[Pasted text #{id}]")
        } else {
            format!("[Pasted text #{id} +{extra_lines} lines]")
        };
        self.segments.push(DraftSegment::Pasted { display, text });
    }

    pub fn push_saved_workspace_ref(&mut self, display: String) {
        self.segments
            .push(DraftSegment::SavedWorkspace { display });
    }

    pub fn display_text(&self) -> String {
        self.segments
            .iter()
            .map(|segment| match segment {
                DraftSegment::Text(text) => text.clone(),
                DraftSegment::Pasted { display, .. } => display.clone(),
                DraftSegment::SavedWorkspace { display } => display.clone(),
            })
            .collect()
    }

    pub fn submission_text(&self) -> String {
        self.segments
            .iter()
            .map(|segment| match segment {
                DraftSegment::Text(text) => text.clone(),
                DraftSegment::Pasted { text, .. } => text.clone(),
                DraftSegment::SavedWorkspace { display } => display.clone(),
            })
            .collect()
    }

    pub fn take_submission_text(&mut self) -> String {
        std::mem::take(self).submission_text()
    }
}

impl From<String> for DraftInput {
    fn from(value: String) -> Self {
        if value.is_empty() {
            Self::default()
        } else {
            Self {
                segments: vec![DraftSegment::Text(value)],
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
}
