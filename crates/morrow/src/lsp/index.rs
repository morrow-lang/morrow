//! Source identities and cursor-local lexical scopes, independent of generated IR.
use super::{Span, ast, modules, parse};
use std::{collections::BTreeMap, path::Path};

#[path = "index/labels.rs"]
mod labels;
pub(super) use labels::LabelSelection;

type Bindings = BTreeMap<String, Span>;
#[derive(Clone)]
pub(super) struct Symbol {
    pub span: Span,
    pub kind: i64,
}
pub(super) struct Source<'a> {
    pub path: &'a Path,
    pub text: &'a str,
    pub start: usize,
    pub tokens: parse::IdentifierIndex,
    annotations: Vec<Span>,
    selectors: Vec<Span>,
}
pub(super) struct Index<'a> {
    pub sources: Vec<Source<'a>>,
    visible_types: BTreeMap<String, String>,
    visible_values: BTreeMap<String, String>,
    pub globals: BTreeMap<String, Symbol>,
    types: BTreeMap<String, Symbol>,
    interfaces: BTreeMap<String, Vec<Option<ast::ArgumentLabel>>>,
    pub label: Option<LabelSelection>,
    completion_site: Option<parse::LabelSite>,
    pub call_labels: Option<Vec<String>>,
    type_context: bool,
    selector_context: bool,
    pub locals: Bindings,
    pub target: Option<Span>,
    pub additional_targets: Vec<Span>,
    cursor: usize,
    token: Option<Span>,
    work: usize,
    blocked: bool,
}

impl<'a> Index<'a> {
    /// Build from the exact source graph already resolved with all current editor overlays.
    pub fn loaded(loaded: &'a modules::Loaded, path: &Path, cursor: usize) -> Option<Self> {
        Self::loaded_labels(loaded, path, cursor, None)
    }
    /// Use recovered source roles only for the selected entry, retaining normal dependency indices.
    pub fn loaded_labels(
        loaded: &'a modules::Loaded,
        path: &Path,
        cursor: usize,
        site: Option<parse::LabelSite>,
    ) -> Option<Self> {
        let source = loaded.sources().find(|s| s.path == path)?;
        let offset = source.start;
        let sources = loaded
            .sources()
            .map(|s| {
                if site.is_some() && s.path == path {
                    Self::label_source(s.path, s.text, s.start)
                } else {
                    Self::source(s.path, s.text, s.start)
                }
            })
            .collect::<Option<Vec<_>>>()?;
        let symbols = loaded.symbols.iter().find(|s| s.path == path)?;
        Self::build(
            &loaded.program,
            sources,
            symbols.types.clone(),
            symbols.values.clone(),
            offset + cursor,
            site,
        )
    }
    /// Non-file documents use the same lexical rules without inventing filesystem imports.
    pub fn single(
        program: &ast::Program,
        source: &'a str,
        path: &'a Path,
        cursor: usize,
    ) -> Option<Self> {
        Self::single_labels(program, source, path, cursor, None)
    }
    /// Label recovery uses lexical source roles only after a parser-proven call position.
    pub fn single_labels(
        program: &ast::Program,
        source: &'a str,
        path: &'a Path,
        cursor: usize,
        site: Option<parse::LabelSite>,
    ) -> Option<Self> {
        let sources = vec![if site.is_some() {
            Self::label_source(path, source, 0)?
        } else {
            Self::source(path, source, 0)?
        }];
        let mut types = BTreeMap::new();
        let mut values = BTreeMap::new();
        for function in program
            .functions
            .iter()
            .filter(|f| f.span != Span::default())
        {
            if !function.name.starts_with('$') {
                values.insert(function.name.clone(), function.name.clone());
            }
        }
        for declaration in &program.traits {
            types.insert(declaration.name.clone(), declaration.name.clone());
        }
        for alias in &program.aliases {
            types.insert(alias.name.clone(), alias.name.clone());
        }
        for decl in &program.newtypes {
            types.insert(decl.name.clone(), decl.name.clone());
            values.insert(decl.constructor.clone(), decl.constructor.clone());
            values.insert(
                format!("{}.{}", decl.name, decl.constructor),
                decl.constructor.clone(),
            );
        }
        for ty in &program.types {
            types.insert(ty.name.clone(), ty.name.clone());
            for variant in &ty.variants {
                values.insert(variant.name.clone(), variant.name.clone());
                let leaf = variant.name.rsplit('.').next().unwrap_or(&variant.name);
                values.insert(format!("{}.{leaf}", ty.name), variant.name.clone());
            }
        }
        Self::build(program, sources, types, values, cursor, site)
    }
    /// Keep lexical indexing bounded independently of parser layout and checker work.
    fn source(path: &'a Path, text: &'a str, start: usize) -> Option<Source<'a>> {
        let roles = parse::source_roles(text).ok()?;
        Some(Source {
            path,
            text,
            start,
            tokens: parse::identifier_index(text).ok()?,
            annotations: roles.types,
            selectors: roles.selectors,
        })
    }
    /// Bound token inventories and visible namespaces before constructing index tables.
    fn source_budget(sources: &[Source<'_>], types: usize, values: usize) -> Option<()> {
        let count: usize = sources
            .iter()
            .map(|s| s.tokens.identifiers.len() + s.tokens.numbers.len())
            .sum();
        (count <= 100_000 && types.saturating_add(values) <= 100_000).then_some(())
    }
    /// Collect declaration identities once; inspect only scopes containing this request's cursor.
    fn build(
        program: &ast::Program,
        sources: Vec<Source<'a>>,
        visible_types: BTreeMap<String, String>,
        visible_values: BTreeMap<String, String>,
        cursor: usize,
        completion_site: Option<parse::LabelSite>,
    ) -> Option<Self> {
        Self::source_budget(&sources, visible_types.len(), visible_values.len())?;
        let token = cursor_token(&sources, cursor);
        let type_context = type_context(&sources, token);
        let selector_context = selector_context(&sources, token);
        let mut index = Self {
            sources,
            visible_types,
            visible_values,
            globals: BTreeMap::new(),
            types: BTreeMap::new(),
            interfaces: BTreeMap::new(),
            label: None,
            completion_site,
            call_labels: None,
            type_context,
            selector_context,
            locals: Bindings::new(),
            target: None,
            additional_targets: Vec::new(),
            cursor,
            token,
            work: 0,
            blocked: false,
        };
        index.declarations(program);
        index.interfaces(program)?;
        let extended = index.extended_function(program);
        for function in program
            .functions
            .iter()
            .filter(|f| f.span != Span::default())
        {
            if !index.contains(function.span) && extended != Some(function.span.start) {
                continue;
            }
            let mut locals = Bindings::new();
            for param in &function.params {
                if let Some(label) = &param.label {
                    index.argument_label(label, Some(&function.name));
                }
                index.pattern(&param.pattern, &mut locals, 0)?;
            }
            if let Some(guard) = &function.guard {
                index.expression(guard, &locals, 0)?;
            }
            if extended == Some(function.span.start) {
                if let ast::ExprKind::Block(statements) = &function.body.kind {
                    index.capture(&locals)?;
                    index.block(statements, &locals, 0)?;
                }
            } else {
                index.expression(&function.body, &locals, 0)?;
            }
        }
        index.global_reference();
        Some(index)
    }
    /// An indented trailing blank line remains inside its last block until another declaration begins.
    fn extended_function(&self, program: &ast::Program) -> Option<usize> {
        let source = self
            .sources
            .iter()
            .find(|s| self.cursor >= s.start && self.cursor <= s.start + s.text.len())?;
        let before = source.text.get(..self.cursor - source.start)?;
        let line = before.rsplit('\n').next()?;
        if line.is_empty() || !line.bytes().all(|b| b == b' ') {
            return None;
        }
        let function = program
            .functions
            .iter()
            .filter(|f| {
                f.span != Span::default()
                    && f.span.start >= source.start
                    && f.span.start <= self.cursor
            })
            .max_by_key(|f| f.span.start)?;
        if self.cursor <= function.span.end
            || !matches!(function.body.kind, ast::ExprKind::Block(_))
            || !self.extends(function.span, false)
        {
            return None;
        }
        let separated = program
            .types
            .iter()
            .map(|d| d.span.start)
            .chain(program.imports.iter().map(|d| d.span.start))
            .chain(program.aliases.iter().map(|d| d.span.start))
            .chain(program.newtypes.iter().map(|d| d.span.start))
            .chain(program.traits.iter().map(|d| d.span.start))
            .chain(program.implementations.iter().map(|d| d.span.start))
            .any(|start| start > function.span.start && start <= self.cursor);
        (!separated).then_some(function.span.start)
    }

    /// Source declarations retain their first clause and exact selected identifier range.
    fn declarations(&mut self, program: &ast::Program) {
        self.newtype_declarations(program);
        for declaration in &program.traits {
            if let Some(span) = self.identifier(declaration.span, 1) {
                self.types
                    .insert(declaration.name.clone(), Symbol { span, kind: 8 });
                if self.contains(span) {
                    self.target = Some(span);
                }
            }
        }
        for function in program
            .functions
            .iter()
            .filter(|f| f.span != Span::default())
        {
            if let Some(span) = self.identifier(function.span, 1) {
                self.globals.entry(function.name.clone()).or_insert(Symbol {
                    span,
                    kind: if function.syntax == ast::FunctionSyntax::Constant {
                        21
                    } else {
                        3
                    },
                });
                if self.contains(span) {
                    self.target = self.globals.get(&function.name).map(|s| s.span);
                }
            }
        }
        for alias in &program.aliases {
            if let Some(span) = self.identifier(alias.span, 1) {
                self.types
                    .insert(alias.name.clone(), Symbol { span, kind: 7 });
                if self.contains(span) {
                    self.target = Some(span);
                }
            }
        }
        for ty in &program.types {
            if let Some(span) = self.identifier(ty.span, 1) {
                self.types.insert(ty.name.clone(), Symbol { span, kind: 7 });
                if self.contains(span) {
                    self.target = Some(span);
                }
            }
            for variant in &ty.variants {
                for field in &variant.fields {
                    if field.name.is_some()
                        && let Some(span) = self.identifier(field.span, 0)
                        && self.contains(span)
                    {
                        self.target = Some(span);
                    }
                }
                if let Some(span) = self.identifier(variant.span, usize::from(ty.record)) {
                    self.globals
                        .insert(variant.name.clone(), Symbol { span, kind: 4 });
                    if self.contains(span) {
                        self.target = Some(span);
                    }
                }
            }
        }
    }
    /// A newtype's type spelling and value constructor retain distinct exact declaration spans.
    fn newtype_declarations(&mut self, program: &ast::Program) {
        for decl in &program.newtypes {
            if let Some(span) = self.identifier(decl.span, 1) {
                self.types
                    .insert(decl.name.clone(), Symbol { span, kind: 7 });
                if self.contains(span) {
                    self.target = Some(span);
                }
            }
            let span = decl.constructor_span;
            self.globals
                .insert(decl.constructor.clone(), Symbol { span, kind: 4 });
            if self.contains(span) {
                self.target = Some(span);
            }
        }
    }

    /// Complete unresolved global occurrences, including annotations and import selectors.
    fn global_reference(&mut self) {
        if self.target.is_some() || self.blocked {
            return;
        }
        let Some(token) = self.token else {
            return;
        };
        let Some(word) = self.word(token) else {
            return;
        };
        let Some(text) = self.text(word).map(str::to_owned) else {
            return;
        };
        if self.selector_targets(&text) {
            return;
        }
        if self.type_context {
            self.target = self
                .visible_types
                .get(&text)
                .and_then(|name| self.types.get(name))
                .map(|s| s.span);
            return;
        }
        let root = text.split('.').next().unwrap_or(&text);
        if let Some(binding) = self.locals.get(root) {
            if word == token {
                self.target = Some(*binding);
            }
            return;
        }
        if token.end != word.end {
            return;
        }
        if let Some(name) = self.visible_values.get(&text) {
            self.target = self.globals.get(name).map(|s| s.span);
        }
    }

    /// Resolve source-qualified spellings for annotations and pipeline targets without substring search.
    fn word(&self, token: Span) -> Option<Span> {
        let (_, source, local) = self.location(token)?;
        let index = self
            .sources
            .iter()
            .find(|s| token.start >= s.start && token.end <= s.start + s.text.len())?;
        let mut first = index
            .tokens
            .identifiers
            .partition_point(|s| s.start < local.start);
        if index.tokens.identifiers.get(first) != Some(&local) {
            return None;
        }
        let mut last = first;
        for _ in 0..128 {
            if first == 0 {
                break;
            }
            let previous = index.tokens.identifiers[first - 1];
            let current = index.tokens.identifiers[first];
            if source.get(previous.end..current.start) != Some(".") {
                break;
            }
            first -= 1;
        }
        for _ in 0..128 {
            let Some(next) = index.tokens.identifiers.get(last + 1) else {
                break;
            };
            let current = index.tokens.identifiers[last];
            if source.get(current.end..next.start) != Some(".") {
                break;
            }
            last += 1;
        }
        Some(Span {
            start: index.tokens.identifiers[first].start + index.start,
            end: index.tokens.identifiers[last].end + index.start,
        })
    }
    /// Offer only symbols from the cursor's namespace; selectors include both, type first.
    pub(super) fn completions(&self) -> impl Iterator<Item = (&str, i64)> {
        let types = self
            .visible_types
            .iter()
            .filter(|_| self.type_context || self.selector_context)
            .filter_map(|(name, target)| {
                self.types
                    .get(target)
                    .map(|symbol| (name.as_str(), symbol.kind))
            });
        let values = self
            .visible_values
            .iter()
            .filter(|_| !self.type_context)
            .filter_map(|(name, target)| {
                self.globals
                    .get(target)
                    .map(|symbol| (name.as_str(), symbol.kind))
            });
        types.chain(values)
    }

    /// Type annotations never inherit a lexical value's visibility or shadowing.
    pub(super) fn in_type_context(&self) -> bool {
        self.type_context
    }

    /// An import selector may deliberately identify both namespaces; preserve type-first order.
    fn selector_targets(&mut self, text: &str) -> bool {
        if !self.selector_context {
            return false;
        }
        let ty = self
            .visible_types
            .get(text)
            .and_then(|name| self.types.get(name))
            .map(|s| s.span);
        let value = self
            .visible_values
            .get(text)
            .and_then(|name| self.globals.get(name))
            .map(|s| s.span);
        self.target = ty.or(value);
        if let Some(value) = value.filter(|value| Some(*value) != self.target) {
            self.additional_targets.push(value);
        }
        true
    }

    /// Return a selected declaration's fully qualified resolver identity, never just its leaf.
    pub(super) fn symbol_name(&self) -> Option<&str> {
        let target = self.target?;
        self.globals
            .iter()
            .chain(self.types.iter())
            .find(|(_, symbol)| symbol.span == target)
            .map(|(name, _)| name.as_str())
    }

    /// Select exact current-source identities for optional finalized checker metadata.
    pub(super) fn query(&self) -> Option<crate::check::editor::Query> {
        let occurrence = self.token?;
        if let Some(label) = &self.label {
            return Some(crate::check::editor::Query {
                occurrence,
                binding: None,
                function: Some(label.function.clone()),
            });
        }
        let function = self.target.and_then(|target| {
            self.globals
                .iter()
                .find(|(_, symbol)| symbol.span == target && matches!(symbol.kind, 3 | 21))
                .map(|(name, _)| name.clone())
        });
        let binding = self.target.filter(|target| {
            !self
                .globals
                .values()
                .chain(self.types.values())
                .any(|s| s.span == *target)
        });
        Some(crate::check::editor::Query {
            occurrence,
            binding,
            function,
        })
    }

    /// Locate a globally shifted span while borrowing its original source bytes.
    pub fn location(&self, span: Span) -> Option<(&Path, &str, Span)> {
        self.sources
            .iter()
            .find(|s| span.start >= s.start && span.end <= s.start + s.text.len())
            .map(|s| {
                (
                    s.path,
                    s.text,
                    Span {
                        start: span.start - s.start,
                        end: span.end - s.start,
                    },
                )
            })
    }
    /// Read a global source span through its original file without copying text.
    fn text(&self, span: Span) -> Option<&str> {
        let (_, source, span) = self.location(span)?;
        source.get(span.start..span.end)
    }
    /// Select an identifier by token identity, never by searching comments or literal text.
    pub(super) fn identifier(&self, span: Span, skip: usize) -> Option<Span> {
        let source = self
            .sources
            .iter()
            .find(|s| span.start >= s.start && span.end <= s.start + s.text.len())?;
        let first = source
            .tokens
            .identifiers
            .partition_point(|id| id.start + source.start < span.start);
        let id = source.tokens.identifiers.get(first.checked_add(skip)?)?;
        (id.end + source.start <= span.end).then_some(Span {
            start: id.start + source.start,
            end: id.end + source.start,
        })
    }
    /// Treat the caret immediately after a token as belonging to its source span.
    fn contains(&self, span: Span) -> bool {
        span.start <= self.cursor && self.cursor <= span.end
    }
    /// A trailing blank caret inherits a completed suite only while indentation keeps it open.
    fn extends(&self, span: Span, block: bool) -> bool {
        if self.cursor <= span.end {
            return false;
        }
        let Some(source) = self
            .sources
            .iter()
            .find(|s| span.start >= s.start && self.cursor <= s.start + s.text.len())
        else {
            return false;
        };
        let cursor = self.cursor - source.start;
        let line = source.text[..cursor].rsplit('\n').next().unwrap_or("");
        if line.is_empty() || !line.bytes().all(|b| b == b' ') {
            return false;
        }
        let head = source.text[..span.start - source.start]
            .rsplit('\n')
            .next()
            .unwrap_or("");
        let indent = head.bytes().take_while(|b| *b == b' ').count();
        if line.len() < indent + usize::from(!block) {
            return false;
        }
        Self::trivia(source, span.end - source.start, cursor)
    }
    /// Skip only whitespace and indexed comments, never intervening declarations or literal text.
    fn trivia(source: &Source<'_>, mut at: usize, end: usize) -> bool {
        while at < end {
            if source.text.as_bytes()[at].is_ascii_whitespace() {
                at += 1;
                continue;
            }
            let index = source
                .tokens
                .excluded
                .partition_point(|span| span.start < at);
            let Some(span) = source.tokens.excluded.get(index) else {
                return false;
            };
            if span.start != at || span.end > end {
                return false;
            }
            let text = &source.text[span.start..span.end];
            if !text.starts_with('#') && !text.starts_with("/*") {
                return false;
            }
            at = span.end;
        }
        true
    }
    /// Bound traversal and scope copying, including very large caller-created syntax graphs.
    fn charge(&mut self, amount: usize, depth: usize) -> Option<()> {
        self.work = self.work.checked_add(amount)?;
        (self.work <= 400_000 && depth <= 128).then_some(())
    }
    /// Save only the currently visible lexical bindings under the shared work budget.
    fn capture(&mut self, locals: &Bindings) -> Option<()> {
        self.charge(locals.len(), 0)?;
        self.locals = locals.clone();
        Some(())
    }
    /// Match references only against their head tokens; argument names belong to their own scopes.
    fn reference(&mut self, name: &str, span: Span, locals: &Bindings) {
        let Some(token) = self.token else {
            return;
        };
        let Some(first) = self.identifier(span, 0) else {
            return;
        };
        let Some((_, text, local)) = self.location(span) else {
            return;
        };
        let prefix = text
            .get(local.start..)
            .unwrap_or("")
            .split(|c: char| c == '(' || c.is_whitespace())
            .next()
            .unwrap_or("");
        if token.start < first.start || token.end > first.start + prefix.len() {
            return;
        }
        let Some(root) = self.text(first) else {
            return;
        };
        if let Some(binding) = locals.get(root) {
            if token == first {
                self.target = Some(*binding);
            } else {
                self.blocked = true;
            }
        } else if let Some(symbol) = self.globals.get(name) {
            self.target = Some(symbol.span);
        }
    }
    /// Introduce recursive pattern binders only within the scope supplied by their owner.
    fn pattern(
        &mut self,
        pattern: &ast::Pattern,
        locals: &mut Bindings,
        depth: usize,
    ) -> Option<()> {
        self.charge(1, depth)?;
        match &pattern.kind {
            ast::PatternKind::Typed { pattern, .. } => self.pattern(pattern, locals, depth + 1)?,
            ast::PatternKind::Bind(name) => {
                locals.insert(name.clone(), pattern.span);
                if self.contains(pattern.span) {
                    self.target = Some(pattern.span);
                }
            }
            ast::PatternKind::Tuple(fields) | ast::PatternKind::NamedConstructor { fields, .. } => {
                if let ast::PatternKind::NamedConstructor { name, .. } = &pattern.kind {
                    self.reference(name, pattern.span, locals);
                }
                for field in fields {
                    self.pattern(field, locals, depth + 1)?;
                }
            }
            ast::PatternKind::List { prefix, rest } => {
                for field in prefix {
                    self.pattern(field, locals, depth + 1)?;
                }
                if let Some(rest) = rest {
                    self.pattern(rest, locals, depth + 1)?;
                }
            }
            ast::PatternKind::TupleRest { prefix, rest } => {
                for field in prefix {
                    self.pattern(field, locals, depth + 1)?;
                }
                self.pattern(rest, locals, depth + 1)?;
            }
            ast::PatternKind::Constructor {
                binding: Some(name),
                ..
            } if name != "_" => {
                if let Some(span) = self.identifier(pattern.span, 1) {
                    locals.insert(name.clone(), span);
                    if self.contains(span) {
                        self.target = Some(span);
                    }
                }
            }
            _ => {}
        }
        Some(())
    }
    /// Select only the current expression subtree while retaining surrounding visible bindings.
    fn expression(
        &mut self,
        expression: &ast::Expr,
        locals: &Bindings,
        depth: usize,
    ) -> Option<()> {
        if !self.contains(expression.span)
            && !self.extends(
                expression.span,
                matches!(expression.kind, ast::ExprKind::Block(_)),
            )
        {
            return Some(());
        }
        self.charge(1, depth)?;
        self.capture(locals)?;
        use ast::ExprKind as E;
        match &expression.kind {
            E::Name(name) => self.reference(name, expression.span, locals),
            E::GlobalName { resolved, .. } => self.reference(resolved, expression.span, locals),
            E::GlobalCall { resolved, args, .. } => {
                self.reference(resolved, expression.span, locals);
                self.complete_labels(expression, args, Some(resolved), locals);
                self.arguments(args, Some(resolved), locals, depth)?;
            }
            E::Call { name, args } => {
                self.reference(name, expression.span, locals);
                let callee = self.source_callee(name, locals);
                self.complete_labels(expression, args, callee.as_deref(), locals);
                self.arguments(args, callee.as_deref(), locals, depth)?;
            }
            E::Block(statements) => self.block(statements, locals, depth + 1)?,
            E::Match { value, arms } => {
                self.expression(value, locals, depth + 1)?;
                self.arms(arms, locals, depth + 1)?;
            }
            E::Lambda { params, body } => {
                let mut inner = locals.clone();
                for p in params {
                    let span = self.identifier(p.span, 0)?;
                    inner.insert(p.name.clone(), span);
                    if self.contains(span) {
                        self.target = Some(span);
                    }
                }
                self.expression(body, &inner, depth + 1)?;
            }
            E::For {
                pattern,
                iterable,
                body,
            } => {
                self.expression(iterable, locals, depth + 1)?;
                let mut inner = locals.clone();
                self.pattern(pattern, &mut inner, depth + 1)?;
                self.expression(body, &inner, depth + 1)?;
            }
            E::With {
                bindings,
                body,
                arms,
            } => self.with(bindings, body, arms.as_deref(), locals, depth + 1)?,
            _ => self.children(expression, locals, depth + 1)?,
        }
        Some(())
    }
    /// Visit argument values in lexical scope and labels in the selected source interface.
    fn arguments(
        &mut self,
        args: &[ast::Argument],
        callee: Option<&str>,
        locals: &Bindings,
        depth: usize,
    ) -> Option<()> {
        for arg in args {
            if let Some(label) = &arg.label {
                self.argument_label(label, callee);
            }
            self.expression(&arg.value, locals, depth + 1)?;
        }
        Some(())
    }

    /// Visit collection elements while preserving their surrounding scope.
    fn values(&mut self, values: &[ast::Expr], locals: &Bindings, depth: usize) -> Option<()> {
        for value in values {
            self.expression(value, locals, depth + 1)?;
        }
        Some(())
    }
    /// Sequential initializers and let-else failures use old bindings; successful patterns begin later.
    fn block(&mut self, statements: &[ast::Stmt], locals: &Bindings, depth: usize) -> Option<()> {
        let mut inner = locals.clone();
        for statement in statements {
            self.charge(1, depth)?;
            let (value, span) = match statement {
                ast::Stmt::Expr(value) => {
                    self.expression(value, &inner, depth + 1)?;
                    continue;
                }
                ast::Stmt::Let { value, span, .. }
                | ast::Stmt::LetPattern { value, span, .. }
                | ast::Stmt::LetElse { value, span, .. } => (value, *span),
            };
            self.expression(value, &inner, depth + 1)?;
            if let ast::Stmt::LetElse { else_branch, .. } = statement {
                self.expression(else_branch, &inner, depth + 1)?;
            }
            if let ast::Stmt::Let { .. } = statement
                && let Some(binding) = self.identifier(span, 1)
                && self.contains(binding)
            {
                self.target = Some(binding);
            }
            if self.cursor < span.end {
                if let ast::Stmt::LetPattern { pattern, .. } | ast::Stmt::LetElse { pattern, .. } =
                    statement
                    && self.contains(pattern.span)
                {
                    self.pattern(pattern, &mut Bindings::new(), depth + 1)?;
                }
                continue;
            }
            match statement {
                ast::Stmt::Let { name, .. } => {
                    if let Some(span) = self.identifier(span, 1) {
                        inner.insert(name.clone(), span);
                    }
                }
                ast::Stmt::LetPattern { pattern, .. } | ast::Stmt::LetElse { pattern, .. } => {
                    self.pattern(pattern, &mut inner, depth + 1)?
                }
                _ => {}
            }
            self.capture(&inner)?;
        }
        Some(())
    }
    /// Expose pattern bindings only within the arm containing the current cursor.
    fn arms(&mut self, arms: &[ast::MatchArm], locals: &Bindings, depth: usize) -> Option<()> {
        for arm in arms {
            if !self.contains(arm.span) && !self.extends(arm.span, false) {
                continue;
            }
            let mut inner = locals.clone();
            self.pattern(&arm.pattern, &mut inner, depth + 1)?;
            self.capture(&inner)?;
            if let Some(guard) = &arm.guard {
                self.expression(guard, &inner, depth + 1)?;
            }
            self.expression(&arm.body, &inner, depth + 1)?;
        }
        Some(())
    }
    /// Introduce successful bindings sequentially while error arms retain the outer scope.
    fn with(
        &mut self,
        bindings: &[ast::WithBinding],
        body: &ast::Expr,
        arms: Option<&[ast::MatchArm]>,
        locals: &Bindings,
        depth: usize,
    ) -> Option<()> {
        let mut inner = locals.clone();
        for binding in bindings {
            self.expression(&binding.value, &inner, depth + 1)?;
            if self.cursor >= binding.span.end {
                self.pattern(&binding.pattern, &mut inner, depth + 1)?;
            } else if self.contains(binding.pattern.span) {
                self.pattern(&binding.pattern, &mut Bindings::new(), depth + 1)?;
            }
        }
        self.expression(body, &inner, depth + 1)?;
        if let Some(arms) = arms {
            self.arms(arms, locals, depth + 1)?;
        }
        Some(())
    }
    /// Resolve a pipe target from its actual source head after the input expression.
    fn pipe_reference(&mut self, expression: &ast::Expr, locals: &Bindings) {
        let ast::ExprKind::GlobalPipe {
            value, resolved, ..
        } = &expression.kind
        else {
            return;
        };
        let span = Span {
            start: value.span.end,
            end: expression.span.end,
        };
        if let Some(start) = self.identifier(span, 0) {
            self.reference(
                resolved,
                Span {
                    start: start.start,
                    end: span.end,
                },
                locals,
            );
        }
    }

    /// Visit embedded expressions without interpreting literal string text as code.
    fn interpolation(
        &mut self,
        parts: &[ast::StringPart],
        locals: &Bindings,
        depth: usize,
    ) -> Option<()> {
        for part in parts {
            if let ast::StringPart::Value(value) = part {
                self.expression(value, locals, depth)?;
            }
        }
        Some(())
    }
    /// Untyped record members cannot navigate to an unrelated global with the same spelling.
    fn field_reference(
        &mut self,
        value: &ast::Expr,
        locals: &Bindings,
        depth: usize,
    ) -> Option<()> {
        if self.cursor > value.span.end {
            self.blocked = true;
        }
        self.expression(value, locals, depth)
    }

    /// Nonbinding expression children inherit the same lexical scope.
    fn children(&mut self, expression: &ast::Expr, locals: &Bindings, depth: usize) -> Option<()> {
        use ast::ExprKind as E;
        match &expression.kind {
            E::Unary { value, .. } | E::Try(value) | E::Return(value) | E::Defer(value) => {
                self.expression(value, locals, depth)?
            }
            E::Field { value, .. } => self.field_reference(value, locals, depth)?,
            E::Binary { left, right, .. }
            | E::Range {
                start: left,
                end: right,
                ..
            }
            | E::PostfixIf {
                value: left,
                condition: right,
            } => {
                self.expression(left, locals, depth)?;
                self.expression(right, locals, depth)?;
            }
            E::Apply { callee, args } => {
                self.expression(callee, locals, depth)?;
                self.arguments(args, None, locals, depth)?;
            }
            E::List(values) | E::Tuple(values) => self.values(values, locals, depth)?,
            E::Map(pairs) => {
                for (key, value) in pairs {
                    self.expression(key, locals, depth)?;
                    self.expression(value, locals, depth)?;
                }
            }
            E::RecordUpdate { value, fields } => {
                self.expression(value, locals, depth)?;
                for field in fields {
                    self.expression(&field.value, locals, depth)?;
                }
            }
            E::Pipe { .. } | E::GlobalPipe { .. } => self.pipe(expression, locals, depth)?,
            E::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expression(condition, locals, depth)?;
                self.expression(then_branch, locals, depth)?;
                if let Some(otherwise) = else_branch {
                    self.expression(otherwise, locals, depth)?;
                }
            }
            E::ConditionMatch(arms) => {
                for arm in arms {
                    if let Some(condition) = &arm.condition {
                        self.expression(condition, locals, depth)?;
                    }
                    self.expression(&arm.body, locals, depth)?;
                }
            }
            E::Interpolate(parts) | E::MultilineString(parts) => {
                self.interpolation(parts, locals, depth)?
            }
            _ => {}
        }
        Some(())
    }
}

/// Exact lexical completion prefix and replacement range, excluding comments and literal contents.
pub(super) fn prefix(source: &str, cursor: usize) -> Option<(String, String, Span)> {
    let tokens = parse::identifier_index(source).ok()?;
    if tokens.excluded.iter().any(|s| {
        s.start <= cursor
            && (cursor < s.end || (cursor == s.end && source[s.start..s.end].starts_with('#')))
    }) {
        return None;
    }
    let token = tokens
        .identifiers
        .iter()
        .find(|s| s.start <= cursor && cursor <= s.end);
    let span = token.copied().unwrap_or(Span {
        start: cursor,
        end: cursor,
    });
    let prefix = source.get(span.start..cursor)?.to_owned();
    let mut start = span.start;
    let mut depth = 0;
    while start > 0 && source.as_bytes().get(start - 1) == Some(&b'.') {
        depth += 1;
        if depth > 128 {
            return None;
        }
        let index = tokens.identifiers.partition_point(|s| s.end < start - 1);
        let previous = tokens
            .identifiers
            .get(index)
            .filter(|s| s.end == start - 1)?;
        start = previous.start;
    }
    let receiver = source
        .get(start..span.start)?
        .trim_end_matches('.')
        .to_owned();
    Some((receiver, prefix, span))
}

const CORE: &[&str] = &[
    "Ptr.null",
    "Ptr.is_null",
    "Ptr.equal",
    "Ptr.to_string",
    "String.as_ptr",
    "CInt8.from_int",
    "CInt8.to_int",
    "CInt16.from_int",
    "CInt16.to_int",
    "CInt32.from_int",
    "CInt32.to_int",
    "CUInt8.from_int",
    "CUInt8.to_int",
    "CUInt16.from_int",
    "CUInt16.to_int",
    "CUInt32.from_int",
    "CUInt32.to_int",
    "CUInt64.from_int",
    "CUInt64.to_int",
    "CFloat32.from_float",
    "CFloat32.to_float",
    "Set.new",
    "Set.insert",
    "Set.delete",
    "Set.contains",
    "Set.len",
    "Set.is_empty",
    "Set.to_list",
    "Set.from_list",
    "Set.union",
    "Set.intersection",
    "Set.difference",
    "Set.is_subset",
    "Set.equal",
    "Map.new",
    "Map.get",
    "Map.put",
    "Map.delete",
    "Map.len",
    "Map.is_empty",
    "Map.contains",
    "Map.keys",
    "Map.values",
    "print",
    "println",
    "String.concat",
    "String.eq",
    "String.len",
    "List.enumerate",
    "List.map",
    "List.fold",
    "List.filter",
    "List.find",
    "List.any",
    "List.all",
    "Option.map",
    "Result.map",
    "Result.and_then",
    "Result.unwrap_or_else",
    "List.len",
    "List.get",
    "List.head",
    "List.tail",
    "List.is_empty",
    "List.push",
    "List.reverse",
    "List.concat",
    "List.contains",
    "Option.is_some",
    "Option.is_none",
    "Option.unwrap_or",
    "Result.is_ok",
    "Result.is_err",
    "Result.unwrap_or",
    "Some",
    "None",
    "Ok",
    "Err",
];

/// Compiler builtins complement the central runtime registry; unsupported names are filtered.
pub(super) fn builtins() -> Vec<String> {
    let mut names: Vec<_> = super::runtime::names()
        .iter()
        .filter(|n| super::runtime::lookup(n).is_some())
        .map(|s| (*s).to_owned())
        .collect();
    names.extend(
        CORE.iter()
            .filter(|n| {
                super::check::builtin(n).is_some() || matches!(**n, "Some" | "None" | "Ok" | "Err")
            })
            .map(|s| (*s).to_owned()),
    );
    names
}

/// Identify annotation roles from committed parser ranges across the current source graph.
fn type_context(sources: &[Source<'_>], token: Option<Span>) -> bool {
    token.is_some_and(|token| {
        sources.iter().any(|source| {
            source.annotations.iter().any(|span| {
                source.start + span.start <= token.start && token.end <= source.start + span.end
            })
        })
    })
}

/// Import selector roles come only from parser-confirmed delimiters in this exact source.
fn selector_context(sources: &[Source<'_>], token: Option<Span>) -> bool {
    token.is_some_and(|token| {
        sources.iter().any(|source| {
            source.selectors.iter().any(|span| {
                source.start + span.start <= token.start && token.end <= source.start + span.end
            })
        })
    })
}

/// Locate the caret in actual identifier or scalar tokens under the already charged source budget.
fn cursor_token(sources: &[Source<'_>], cursor: usize) -> Option<Span> {
    sources.iter().find_map(|s| {
        s.tokens
            .identifiers
            .iter()
            .chain(s.tokens.numbers.iter())
            .find(|span| span.start + s.start <= cursor && cursor <= span.end + s.start)
            .map(|span| Span {
                start: span.start + s.start,
                end: span.end + s.start,
            })
    })
}
