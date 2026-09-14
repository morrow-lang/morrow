//! Completion-only recovery over one current token-level member hole, never cached type evidence.
use super::*;
impl Server {
    /// Recheck current overlays; a failed proof simply retains ordinary lexical fallback.
    pub(super) fn member_completion(&self, uri: &str, source: &str, cursor: usize) -> Option<Json> {
        let (program, site) = if let Some(path) = file_path(uri).ok()? {
            let loaded = modules::load_member_sources(&path, &self.overlays(), cursor).ok()?;
            (loaded.program, loaded.recovery?)
        } else {
            parse::recover_member(source, cursor).ok()?
        };
        let facts = check::recovery::analyze(&program, &site).ok()?;
        let candidates = facts
            .members
            .iter()
            .filter(|member| member.name.starts_with(site.prefix()))
            .map(|member| (member.name.clone(), 5))
            .collect();
        Some(navigation::completion_items(
            candidates,
            navigation::source_range(source, site.selector()),
            Some(&facts),
        ))
    }
}

impl Server {
    /// Suggest source names only after canonical lexical resolution of the current argument site.
    pub(super) fn label_completion(&self, uri: &str, source: &str, cursor: usize) -> Option<Json> {
        let (labels, site) = if let Some(path) = file_path(uri).ok()? {
            let identity = modules::source_identity(&path).ok()?;
            let loaded = modules::load_label_sources(&path, &self.overlays(), cursor).ok()?;
            let site = loaded.label_recovery.clone()?;
            let index =
                index::Index::loaded_labels(&loaded, &identity, cursor, Some(site.clone()))?;
            (index.call_labels?, site)
        } else {
            let (program, site) = parse::recover_labels(source, cursor).ok()?;
            if !program.imports.is_empty() {
                return None;
            }
            let path = PathBuf::from(uri);
            let index =
                index::Index::single_labels(&program, source, &path, cursor, Some(site.clone()))?;
            (index.call_labels?, site)
        };
        Some(label_items(labels, source, &site))
    }
}

/// Bound names and serialized edits independently of semantic checking or recovery provenance.
fn label_items(labels: Vec<String>, source: &str, site: &parse::LabelSite) -> Json {
    let range = navigation::source_range(source, site.selector());
    let mut items = Vec::new();
    let mut bytes = 0usize;
    for name in labels {
        bytes = bytes
            .saturating_add(name.len().saturating_mul(2))
            .saturating_add(256);
        if items.len() >= 256 || bytes > 1024 * 1024 {
            return navigation::completion_list(items, true);
        }
        let text = if site.colon() {
            name.clone()
        } else {
            format!("{name}: ")
        };
        items.push(object([
            ("label", string(name)),
            ("kind", number(5)),
            ("detail", string("Source parameter name")),
            (
                "textEdit",
                object([("range", range.clone()), ("newText", string(text))]),
            ),
        ]));
    }
    navigation::completion_list(items, false)
}
