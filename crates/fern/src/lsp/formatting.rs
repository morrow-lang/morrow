//! Canonical whole-document edits from the accepted open buffer, without effects.
use super::*;

impl Server {
    /// Return no edit for clean source or one syntax-preserving UTF-16 replacement.
    /// Invalid parameters and formatting failures have distinct protocol error codes.
    pub(super) fn formatting(&self, params: &Json) -> std::result::Result<Json, (i64, String)> {
        let invalid = |message: String| (-32602, message);
        options(field(params, "options").map_err(invalid)?).map_err(invalid)?;
        let uri = document_uri(params).map_err(invalid)?;
        let document = self
            .documents
            .get(uri)
            .ok_or_else(|| invalid("request document is not open".into()))?;
        let text = crate::format::format(&document.source)
            .map_err(|error| (-32803, format!("cannot format {uri}: {}", error.message)))?;
        if text == document.source {
            return Ok(Json::Array(Vec::new()));
        }
        Ok(Json::Array(vec![object([
            (
                "range",
                navigation::source_range(
                    &document.source,
                    Span {
                        start: 0,
                        end: document.source.len(),
                    },
                ),
            ),
            ("newText", string(text)),
        ])]))
    }
}

/// Validate standard protocol options while retaining Fern's fixed canonical style.
/// Additional options may be scalar strings, booleans or signed 32-bit integers.
fn options(value: &Json) -> Result<()> {
    let Json::Object(fields) = value else {
        return Err("formatting options must be an object".into());
    };
    let tab_size = field(value, "tabSize")?.integer()?;
    if !(1..=i64::from(i32::MAX)).contains(&tab_size) {
        return Err("formatting tabSize must be a positive protocol uinteger".into());
    }
    if !matches!(field(value, "insertSpaces")?, Json::Bool(_)) {
        return Err("formatting insertSpaces must be boolean".into());
    }
    for (name, value) in fields {
        if matches!(
            name.as_str(),
            "trimTrailingWhitespace" | "insertFinalNewline" | "trimFinalNewlines"
        ) && !matches!(value, Json::Bool(_))
        {
            return Err(format!("formatting {name} must be boolean"));
        }
        let valid = match value {
            Json::Bool(_) | Json::String(_) => true,
            Json::Number(_) => value
                .integer()
                .is_ok_and(|value| i32::try_from(value).is_ok()),
            _ => false,
        };
        if !valid {
            return Err(format!("invalid scalar formatting option {name}"));
        }
    }
    Ok(())
}
