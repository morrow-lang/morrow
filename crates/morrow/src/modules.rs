//! Bounded module graph loading, visibility resolution, and source attribution.
use crate::{Diagnostic, Span, Type, ast, parse};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_FILES: usize = 128;
const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_EDITOR_SYMBOL_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug)]
pub struct Error {
    pub message: String,
    pub location: Option<Box<SourceDiagnostic>>,
    span: Option<Span>,
}
/// A diagnostic with byte offsets in its original source, suitable for editor positions.
#[derive(Debug)]
pub struct SourceDiagnostic {
    pub path: PathBuf,
    pub source: String,
    pub diagnostic: Diagnostic,
}
#[derive(Debug)]
pub struct Loaded {
    pub program: ast::Program,
    sources: Vec<Source>,
    pub symbols: Vec<ModuleSymbols>,
    pub(crate) recovery: Option<parse::HoleSite>,
    pub(crate) label_recovery: Option<parse::LabelSite>,
}
/// Visible spellings retained before import aliases are flattened for compilation.
#[derive(Debug)]
pub struct ModuleSymbols {
    pub path: PathBuf,
    /// Visible type spellings mapped to their defining module identities.
    pub types: BTreeMap<String, String>,
    /// Visible function and constructor spellings, independently filtered by value visibility.
    pub values: BTreeMap<String, String>,
}

/// Borrowed source mapping for editor positions without cloning entire documents.
pub struct SourceView<'a> {
    pub path: &'a Path,
    pub text: &'a str,
    pub start: usize,
}

#[derive(Debug)]
struct Source {
    path: PathBuf,
    text: String,
    start: usize,
}
struct Module {
    source: Source,
    name: String,
    syntax: ast::Program,
    dependencies: Vec<usize>,
}
struct Loader<'a> {
    snapshots: HashMap<PathBuf, &'a str>,
    root: PathBuf,
    modules: Vec<Module>,
    seen: BTreeMap<PathBuf, usize>,
    visiting: BTreeSet<PathBuf>,
    bytes: usize,
    editor: bool,
    entry_syntax: Option<(PathBuf, ast::Program, String)>,
    recovery: Option<parse::HoleSite>,
    label_recovery: Option<parse::LabelSite>,
}
type NameMap = BTreeMap<String, String>;
#[derive(Clone, Default)]
struct Names {
    types: NameMap,
    values: NameMap,
}

/// Load an entry and its imports, enforcing visibility before handing syntax to checking.
pub fn load(entry: &Path) -> Result<Loaded, Error> {
    load_with_sources(entry, &HashMap::new())
}

/// Overlay bounded editor snapshots on disk; no source files are created or modified.
pub fn load_with_sources(
    entry: &Path,
    sources: &HashMap<PathBuf, String>,
) -> Result<Loaded, Error> {
    load_sources(entry, sources, false, None, MAX_FILES)
}

/// Load a navigation snapshot with bounded per-file visible names; compiler loading omits these tables.
pub fn load_editor_sources(
    entry: &Path,
    sources: &HashMap<PathBuf, String>,
) -> Result<Loaded, Error> {
    load_sources(entry, sources, true, None, MAX_FILES)
}

/// Load one private token-level hole in the entry; every dependency uses ordinary current parsing.
pub(crate) fn load_member_sources(
    entry: &Path,
    sources: &HashMap<PathBuf, String>,
    cursor: usize,
) -> Result<Loaded, Error> {
    load_sources(
        entry,
        sources,
        false,
        Some(RecoveryRequest::Member(cursor)),
        MAX_FILES,
    )
}

/// Resolve one syntactic argument site with the same current visibility and overlay rules.
pub(crate) fn load_label_sources(
    entry: &Path,
    sources: &HashMap<PathBuf, String>,
    cursor: usize,
) -> Result<Loaded, Error> {
    load_sources(
        entry,
        sources,
        true,
        Some(RecoveryRequest::Label(cursor)),
        MAX_FILES,
    )
}

enum RecoveryRequest {
    Member(usize),
    Label(usize),
}
type EntrySyntax = (
    ast::Program,
    Option<parse::HoleSite>,
    Option<parse::LabelSite>,
);

/// Recover only the requested parser operation; dependencies always parse normally.
fn entry_syntax(text: &str, request: Option<RecoveryRequest>) -> Result<EntrySyntax, Diagnostic> {
    match request {
        Some(RecoveryRequest::Member(cursor)) => {
            parse::recover_member(text, cursor).map(|(program, site)| (program, Some(site), None))
        }
        Some(RecoveryRequest::Label(cursor)) => {
            parse::recover_labels(text, cursor).map(|(program, site)| (program, None, Some(site)))
        }
        None => parse::parse(text).map(|program| (program, None, None)),
    }
}

/// Load one checked documentation graph from a bounded project snapshot cache.
/// Each resolved graph retains 128-file/8 MiB limits; cached snapshots cap at 1024/16 MiB.
pub fn load_documentation_sources(
    entry: &Path,
    sources: &HashMap<PathBuf, String>,
) -> Result<Loaded, Error> {
    load_sources(entry, sources, false, None, 1024)
}

/// Share identical import resolution while making editor-only metadata an explicit bounded opt-in.
fn load_sources(
    entry: &Path,
    sources: &HashMap<PathBuf, String>,
    editor: bool,
    cursor: Option<RecoveryRequest>,
    snapshot_limit: usize,
) -> Result<Loaded, Error> {
    let snapshots = snapshots(sources, snapshot_limit)?;
    let entry = source_identity(entry)?;
    let text = read_source(&entry, &snapshots)?;
    let (syntax, recovery, label_recovery) =
        entry_syntax(&text, cursor).map_err(|e| located(&entry, &text, e))?;
    let name = syntax.module.clone().unwrap_or_else(|| {
        let candidate = entry
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        if valid_module(&candidate).is_ok() {
            candidate
        } else {
            "__entry".into()
        }
    });
    valid_module(&name)?;
    let mut root = entry.parent().unwrap_or_else(|| Path::new(".")).to_owned();
    let levels =
        name.split('.').count() - usize::from(entry.file_stem().is_some_and(|s| s != "mod"));
    for _ in 0..levels {
        if !root.pop() {
            return Err(failure("module name does not match entry path"));
        }
    }
    if syntax.module.is_some() {
        let stem = root.join(name.replace('.', "/"));
        if entry != stem.with_extension("mr") && entry != stem.join("mod.mr") {
            return Err(failure(format!(
                "{}: module declaration {name} does not match file path",
                entry.display()
            )));
        }
    }
    let mut loader = Loader {
        snapshots,
        root,
        modules: Vec::new(),
        seen: BTreeMap::new(),
        visiting: BTreeSet::new(),
        bytes: 0,
        editor,
        entry_syntax: Some((entry.clone(), syntax, text)),
        recovery,
        label_recovery,
    };
    let entry_id = loader.visit(&entry, Some(&name), 0)?;
    loader.resolve(entry_id)
}

impl Loaded {
    /// Borrow every original source and its offset in the flattened syntax.
    pub fn sources(&self) -> impl Iterator<Item = SourceView<'_>> {
        self.sources.iter().map(|source| SourceView {
            path: &source.path,
            text: &source.text,
            start: source.start,
        })
    }

    /// Locate a source span without modifying or allocating source contents.
    pub fn locate_span(&self, span: Span) -> Option<(&Path, Span)> {
        let source = self.sources.iter().find(|source| {
            span.start <= span.end
                && span.start >= source.start
                && span.end <= source.start + source.text.len()
        })?;
        Some((
            &source.path,
            Span {
                start: span.start - source.start,
                end: span.end - source.start,
            },
        ))
    }

    /// Locate a checker diagnostic without losing its original source or byte range.
    pub fn locate(&self, mut diagnostic: Diagnostic) -> Option<SourceDiagnostic> {
        let source = self.sources.iter().find(|s| {
            diagnostic.span.start >= s.start && diagnostic.span.start <= s.start + s.text.len()
        })?;
        diagnostic.span.start -= source.start;
        diagnostic.span.end = diagnostic.span.end.saturating_sub(source.start);
        Some(SourceDiagnostic {
            path: source.path.clone(),
            source: source.text.clone(),
            diagnostic,
        })
    }
    /// Render a checker/emitter diagnostic against the original imported source file.
    pub fn render(&self, error: Diagnostic) -> String {
        let fallback = format!("error: {}", error.message);
        self.locate(error)
            .map_or(fallback, |e| render(&e.path, &e.source, e.diagnostic))
    }
}

/// Format errors consistently with the single-file CLI, using Unicode-aware columns.
fn render(path: &Path, source: &str, error: Diagnostic) -> String {
    let mut offset = error.span.start.min(source.len());
    while !source.is_char_boundary(offset) {
        offset -= 1;
    }
    let before = &source[..offset];
    let line = before.bytes().filter(|b| *b == b'\n').count() + 1;
    let start = before.rfind('\n').map_or(0, |n| n + 1);
    let column = source[start..offset].chars().count() + 1;
    format!(
        "{}:{line}:{column}: error: {}\n  {}",
        path.display(),
        error
            .message
            .strip_prefix("error: ")
            .unwrap_or(&error.message),
        source[start..].lines().next().unwrap_or("")
    )
}
/// Attach the stable CLI marker without duplicating already located diagnostics.
fn failure(message: impl Into<String>) -> Error {
    let message = message.into();
    let message = if message.starts_with("error:") || message.contains(": error:") {
        message
    } else {
        format!("error: {message}")
    };
    Error {
        message,
        location: None,
        span: None,
    }
}

/// Canonicalize existing files and new unsaved files whose parent directory exists.
pub fn source_identity(path: &Path) -> Result<PathBuf, Error> {
    if fs::symlink_metadata(path).is_ok() {
        return path
            .canonicalize()
            .map_err(|e| failure(format!("{}: {e}", path.display())));
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| failure("source path has no file name"))?;
    Ok(parent
        .canonicalize()
        .map_err(|e| failure(format!("{}: {e}", path.display())))?
        .join(name))
}

/// Normalize aliases once, rejecting conflicting snapshots and oversized editor inputs.
fn snapshots(
    sources: &HashMap<PathBuf, String>,
    maximum: usize,
) -> Result<HashMap<PathBuf, &str>, Error> {
    if sources.len() > maximum || sources.values().map(String::len).sum::<usize>() > 2 * MAX_BYTES {
        return Err(failure(format!(
            "source snapshots exceed {maximum}-file or 16 MiB limit"
        )));
    }
    let mut result = HashMap::new();
    for (path, text) in sources {
        if text.len() > 1024 * 1024 {
            return Err(failure("source exceeds 1 MiB limit"));
        }
        let path = source_identity(path)?;
        if result.get(&path).is_some_and(|old| *old != text.as_str()) {
            return Err(failure("conflicting snapshots for the same source file"));
        }
        result.insert(path, text.as_str());
    }
    Ok(result)
}

/// Copy only the selected source from a borrowed snapshot, or read bounded current disk bytes.
fn read_source(path: &Path, sources: &HashMap<PathBuf, &str>) -> Result<String, Error> {
    sources
        .get(path)
        .map_or_else(|| read(path), |text| Ok((*text).to_owned()))
}

fn located(path: &Path, source: &str, diagnostic: Diagnostic) -> Error {
    Error {
        message: render(path, source, diagnostic.clone()),
        span: None,
        location: Some(Box::new(SourceDiagnostic {
            path: path.to_owned(),
            source: source.into(),
            diagnostic,
        })),
    }
}

/// Preserve a deeper location, otherwise attribute loader failures to their originating module.
fn at_source(error: Error, source: &Source) -> Error {
    if error.location.is_some() {
        return error;
    }
    located(
        &source.path,
        &source.text,
        Diagnostic::new(error.span.unwrap_or_default(), error.message),
    )
}

fn at_span(mut error: Error, span: Span) -> Error {
    if error.span.is_none() {
        error.span = Some(span);
    }
    error
}

/// Read one bounded UTF-8 source, never allocating an unbounded file into memory.
fn read(path: &Path) -> Result<String, Error> {
    let file = fs::File::open(path).map_err(|e| failure(format!("{}: {e}", path.display())))?;
    let mut text = String::new();
    file.take(1024 * 1024 + 1)
        .read_to_string(&mut text)
        .map_err(|e| failure(format!("{}: {e}", path.display())))?;
    if text.len() > 1024 * 1024 {
        return Err(failure(format!(
            "{}: source exceeds 1 MiB limit",
            path.display()
        )));
    }
    Ok(text)
}

/// Restrict module paths to dotted identifiers so imports cannot express path traversal.
fn valid_module(name: &str) -> Result<(), Error> {
    if name.split('.').any(|p| {
        p.is_empty()
            || !p
                .chars()
                .enumerate()
                .all(|(i, c)| crate::parse::identifier_char(c, i == 0))
    }) {
        return Err(failure(format!("invalid module name: {name}")));
    }
    Ok(())
}

impl Loader<'_> {
    /// Visit one dependency after recording the current DFS stack to reject cycles.
    fn visit(&mut self, path: &Path, expected: Option<&str>, depth: usize) -> Result<usize, Error> {
        let path = source_identity(path)?;
        if self.visiting.contains(&path) {
            return Err(failure(format!("import cycle at {}", path.display())));
        }
        if let Some(id) = self.seen.get(&path) {
            if expected.is_some_and(|e| e != self.modules[*id].name) {
                return Err(failure("one file imported under inconsistent module names"));
            }
            return Ok(*id);
        }
        if depth >= MAX_FILES || self.modules.len() + self.visiting.len() >= MAX_FILES {
            return Err(failure("module graph exceeds 128-file limit"));
        }
        let text = self.source_text(&path)?;
        let start = self.bytes;
        self.bytes += text.len() + 1;
        if self.bytes > MAX_BYTES {
            return Err(failure("module sources exceed 8 MiB limit"));
        }
        let syntax = self.source_syntax(&path, &text)?;
        let name = syntax
            .module
            .clone()
            .or_else(|| expected.map(str::to_owned))
            .unwrap_or_default();
        valid_module(&name)?;
        if expected.is_some_and(|e| e != name) {
            return Err(located(
                &path,
                &text,
                Diagnostic::new(
                    Span::default(),
                    format!(
                        "declared module {name} does not match imported path {}",
                        expected.unwrap_or("")
                    ),
                ),
            ));
        }
        self.visiting.insert(path.clone());
        let mut dependencies = Vec::new();
        for import in &syntax.imports {
            let imported = self
                .import_path(&import.module)
                .map_err(|e| located(&path, &text, Diagnostic::new(import.span, e.message)))?;
            dependencies.push(
                self.visit(&imported, Some(&import.module), depth + 1)
                    .map_err(|e| {
                        if e.location.is_some() {
                            e
                        } else {
                            located(&path, &text, Diagnostic::new(import.span, e.message))
                        }
                    })?,
            );
        }
        self.visiting.remove(&path);
        let id = self.modules.len();
        self.seen.insert(path.clone(), id);
        self.modules.push(Module {
            source: Source { path, text, start },
            name,
            syntax,
            dependencies,
        });
        Ok(id)
    }

    /// Consume the exact entry bytes paired with cached syntax, or copy only the selected dependency.
    fn source_text(&mut self, path: &Path) -> Result<String, Error> {
        match &mut self.entry_syntax {
            Some((entry, _, text)) if entry == path => Ok(std::mem::take(text)),
            _ => read_source(path, &self.snapshots),
        }
    }

    /// Reuse the entry's proven syntax once; dependencies always parse their current snapshots.
    fn source_syntax(&mut self, path: &Path, text: &str) -> Result<ast::Program, Error> {
        if self
            .entry_syntax
            .as_ref()
            .is_some_and(|(entry, _, _)| entry == path)
        {
            return Ok(self.entry_syntax.take().expect("matching entry syntax").1);
        }
        parse::parse(text).map_err(|e| located(path, text, e))
    }

    /// Resolve module.mr or module/mod.mr within the project, rejecting ambiguity.
    fn import_path(&self, name: &str) -> Result<PathBuf, Error> {
        valid_module(name)?;
        let stem = self.root.join(name.replace('.', "/"));
        let candidates: Vec<_> = [stem.with_extension("mr"), stem.join("mod.mr")]
            .into_iter()
            .filter(|p| {
                p.is_file() || source_identity(p).is_ok_and(|id| self.snapshots.contains_key(&id))
            })
            .collect();
        if candidates.len() != 1 {
            return Err(failure(format!(
                "module {name}: expected exactly one module file; found {}",
                candidates.len()
            )));
        }
        let path = source_identity(&candidates[0])?;
        if !path.starts_with(&self.root) {
            return Err(failure(format!(
                "module {name} resolves outside project root"
            )));
        }
        Ok(path)
    }

    /// Keep the private member operation aligned with ordinary flattened source spans.
    fn relocate_recovery(&mut self, entry: usize) {
        if let Some(site) = &mut self.recovery {
            site.shift(self.modules[entry].source.start);
        }
        if let Some(site) = &mut self.label_recovery {
            site.shift(self.modules[entry].source.start);
        }
    }

    /// Qualify declarations in dependency order, then merge their concrete namespaces.
    fn resolve(mut self, entry: usize) -> Result<Loaded, Error> {
        self.relocate_recovery(entry);
        let mut exports: Vec<Names> = Vec::new();
        let mut program = ast::Program::default();
        let mut sources = Vec::new();
        let mut symbols = Vec::new();
        let (mut symbol_count, mut symbol_bytes) = (0, 0);
        for (id, mut module) in self.modules.into_iter().enumerate() {
            let public = (|| {
                let mut visible = own_names(&module, id == entry)?;
                let own = visible.clone();
                let mut public = exported_names(&module, &own)?;
                let mut imported_prefixes = BTreeSet::new();
                for (import, dependency) in module.syntax.imports.iter().zip(&module.dependencies) {
                    import_names(
                        import,
                        &exports[*dependency],
                        &mut visible,
                        &mut public,
                        &mut imported_prefixes,
                    )
                    .map_err(|e| at_span(e, import.span))?;
                }
                if self.editor {
                    record_symbols(
                        &mut symbols,
                        &mut symbol_count,
                        &mut symbol_bytes,
                        &module.source.path,
                        &visible,
                    )?;
                }
                qualify_docs(&mut module.syntax, &own, module.source.start)?;
                for function in &mut module.syntax.functions {
                    function.name = own.values[&function.name].clone();
                    qualify_function(function, &visible, &imported_prefixes, module.source.start)?;
                    shift(&mut function.span, module.source.start);
                }
                qualify_source_types(&mut module.syntax, &own, &visible, module.source.start)?;
                Ok(public)
            })()
            .map_err(|e| at_source(e, &module.source))?;
            program.docs.extend(module.syntax.docs);
            program.functions.extend(module.syntax.functions);
            program.types.extend(module.syntax.types);
            program.aliases.extend(module.syntax.aliases);
            program.newtypes.extend(module.syntax.newtypes);
            program.traits.extend(module.syntax.traits);
            program
                .implementations
                .extend(module.syntax.implementations);
            exports.push(public);
            sources.push(module.source);
        }
        Ok(Loaded {
            recovery: self.recovery,
            label_recovery: self.label_recovery,
            program,
            sources,
            symbols,
        })
    }
}

/// Qualify documentation by its following declaration's namespace, not its spelling.
fn qualify_docs(program: &mut ast::Program, names: &Names, offset: usize) -> Result<(), Error> {
    let mut roles: Vec<_> = program
        .functions
        .iter()
        .map(|decl| (decl.span.start, true))
        .chain(program.types.iter().map(|decl| (decl.span.start, false)))
        .chain(program.aliases.iter().map(|decl| (decl.span.start, false)))
        .chain(program.newtypes.iter().map(|decl| (decl.span.start, false)))
        .chain(program.traits.iter().map(|decl| (decl.span.start, false)))
        .collect();
    roles.sort_unstable_by_key(|role| role.0);
    for doc in &mut program.docs {
        let position = roles.partition_point(|role| role.0 < doc.span.end);
        let (_, value) = roles
            .get(position)
            .ok_or_else(|| failure("missing documentation declaration"))?;
        let namespace = if *value { &names.values } else { &names.types };
        doc.target = namespace
            .get(&doc.target)
            .ok_or_else(|| failure("unknown documentation target"))?
            .clone();
        shift(&mut doc.span, offset);
    }
    Ok(())
}

/// Bound aliases to 100,000 entries and 8 MiB of name bytes before cloning across the graph.
/// The navigation index's one active visibility-table clone is bounded by this same aggregate cap.
fn record_symbols(
    symbols: &mut Vec<ModuleSymbols>,
    count: &mut usize,
    bytes: &mut usize,
    path: &Path,
    names: &Names,
) -> Result<(), Error> {
    *count = count.saturating_add(names.types.len() + names.values.len());
    if *count > 100_000 {
        return Err(failure("editor symbol index limit exceeded"));
    }
    for (name, target) in names.types.iter().chain(&names.values) {
        *bytes = bytes
            .saturating_add(name.len())
            .saturating_add(target.len());
        if *bytes > MAX_EDITOR_SYMBOL_BYTES {
            return Err(failure("editor symbol byte limit exceeded"));
        }
    }
    symbols.push(ModuleSymbols {
        path: path.to_owned(),
        types: names.types.clone(),
        values: names.values.clone(),
    });
    Ok(())
}

/// Expand exported types to include only their own constructors.
fn exported_names(module: &Module, own: &Names) -> Result<Names, Error> {
    let mut public = Names::default();
    for declaration in module.syntax.traits.iter().filter(|t| t.public) {
        public.types.insert(
            declaration.name.clone(),
            own.types[&declaration.name].clone(),
        );
    }
    for function in module.syntax.functions.iter().filter(|f| f.public) {
        public
            .values
            .insert(function.name.clone(), own.values[&function.name].clone());
    }
    for alias in module.syntax.aliases.iter().filter(|d| d.public) {
        public
            .types
            .insert(alias.name.clone(), own.types[&alias.name].clone());
    }
    for decl in module.syntax.newtypes.iter().filter(|d| d.public) {
        public
            .types
            .insert(decl.name.clone(), own.types[&decl.name].clone());
        export_constructor(&mut public.values, own, &decl.name, &decl.constructor);
    }
    for decl in module.syntax.types.iter().filter(|d| d.public) {
        public
            .types
            .insert(decl.name.clone(), own.types[&decl.name].clone());
        for variant in &decl.variants {
            export_constructor(&mut public.values, own, &decl.name, &variant.name);
        }
    }
    Ok(public)
}

/// Constructors inherit visibility exclusively from their own nominal declaration.
fn export_constructor(public: &mut NameMap, own: &Names, owner: &str, name: &str) {
    public.insert(name.into(), own.values[name].clone());
    public.insert(format!("{owner}.{name}"), own.values[name].clone());
}

/// Qualify source type declarations while keeping distinct owner and constructor identities.
fn qualify_source_types(
    program: &mut ast::Program,
    own: &Names,
    visible: &Names,
    offset: usize,
) -> Result<(), Error> {
    qualify_aliases(&mut program.aliases, own, visible, offset)?;
    for declaration in &mut program.traits {
        declaration.name = own.types[&declaration.name].clone();
        for parent in &mut declaration.parents {
            qualify_trait_bound(parent, visible, offset)?;
        }
        for method in &mut declaration.methods {
            method.function = own.values[&method.function].clone();
            if let Some(default) = &mut method.default {
                *default = own.values[default].clone();
            }
        }
        shift(&mut declaration.span, offset);
    }
    for implementation in &mut program.implementations {
        qualify_trait_bound(&mut implementation.bound, visible, offset)?;
        for bound in &mut implementation.constraints {
            qualify_trait_bound(bound, visible, offset)?;
        }
        for (_, function) in &mut implementation.methods {
            *function = own.values[function].clone();
        }
        shift(&mut implementation.span, offset);
    }
    qualify_declarations(&mut program.types, own, visible, offset)?;
    for decl in &mut program.newtypes {
        decl.name = own.types[&decl.name].clone();
        decl.constructor = own.values[&decl.constructor].clone();
        qualify_type(&mut decl.inner, visible).map_err(|e| at_span(e, decl.inner_span))?;
        shift(&mut decl.span, offset);
        shift(&mut decl.constructor_span, offset);
        shift(&mut decl.inner_span, offset);
        for derive in &mut decl.derives {
            shift(&mut derive.span, offset);
        }
    }
    Ok(())
}

/// Alias targets resolve in their defining module, including private transparent dependencies.
fn qualify_aliases(
    aliases: &mut [ast::TypeAlias],
    own: &Names,
    visible: &Names,
    offset: usize,
) -> Result<(), Error> {
    for alias in aliases {
        alias.name = own.types[&alias.name].clone();
        qualify_type(&mut alias.target, visible).map_err(|e| at_span(e, alias.span))?;
        shift(&mut alias.span, offset);
    }
    Ok(())
}

/// Qualify owned type identities and field annotations with the module's visibility map.
fn qualify_declarations(
    declarations: &mut [ast::TypeDecl],
    own: &Names,
    visible: &Names,
    offset: usize,
) -> Result<(), Error> {
    for decl in declarations {
        decl.name = own.types[&decl.name].clone();
        shift(&mut decl.span, offset);
        for derive in &mut decl.derives {
            shift(&mut derive.span, offset);
        }
        for variant in &mut decl.variants {
            variant.name = own.values[&variant.name].clone();
            shift(&mut variant.span, offset);
            for field in &mut variant.fields {
                qualify_type(&mut field.ty, visible).map_err(|e| at_span(e, field.span))?;
                shift(&mut field.span, offset);
            }
        }
    }
    Ok(())
}

/// Resolve each clause's parameter bindings before its guard and body, retaining group identity.
fn qualify_trait_bound(
    bound: &mut ast::TraitBound,
    visible: &Names,
    offset: usize,
) -> Result<(), Error> {
    bound.name = visible
        .types
        .get(&bound.name)
        .cloned()
        .or_else(|| {
            matches!(
                bound.name.as_str(),
                "Show" | "Eq" | "Ord" | "Clone" | "Json"
            )
            .then(|| bound.name.clone())
        })
        .ok_or_else(|| {
            at_span(
                failure(format!("unknown or private trait '{}'", bound.name)),
                bound.span,
            )
        })?;
    qualify_type(&mut bound.ty, visible).map_err(|e| at_span(e, bound.span))?;
    shift(&mut bound.span, offset);
    Ok(())
}

fn qualify_function(
    function: &mut ast::Function,
    visible: &Names,
    prefixes: &BTreeSet<String>,
    offset: usize,
) -> Result<(), Error> {
    for bound in &mut function.constraints {
        qualify_trait_bound(bound, visible, offset)?;
    }
    let mut bound = BTreeSet::new();
    for param in &mut function.params {
        if let Some(ty) = &mut param.annotation {
            qualify_type(ty, visible).map_err(|e| at_span(e, param.span))?;
        }
        if let Some(label) = &mut param.label {
            shift(&mut label.span, offset);
        }
        pattern(&mut param.pattern, visible, &mut bound, offset)?;
        shift(&mut param.span, offset);
    }
    if let Some(ty) = &mut function.return_type {
        qualify_type(ty, visible).map_err(|e| at_span(e, function.span))?;
    }
    let mut scopes = vec![bound];
    if let Some(guard) = &mut function.guard {
        rewrite(guard, visible, prefixes, &mut scopes, offset)?;
    }
    rewrite(&mut function.body, visible, prefixes, &mut scopes, offset)?;
    function.group_start += offset;
    Ok(())
}

/// Map local function/type/constructor identities into a module-qualified namespace.
fn own_names(module: &Module, entry: bool) -> Result<Names, Error> {
    let mut names = Names::default();
    let mut groups = BTreeMap::new();
    for declaration in &module.syntax.traits {
        register_type(
            module,
            &mut names.types,
            &declaration.name,
            declaration.span,
        )?;
    }
    for function in &module.syntax.functions {
        if groups.get(&function.name) == Some(&function.group_start) {
            continue;
        }
        groups.insert(function.name.clone(), function.group_start);
        if reserved_declaration(&function.name) {
            return Err(failure(format!(
                "{} is reserved for a builtin",
                function.name
            )));
        }
        let qualified = if entry && function.name == "main" {
            "main".into()
        } else {
            format!("{}.{}", module.name, function.name)
        };
        insert(&mut names.values, function.name.clone(), qualified)?;
    }
    for decl in &module.syntax.types {
        register_type(module, &mut names.types, &decl.name, decl.span)?;
        for variant in &decl.variants {
            register_constructor(
                module,
                &mut names.values,
                &decl.name,
                &variant.name,
                variant.span,
            )?;
        }
    }
    for alias in &module.syntax.aliases {
        register_type(module, &mut names.types, &alias.name, alias.span)?;
    }
    for decl in &module.syntax.newtypes {
        register_type(module, &mut names.types, &decl.name, decl.span)?;
        register_constructor(
            module,
            &mut names.values,
            &decl.name,
            &decl.constructor,
            decl.constructor_span,
        )?;
    }
    for namespace in [&mut names.types, &mut names.values] {
        let aliases: Vec<_> = namespace
            .iter()
            .map(|(name, value)| (format!("{}.{name}", module.name), value.clone()))
            .collect();
        for (name, value) in aliases {
            insert(namespace, name, value)?;
        }
    }
    Ok(names)
}

/// Register one type identity, rejecting duplicates only in the type namespace.
fn register_type(
    module: &Module,
    names: &mut NameMap,
    name: &str,
    span: Span,
) -> Result<(), Error> {
    if reserved_declaration(name) {
        return Err(at_span(
            failure(format!("type name {name} is reserved")),
            span,
        ));
    }
    insert(names, name.into(), format!("{}.{name}", module.name)).map_err(|e| at_span(e, span))
}

/// Register constructor values independently of their owner type's spelling.
fn register_constructor(
    module: &Module,
    names: &mut NameMap,
    owner: &str,
    name: &str,
    span: Span,
) -> Result<(), Error> {
    if reserved_declaration(name) {
        return Err(at_span(
            failure(format!("constructor {name} is reserved")),
            span,
        ));
    }
    let qualified = format!("{}.{name}", module.name);
    insert(names, name.into(), qualified.clone()).map_err(|e| at_span(e, span))?;
    insert(names, format!("{owner}.{name}"), qualified).map_err(|e| at_span(e, span))
}

fn reserved_declaration(name: &str) -> bool {
    name == "Set"
        || crate::ffi::reserved_type(name)
        || crate::ffi::is_api(name)
        || crate::check::sets::is_api(name)
        || crate::codec_syntax::is_codec(name)
        || crate::runtime::native_type(name).is_some()
        || crate::runtime::lookup(name).is_some()
        || name == "Process"
        || name.starts_with("Process.")
        || crate::runtime::reserved_namespace(name)
        || matches!(
            name,
            "print"
                | "println"
                | "Int"
                | "Bool"
                | "Float"
                | "String"
                | "Unit"
                | "List"
                | "Map"
                | "Range"
                | "Option"
                | "Result"
                | "Some"
                | "None"
                | "Ok"
                | "Err"
        )
}

/// Reject ambiguous source names rather than allowing import order to choose behavior.
fn insert(names: &mut NameMap, key: String, value: String) -> Result<(), Error> {
    if names.insert(key.clone(), value).is_some() {
        return Err(failure(format!(
            "ambiguous or duplicate declaration/import: {key}"
        )));
    }
    Ok(())
}

/// Expose only exported dependency names, applying selected imports and aliases.
fn import_names(
    import: &ast::Import,
    exported: &Names,
    visible: &mut Names,
    public: &mut Names,
    prefixes: &mut BTreeSet<String>,
) -> Result<(), Error> {
    let prefix = import.alias.as_ref().unwrap_or(&import.module);
    if import.items.is_none() {
        prefixes.insert(prefix.clone());
    }
    if let Some(items) = &import.items {
        let mut selected = BTreeSet::new();
        for item in items.iter().filter(|item| item.as_str() != "*") {
            if !selected.insert(item) {
                return Err(failure(format!("duplicate import selector: {item}")));
            }
            if !exported.types.contains_key(item) && !exported.values.contains_key(item) {
                return Err(failure(format!(
                    "{}: {item} is private or not exported",
                    import.module
                )));
            }
        }
    }
    import_namespace(
        import,
        &exported.types,
        &mut visible.types,
        &mut public.types,
    )?;
    import_namespace(
        import,
        &exported.values,
        &mut visible.values,
        &mut public.values,
    )
}

/// Apply identical import syntax independently to each public namespace.
fn import_namespace(
    import: &ast::Import,
    exported: &NameMap,
    visible: &mut NameMap,
    public: &mut NameMap,
) -> Result<(), Error> {
    let prefix = import.alias.as_ref().unwrap_or(&import.module);
    for (name, value) in exported {
        let selected = import.items.as_ref().is_none_or(|items| {
            items.iter().any(|item| {
                item == "*"
                    || name == item
                    || name
                        .strip_prefix(item)
                        .is_some_and(|tail| tail.starts_with('.'))
            })
        });
        if !selected {
            continue;
        }
        let key = if import.items.is_none() {
            format!("{prefix}.{name}")
        } else {
            name.clone()
        };
        insert(visible, key.clone(), value.clone())?;
        if import.public {
            insert(public, key, value.clone())?;
        }
    }
    Ok(())
}

fn shift(span: &mut Span, offset: usize) {
    span.start += offset;
    span.end += offset;
}
fn local(scopes: &[BTreeSet<String>], name: &str) -> bool {
    scopes
        .iter()
        .rev()
        .any(|s| s.contains(name.split('.').next().unwrap_or(name)))
}

/// Qualify nominal annotations while leaving declared lowercase generic variables intact.
fn qualify_type(ty: &mut Type, names: &Names) -> Result<(), Error> {
    match ty {
        Type::Named(name, args) => {
            if !crate::processes::nominal_name(name) && !crate::supervisors::nominal_name(name) {
                resolve_global(name, &names.types, false)?;
            }
            for arg in args {
                qualify_type(arg, names)?;
            }
        }
        Type::Function(params, result) => {
            for param in params {
                qualify_type(param, names)?;
            }
            qualify_type(result, names)?;
        }
        Type::Union(fields) | Type::Tuple(fields) => {
            for field in fields {
                qualify_type(field, names)?;
            }
        }
        Type::Pid(t) | Type::ChildKey(t) | Type::RootFunction(t) | Type::List(t) | Type::Option(t) => qualify_type(t, names)?,
        Type::ActorFunction(a, b) | Type::Result(a, b) | Type::Map(a, b) => {
            qualify_type(a, names)?;
            qualify_type(b, names)?;
        }
        _ => {}
    }
    Ok(())
}

/// Resolve names without rewriting lexical locals or accepting private qualified calls.
fn resolve_name(
    name: &mut String,
    names: &Names,
    prefixes: &BTreeSet<String>,
    scopes: &[BTreeSet<String>],
) -> Result<(), Error> {
    if local(scopes, name) {
        return Ok(());
    }
    if !names.values.contains_key(name)
        && prefixes.iter().any(|p| name.starts_with(&format!("{p}.")))
    {
        return Err(failure(format!(
            "{name} is private or not exported by its imported module"
        )));
    }
    resolve_global(name, &names.values, true)?;
    Ok(())
}

/// Resolve only visible global identities; merged dependency declarations are not implicit imports.
fn resolve_global(name: &mut String, names: &NameMap, allow_builtin: bool) -> Result<(), Error> {
    if let Some(value) = names.get(name) {
        *name = value.clone();
    } else if (name.contains('.') || name == "main") && !(allow_builtin && builtin_path(name)) {
        return Err(failure(unresolved_global(name, names, allow_builtin)));
    }
    Ok(())
}

/// Describe an unresolved qualified name: builtin modules name their missing member and the
/// nearest existing API; other prefixes keep the import guidance with a nearest visible name.
fn unresolved_global(name: &str, names: &NameMap, allow_builtin: bool) -> String {
    let builtins = crate::check::builtin_api_names();
    if allow_builtin
        && let Some((prefix, member)) = name.rsplit_once('.')
        && builtins.iter().any(|api| {
            api.strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with('.'))
        })
    {
        let members = builtins.iter().copied().filter(|api| {
            api.strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with('.'))
        });
        let suggestion = crate::suggest::nearest_member(prefix, member, members)
            .map_or_else(String::new, |found| format!("; did you mean '{found}'?"));
        return format!("{prefix} has no function '{member}'{suggestion}");
    }
    format!(
        "{name} is private, not exported, or not imported{}",
        crate::suggest::hint(name, names.keys().map(String::as_str))
    )
}

/// Builtin-qualified calls need no source import; arbitrary module prefixes do.
fn builtin_path(name: &str) -> bool {
    if crate::processes::is_api(name) || crate::processes::constructor_path(name) || crate::supervisors::is_api(name) || crate::supervisors::constructor_path(name) {
        return true;
    }
    crate::check::builtin(name).is_some()
        || crate::check::sets::is_api(name)
        || crate::ffi::is_api(name)
        || crate::codec_syntax::is_codec(name)
        || crate::runtime::lookup(name).is_some()
        || crate::runtime::omissions()
            .iter()
            .any(|entry| entry.names.contains(&name))
}

/// Resolve a codec's callee first, so its static target never enters the value namespace.
fn prepare_codec(
    expr: &mut ast::Expr,
    names: &Names,
    scopes: &[BTreeSet<String>],
) -> Result<(), Error> {
    if let ast::ExprKind::TypeTarget(ty) = &mut expr.kind {
        return qualify_type(ty, names);
    }
    let canonical = match &expr.kind {
        ast::ExprKind::Call { name, .. } | ast::ExprKind::Pipe { name, .. }
            if !local(scopes, name) =>
        {
            names.values.get(name).unwrap_or(name).clone()
        }
        ast::ExprKind::GlobalCall { resolved, .. } | ast::ExprKind::GlobalPipe { resolved, .. } => {
            resolved.clone()
        }
        _ => return Ok(()),
    };
    crate::codec_syntax::prepare(expr, &canonical).map_err(|e| at_span(failure(e.message), e.span))
}

/// Rewrite expression identities and relocate byte spans; block-local bindings stay local.
fn rewrite(
    expr: &mut ast::Expr,
    names: &Names,
    prefixes: &BTreeSet<String>,
    scopes: &mut Vec<BTreeSet<String>>,
    offset: usize,
) -> Result<(), Error> {
    let original = expr.span;
    prepare_codec(expr, names, scopes).map_err(|e| at_span(e, original))?;
    shift(&mut expr.span, offset);
    mark_global(&mut expr.kind, names, prefixes, scopes).map_err(|e| at_span(e, original))?;
    match &mut expr.kind {
        ast::ExprKind::ForeignCall { declaration, args } => {
            for abi in declaration
                .params
                .iter_mut()
                .chain([&mut declaration.result])
            {
                if let crate::ffi::AbiType::Pointer(ty) = abi {
                    qualify_type(ty, names)?;
                }
            }
            rewrite_values(args, names, prefixes, scopes, offset)?;
        }
        ast::ExprKind::Name(_) | ast::ExprKind::GlobalName { .. } => {}
        ast::ExprKind::Pipe {
            value, args, label, ..
        }
        | ast::ExprKind::GlobalPipe {
            value, args, label, ..
        } => {
            if let Some(label) = label {
                shift(&mut label.span, offset);
            }
            rewrite(value, names, prefixes, scopes, offset)?;
            rewrite_arguments(args, names, prefixes, scopes, offset)?;
        }
        ast::ExprKind::Lambda { params, body } => {
            rewrite_lambda(params, body, names, prefixes, scopes, offset)?
        }
        ast::ExprKind::Apply { callee, args } => {
            rewrite(callee, names, prefixes, scopes, offset)?;
            rewrite_arguments(args, names, prefixes, scopes, offset)?;
        }
        ast::ExprKind::Interpolate(parts) | ast::ExprKind::MultilineString(parts) => {
            rewrite_string(parts, names, prefixes, scopes, offset)?
        }
        ast::ExprKind::Call { args, .. } | ast::ExprKind::GlobalCall { args, .. } => {
            rewrite_arguments(args, names, prefixes, scopes, offset)?;
        }
        ast::ExprKind::Tuple(values) | ast::ExprKind::List(values) => {
            rewrite_values(values, names, prefixes, scopes, offset)?;
        }
        ast::ExprKind::Try(value)
        | ast::ExprKind::Return(value)
        | ast::ExprKind::Defer(value)
        | ast::ExprKind::Unary { value, .. }
        | ast::ExprKind::Field { value, .. } => rewrite(value, names, prefixes, scopes, offset)?,
        kind @ (ast::ExprKind::Binary { .. }
        | ast::ExprKind::Range { .. }
        | ast::ExprKind::If { .. }
        | ast::ExprKind::PostfixIf { .. }
        | ast::ExprKind::ConditionMatch(_)) => {
            rewrite_control(kind, names, prefixes, scopes, offset)?
        }
        ast::ExprKind::Block(stmts) => rewrite_block(stmts, names, prefixes, scopes, offset)?,
        ast::ExprKind::Match { value, arms } => {
            rewrite_match(value, arms, names, prefixes, scopes, offset)?
        }
        ast::ExprKind::Map(pairs) => rewrite_pairs(pairs, names, prefixes, scopes, offset)?,
        ast::ExprKind::RecordUpdate { value, fields } => {
            rewrite_update(value, fields, names, prefixes, scopes, offset)?
        }
        kind @ (ast::ExprKind::Receive { .. }
        | ast::ExprKind::For { .. }
        | ast::ExprKind::With { .. }) => {
            rewrite_binding_flow(kind, names, prefixes, scopes, offset)?
        }
        _ => {}
    }
    Ok(())
}

/// Dispatch expressions whose bindings have success-only or per-iteration scope.
fn rewrite_binding_flow(
    kind: &mut ast::ExprKind,
    names: &Names,
    prefixes: &BTreeSet<String>,
    scopes: &mut Vec<BTreeSet<String>>,
    offset: usize,
) -> Result<(), Error> {
    match kind {
        ast::ExprKind::Receive { .. } => rewrite_receive(kind, names, prefixes, scopes, offset),
        ast::ExprKind::For {
            pattern,
            iterable,
            body,
        } => rewrite_for(pattern, iterable, body, names, prefixes, scopes, offset),
        ast::ExprKind::With {
            bindings,
            body,
            arms,
        } => rewrite_with(bindings, body, arms, names, prefixes, scopes, offset),
        _ => Ok(()),
    }
}

/// Loop iterable names resolve before its per-iteration pattern bindings become visible.
fn rewrite_for(
    binding: &mut ast::Pattern,
    iterable: &mut ast::Expr,
    body: &mut ast::Expr,
    names: &Names,
    prefixes: &BTreeSet<String>,
    scopes: &mut Vec<BTreeSet<String>>,
    offset: usize,
) -> Result<(), Error> {
    rewrite(iterable, names, prefixes, scopes, offset)?;
    scopes.push(BTreeSet::new());
    pattern(binding, names, scopes.last_mut().unwrap(), offset)?;
    let result = rewrite(body, names, prefixes, scopes, offset);
    scopes.pop();
    result
}

/// With success scopes accumulate sequentially; error arms see only the outer scope.
fn rewrite_with(
    bindings: &mut [ast::WithBinding],
    body: &mut ast::Expr,
    arms: &mut Option<Vec<ast::MatchArm>>,
    names: &Names,
    prefixes: &BTreeSet<String>,
    scopes: &mut Vec<BTreeSet<String>>,
    offset: usize,
) -> Result<(), Error> {
    scopes.push(BTreeSet::new());
    for binding in bindings {
        rewrite(&mut binding.value, names, prefixes, scopes, offset)?;
        pattern(
            &mut binding.pattern,
            names,
            scopes.last_mut().unwrap(),
            offset,
        )?;
        shift(&mut binding.span, offset);
    }
    rewrite(body, names, prefixes, scopes, offset)?;
    scopes.pop();
    if let Some(arms) = arms {
        for arm in arms {
            rewrite_arm(arm, names, prefixes, scopes, offset)?;
        }
    }
    Ok(())
}

/// Resolve conditions and branch expressions without exposing branch-local bindings.
fn rewrite_control(
    kind: &mut ast::ExprKind,
    names: &Names,
    prefixes: &BTreeSet<String>,
    scopes: &mut Vec<BTreeSet<String>>,
    offset: usize,
) -> Result<(), Error> {
    match kind {
        ast::ExprKind::Binary { left, right, .. }
        | ast::ExprKind::Range {
            start: left,
            end: right,
            ..
        } => {
            rewrite(left, names, prefixes, scopes, offset)?;
            rewrite(right, names, prefixes, scopes, offset)?;
        }
        ast::ExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            rewrite(condition, names, prefixes, scopes, offset)?;
            rewrite(then_branch, names, prefixes, scopes, offset)?;
            if let Some(value) = else_branch {
                rewrite(value, names, prefixes, scopes, offset)?;
            }
        }
        ast::ExprKind::PostfixIf { value, condition } => {
            rewrite(condition, names, prefixes, scopes, offset)?;
            rewrite(value, names, prefixes, scopes, offset)?;
        }
        ast::ExprKind::ConditionMatch(arms) => {
            for arm in arms {
                shift(&mut arm.span, offset);
                if let Some(condition) = &mut arm.condition {
                    rewrite(condition, names, prefixes, scopes, offset)?;
                }
                rewrite(&mut arm.body, names, prefixes, scopes, offset)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Resolve embedded expressions without treating literal text as module names.
fn rewrite_string(
    parts: &mut [ast::StringPart],
    names: &Names,
    prefixes: &BTreeSet<String>,
    scopes: &mut Vec<BTreeSet<String>>,
    offset: usize,
) -> Result<(), Error> {
    for part in parts {
        if let ast::StringPart::Value(value) = part {
            rewrite(value, names, prefixes, scopes, offset)?;
        }
    }
    Ok(())
}

/// Qualify map keys and values in their original evaluation order.
fn rewrite_pairs(
    pairs: &mut [(ast::Expr, ast::Expr)],
    names: &Names,
    prefixes: &BTreeSet<String>,
    scopes: &mut Vec<BTreeSet<String>>,
    offset: usize,
) -> Result<(), Error> {
    for (key, value) in pairs {
        rewrite(key, names, prefixes, scopes, offset)?;
        rewrite(value, names, prefixes, scopes, offset)?;
    }
    Ok(())
}

/// Resolve update expressions while leaving record field labels unqualified.
fn rewrite_update(
    value: &mut ast::Expr,
    fields: &mut [ast::RecordField],
    names: &Names,
    prefixes: &BTreeSet<String>,
    scopes: &mut Vec<BTreeSet<String>>,
    offset: usize,
) -> Result<(), Error> {
    rewrite(value, names, prefixes, scopes, offset)?;
    for field in fields {
        shift(&mut field.span, offset);
        rewrite(&mut field.value, names, prefixes, scopes, offset)?;
    }
    Ok(())
}

/// Rewrite ordered expression children without changing their lexical scope.
fn rewrite_arguments(
    args: &mut [ast::Argument],
    names: &Names,
    prefixes: &BTreeSet<String>,
    scopes: &mut Vec<BTreeSet<String>>,
    offset: usize,
) -> Result<(), Error> {
    for arg in args {
        shift(&mut arg.span, offset);
        if let Some(label) = &mut arg.label {
            shift(&mut label.span, offset);
        }
        rewrite(&mut arg.value, names, prefixes, scopes, offset)?;
    }
    Ok(())
}

/// Rewrite ordered collection elements.
fn rewrite_values(
    values: &mut [ast::Expr],
    names: &Names,
    prefixes: &BTreeSet<String>,
    scopes: &mut Vec<BTreeSet<String>>,
    offset: usize,
) -> Result<(), Error> {
    for value in values {
        rewrite(value, names, prefixes, scopes, offset)?;
    }
    Ok(())
}

/// Keep pattern bindings local to one arm while relocating its guard and body spans.
fn rewrite_arm(
    arm: &mut ast::MatchArm,
    names: &Names,
    prefixes: &BTreeSet<String>,
    scopes: &mut Vec<BTreeSet<String>>,
    offset: usize,
) -> Result<(), Error> {
    scopes.push(BTreeSet::new());
    pattern(&mut arm.pattern, names, scopes.last_mut().unwrap(), offset)?;
    if let Some(guard) = &mut arm.guard {
        rewrite(guard, names, prefixes, scopes, offset)?;
    }
    rewrite(&mut arm.body, names, prefixes, scopes, offset)?;
    shift(&mut arm.span, offset);
    scopes.pop();
    Ok(())
}

/// Resolve callback annotations before introducing its parameter scope for the body.
fn rewrite_lambda(
    params: &mut [ast::LambdaParam],
    body: &mut ast::Expr,
    names: &Names,
    prefixes: &BTreeSet<String>,
    scopes: &mut Vec<BTreeSet<String>>,
    offset: usize,
) -> Result<(), Error> {
    let mut bound = BTreeSet::new();
    for param in params {
        if let Some(ty) = &mut param.annotation {
            qualify_type(ty, names).map_err(|e| at_span(e, param.span))?;
        }
        shift(&mut param.span, offset);
        bound.insert(param.name.clone());
    }
    scopes.push(bound);
    let result = rewrite(body, names, prefixes, scopes, offset);
    scopes.pop();
    result
}

/// Scope match bindings independently while preserving each guard and arm location.
fn rewrite_match(
    value: &mut ast::Expr,
    arms: &mut [ast::MatchArm],
    names: &Names,
    prefixes: &BTreeSet<String>,
    scopes: &mut Vec<BTreeSet<String>>,
    offset: usize,
) -> Result<(), Error> {
    rewrite(value, names, prefixes, scopes, offset)?;
    for arm in arms {
        rewrite_arm(arm, names, prefixes, scopes, offset)?;
    }
    Ok(())
}

/// Preserve lexical block scopes while resolving annotations and binding initializers.
fn rewrite_block(
    stmts: &mut [ast::Stmt],
    names: &Names,
    prefixes: &BTreeSet<String>,
    scopes: &mut Vec<BTreeSet<String>>,
    offset: usize,
) -> Result<(), Error> {
    scopes.push(BTreeSet::new());
    for stmt in stmts {
        match stmt {
            ast::Stmt::LetElse { .. } => rewrite_let_else(stmt, names, prefixes, scopes, offset)?,
            ast::Stmt::Let {
                name,
                annotation,
                value,
                span,
            } => {
                rewrite(value, names, prefixes, scopes, offset)?;
                if let Some(ty) = annotation {
                    qualify_type(ty, names).map_err(|e| at_span(e, *span))?;
                }
                shift(span, offset);
                scopes.last_mut().unwrap().insert(name.clone());
            }
            ast::Stmt::LetPattern {
                pattern: binding,
                annotation,
                value,
                span,
            } => {
                rewrite(value, names, prefixes, scopes, offset)?;
                if let Some(ty) = annotation {
                    qualify_type(ty, names).map_err(|e| at_span(e, *span))?;
                }
                pattern(binding, names, scopes.last_mut().unwrap(), offset)?;
                shift(span, offset);
            }
            ast::Stmt::Expr(value) => rewrite(value, names, prefixes, scopes, offset)?,
        }
    }
    scopes.pop();

    Ok(())
}

/// The failure branch sees the outer scope; successful bindings begin after its initializer.
fn rewrite_let_else(
    stmt: &mut ast::Stmt,
    names: &Names,
    prefixes: &BTreeSet<String>,
    scopes: &mut Vec<BTreeSet<String>>,
    offset: usize,
) -> Result<(), Error> {
    if let ast::Stmt::LetElse {
        pattern: binding,
        annotation,
        value,
        else_branch,
        span,
    } = stmt
    {
        rewrite(value, names, prefixes, scopes, offset)?;
        rewrite(else_branch, names, prefixes, scopes, offset)?;
        if let Some(ty) = annotation {
            qualify_type(ty, names).map_err(|e| at_span(e, *span))?;
        }
        pattern(binding, names, scopes.last_mut().unwrap(), offset)?;
        shift(span, offset);
    }
    Ok(())
}

/// Qualify constructor patterns and gather all nested lexical payload bindings.
fn pattern(
    pattern: &mut ast::Pattern,
    names: &Names,
    bound: &mut BTreeSet<String>,
    offset: usize,
) -> Result<(), Error> {
    let original = pattern.span;
    shift(&mut pattern.span, offset);
    match &mut pattern.kind {
        ast::PatternKind::Typed {
            pattern: inner,
            annotation,
        } => {
            qualify_type(annotation, names).map_err(|e| at_span(e, original))?;
            pattern_binding(inner, names, bound, offset)?;
        }
        ast::PatternKind::List { prefix, rest } => {
            for field in prefix {
                pattern_binding(field, names, bound, offset)?;
            }
            if let Some(rest) = rest {
                pattern_binding(rest, names, bound, offset)?;
            }
        }
        ast::PatternKind::TupleRest { prefix, rest } => {
            for field in prefix {
                pattern_binding(field, names, bound, offset)?;
            }
            pattern_binding(rest, names, bound, offset)?;
        }
        ast::PatternKind::Tuple(fields) => {
            for field in fields {
                pattern_binding(field, names, bound, offset)?;
            }
        }
        ast::PatternKind::Bind(name) => {
            bound.insert(name.clone());
        }
        ast::PatternKind::Constructor {
            binding: Some(name),
            ..
        } => {
            bound.insert(name.clone());
        }
        ast::PatternKind::NamedConstructor { name, fields } => {
            resolve_global(
                name,
                &names.values,
                crate::processes::constructor_path(name),
            )
            .map_err(|e| at_span(e, original))?;
            for field in fields {
                pattern_binding(field, names, bound, offset)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn pattern_binding(
    value: &mut ast::Pattern,
    names: &Names,
    bound: &mut BTreeSet<String>,
    offset: usize,
) -> Result<(), Error> {
    pattern(value, names, bound, offset)
}

/// Mark a proven global before canonical qualification can collide with a lexical root.
fn mark_global(
    kind: &mut ast::ExprKind,
    names: &Names,
    prefixes: &BTreeSet<String>,
    scopes: &[BTreeSet<String>],
) -> Result<(), Error> {
    // A visible constant can own record fields; only its visible declaration
    // prefix is qualified, so a field cannot bypass module export filtering.
    if let ast::ExprKind::Name(name) = kind
        && !local(scopes, name)
        && !names.values.contains_key(name)
    {
        for (offset, _) in name.match_indices('.').rev() {
            if let Some(root) = names.values.get(&name[..offset]) {
                let resolved = format!("{root}{}", &name[offset..]);
                *kind = ast::ExprKind::GlobalName {
                    name: name.clone(),
                    resolved,
                };
                return Ok(());
            }
        }
    }
    let name = match kind {
        ast::ExprKind::Name(name)
        | ast::ExprKind::Call { name, .. }
        | ast::ExprKind::Pipe { name, .. } => name,
        _ => return Ok(()),
    };
    if local(scopes, name) {
        return Ok(());
    }
    let mut resolved = name.clone();
    resolve_name(&mut resolved, names, prefixes, scopes)?;
    if !names.values.contains_key(name) {
        return Ok(());
    }
    *kind = match std::mem::replace(kind, ast::ExprKind::Unit) {
        ast::ExprKind::Name(name) => ast::ExprKind::GlobalName { name, resolved },
        ast::ExprKind::Call { name, args } => ast::ExprKind::GlobalCall {
            name,
            resolved,
            args,
        },
        ast::ExprKind::Pipe {
            value,
            name,
            args,
            position,
            label,
        } => ast::ExprKind::GlobalPipe {
            value,
            name,
            resolved,
            args,
            position,
            label,
        },
        _ => unreachable!("only named references reach global marking"),
    };
    Ok(())
}

/// Rewrite each receive arm in its own binder scope; timeout expressions use the outer scope.
fn rewrite_receive(
    kind: &mut ast::ExprKind,
    names: &Names,
    prefixes: &BTreeSet<String>,
    scopes: &mut Vec<BTreeSet<String>>,
    offset: usize,
) -> Result<(), Error> {
    if let ast::ExprKind::Receive { arms, timeout, .. } = kind {
        for arm in arms {
            rewrite_arm(arm, names, prefixes, scopes, offset)?;
        }
        if let Some((duration, body)) = timeout {
            rewrite(duration, names, prefixes, scopes, offset)?;
            rewrite(body, names, prefixes, scopes, offset)?;
        }
    }
    Ok(())
}
