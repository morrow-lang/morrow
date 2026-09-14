//! Negotiate edit payloads instead of assuming every client implements newer LSP forms.
use super::*;

#[derive(Default)]
pub(super) struct ClientEdits {
    pub document_changes: bool,
    pub prepare_rename: bool,
    pub literal_actions: bool,
}
impl ClientEdits {
    /// Missing, false or malformed declarations never enable an incompatible payload.
    pub fn initialize(params: &Json) -> Self {
        let document_changes = matches!(
            at(
                params,
                &[
                    "capabilities",
                    "workspace",
                    "workspaceEdit",
                    "documentChanges"
                ]
            ),
            Some(Json::Bool(true))
        );
        let prepare_rename = document_changes
            && matches!(
                at(
                    params,
                    &["capabilities", "textDocument", "rename", "prepareSupport"]
                ),
                Some(Json::Bool(true))
            );
        // LSP guarantees clients gracefully handle kinds outside their declared valueSet.
        let literal_actions = document_changes
            && at(
                params,
                &[
                    "capabilities",
                    "textDocument",
                    "codeAction",
                    "codeActionLiteralSupport",
                    "codeActionKind",
                    "valueSet",
                ],
            )
            .and_then(|value| value.array().ok())
            .is_some_and(|values| values.iter().all(|value| value.string().is_ok()));
        Self {
            document_changes,
            prepare_rename,
            literal_actions,
        }
    }
    /// A boolean rename provider retains compatibility when prepareRename is not supported.
    pub fn rename_provider(&self) -> Json {
        if self.prepare_rename {
            object([("prepareProvider", Json::Bool(true))])
        } else {
            Json::Bool(self.document_changes)
        }
    }
    /// Offer source actions only when literal actions and their versioned edits are supported.
    pub fn action_provider(&self) -> Json {
        if self.literal_actions {
            object([(
                "codeActionKinds",
                Json::Array(vec![string("source.fixAll.morrow")]),
            )])
        } else {
            Json::Bool(false)
        }
    }
}
/// Walk only object members; malformed or absent intermediate shapes mean no support.
fn at<'a>(value: &'a Json, path: &[&str]) -> Option<&'a Json> {
    path.iter().try_fold(value, |value, key| value.get(key))
}
