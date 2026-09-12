//! Conservative, versioned local rename with binding identity checks before and after edits.
use super::{index::Index, *};

type EditResult<T> = std::result::Result<T, (i64, String)>;
type ReferenceInventory = (Vec<Span>, Vec<(Span, Option<Identity>)>);
/// Map malformed protocol parameters to the standard invalid-params error.
fn invalid(message: String) -> (i64, String) {
    (-32602, message)
}
/// Distinguish unsafe or unavailable semantic edits from malformed protocol input.
fn failed(message: impl Into<String>) -> (i64, String) {
    (-32803, message.into())
}

enum Graph {
    Loaded(Box<modules::Loaded>),
    Single(ast::Program),
}
struct Snapshot {
    graph: Graph,
    path: PathBuf,
    source: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Identity {
    path: PathBuf,
    span: Span,
}
impl Snapshot {
    /// Load current unsaved dependencies and require a valid checked graph for semantic edits.
    fn load(server: &Server, uri: &str, source: &str) -> EditResult<Self> {
        let (path, graph) = if let Some(path) = file_path(uri).map_err(invalid)? {
            let path = modules::source_identity(&path).map_err(|e| failed(e.message))?;
            let mut overlays = server.overlays();
            overlays.insert(path.clone(), source.into());
            let loaded =
                modules::load_editor_sources(&path, &overlays).map_err(|e| failed(e.message))?;
            if loaded.sources().map(|s| s.text.len()).sum::<usize>() > 2 * MAX_SOURCE {
                return Err(failed("rename source graph exceeds 2 MiB limit"));
            }
            (path, Graph::Loaded(Box::new(loaded)))
        } else {
            (
                PathBuf::from(uri),
                Graph::Single(parse::parse(source).map_err(|e| failed(e.message))?),
            )
        };
        let snapshot = Self {
            graph,
            path,
            source: source.into(),
        };
        check::check_library(snapshot.program())
            .map_err(|e| failed(format!("invalid rename source: {}", e.message)))?;
        Ok(snapshot)
    }
    /// Access the original checked AST regardless of document URI scheme.
    fn program(&self) -> &ast::Program {
        match &self.graph {
            Graph::Loaded(l) => &l.program,
            Graph::Single(p) => p,
        }
    }
    /// Resolve one cursor against the complete current source graph.
    fn index(&self, cursor: usize) -> EditResult<Index<'_>> {
        match &self.graph {
            Graph::Loaded(l) => Index::loaded(l, &self.path, cursor),
            Graph::Single(p) => Index::single(p, &self.source, &self.path, cursor),
        }
        .ok_or_else(|| failed("rename indexing limit exceeded"))
    }
    /// Account all dependency bytes for repeated indexing work.
    fn bytes(&self) -> usize {
        match &self.graph {
            Graph::Loaded(l) => l.sources().map(|s| s.text.len()).sum(),
            Graph::Single(_) => self.source.len(),
        }
    }
}
/// Turn merged-program spans into stable source-local binding identities.
fn identity(index: &Index<'_>) -> Option<Identity> {
    let (path, _, span) = index.location(index.target?)?;
    Some(Identity {
        path: path.into(),
        span,
    })
}

impl Server {
    /// Prepare only source bindings whose complete reference set is in this document.
    pub(super) fn prepare_rename(&self, params: &Json) -> EditResult<Json> {
        if !self.client_edits.prepare_rename {
            return Err(failed("prepareRename requires client capabilities workspace.workspaceEdit.documentChanges and textDocument.rename.prepareSupport"));
        }
        let (_, source, token, _) = self.rename_selection(params)?;
        Ok(object([
            ("range", navigation::source_range(&source, token)),
            ("placeholder", string(&source[token.start..token.end])),
        ]))
    }
    /// Validate that the selected symbol can be changed without external callers.
    fn rename_selection(&self, params: &Json) -> EditResult<(String, String, Span, Identity)> {
        let uri = document_uri(params).map_err(invalid)?;
        let document = self
            .documents
            .get(uri)
            .ok_or_else(|| invalid("request document is not open".into()))?;
        let cursor = byte_position(
            &document.source,
            field(params, "position").map_err(invalid)?,
        )
        .map_err(invalid)?;
        let tokens = parse::identifier_index(&document.source).map_err(|e| failed(e.message))?;
        let token = tokens
            .identifiers
            .into_iter()
            .find(|s| s.start <= cursor && cursor < s.end)
            .ok_or_else(|| failed("no renameable identifier at cursor"))?;
        let snapshot = Snapshot::load(self, uri, &document.source)?;
        let index = snapshot.index(cursor)?;
        let target = identity(&index).ok_or_else(|| failed("identifier has no source binding"))?;
        if target.path != snapshot.path
            || index.label.is_some()
            || !index.additional_targets.is_empty()
        {
            return Err(failed(
                "rename requires an unambiguous local source binding",
            ));
        }
        // A default parameter label is part of a public call interface. Its callers
        // may live outside the loaded dependency graph, so local edits cannot prove safety.
        let exported_parameter = snapshot.program().functions.iter().any(|function| {
            (function.public || snapshot.program().exports.contains(&function.name))
                && function.params.iter().any(|param| {
                    param.label.is_none()
                        && index.target.is_some_and(|binding| {
                            param.span.start <= binding.start && binding.end <= param.span.end
                        })
                })
        });
        if exported_parameter {
            return Err(failed(
                "exported parameter labels require workspace refactoring",
            ));
        }
        let allowed = if let Some(name) = index.symbol_name() {
            let program = snapshot.program();
            !program.exports.iter().any(|n| n == name)
                && (program
                    .functions
                    .iter()
                    .any(|f| f.name == name && !f.public && name != "main")
                    || program.aliases.iter().any(|a| a.name == name && !a.public))
        } else {
            index
                .query()
                .filter(|q| q.binding.is_some())
                .and_then(|q| check::editor::analyze(snapshot.program(), q).ok())
                .is_some_and(|f| f.value.is_some())
        };
        if !allowed {
            return Err(failed("rename supports local bindings and private functions or aliases; exported declarations and members require workspace refactoring"));
        }
        Ok((uri.into(), document.source.clone(), token, target))
    }
    /// Check all old/new-name identities, so equal types cannot conceal variable capture.
    pub(super) fn rename(&self, params: &Json) -> EditResult<Json> {
        if !self.client_edits.document_changes {
            return Err(failed(
                "rename requires client capability workspace.workspaceEdit.documentChanges",
            ));
        }
        let new = field(params, "newName")
            .map_err(invalid)?
            .string()
            .map_err(invalid)?;
        validate_name(new)?;
        let (uri, source, selected, target) = self.rename_selection(params)?;
        let old = &source[selected.start..selected.end];
        if new == old {
            return Ok(object([("documentChanges", Json::Array(vec![]))]));
        }
        let snapshot = Snapshot::load(self, &uri, &source)?;
        let (edits, references) = reference_inventory(&snapshot, old, new, &target)?;
        let mut changed = source.clone();
        for span in edits.iter().rev() {
            changed.replace_range(span.start..span.end, new);
        }
        self.capacity(&uri, &changed).map_err(failed)?;
        let after = Snapshot::load(self, &uri, &changed)?;
        for (span, binding) in references {
            let moved = shifted(span, &edits, new.len());
            let actual = identity(&after.index(moved.start)?);
            let expected = binding.map(|mut id| {
                if id.path == snapshot.path {
                    id.span = shifted(id.span, &edits, new.len());
                }
                id
            });
            if actual != expected {
                return Err(failed("rename would capture or change a source binding"));
            }
        }
        let edits = edits
            .into_iter()
            .map(|span| {
                object([
                    ("range", navigation::source_range(&source, span)),
                    ("newText", string(new)),
                ])
            })
            .collect();
        Ok(workspace_edit(&uri, self.documents[&uri].version, edits))
    }
}
/// Translate original byte spans through ordered disjoint identifier edits.
fn shifted(span: Span, edits: &[Span], length: usize) -> Span {
    let offset = |position: usize| -> usize {
        edits
            .iter()
            .filter(|s| s.end <= position)
            .fold(position as isize, |p, s| {
                p + length as isize - (s.end - s.start) as isize
            }) as usize
    };
    Span {
        start: offset(span.start),
        end: offset(span.end),
    }
}
/// Keep proposed edits conditional on the client document version we inspected.
pub(super) fn workspace_edit(uri: &str, version: i64, edits: Vec<Json>) -> Json {
    object([(
        "documentChanges",
        Json::Array(vec![object([
            (
                "textDocument",
                object([("uri", string(uri)), ("version", number(version))]),
            ),
            ("edits", Json::Array(edits)),
        ])]),
    )])
}

/// Reject reserved or non-identifier names before inspecting any source references.
fn validate_name(new: &str) -> EditResult<()> {
    if new.len() > 128 {
        return Err(invalid("rename identifier exceeds 128 bytes".into()));
    }
    let identifiers =
        parse::identifier_index(new).map_err(|_| invalid("invalid rename identifier".into()))?;
    if new.is_empty()
        || runtime::reserved_namespace(new)
        || matches!(
            new,
            "_" | "List"
                | "Map"
                | "Option"
                | "Result"
                | "Int"
                | "Float"
                | "Bool"
                | "String"
                | "Unit"
        )
        || new.len() > 128
        || identifiers.identifiers
            != [Span {
                start: 0,
                end: new.len(),
            }]
    {
        return Err(invalid("invalid rename identifier".into()));
    }
    Ok(())
}
/// Collect only exact bound occurrences and the possible capture frontier under a work cap.
fn reference_inventory(
    snapshot: &Snapshot,
    old: &str,
    new: &str,
    target: &Identity,
) -> EditResult<ReferenceInventory> {
    let tokens = parse::identifier_index(&snapshot.source).map_err(|e| failed(e.message))?;
    let candidates: Vec<_> = tokens
        .identifiers
        .into_iter()
        .filter(|s| {
            let word = &snapshot.source[s.start..s.end];
            word == old || word == new
        })
        .collect();
    if candidates.len() > 512 || candidates.len().saturating_mul(snapshot.bytes()) > 32 * MAX_SOURCE
    {
        return Err(failed("rename exceeds bounded reference indexing budget"));
    }
    let mut references = Vec::new();
    let mut edits = Vec::new();
    for span in candidates {
        let index = snapshot.index(span.start)?;
        let binding = identity(&index);
        if binding.as_ref() == Some(target) && &snapshot.source[span.start..span.end] == old {
            if index.label.is_some() || !index.additional_targets.is_empty() {
                return Err(failed("ambiguous rename reference"));
            }
            edits.push(span);
        }
        references.push((span, binding));
    }
    if edits.is_empty() {
        return Err(failed("no rename references found"));
    }
    Ok((edits, references))
}
