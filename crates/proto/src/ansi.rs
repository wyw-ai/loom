/// Strip ANSI escape sequences from a string.
///
/// Handles CSI (`ESC[`), OSC (`ESC]`), and other ESC variants
/// (DCS `ESC P`, APC `ESC _`, PM `ESC ^`, SOS `ESC X`).
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            out.push(ch);
            continue;
        }
        match chars.peek().copied() {
            // CSI: ESC [ ... alpha
            Some('[') => {
                let _ = chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            // OSC: ESC ] ... BEL or ST (ESC \)
            Some(']') => {
                let _ = chars.next();
                let mut prev = '\0';
                for c in chars.by_ref() {
                    if c == '\u{07}' || (prev == '\u{1b}' && c == '\\') {
                        break;
                    }
                    prev = c;
                }
            }
            // Other ESC-prefixed sequences: DCS (P), SOS (X), APC (_), PM (^)
            Some(c) if "PX_^".contains(c) => {
                let _ = chars.next();
                let mut prev = '\0';
                for c2 in chars.by_ref() {
                    if c2 == '\u{07}' || (prev == '\u{1b}' && c2 == '\\') {
                        break;
                    }
                    prev = c2;
                }
            }
            _ => {
                // Lone ESC — skip it and continue
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi_stripper_removes_csi_sequences() {
        assert_eq!(strip_ansi("\u{1b}[31mred\u{1b}[0m"), "red");
    }
}
