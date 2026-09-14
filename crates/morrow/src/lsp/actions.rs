//! Source actions contain executable, versioned edits instead of unhandled commands.
use super::*;
impl Server {
    /// Offer the canonical source fix only when the client's kind filter permits it.
    pub(super) fn code_actions(&self, params: &Json) -> std::result::Result<Json, (i64, String)> {
        if !self.client_edits.literal_actions {
            return Err((-32803, "codeAction requires client capabilities workspace.workspaceEdit.documentChanges and textDocument.codeAction.codeActionLiteralSupport".into()));
        }
        let invalid = |message: String| (-32602, message);
        let uri = document_uri(params).map_err(invalid)?;
        let document = self
            .documents
            .get(uri)
            .ok_or_else(|| invalid("request document is not open".into()))?;
        let range = field(params, "range").map_err(invalid)?;
        let start = byte_position(&document.source, field(range, "start").map_err(invalid)?)
            .map_err(invalid)?;
        let end = byte_position(&document.source, field(range, "end").map_err(invalid)?)
            .map_err(invalid)?;
        if start > end {
            return Err(invalid("code action range is reversed".into()));
        }
        let context = field(params, "context").map_err(invalid)?;
        field(context, "diagnostics")
            .map_err(invalid)?
            .array()
            .map_err(invalid)?;
        let kind = "source.fixAll.morrow";
        if let Some(only) = context.get("only") {
            let filters = only
                .array()
                .map_err(invalid)?
                .iter()
                .map(Json::string)
                .collect::<Result<Vec<_>>>()
                .map_err(invalid)?;
            if !filters.iter().any(|filter| {
                filter.is_empty()
                    || *filter == kind
                    || kind
                        .strip_prefix(*filter)
                        .is_some_and(|rest| rest.starts_with('.'))
            }) {
                return Ok(Json::Array(vec![]));
            }
        }
        let Ok(formatted) = crate::format::format(&document.source) else {
            return Ok(Json::Array(vec![]));
        };
        if formatted == document.source || formatted.len() > MAX_SOURCE {
            return Ok(Json::Array(vec![]));
        }
        let edit = object([
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
            ("newText", string(formatted)),
        ]);
        Ok(Json::Array(vec![object([
            ("title", string("Format document")),
            ("kind", string(kind)),
            (
                "edit",
                refactor::workspace_edit(uri, document.version, vec![edit]),
            ),
        ])]))
    }
}
