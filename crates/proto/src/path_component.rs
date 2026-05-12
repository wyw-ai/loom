use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PathComponentError {
    #[error("{what} `{value}` length is invalid")]
    InvalidLength { what: String, value: String },
    #[error("{what} `{value}` must not be `.`, `..`, or start with `-`")]
    Reserved { what: String, value: String },
    #[error("{what} `{value}` only allows [A-Za-z0-9_.-]")]
    InvalidChars { what: String, value: String },
    #[error("{what} `{value}` must not contain empty dot segments")]
    EmptyDotSegment { what: String, value: String },
}

/// Validate a single filesystem path component before joining it into
/// runtime-managed directories.
pub fn validate_path_component(value: &str, what: &str) -> Result<(), PathComponentError> {
    if value.is_empty() || value.len() > 64 {
        return Err(PathComponentError::InvalidLength {
            what: what.to_string(),
            value: value.to_string(),
        });
    }
    if value == "." || value == ".." || value.starts_with('-') {
        return Err(PathComponentError::Reserved {
            what: what.to_string(),
            value: value.to_string(),
        });
    }
    if value
        .chars()
        .any(|c| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')))
    {
        return Err(PathComponentError::InvalidChars {
            what: what.to_string(),
            value: value.to_string(),
        });
    }
    if value.split('.').any(str::is_empty) {
        return Err(PathComponentError::EmptyDotSegment {
            what: what.to_string(),
            value: value.to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_safe_components() {
        for value in [
            "actor_alice",
            "actor-42",
            "thread_abc",
            "chan_31f8fa85d909",
            "mr-detector",
            "v1.2.3",
        ] {
            validate_path_component(value, "id").expect(value);
        }
    }

    #[test]
    fn rejects_unsafe_components() {
        for value in [
            "", ".", "..", "../etc", "a/b", "a\\b", "a\0b", "-flag", ".hidden", "foo..bar", "a:b",
        ] {
            validate_path_component(value, "id").expect_err(value);
        }
    }
}
