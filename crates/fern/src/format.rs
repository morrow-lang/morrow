//! Canonical AST formatting with source-anchored comments and structural validation.
use crate::ast::{self, BinaryOp, Expr, ExprKind, Pattern, PatternKind, Stmt, UnaryOp};
use crate::{Constructor, Diagnostic, Span, Type, parse};

type Result<T> = std::result::Result<T, Diagnostic>;

/// Format supported Fern source using four-space indentation and canonical spacing.
/// Preserves comments in order and validates the rendered syntax tree before returning.
/// Invalid/unsupported source returns a diagnostic; no C formatter is invoked.
pub fn format(source: &str) -> Result<String> {
    let program = parse::parse(source)?;
    let lines = Renderer { source }.program(&program)?;
    let comments = comments(source);
    let output = attach_comments(source, lines, &comments);
    let formatted = parse::parse(&output).map_err(|error| {
        Diagnostic::new(
            Span::default(),
            format!(
                "cannot safely format this expression layout: {}",
                error.message
            ),
        )
    })?;
    if structural(program) != structural(formatted) {
        return Err(Diagnostic::new(
            Span::default(),
            "formatting would change the syntax tree; original source was preserved",
        ));
    }
    if comments
        .iter()
        .map(|comment| &comment.text)
        .ne(self::comments(&output).iter().map(|comment| &comment.text))
    {
        return Err(Diagnostic::new(
            Span::default(),
            "cannot preserve comment order in this layout",
        ));
    }
    Ok(output)
}

struct Line {
    indent: usize,
    text: String,
    anchor: usize,
}

/// Construct a rendered line with its original source anchor.
fn line(indent: usize, text: impl Into<String>, anchor: usize) -> Line {
    Line {
        indent,
        text: text.into(),
        anchor,
    }
}

struct Renderer<'a> {
    source: &'a str,
}

impl Renderer<'_> {
    /// Render declarations in source order so neighboring comments retain context.
    fn program(&self, program: &ast::Program) -> Result<Vec<Line>> {
        let mut declarations = Vec::new();
        if let Some(module) = &program.module {
            let anchor = module_anchor(self.source);
            declarations.push((anchor, vec![line(0, format!("module {module}"), anchor)]));
        }
        for import in &program.imports {
            let mut text = if import.public {
                "pub import ".to_owned()
            } else {
                "import ".to_owned()
            };
            text.push_str(&import.module);
            if let Some(items) = &import.items {
                if items == &["*"] {
                    text.push_str(".*");
                } else {
                    text.push_str(&format!(".{{{}}}", items.join(", ")));
                }
            }
            if let Some(alias) = &import.alias {
                text.push_str(&format!(" as {alias}"));
            }
            declarations.push((import.span.start, vec![line(0, text, import.span.start)]));
        }
        for alias in &program.aliases {
            declarations.push((alias.span.start, vec![self.alias(alias, alias.public)?]));
        }
        for decl in &program.newtypes {
            declarations.push((decl.span.start, vec![self.newtype(decl, decl.public)?]));
        }
        for declaration in &program.types {
            declarations.push((
                declaration.span.start,
                self.declaration(declaration, declaration.public)?,
            ));
        }
        for function in &program.functions {
            declarations.push((
                function.span.start,
                self.function(function, function.public)?,
            ));
        }
        self.documentation(program, &mut declarations);
        declarations.sort_by_key(|(anchor, _)| *anchor);
        let mut lines = Vec::new();
        for (anchor, declaration) in declarations {
            if !lines.is_empty() {
                lines.push(line(0, "", anchor));
            }
            lines.extend(declaration);
        }
        Ok(lines)
    }

    /// Attach documentation to its declaration before sorting source anchors.
    fn documentation(&self, program: &ast::Program, declarations: &mut [(usize, Vec<Line>)]) {
        let targets: std::collections::BTreeMap<_, _> = program
            .functions
            .iter()
            .map(|f| (f.span.start, &f.name))
            .chain(program.types.iter().map(|d| (d.span.start, &d.name)))
            .chain(program.aliases.iter().map(|d| (d.span.start, &d.name)))
            .chain(program.newtypes.iter().map(|d| (d.span.start, &d.name)))
            .collect();
        let indices: std::collections::BTreeMap<_, _> = declarations
            .iter()
            .enumerate()
            .map(|(index, (anchor, _))| (*anchor, index))
            .collect();
        for doc in &program.docs {
            if let Some(index) = targets
                .range(doc.span.end..)
                .next()
                .filter(|(_, name)| **name == &doc.target)
                .and_then(|(anchor, _)| indices.get(anchor))
            {
                let (anchor, lines) = &mut declarations[*index];
                *anchor = doc.span.start;
                lines.insert(
                    0,
                    line(
                        0,
                        format!("@doc {}", quote_multiline(&doc.text, true)),
                        doc.span.start,
                    ),
                );
            }
        }
    }

    /// Canonicalize transparent aliases without changing their source parameters.
    fn alias(&self, alias: &ast::TypeAlias, public: bool) -> Result<Line> {
        let owner = type_text(&Type::Named(
            alias.name.clone(),
            alias
                .parameters
                .iter()
                .cloned()
                .map(Type::Generic)
                .collect(),
        ))?;
        Ok(line(
            0,
            format!(
                "{}type {owner} = {}",
                if public { "pub " } else { "" },
                type_text(&alias.target)?
            ),
            alias.span.start,
        ))
    }

    /// Preserve distinct newtype and constructor names and the exact payload type.
    fn newtype(&self, decl: &ast::NewtypeDecl, public: bool) -> Result<Line> {
        let owner = type_text(&Type::Named(
            decl.name.clone(),
            decl.parameters.iter().cloned().map(Type::Generic).collect(),
        ))?;
        Ok(line(
            0,
            format!(
                "{}newtype {owner}{} = {}({})",
                if public { "pub " } else { "" },
                derive_text(&decl.derives),
                decl.constructor,
                type_text(&decl.inner)?
            ),
            decl.span.start,
        ))
    }

    /// Render a record's named fields or a sum's variant payload declarations.
    fn declaration(&self, declaration: &ast::TypeDecl, public: bool) -> Result<Vec<Line>> {
        let mut header = format!(
            "{}type {}",
            if public { "pub " } else { "" },
            declaration.name
        );
        if !declaration.parameters.is_empty() {
            header.push_str(&format!("({})", declaration.parameters.join(", ")));
        }
        if !declaration.derives.is_empty() {
            let names: Vec<_> = declaration
                .derives
                .iter()
                .map(|d| d.name.as_str())
                .collect();
            header.push_str(&format!(" derive({})", names.join(", ")));
        }
        header.push(':');
        let mut lines = vec![line(0, header, declaration.span.start)];
        for variant in &declaration.variants {
            if declaration.record {
                for field in &variant.fields {
                    lines.push(line(1, field_text(field)?, field.span.start));
                }
            } else {
                let mut text = variant.name.clone();
                if !variant.fields.is_empty() {
                    let fields = variant
                        .fields
                        .iter()
                        .map(field_text)
                        .collect::<Result<Vec<_>>>()?;
                    text.push_str(&format!("({})", fields.join(", ")));
                }
                lines.push(line(1, text, variant.span.start));
            }
        }
        Ok(lines)
    }

    /// Render a signature while preserving omitted return annotations and body shape.
    fn function(&self, function: &ast::Function, public: bool) -> Result<Vec<Line>> {
        let params = function
            .params
            .iter()
            .map(|param| match &param.annotation {
                Some(ty) => Ok(format!("{}: {}", parameter_text(param), type_text(ty)?)),
                None => Ok(parameter_text(param)),
            })
            .collect::<Result<Vec<_>>>()?;
        let mut header = format!(
            "{}fn {}({})",
            if public { "pub " } else { "" },
            function.name,
            params.join(", ")
        );
        if let Some(guard) = &function.guard {
            header.push_str(&format!(" if {}", self.inline(guard, 0)?));
        }
        if let Some(ty) = &function.return_type {
            header.push_str(&format!(" -> {}", type_text(ty)?));
        }
        header.push_str(match function.syntax {
            ast::FunctionSyntax::Colon => ":",
            ast::FunctionSyntax::Arrow => " ->",
        });
        self.suite(header, function.span.start, &function.body, 0)
    }

    /// Keep inline suites inline and indent block suites one canonical level.
    fn suite(
        &self,
        header: String,
        anchor: usize,
        body: &Expr,
        indent: usize,
    ) -> Result<Vec<Line>> {
        if let ExprKind::Block(statements) = &body.kind {
            let mut lines = vec![line(indent, header, anchor)];
            lines.extend(self.statements(statements, indent + 1)?);
            Ok(lines)
        } else {
            let mut lines = self.expression(body, indent)?;
            if let Some(first) = lines.first_mut() {
                first.text = format!("{header} {}", first.text);
                first.anchor = anchor;
            }
            Ok(lines)
        }
    }

    /// Preserve text and value boundaries while formatting nested scalar expressions.
    fn interpolation(
        &self,
        parts: &[ast::StringPart],
        indent: usize,
        multiline: bool,
    ) -> Result<String> {
        let delimiter = if multiline { "\"\"\"" } else { "\"" };
        let mut text = String::from(delimiter);
        for part in parts {
            match part {
                ast::StringPart::Text(value) => {
                    let escaped = if multiline {
                        quote_multiline(value, false)
                    } else {
                        quote(value)
                    };
                    text.push_str(&escaped[delimiter.len()..escaped.len() - delimiter.len()]);
                }
                ast::StringPart::Value(value) => {
                    text.push('{');
                    text.push_str(&self.inline(value, indent)?);
                    text.push('}');
                }
            }
        }
        text.push_str(delimiter);
        Ok(text)
    }

    /// Render immutable bindings and expression statements without merging scopes.
    fn statements(&self, statements: &[Stmt], indent: usize) -> Result<Vec<Line>> {
        let mut lines = Vec::new();
        for statement in statements {
            match statement {
                Stmt::LetElse {
                    pattern,
                    annotation,
                    value,
                    else_branch,
                    span,
                } => {
                    lines.extend(self.let_else(
                        pattern,
                        annotation.as_ref(),
                        value,
                        else_branch,
                        *span,
                        indent,
                    )?);
                }
                Stmt::LetPattern {
                    pattern,
                    annotation,
                    value,
                    span,
                } => {
                    let mut prefix = format!("let {}", pattern_text(pattern));
                    if let Some(ty) = annotation {
                        prefix.push_str(&format!(": {}", type_text(ty)?));
                    }
                    let mut value = self.expression(value, indent)?;
                    if let Some(first) = value.first_mut() {
                        first.text = format!("{prefix} = {}", first.text);
                        first.anchor = span.start;
                    }
                    lines.extend(value);
                }
                Stmt::Expr(value) => lines.extend(self.expression(value, indent)?),
                Stmt::Let {
                    name,
                    annotation,
                    value,
                    span,
                } => {
                    let mut prefix = format!("let {name}");
                    if let Some(ty) = annotation {
                        prefix.push_str(&format!(": {}", type_text(ty)?));
                    }
                    let mut value = self.expression(value, indent)?;
                    if let Some(first) = value.first_mut() {
                        first.text = format!("{prefix} = {}", first.text);
                        first.anchor = span.start;
                    }
                    lines.extend(value);
                }
            }
        }
        Ok(lines)
    }

    /// Render expressions with explicit grouping where it does not interfere with layout.
    fn expression(&self, expression: &Expr, indent: usize) -> Result<Vec<Line>> {
        let text = match &expression.kind {
            ExprKind::TypeTarget(ty) => type_text(ty)?,
            ExprKind::Break => "break".into(),
            ExprKind::Continue => "continue".into(),
            ExprKind::Range {
                start,
                end,
                inclusive,
            } => format!(
                "({}..{}{})",
                self.inline(start, indent)?,
                if *inclusive { "=" } else { "" },
                self.inline(end, indent)?
            ),
            ExprKind::Receive { .. }
            | ExprKind::For { .. }
            | ExprKind::With { .. }
            | ExprKind::Return(_)
            | ExprKind::Defer(_)
            | ExprKind::PostfixIf { .. }
            | ExprKind::ConditionMatch(_)
            | ExprKind::If { .. } => return self.control_expression(expression, indent),
            ExprKind::Int(value) => value.to_string(),
            ExprKind::Float(value) => format!("{value:?}"),
            ExprKind::Bool(value) => value.to_string(),
            ExprKind::String(value) => quote(value),
            ExprKind::Interpolate(parts) => self.interpolation(parts, indent, false)?,
            ExprKind::MultilineString(parts) => self.interpolation(parts, indent, true)?,
            ExprKind::Name(name) | ExprKind::GlobalName { name, .. } => name.clone(),
            ExprKind::Unit => "()".into(),
            ExprKind::Tuple(items) => {
                return self.delimited_values("(", ")", items, indent, expression.span, true);
            }
            ExprKind::List(items) => {
                return self.delimited_values("[", "]", items, indent, expression.span, false);
            }
            ExprKind::Map(pairs) => return self.map(pairs, indent, expression.span),
            ExprKind::RecordUpdate { value, fields } => {
                return self.record_update(value, fields, indent, expression.span);
            }
            ExprKind::Call { name, args } | ExprKind::GlobalCall { name, args, .. } => {
                return self.call(name, args, indent, expression.span);
            }
            ExprKind::Apply { callee, args } => {
                return self.apply(callee, args, indent, expression.span);
            }
            ExprKind::Lambda { params, body } => {
                return self.lambda(params, body, indent, expression.span);
            }
            ExprKind::Pipe { .. } | ExprKind::GlobalPipe { .. } => {
                return self.pipe_expression(expression, indent);
            }
            ExprKind::Unary { op, value } => {
                return self.unary(*op, value, indent, expression.span);
            }
            ExprKind::Binary { op, left, right } => {
                return self.binary(*op, left, right, indent, expression.span);
            }
            ExprKind::Try(value) => return self.postfix(value, "?", indent, expression.span),
            ExprKind::Field { value, name } => {
                return self.postfix(value, &format!(".{name}"), indent, expression.span);
            }
            ExprKind::Match { value, arms } => {
                return self.matching(value, arms, indent, expression.span);
            }
            ExprKind::Block(statements) => return self.statements(statements, indent),
        };
        Ok(vec![line(indent, text, expression.span.start)])
    }

    /// Format receive arms and the optional timeout as independently scoped suites.
    fn receive(
        &self,
        arms: &[ast::MatchArm],
        timeout: &Option<(Box<Expr>, Box<Expr>)>,
        span: Span,
        indent: usize,
    ) -> Result<Vec<Line>> {
        let mut lines = vec![line(indent, "receive:", span.start)];
        lines.extend(self.match_arms(arms, indent + 1)?);
        if let Some((duration, body)) = timeout {
            lines.extend(self.suite(
                format!("_ after {} ->", self.inline(duration, indent)?),
                duration.span.start,
                body,
                indent + 1,
            )?);
        }
        Ok(lines)
    }

    /// Dispatch scoped and terminating expressions separately from ordinary values.
    fn control_expression(&self, expression: &Expr, indent: usize) -> Result<Vec<Line>> {
        match &expression.kind {
            ExprKind::Receive { arms, timeout } => {
                self.receive(arms, timeout, expression.span, indent)
            }
            ExprKind::For {
                pattern,
                iterable,
                body,
            } => self.for_loop(pattern, iterable, body, indent, expression.span),
            ExprKind::With {
                bindings,
                body,
                arms,
            } => self.with_expression(bindings, body, arms.as_deref(), indent, expression.span),
            ExprKind::Return(value) => {
                self.control_prefix("return", value, indent, expression.span)
            }
            ExprKind::Defer(value) => self.control_prefix("defer", value, indent, expression.span),
            ExprKind::PostfixIf { value, condition } => {
                self.postfix_if(value, condition, indent, expression.span)
            }
            ExprKind::ConditionMatch(arms) => self.condition_match(arms, indent, expression.span),
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => self.conditional(
                condition,
                then_branch,
                else_branch.as_deref(),
                indent,
                expression.span,
            ),
            _ => Err(Diagnostic::new(
                expression.span,
                "expected control expression",
            )),
        }
    }

    /// Require a delimiter-safe inline expression for arguments and guards.
    fn inline(&self, expression: &Expr, indent: usize) -> Result<String> {
        let lines = self.expression(expression, indent)?;
        if lines.len() != 1 {
            return Err(Diagnostic::new(
                expression.span,
                "cannot format a block expression inside delimiters",
            ));
        }
        Ok(lines
            .into_iter()
            .next()
            .map(|line| line.text)
            .unwrap_or_default())
    }

    /// Render loop suites without changing collection evaluation or pattern scope.
    fn for_loop(
        &self,
        pattern: &Pattern,
        iterable: &Expr,
        body: &Expr,
        indent: usize,
        span: Span,
    ) -> Result<Vec<Line>> {
        let mut header = self.grouped(iterable, indent)?;
        if let Some(first) = header.first_mut() {
            first.text = format!("for {} in {}", pattern_text(pattern), first.text);
            first.anchor = span.start;
        }
        let mut last = header
            .pop()
            .ok_or_else(|| Diagnostic::new(span, "missing loop iterable"))?;
        last.text.push(':');
        header.extend(self.suite(last.text, last.anchor, body, indent)?);
        Ok(header)
    }

    /// Canonicalize Result binding layout while retaining inline versus block success bodies.
    fn with_expression(
        &self,
        bindings: &[ast::WithBinding],
        body: &Expr,
        arms: Option<&[ast::MatchArm]>,
        indent: usize,
        span: Span,
    ) -> Result<Vec<Line>> {
        let mut lines = vec![line(indent, "with", span.start)];
        for (index, binding) in bindings.iter().enumerate() {
            let mut value = self.expression(&binding.value, indent + 1)?;
            if let Some(first) = value.first_mut() {
                first.text = format!("{} <- {}", pattern_text(&binding.pattern), first.text);
                first.anchor = binding.span.start;
            }
            if index + 1 < bindings.len() {
                if value.len() == 1 {
                    value[0].text.push(',');
                } else {
                    value.push(line(indent + 1, ",", binding.span.end));
                }
            }
            lines.extend(value);
        }
        lines.extend(self.suite("do".into(), body.span.start, body, indent)?);
        if let Some(arms) = arms {
            let anchor = arms.first().map_or(span.end, |arm| arm.span.start);
            lines.push(line(
                indent,
                "else",
                self.else_anchor(body.span.end, anchor),
            ));
            lines.extend(self.match_arms(arms, indent + 1)?);
        }
        Ok(lines)
    }

    /// Keep return and defer spelling separate from their complete operand expressions.
    fn control_prefix(
        &self,
        keyword: &str,
        value: &Expr,
        indent: usize,
        span: Span,
    ) -> Result<Vec<Line>> {
        let mut lines = self.expression(value, indent)?;
        if let Some(first) = lines.first_mut() {
            first.text = format!("{keyword} {}", first.text);
            first.anchor = span.start;
        }
        Ok(lines)
    }

    /// Retain a postfix condition after the whole action, rather than moving it into return.
    fn postfix_if(
        &self,
        value: &Expr,
        condition: &Expr,
        indent: usize,
        span: Span,
    ) -> Result<Vec<Line>> {
        let mut lines = self.expression(value, indent)?;
        if let Some(last) = lines.last_mut() {
            last.text
                .push_str(&format!(" if {}", self.inline(condition, indent)?));
        }
        if let Some(first) = lines.first_mut() {
            first.anchor = span.start;
        }
        Ok(lines)
    }

    /// Keep condition matches recognizable instead of expanding them into if chains.
    fn condition_match(
        &self,
        arms: &[ast::ConditionArm],
        indent: usize,
        span: Span,
    ) -> Result<Vec<Line>> {
        let mut lines = vec![line(indent, "match:", span.start)];
        for arm in arms {
            let condition = match &arm.condition {
                Some(value) => self.inline(value, indent + 1)?,
                None => "_".into(),
            };
            lines.extend(self.suite(
                format!("{condition} ->"),
                arm.span.start,
                &arm.body,
                indent + 1,
            )?);
        }
        Ok(lines)
    }

    /// Format the failure suite without changing the initializer or binding pattern.
    fn let_else(
        &self,
        pattern: &Pattern,
        annotation: Option<&Type>,
        value: &Expr,
        otherwise: &Expr,
        span: Span,
        indent: usize,
    ) -> Result<Vec<Line>> {
        let mut prefix = format!("let {}", pattern_text(pattern));
        if let Some(ty) = annotation {
            prefix.push_str(&format!(": {}", type_text(ty)?));
        }
        let mut lines = self.expression(value, indent)?;
        if let Some(first) = lines.first_mut() {
            first.text = format!("{prefix} = {}", first.text);
            first.anchor = span.start;
        }
        let mut failure = self.suite(
            "else:".into(),
            self.else_anchor(value.span.end, otherwise.span.start),
            otherwise,
            indent,
        )?;
        if lines.len() == 1 {
            if let Some(first) = failure.first() {
                lines[0].text.push_str(&format!(" {}", first.text));
            }
            lines.extend(failure.drain(1..));
        } else {
            lines.extend(failure);
        }
        Ok(lines)
    }

    /// Keep map pairs ordered and permit indented callback values inside their braces.
    fn map(&self, pairs: &[(Expr, Expr)], indent: usize, span: Span) -> Result<Vec<Line>> {
        let entries = pairs
            .iter()
            .map(|(key, value)| {
                let mut key_lines = self.grouped(key, indent + 1)?;
                let mut value_lines = self.expression(value, indent + 1)?;
                if let (Some(last), Some(first)) = (key_lines.last_mut(), value_lines.first()) {
                    last.text.push_str(&format!(": {}", first.text));
                }
                key_lines.extend(value_lines.drain(1..));
                Ok(key_lines)
            })
            .collect::<Result<Vec<_>>>()?;
        self.brace_entries(vec![line(indent, "%{", span.start)], entries, indent, span)
    }

    /// Preserve the update base and written field order instead of sorting declaration slots.
    fn record_update(
        &self,
        value: &Expr,
        fields: &[ast::RecordField],
        indent: usize,
        span: Span,
    ) -> Result<Vec<Line>> {
        let mut header = self.grouped(value, indent)?;
        if let Some(first) = header.first_mut() {
            first.text.insert_str(0, "%{ ");
            first.anchor = span.start;
        }
        if let Some(last) = header.last_mut() {
            last.text.push_str(" |");
        }
        let entries = fields
            .iter()
            .map(|field| {
                let mut lines = self.expression(&field.value, indent + 1)?;
                if let Some(first) = lines.first_mut() {
                    first.text = format!("{}: {}", field.name, first.text);
                    first.anchor = field.span.start;
                }
                Ok(lines)
            })
            .collect::<Result<Vec<_>>>()?;
        self.brace_entries(header, entries, indent, span)
    }

    /// Parentheses delimit multiline key/base expressions without changing their AST.
    fn grouped(&self, value: &Expr, indent: usize) -> Result<Vec<Line>> {
        let lines = self.expression(value, indent + 1)?;
        if lines.len() == 1 {
            return Ok(vec![line(indent, lines[0].text.clone(), value.span.start)]);
        }
        let mut result = vec![line(indent, "(", value.span.start)];
        result.extend(lines);
        result.push(line(indent, ")", value.span.end));
        Ok(result)
    }

    /// Share brace layout while keeping callback terminators on a dedented line.
    fn brace_entries(
        &self,
        mut header: Vec<Line>,
        entries: Vec<Vec<Line>>,
        indent: usize,
        span: Span,
    ) -> Result<Vec<Line>> {
        if header.len() == 1 && entries.iter().all(|lines| lines.len() == 1) {
            let text = entries
                .iter()
                .map(|lines| lines[0].text.clone())
                .collect::<Vec<_>>()
                .join(", ");
            let spacing = if header[0].text.ends_with('|') {
                " "
            } else {
                ""
            };
            header[0]
                .text
                .push_str(&format!("{spacing}{text}{spacing}}}"));
            return Ok(header);
        }
        let count = entries.len();
        for (index, mut entry) in entries.into_iter().enumerate() {
            if index + 1 < count {
                if entry.len() == 1 {
                    entry[0].text.push(',');
                } else {
                    entry.push(line(indent + 1, ",", span.end));
                }
            }
            header.extend(entry);
        }
        header.push(line(indent, "}", span.end));
        Ok(header)
    }

    /// Canonicalize both lambda spellings while preserving optional parameter annotations.
    fn lambda(
        &self,
        params: &[ast::LambdaParam],
        body: &Expr,
        indent: usize,
        span: Span,
    ) -> Result<Vec<Line>> {
        let params = params
            .iter()
            .map(|p| {
                Ok(match &p.annotation {
                    Some(ty) => format!("{}: {}", p.name, type_text(ty)?),
                    None => p.name.clone(),
                })
            })
            .collect::<Result<Vec<_>>>()?
            .join(", ");
        self.suite(format!("({params}) ->"), span.start, body, indent)
    }

    /// Render pipe placeholders through ordinary call layout, including callback blocks.
    fn pipe(
        &self,
        value: &Expr,
        name: &str,
        args: &[ast::Argument],
        slot: (usize, &Option<ast::ArgumentLabel>),
        indent: usize,
        span: Span,
    ) -> Result<Vec<Line>> {
        let (position, label) = slot;
        let mut args = args.to_vec();
        if position > args.len() {
            return Err(Diagnostic::new(span, "invalid pipe placeholder position"));
        }
        args.insert(
            position,
            ast::Argument {
                label: label.clone(),
                value: Expr {
                    kind: ExprKind::Name("_".into()),
                    span,
                },
                span,
            },
        );
        let callee = format!("({} |> {name}", self.inline(value, indent)?);
        let mut lines = self.call(&callee, &args, indent, span)?;
        if let Some(last) = lines.last_mut() {
            last.text.push(')');
        }
        Ok(lines)
    }

    /// Format either source or resolved pipes through the preserved source spelling.
    fn pipe_expression(&self, expression: &Expr, indent: usize) -> Result<Vec<Line>> {
        match &expression.kind {
            ExprKind::Pipe {
                value,
                name,
                args,
                position,
                label,
            }
            | ExprKind::GlobalPipe {
                value,
                name,
                args,
                position,
                label,
                ..
            } => self.pipe(
                value,
                name,
                args,
                (*position, label),
                indent,
                expression.span,
            ),
            _ => unreachable!("pipe formatter receives a pipe expression"),
        }
    }

    /// Keep compact calls inline and give embedded suites their own argument layout.
    fn call(
        &self,
        callee: &str,
        args: &[ast::Argument],
        indent: usize,
        span: Span,
    ) -> Result<Vec<Line>> {
        let rendered = args
            .iter()
            .map(|arg| {
                let mut lines = self.expression(&arg.value, indent + 1)?;
                if let (Some(label), Some(first)) = (&arg.label, lines.first_mut()) {
                    first.text = format!("{}: {}", label.name, first.text);
                }
                Ok(lines)
            })
            .collect::<Result<Vec<_>>>()?;
        self.delimited_lines(
            (&format!("{callee}("), ")"),
            rendered,
            &args.iter().map(|a| a.span).collect::<Vec<_>>(),
            indent,
            span,
            false,
        )
    }

    /// Preserve tuple identity and close embedded suites before argument separators.
    fn delimited_values(
        &self,
        open: &str,
        close: &str,
        args: &[Expr],
        indent: usize,
        span: Span,
        tuple: bool,
    ) -> Result<Vec<Line>> {
        let arguments = args
            .iter()
            .map(|arg| self.expression(arg, indent + 1))
            .collect::<Result<Vec<_>>>()?;
        self.delimited_lines(
            (open, close),
            arguments,
            &args.iter().map(|a| a.span).collect::<Vec<_>>(),
            indent,
            span,
            tuple,
        )
    }

    /// Close delimited suites after preserving each source argument's rendered lines.
    fn delimited_lines(
        &self,
        delimiters: (&str, &str),
        arguments: Vec<Vec<Line>>,
        spans: &[Span],
        indent: usize,
        span: Span,
        tuple: bool,
    ) -> Result<Vec<Line>> {
        let (open, close) = delimiters;
        let singleton = tuple && spans.len() == 1;
        if arguments.iter().all(|lines| lines.len() == 1) {
            let mut text = arguments
                .iter()
                .map(|lines| lines[0].text.clone())
                .collect::<Vec<_>>()
                .join(", ");
            if singleton {
                text.push(',');
            }
            return Ok(vec![line(
                indent,
                format!("{open}{text}{close}"),
                span.start,
            )]);
        }
        let mut lines = vec![line(indent, open, span.start)];
        let count = arguments.len();
        for (index, mut argument) in arguments.into_iter().enumerate() {
            if index + 1 < count || singleton {
                if argument.len() == 1 {
                    argument[0].text.push(',');
                } else {
                    argument.push(line(indent + 1, ",", spans[index].end));
                }
            }
            lines.extend(argument);
        }
        lines.push(line(indent, close, span.end));
        Ok(lines)
    }

    /// Parenthesize arbitrary callees so lambda bodies cannot capture the invocation suffix.
    fn apply(
        &self,
        callee: &Expr,
        args: &[ast::Argument],
        indent: usize,
        span: Span,
    ) -> Result<Vec<Line>> {
        let callee = self.expression(callee, indent + 1)?;
        if callee.len() == 1 {
            return self.call(&format!("({})", callee[0].text), args, indent, span);
        }
        let mut lines = vec![line(indent, "(", span.start)];
        lines.extend(callee);
        lines.extend(self.call(")", args, indent, span)?);
        Ok(lines)
    }

    /// Preserve binary grouping and keep continuation operators outside nested blocks.
    fn binary(
        &self,
        op: BinaryOp,
        left: &Expr,
        right: &Expr,
        indent: usize,
        span: Span,
    ) -> Result<Vec<Line>> {
        let mut left = self.expression(left, indent)?;
        let mut right = self.expression(right, indent)?;
        let operator = binary_text(op);
        if left.len() == 1 && right.len() == 1 {
            return Ok(vec![line(
                indent,
                format!("({} {operator} {})", left[0].text, right[0].text),
                span.start,
            )]);
        }
        if left.len() == 1 {
            if let Some(first) = right.first_mut() {
                first.text = format!("{} {operator} {}", left[0].text, first.text);
                first.anchor = span.start;
            }
            Ok(right)
        } else {
            if let Some(first) = right.first_mut() {
                first.text = format!("{operator} {}", first.text);
            }
            left.extend(right);
            Ok(left)
        }
    }

    /// Keep unary expressions grouped while preserving block operand indentation.
    fn unary(&self, op: UnaryOp, value: &Expr, indent: usize, span: Span) -> Result<Vec<Line>> {
        let mut lines = self.expression(value, indent)?;
        let operator = match op {
            UnaryOp::Not => "not ",
            UnaryOp::BitNot => "~~~",
            UnaryOp::Negate => "-",
        };
        let inline = lines.len() == 1;
        if let Some(first) = lines.first_mut() {
            first.text = if inline {
                format!("{operator}({})", first.text)
            } else {
                format!("{operator}{}", first.text)
            };
            first.anchor = span.start;
        }
        Ok(lines)
    }

    /// Append fields/propagation after their complete operand rather than its last arm.
    fn postfix(&self, value: &Expr, suffix: &str, indent: usize, span: Span) -> Result<Vec<Line>> {
        let mut lines = self.expression(value, indent)?;
        if lines.len() == 1 {
            if let Some(first) = lines.first_mut() {
                first.text.push_str(suffix);
                first.anchor = span.start;
            }
        } else {
            lines.push(line(indent, suffix, span.end));
        }
        Ok(lines)
    }

    /// Render if/else expressions with their original inline versus block branch shapes.
    fn conditional(
        &self,
        condition: &Expr,
        then_branch: &Expr,
        else_branch: Option<&Expr>,
        indent: usize,
        span: Span,
    ) -> Result<Vec<Line>> {
        let condition = self.inline(condition, indent)?;
        let mut lines = self.suite(format!("if {condition}:"), span.start, then_branch, indent)?;
        if let Some(otherwise) = else_branch {
            let anchor = self.else_anchor(then_branch.span.end, otherwise.span.start);
            let other = self.suite("else:".into(), anchor, otherwise, indent)?;
            if lines.len() == 1 && other.len() == 1 {
                lines[0].text.push_str(&format!(" {}", other[0].text));
            } else {
                lines.extend(other);
            }
        }
        if lines.len() == 1 {
            lines[0].text = format!("({})", lines[0].text);
        }
        Ok(lines)
    }

    /// Find the else keyword between checked branch spans for inline comment anchoring.
    fn else_anchor(&self, start: usize, end: usize) -> usize {
        self.source
            .get(start..end)
            .and_then(|gap| gap.rfind("else").map(|offset| start + offset))
            .unwrap_or(end)
    }

    /// Render match arms with guard and nested-constructor spelling preserved.
    fn matching(
        &self,
        value: &Expr,
        arms: &[ast::MatchArm],
        indent: usize,
        span: Span,
    ) -> Result<Vec<Line>> {
        let mut lines = vec![line(
            indent,
            format!("match {}:", self.inline(value, indent)?),
            span.start,
        )];
        lines.extend(self.match_arms(arms, indent + 1)?);
        Ok(lines)
    }

    /// Share pattern/guard spelling for ordinary matches and with error handlers.
    fn match_arms(&self, arms: &[ast::MatchArm], indent: usize) -> Result<Vec<Line>> {
        let mut lines = Vec::new();
        for arm in arms {
            let mut header = pattern_text(&arm.pattern);
            if let Some(guard) = &arm.guard {
                header.push_str(&format!(" if {}", self.inline(guard, indent)?));
            }
            header.push_str(" ->");
            lines.extend(self.suite(header, arm.pattern.span.start, &arm.body, indent)?);
        }
        Ok(lines)
    }
}

/// Locate a real module declaration instead of matching its name inside a comment.
fn module_anchor(source: &str) -> usize {
    let mut offset = 0;
    for line in source.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed
            .strip_prefix("module")
            .is_some_and(|rest| rest.starts_with(char::is_whitespace))
        {
            return offset + line.len() - trimmed.len();
        }
        offset += line.len();
    }
    0
}

/// Render concrete/generic source type syntax; inference variables never originate in parsing.
fn type_text(ty: &Type) -> Result<String> {
    Ok(match ty {
        Type::Pid(value) => format!("Pid({})", type_text(value)?),
        Type::ActorFunction(_, _) => {
            return Err(Diagnostic::new(
                Span::default(),
                "internal actor function type cannot be formatted",
            ));
        }
        Type::Int => "Int".into(),
        Type::Range => "Range".into(),
        Type::Float => "Float".into(),
        Type::Bool => "Bool".into(),
        Type::String => "String".into(),
        Type::Unit => "()".into(),
        Type::Native(ty) => ty.name().into(),
        Type::Function(params, result) => format!(
            "({}) -> {}",
            params
                .iter()
                .map(type_text)
                .collect::<Result<Vec<_>>>()?
                .join(", "),
            type_text(result)?
        ),
        Type::Union(members) => members
            .iter()
            .map(|ty| match ty {
                Type::Function(..) | Type::Union(_) => Ok(format!("({})", type_text(ty)?)),
                _ => type_text(ty),
            })
            .collect::<Result<Vec<_>>>()?
            .join(" | "),
        Type::Tuple(fields) => tuple_text(
            fields
                .iter()
                .map(type_text)
                .collect::<Result<Vec<_>>>()?
                .join(", "),
            fields.len(),
        ),
        Type::List(value) => format!("List({})", type_text(value)?),
        Type::Map(key, value) => format!("Map({}, {})", type_text(key)?, type_text(value)?),
        Type::Option(value) => format!("Option({})", type_text(value)?),
        Type::Result(value, error) => {
            format!("Result({}, {})", type_text(value)?, type_text(error)?)
        }
        Type::Generic(name) => name.clone(),
        Type::Named(name, arguments) => {
            if arguments.is_empty() {
                name.clone()
            } else {
                format!(
                    "{name}({})",
                    arguments
                        .iter()
                        .map(type_text)
                        .collect::<Result<Vec<_>>>()?
                        .join(", ")
                )
            }
        }
        Type::Infer(_) | Type::Never => {
            return Err(Diagnostic::new(
                Span::default(),
                "cannot format an unresolved internal type",
            ));
        }
    })
}

/// Render a record or variant field with its optional label.
fn field_text(field: &ast::Field) -> Result<String> {
    let ty = type_text(&field.ty)?;
    Ok(field
        .name
        .as_ref()
        .map_or_else(|| ty.clone(), |name| format!("{name}: {ty}")))
}

/// Render literal, catchall and recursively nested constructor patterns.
fn pattern_text(pattern: &Pattern) -> String {
    match &pattern.kind {
        PatternKind::List { prefix, rest } => {
            format!("[{}]", sequence_pattern_text(prefix, rest.as_deref()))
        }
        PatternKind::TupleRest { prefix, rest } => {
            format!("({})", sequence_pattern_text(prefix, Some(rest)))
        }
        PatternKind::Tuple(fields) => tuple_text(
            fields
                .iter()
                .map(pattern_text)
                .collect::<Vec<_>>()
                .join(", "),
            fields.len(),
        ),
        PatternKind::Wildcard => "_".into(),
        PatternKind::Typed {
            pattern,
            annotation,
        } => format!(
            "{}: {}",
            pattern_text(pattern),
            type_text(annotation).expect("parsed pattern annotation has source syntax")
        ),
        PatternKind::Bind(name) => name.clone(),
        PatternKind::Int(value) => value.to_string(),
        PatternKind::Bool(value) => value.to_string(),
        PatternKind::String(value) => quote(value),
        PatternKind::NamedConstructor { name, fields } => {
            if fields.is_empty() {
                name.clone()
            } else {
                format!(
                    "{name}({})",
                    fields
                        .iter()
                        .map(pattern_text)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }
        PatternKind::Constructor {
            constructor,
            binding,
        } => {
            let name = match constructor {
                Constructor::Some => "Some",
                Constructor::None => "None",
                Constructor::Ok => "Ok",
                Constructor::Err => "Err",
            };
            if *constructor == Constructor::None {
                name.into()
            } else {
                format!("{name}({})", binding.as_deref().unwrap_or("_"))
            }
        }
    }
}

/// Render a source-ordered prefix followed by its optional named or discarded suffix.
fn sequence_pattern_text(prefix: &[Pattern], rest: Option<&Pattern>) -> String {
    let mut fields = prefix.iter().map(pattern_text).collect::<Vec<_>>();
    if let Some(rest) = rest {
        fields.push(format!("..{}", pattern_text(rest)));
    }
    fields.join(", ")
}

/// Preserve physical multiline content while escaping delimiters and interpolation braces.
fn quote_multiline(value: &str, literal: bool) -> String {
    let mut output = String::from("\"\"\"");
    for c in value.chars() {
        match c {
            '\\' => output.push_str("\\\\"),
            '"' => output.push_str("\\\""),
            '{' | '}' if !literal => {
                output.push('\\');
                output.push(c);
            }
            '\r' => output.push_str("\\r"),
            _ => output.push(c),
        }
    }
    output.push_str("\"\"\"");
    output
}

/// Escape supported string characters without altering Unicode or treating # as comments.
fn quote(value: &str) -> String {
    let mut output = String::from("\"");
    for character in value.chars() {
        match character {
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '{' => output.push_str("\\{"),
            '}' => output.push_str("\\}"),
            _ => output.push(character),
        }
    }
    output.push('"');
    output
}

/// Map an AST operator to its canonical Fern token.
fn binary_text(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::In => "in",
        BinaryOp::Power => "**",
        BinaryOp::BitAnd => "&&&",
        BinaryOp::BitOr => "|||",
        BinaryOp::BitXor => "^^^",
        BinaryOp::ShiftLeft => "<<<",
        BinaryOp::ShiftRight => ">>>",
        BinaryOp::Add => "+",
        BinaryOp::Subtract => "-",
        BinaryOp::Multiply => "*",
        BinaryOp::Divide => "/",
        BinaryOp::Remainder => "%",
        BinaryOp::Eq => "==",
        BinaryOp::Ne => "!=",
        BinaryOp::Lt => "<",
        BinaryOp::Le => "<=",
        BinaryOp::Gt => ">",
        BinaryOp::Ge => ">=",
        BinaryOp::And => "and",
        BinaryOp::Or => "or",
    }
}

struct Comment {
    offset: usize,
    line: usize,
    indent: usize,
    inline: bool,
    text: String,
}

/// Collect real line comments, respecting escaped quotes and # within string literals.
fn comments(source: &str) -> Vec<Comment> {
    parse::comment_spans(source)
        .into_iter()
        .map(|span| {
            let start = source[..span.start].rfind('\n').map_or(0, |i| i + 1);
            let prefix = &source[start..span.start];
            Comment {
                offset: span.start,
                line: start,
                indent: prefix.bytes().take_while(|byte| *byte == b' ').count() / 4,
                inline: !prefix.trim().is_empty(),
                text: source[span.start..span.end].trim_end().into(),
            }
        })
        .collect()
}

/// Index source lines and rendered anchors once rather than rescanning them per comment.
fn comment_slots(source: &str, lines: &[Line], comments: &[Comment]) -> Vec<usize> {
    let mut starts = vec![0];
    starts.extend(
        source
            .bytes()
            .enumerate()
            .filter_map(|(index, byte)| (byte == b'\n').then_some(index + 1)),
    );
    let mut same_line = std::collections::BTreeMap::new();
    let mut anchors = Vec::with_capacity(lines.len());
    for (index, line) in lines.iter().enumerate() {
        let source_line = starts[starts
            .partition_point(|start| *start <= line.anchor)
            .saturating_sub(1)];
        if !line.text.is_empty() {
            same_line.insert(source_line, index);
        }
        anchors.push((line.anchor, index));
    }
    anchors.sort_unstable();
    let mut following = vec![lines.len(); anchors.len() + 1];
    for index in (0..anchors.len()).rev() {
        following[index] = following[index + 1].min(anchors[index].1);
    }
    comments
        .iter()
        .map(|comment| {
            let next =
                following[anchors.partition_point(|(anchor, _)| *anchor <= comment.offset)] * 2;
            if comment.inline {
                same_line
                    .get(&comment.line)
                    .map_or(next, |index| index * 2 + 1)
            } else {
                next
            }
        })
        .collect()
}

/// Attach comments monotonically to original line anchors, preserving their total order.
fn attach_comments(source: &str, lines: Vec<Line>, comments: &[Comment]) -> String {
    let mut before: Vec<Vec<(usize, String)>> = (0..=lines.len()).map(|_| Vec::new()).collect();
    let mut suffix: Vec<Option<String>> = (0..lines.len()).map(|_| None).collect();
    let mut previous_slot = 0;
    for (comment, desired) in comments.iter().zip(comment_slots(source, &lines, comments)) {
        let mut slot = desired.max(previous_slot);
        if slot % 2 == 1 && suffix[slot / 2].is_some() {
            slot += 1;
        }
        if slot % 2 == 1 {
            suffix[slot / 2] = Some(comment.text.clone());
        } else {
            let index = slot / 2;
            let indent = lines.get(index).map_or(comment.indent, |line| line.indent);
            before[index].push((indent, comment.text.clone()));
        }
        previous_slot = slot;
    }
    let mut output = String::new();
    for (index, line) in lines.iter().enumerate() {
        for (indent, text) in &before[index] {
            output.push_str(&format!("{}{text}\n", "    ".repeat(*indent)));
        }
        if !line.text.is_empty() {
            output.push_str(&"    ".repeat(line.indent));
            output.push_str(&line.text);
        }
        if let Some(comment) = &suffix[index] {
            output.push_str("  ");
            output.push_str(comment);
        }
        output.push('\n');
    }
    for (indent, text) in &before[lines.len()] {
        output.push_str(&format!("{}{text}\n", "    ".repeat(*indent)));
    }
    output
}

/// Compare source syntax independently of locations after parsing the rendered artifact.
fn structural(mut program: ast::Program) -> String {
    for doc in &mut program.docs {
        doc.span = Span::default();
    }
    let mut groups = std::collections::BTreeMap::new();
    for function in &mut program.functions {
        let next = groups.len();
        function.group_start = *groups
            .entry((function.name.clone(), function.group_start))
            .or_insert(next);
        function.span = Span::default();
        for param in &mut function.params {
            param.span = Span::default();
            clear_label(&mut param.label);
            clear_pattern(&mut param.pattern);
        }
        if let Some(guard) = &mut function.guard {
            clear_expression(guard);
        }
        clear_expression(&mut function.body);
    }
    for alias in &mut program.aliases {
        alias.span = Span::default();
    }
    for decl in &mut program.newtypes {
        decl.span = Span::default();
        decl.constructor_span = Span::default();
        decl.inner_span = Span::default();
        for derive in &mut decl.derives {
            derive.span = Span::default();
        }
    }
    for declaration in &mut program.types {
        declaration.span = Span::default();
        for derive in &mut declaration.derives {
            derive.span = Span::default();
        }
        for variant in &mut declaration.variants {
            variant.span = Span::default();
            for field in &mut variant.fields {
                field.span = Span::default();
            }
        }
    }
    for import in &mut program.imports {
        import.span = Span::default();
    }
    format!("{program:?}")
}

/// Clear expression/statement spans recursively, retaining every semantic AST field.
fn clear_expression(expression: &mut Expr) {
    expression.span = Span::default();
    match &mut expression.kind {
        ExprKind::Receive { arms, timeout } => clear_receive(arms, timeout),
        ExprKind::TypeTarget(_) => {}
        ExprKind::Unary { value, .. }
        | ExprKind::Try(value)
        | ExprKind::Return(value)
        | ExprKind::Defer(value)
        | ExprKind::Field { value, .. } => clear_expression(value),
        ExprKind::Binary { left, right, .. }
        | ExprKind::Range {
            start: left,
            end: right,
            ..
        } => {
            clear_expression(left);
            clear_expression(right);
        }
        kind @ (ExprKind::PostfixIf { .. }
        | ExprKind::ConditionMatch(_)
        | ExprKind::For { .. }
        | ExprKind::With { .. }
        | ExprKind::If { .. }) => clear_control(kind),
        ExprKind::Pipe {
            value, args, label, ..
        }
        | ExprKind::GlobalPipe {
            value, args, label, ..
        } => {
            clear_label(label);
            clear_expression(value);
            args.iter_mut().for_each(clear_argument);
        }
        ExprKind::Lambda { params, body } => {
            for param in params {
                param.span = Span::default();
            }
            clear_expression(body);
        }
        ExprKind::Apply { callee, args } => {
            clear_expression(callee);
            args.iter_mut().for_each(clear_argument);
        }
        ExprKind::Interpolate(parts) | ExprKind::MultilineString(parts) => {
            clear_string_parts(parts)
        }
        ExprKind::Call { args, .. } | ExprKind::GlobalCall { args, .. } => {
            args.iter_mut().for_each(clear_argument);
        }
        ExprKind::Tuple(args) | ExprKind::List(args) => {
            args.iter_mut().for_each(clear_expression);
        }
        ExprKind::Match { value, arms } => clear_match(value, arms),
        ExprKind::Map(pairs) => clear_pairs(pairs),
        ExprKind::RecordUpdate { value, fields } => clear_update(value, fields),
        ExprKind::Block(statements) => clear_statements(statements),
        ExprKind::Int(_)
        | ExprKind::Break
        | ExprKind::Continue
        | ExprKind::Float(_)
        | ExprKind::Bool(_)
        | ExprKind::String(_)
        | ExprKind::Name(_)
        | ExprKind::GlobalName { .. }
        | ExprKind::Unit => {}
    }
}

/// Literal segments carry no source spans; only their embedded values need clearing.
fn clear_string_parts(parts: &mut [ast::StringPart]) {
    for part in parts {
        if let ast::StringPart::Value(value) = part {
            clear_expression(value);
        }
    }
}

/// Clear control children once, preserving iteration and error handler scopes.
fn clear_control(kind: &mut ExprKind) {
    match kind {
        ExprKind::PostfixIf { value, condition } => {
            clear_expression(value);
            clear_expression(condition);
        }
        ExprKind::ConditionMatch(arms) => clear_conditions(arms),
        ExprKind::For {
            pattern,
            iterable,
            body,
        } => {
            clear_pattern(pattern);
            clear_expression(iterable);
            clear_expression(body);
        }
        ExprKind::With {
            bindings,
            body,
            arms,
        } => clear_with(bindings, body, arms),
        ExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            clear_expression(condition);
            clear_expression(then_branch);
            if let Some(otherwise) = else_branch {
                clear_expression(otherwise);
            }
        }
        _ => {}
    }
}

/// Clear sequential-binding and handler locations without changing their distinct scopes.
fn clear_with(
    bindings: &mut [ast::WithBinding],
    body: &mut Expr,
    arms: &mut Option<Vec<ast::MatchArm>>,
) {
    for binding in bindings {
        binding.span = Span::default();
        clear_pattern(&mut binding.pattern);
        clear_expression(&mut binding.value);
    }
    clear_expression(body);
    if let Some(arms) = arms {
        clear_arms(arms);
    }
}

/// Clear condition-arm locations and both expression children in place.
fn clear_conditions(arms: &mut [ast::ConditionArm]) {
    for arm in arms {
        arm.span = Span::default();
        if let Some(condition) = &mut arm.condition {
            clear_expression(condition);
        }
        clear_expression(&mut arm.body);
    }
}

/// Clear both map children without reordering keys or values.
fn clear_pairs(pairs: &mut [(Expr, Expr)]) {
    for (key, value) in pairs {
        clear_expression(key);
        clear_expression(value);
    }
}

/// Clear replacement locations while retaining the base and source field sequence.
fn clear_update(value: &mut Expr, fields: &mut [ast::RecordField]) {
    clear_expression(value);
    for field in fields {
        field.span = Span::default();
        clear_expression(&mut field.value);
    }
}

/// Clear match locations and its nested guards, patterns and values together.
fn clear_match(value: &mut Expr, arms: &mut [ast::MatchArm]) {
    clear_expression(value);
    clear_arms(arms);
}

/// Clear each arm once to keep nested with/match traversal linear.
fn clear_arms(arms: &mut [ast::MatchArm]) {
    for arm in arms {
        arm.span = Span::default();
        clear_pattern(&mut arm.pattern);
        if let Some(guard) = &mut arm.guard {
            clear_expression(guard);
        }
        clear_expression(&mut arm.body);
    }
}

/// Clear block statement locations without changing binding patterns or initializers.
fn clear_statements(statements: &mut [Stmt]) {
    for statement in statements {
        match statement {
            Stmt::LetElse {
                pattern,
                value,
                else_branch,
                span,
                ..
            } => {
                *span = Span::default();
                clear_pattern(pattern);
                clear_expression(value);
                clear_expression(else_branch);
            }
            Stmt::Expr(value) => clear_expression(value),
            Stmt::LetPattern {
                pattern,
                value,
                span,
                ..
            } => {
                *span = Span::default();
                clear_pattern(pattern);
                clear_expression(value);
            }
            Stmt::Let { value, span, .. } => {
                *span = Span::default();
                clear_expression(value);
            }
        }
    }
}

/// Remove pattern locations while keeping nested constructor structure intact.
fn clear_pattern(pattern: &mut Pattern) {
    pattern.span = Span::default();
    match &mut pattern.kind {
        PatternKind::Typed { pattern, .. } => clear_pattern(pattern),
        PatternKind::NamedConstructor { fields, .. } | PatternKind::Tuple(fields) => {
            for field in fields {
                clear_pattern(field);
            }
        }
        PatternKind::List { prefix, rest } => {
            for field in prefix {
                clear_pattern(field);
            }
            if let Some(rest) = rest {
                clear_pattern(rest);
            }
        }
        PatternKind::TupleRest { prefix, rest } => {
            for field in prefix {
                clear_pattern(field);
            }
            clear_pattern(rest);
        }
        _ => {}
    }
}

/// Preserve singleton tuple syntax while rendering positional fields.
fn tuple_text(fields: String, count: usize) -> String {
    format!("({fields}{})", if count == 1 { "," } else { "" })
}

/// Preserve explicit external labels independently of the pattern's local bindings.
fn parameter_text(param: &ast::Param) -> String {
    let pattern = pattern_text(&param.pattern);
    match &param.label {
        Some(label) => format!("{} {pattern}", label.name),
        None => pattern,
    }
}

/// Remove only lexical locations before formatter syntax-equivalence comparison.
fn clear_label(label: &mut Option<ast::ArgumentLabel>) {
    if let Some(label) = label {
        label.span = Span::default();
    }
}

/// Keep argument labels and written order while erasing their source positions.
fn clear_argument(arg: &mut ast::Argument) {
    arg.span = Span::default();
    clear_label(&mut arg.label);
    clear_expression(&mut arg.value);
}

/// Preserve explicit derivation requests on a newtype header.
fn derive_text(derives: &[ast::Derivation]) -> String {
    if derives.is_empty() {
        return String::new();
    }
    let names: Vec<_> = derives.iter().map(|d| d.name.as_str()).collect();
    format!(" derive({})", names.join(", "))
}

/// Clear receive children without leaking arm or timeout source positions into equivalence checks.
fn clear_receive(arms: &mut [ast::MatchArm], timeout: &mut Option<(Box<Expr>, Box<Expr>)>) {
    clear_arms(arms);
    if let Some((duration, body)) = timeout {
        clear_expression(duration);
        clear_expression(body);
    }
}
