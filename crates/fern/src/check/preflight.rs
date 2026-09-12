//! Bound caller-created syntax before recursive cloning for monomorphization.
use super::{
    Checked, MAX_EXPR_COUNT, MAX_EXPR_DEPTH, MAX_FUNCTIONS, MAX_PARAMETERS, validate_type_structure,
};
use crate::{Diagnostic, Span, Type, ast};

struct Budget {
    nodes: usize,
    bytes: usize,
}
impl Budget {
    /// Charge a source node and its owned text before copying it.
    fn charge(&mut self, bytes: usize, span: Span) -> Checked<()> {
        self.nodes += 1;
        self.bytes = self.bytes.saturating_add(bytes);
        if self.nodes > MAX_EXPR_COUNT || self.bytes > 1024 * 1024 {
            return Err(Diagnostic::new(
                span,
                "prototype syntax size limit exceeded",
            ));
        }
        Ok(())
    }
    /// Check recursive annotations iteratively and account for their names.
    fn ty(&mut self, ty: &Type, span: Span) -> Checked<()> {
        validate_type_structure(ty, span, true)?;
        let mut pending = vec![ty];
        while let Some(ty) = pending.pop() {
            let bytes = match ty {
                Type::Named(name, _) | Type::Generic(name) => name.len(),
                _ => 0,
            };
            self.charge(bytes, span)?;
            match ty {
                Type::Named(_, args) => pending.extend(args),
                Type::Function(args, result) => {
                    pending.extend(args);
                    pending.push(result);
                }
                Type::Union(args) | Type::Tuple(args) => pending.extend(args),
                Type::List(a) | Type::Option(a) => pending.push(a),
                Type::ActorFunction(a, b) | Type::Result(a, b) | Type::Map(a, b) => {
                    pending.push(a);
                    pending.push(b);
                }
                _ => {}
            }
        }
        Ok(())
    }
    /// Validate one bounded pattern tree, including otherwise unused generic bodies.
    fn pattern(&mut self, pattern: &ast::Pattern) -> Checked<()> {
        let mut pending = vec![(pattern, 0)];
        while let Some((pattern, depth)) = pending.pop() {
            if depth >= MAX_EXPR_DEPTH {
                return Err(Diagnostic::new(
                    pattern.span,
                    "pattern nesting limit exceeded",
                ));
            }
            let bytes = match &pattern.kind {
                ast::PatternKind::Typed {
                    pattern: inner,
                    annotation,
                } => {
                    self.ty(annotation, pattern.span)?;
                    pending.push((inner, depth + 1));
                    0
                }
                ast::PatternKind::List { prefix, rest } => {
                    self.sequence(prefix, rest.as_deref(), &mut pending, depth, pattern.span)?;
                    0
                }
                ast::PatternKind::TupleRest { prefix, rest } => {
                    self.sequence(prefix, Some(rest), &mut pending, depth, pattern.span)?;
                    0
                }
                ast::PatternKind::Tuple(fields) => {
                    if fields.len() > 128 {
                        return Err(Diagnostic::new(
                            pattern.span,
                            "sequence pattern prefix limit exceeded",
                        ));
                    }
                    pending.extend(fields.iter().map(|p| (p, depth + 1)));
                    0
                }
                ast::PatternKind::Bind(n) | ast::PatternKind::String(n) => n.len(),
                ast::PatternKind::Constructor { binding, .. } => {
                    binding.as_ref().map_or(0, String::len)
                }
                ast::PatternKind::NamedConstructor { name, fields } => {
                    pending.extend(fields.iter().map(|p| (p, depth + 1)));
                    name.len()
                }
                _ => 0,
            };
            self.charge(bytes, pattern.span)?;
        }
        Ok(())
    }
    /// Bound flat sequence fields and reject recursively structured rest payloads before copies.
    fn sequence<'a>(
        &mut self,
        prefix: &'a [ast::Pattern],
        rest: Option<&'a ast::Pattern>,
        pending: &mut Vec<(&'a ast::Pattern, usize)>,
        depth: usize,
        span: Span,
    ) -> Checked<()> {
        if prefix.len() > 128 {
            return Err(Diagnostic::new(
                span,
                "sequence pattern prefix limit exceeded",
            ));
        }
        pending.extend(prefix.iter().map(|p| (p, depth + 1)));
        if let Some(rest) = rest {
            if !matches!(
                rest.kind,
                ast::PatternKind::Bind(_) | ast::PatternKind::Wildcard
            ) {
                return Err(Diagnostic::new(
                    rest.span,
                    "sequence rest must be a binding or wildcard",
                ));
            }
            pending.push((rest, depth + 1));
        }
        Ok(())
    }
    /// Check block annotations and enqueue initializers without recursive traversal.
    fn block<'a>(
        &mut self,
        stmts: &'a [ast::Stmt],
        pending: &mut Vec<(&'a ast::Expr, usize)>,
        depth: usize,
    ) -> Checked<()> {
        for stmt in stmts {
            if let ast::Stmt::LetElse { else_branch, .. } = stmt {
                pending.push((else_branch, depth + 1));
            }
            match stmt {
                ast::Stmt::Let {
                    name,
                    annotation,
                    value,
                    span,
                } => {
                    self.charge(name.len(), *span)?;
                    if let Some(ty) = annotation {
                        self.ty(ty, *span)?;
                    }
                    pending.push((value, depth + 1));
                }
                ast::Stmt::LetElse {
                    pattern,
                    annotation,
                    value,
                    span,
                    ..
                }
                | ast::Stmt::LetPattern {
                    pattern,
                    annotation,
                    value,
                    span,
                } => {
                    self.pattern(pattern)?;
                    if let Some(ty) = annotation {
                        self.ty(ty, *span)?;
                    }
                    pending.push((value, depth + 1));
                }
                ast::Stmt::Expr(value) => pending.push((value, depth + 1)),
            }
        }
        Ok(())
    }

    /// Charge interpolation text and enqueue expressions at the enclosing traversal depth.
    fn interpolation<'a>(
        &mut self,
        parts: &'a [ast::StringPart],
        pending: &mut Vec<(&'a ast::Expr, usize)>,
        depth: usize,
        span: Span,
    ) -> Checked<()> {
        for part in parts {
            match part {
                ast::StringPart::Text(text) => self.charge(text.len(), span)?,
                ast::StringPart::Value(value) => pending.push((value, depth + 1)),
            }
        }
        Ok(())
    }

    /// Check lambda annotations and duplicate names even in uninstantiated generic templates.
    fn lambda_params(&mut self, params: &[ast::LambdaParam], span: Span) -> Checked<()> {
        if params.len() > MAX_PARAMETERS {
            return Err(Diagnostic::new(span, "lambda parameter limit exceeded"));
        }
        let mut names = std::collections::HashSet::new();
        for param in params {
            if !names.insert(&param.name) {
                return Err(Diagnostic::new(param.span, "duplicate lambda parameter"));
            }
            self.charge(param.name.len(), param.span)?;
            if let Some(ty) = &param.annotation {
                self.ty(ty, param.span)?;
            }
        }
        Ok(())
    }

    /// Check patterns before enqueueing each arm body and guard at its inherited depth.
    fn match_arms<'a>(
        &mut self,
        arms: &'a [ast::MatchArm],
        pending: &mut Vec<(&'a ast::Expr, usize)>,
        depth: usize,
    ) -> Checked<()> {
        for arm in arms {
            self.pattern(&arm.pattern)?;
            pending.push((&arm.body, depth + 1));
            if let Some(guard) = &arm.guard {
                pending.push((guard, depth + 1));
            }
        }
        Ok(())
    }

    /// Charge source/resolved names and all named-reference children before checker cloning.
    fn reference_expression<'a>(
        &mut self,
        expr: &'a ast::Expr,
        pending: &mut Vec<(&'a ast::Expr, usize)>,
        depth: usize,
    ) -> Checked<bool> {
        match &expr.kind {
            ast::ExprKind::TypeTarget(_) => {
                return Err(Diagnostic::new(
                    expr.span,
                    "compile-time type target cannot be used as a value",
                ));
            }

            ast::ExprKind::GlobalName { name, resolved } => {
                self.charge(name.len(), expr.span)?;
                self.charge(resolved.len(), expr.span)?;
            }
            ast::ExprKind::GlobalCall {
                name,
                resolved,
                args,
            } => {
                self.charge(name.len(), expr.span)?;
                self.charge(resolved.len(), expr.span)?;
                self.reference_arguments(expr, args, pending, depth)?;
            }
            ast::ExprKind::GlobalPipe {
                name,
                resolved,
                value,
                args,
                ..
            } => {
                self.charge(name.len(), expr.span)?;
                self.charge(resolved.len(), expr.span)?;
                pending.push((value, depth + 1));
                self.reference_arguments(expr, args, pending, depth)?;
            }
            ast::ExprKind::Pipe {
                value, name, args, ..
            } => {
                self.charge(name.len(), expr.span)?;
                pending.push((value, depth + 1));
                self.reference_arguments(expr, args, pending, depth)?;
            }
            ast::ExprKind::Call { name, args } => {
                self.charge(name.len(), expr.span)?;
                self.reference_arguments(expr, args, pending, depth)?;
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// A static type leaf is permitted only as the canonical decoder's complete target argument.
    fn reference_arguments<'a>(
        &mut self,
        expr: &'a ast::Expr,
        args: &'a [ast::Argument],
        pending: &mut Vec<(&'a ast::Expr, usize)>,
        depth: usize,
    ) -> Checked<()> {
        let slot = crate::codec_syntax::static_slot(expr);
        for (index, arg) in args.iter().enumerate() {
            if let ast::ExprKind::TypeTarget(ty) = &arg.value.kind {
                if slot != Some(index) {
                    return Err(Diagnostic::new(
                        arg.span,
                        "compile-time type target cannot be used as a value",
                    ));
                }
                self.ty(ty, arg.span)?;
            } else {
                pending.push((&arg.value, depth + 1));
            }
        }
        Ok(())
    }

    /// Charge expression nodes only after checking their current traversal depth.
    fn expression_node(&mut self, span: Span, depth: usize) -> Checked<()> {
        if depth >= MAX_EXPR_DEPTH {
            return Err(Diagnostic::new(
                span,
                "prototype expression nesting limit exceeded (128)",
            ));
        }
        self.charge(0, span)
    }

    /// Charge record field names and queue initializer expressions before cloning.
    fn update_fields<'a>(
        &mut self,
        fields: &'a [ast::RecordField],
        pending: &mut Vec<(&'a ast::Expr, usize)>,
        depth: usize,
    ) -> Checked<()> {
        for field in fields {
            self.charge(field.name.len(), field.span)?;
            pending.push((&field.value, depth + 1));
        }
        Ok(())
    }

    /// Queue flat control flow and charge each newly introduced binding pattern.
    fn iteration<'a>(
        &mut self,
        expr: &'a ast::Expr,
        pending: &mut Vec<(&'a ast::Expr, usize)>,
        depth: usize,
    ) -> Checked<bool> {
        match &expr.kind {
            ast::ExprKind::Range { start, end, .. } => {
                pending.push((start, depth + 1));
                pending.push((end, depth + 1));
            }
            ast::ExprKind::For {
                pattern,
                iterable,
                body,
            } => {
                self.pattern(pattern)?;
                pending.push((iterable, depth + 1));
                pending.push((body, depth + 1));
            }
            ast::ExprKind::With {
                bindings,
                body,
                arms,
            } => {
                for binding in bindings {
                    self.pattern(&binding.pattern)?;
                    pending.push((&binding.value, depth + 1));
                }
                pending.push((body, depth + 1));
                if let Some(arms) = arms {
                    self.match_arms(arms, pending, depth)?;
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Bound receive metadata before pushing its independently scoped expression children.
    fn receive<'a>(
        &mut self,
        arms: &'a [ast::MatchArm],
        timeout: &'a Option<(Box<ast::Expr>, Box<ast::Expr>)>,
        span: Span,
        pending: &mut Vec<(&'a ast::Expr, usize)>,
        depth: usize,
    ) -> Checked<()> {
        if arms.is_empty() || arms.len() > 128 {
            return Err(Diagnostic::new(span, "receive arm limit exceeded"));
        }
        self.match_arms(arms, pending, depth)?;
        if let Some((duration, body)) = timeout {
            pending.extend([(duration.as_ref(), depth + 1), (body.as_ref(), depth + 1)]);
        }
        Ok(())
    }

    /// Validate expression depth and annotations before source-instance cloning.
    fn expression(&mut self, expr: &ast::Expr) -> Checked<()> {
        let mut pending = vec![(expr, 0)];
        while let Some((expr, depth)) = pending.pop() {
            self.expression_node(expr.span, depth)?;
            self.argument_metadata(expr)?;
            if self.iteration(expr, &mut pending, depth)? {
                continue;
            }
            if self.reference_expression(expr, &mut pending, depth)? {
                continue;
            }
            match &expr.kind {
                ast::ExprKind::Receive { arms, timeout } => {
                    self.receive(arms, timeout, expr.span, &mut pending, depth)?
                }
                ast::ExprKind::ConditionMatch(arms) => queue_conditions(arms, &mut pending, depth),
                ast::ExprKind::Interpolate(parts) | ast::ExprKind::MultilineString(parts) => {
                    self.interpolation(parts, &mut pending, depth, expr.span)?
                }
                ast::ExprKind::Map(entries) => queue_map(entries, &mut pending, depth),
                ast::ExprKind::RecordUpdate { value, fields } => {
                    pending.push((value, depth + 1));
                    self.update_fields(fields, &mut pending, depth)?;
                }
                ast::ExprKind::Lambda { params, body } => {
                    self.lambda_params(params, expr.span)?;
                    pending.push((body, depth + 1));
                }
                ast::ExprKind::Apply { callee, args } => {
                    pending.push((callee, depth + 1));
                    pending.extend(args.iter().map(|a| (&a.value, depth + 1)));
                }
                ast::ExprKind::Name(n) | ast::ExprKind::String(n) => {
                    self.charge(n.len(), expr.span)?
                }
                ast::ExprKind::Return(value)
                | ast::ExprKind::Defer(value)
                | ast::ExprKind::Unary { value, .. }
                | ast::ExprKind::Try(value) => pending.push((value, depth + 1)),
                ast::ExprKind::Field { value, name } => {
                    self.charge(name.len(), expr.span)?;
                    pending.push((value, depth + 1));
                }
                ast::ExprKind::PostfixIf {
                    value: left,
                    condition: right,
                }
                | ast::ExprKind::Binary { left, right, .. } => {
                    pending.push((left, depth + 1));
                    pending.push((right, depth + 1));
                }
                ast::ExprKind::Tuple(args) | ast::ExprKind::List(args) => {
                    pending.extend(args.iter().map(|e| (e, depth + 1)))
                }
                ast::ExprKind::If { .. } => queue_if(expr, &mut pending, depth),
                ast::ExprKind::Match { value, arms } => {
                    pending.push((value, depth + 1));
                    self.match_arms(arms, &mut pending, depth)?;
                }
                ast::ExprKind::Block(stmts) => self.block(stmts, &mut pending, depth)?,
                _ => {}
            }
        }
        Ok(())
    }
}

/// Bound the entire source AST before registry copies and specialization work begin.
pub(super) fn check(program: &ast::Program) -> Checked<()> {
    if program.functions.len() > MAX_FUNCTIONS
        || program
            .types
            .len()
            .saturating_add(program.aliases.len())
            .saturating_add(program.newtypes.len())
            > MAX_FUNCTIONS
    {
        return Err(Diagnostic::new(
            Span::default(),
            "prototype declaration count limit exceeded",
        ));
    }
    let mut budget = Budget { nodes: 0, bytes: 0 };
    for doc in &program.docs {
        budget.charge(doc.target.len().saturating_add(doc.text.len()), doc.span)?;
    }
    for function in &program.functions {
        budget.charge(function.name.len(), function.span)?;
        for param in &function.params {
            budget.label(&param.label)?;
            budget.pattern(&param.pattern)?;
            if let Some(ty) = &param.annotation {
                budget.ty(ty, param.span)?;
            }
        }
        if let Some(ty) = &function.return_type {
            budget.ty(ty, function.span)?;
        }
        if let Some(guard) = &function.guard {
            budget.expression(guard)?;
        }
        budget.expression(&function.body)?;
    }
    aliases(program, &mut budget)?;
    newtypes(program, &mut budget)?;
    for decl in &program.types {
        budget.charge(decl.name.len(), decl.span)?;
        derivations(&decl.derives, decl.span, &mut budget)?;
        if decl.parameters.len() > MAX_PARAMETERS || decl.variants.len() > MAX_PARAMETERS {
            return Err(Diagnostic::new(
                decl.span,
                "type declaration arity limit exceeded",
            ));
        }
        for name in &decl.parameters {
            budget.charge(name.len(), decl.span)?;
        }
        for variant in &decl.variants {
            budget.charge(variant.name.len(), variant.span)?;
            if variant.fields.len() > MAX_PARAMETERS {
                return Err(Diagnostic::new(
                    variant.span,
                    "constructor field count limit exceeded",
                ));
            }
            for field in &variant.fields {
                budget.charge(field.name.as_ref().map_or(0, String::len), field.span)?;
                budget.ty(&field.ty, field.span)?;
            }
        }
    }
    Ok(())
}

/// Account for alias targets and formal names before any source cloning or expansion.
fn aliases(program: &ast::Program, budget: &mut Budget) -> Checked<()> {
    for alias in &program.aliases {
        budget.charge(alias.name.len(), alias.span)?;
        if alias.parameters.len() > MAX_PARAMETERS {
            return Err(Diagnostic::new(
                alias.span,
                "type alias arity limit exceeded",
            ));
        }
        for name in &alias.parameters {
            budget.charge(name.len(), alias.span)?;
        }
        budget.ty(&alias.target, alias.span)?;
    }
    Ok(())
}

/// Queue map keys and values at the same bounded expression depth.
fn queue_map<'a>(
    entries: &'a [(ast::Expr, ast::Expr)],
    pending: &mut Vec<(&'a ast::Expr, usize)>,
    depth: usize,
) {
    pending.extend(
        entries
            .iter()
            .flat_map(|(key, value)| [(key, depth + 1), (value, depth + 1)]),
    );
}

/// Queue lazy condition branches without increasing source nesting for sibling arms.
fn queue_conditions<'a>(
    arms: &'a [ast::ConditionArm],
    pending: &mut Vec<(&'a ast::Expr, usize)>,
    depth: usize,
) {
    for arm in arms {
        pending.extend(arm.condition.iter().map(|c| (c, depth + 1)));
        pending.push((&arm.body, depth + 1));
    }
}
/// Queue each branch at its bounded lexical child depth.
fn queue_if<'a>(expr: &'a ast::Expr, pending: &mut Vec<(&'a ast::Expr, usize)>, depth: usize) {
    if let ast::ExprKind::If {
        condition,
        then_branch,
        else_branch,
    } = &expr.kind
    {
        pending.push((condition, depth + 1));
        pending.push((then_branch, depth + 1));
        pending.extend(else_branch.as_deref().map(|n| (n, depth + 1)));
    }
}

/// Bound newtype declarations before copying or expanding any storage metadata.
fn newtypes(program: &ast::Program, budget: &mut Budget) -> Checked<()> {
    for decl in &program.newtypes {
        budget.charge(decl.name.len() + decl.constructor.len(), decl.span)?;
        if decl.parameters.len() > MAX_PARAMETERS {
            return Err(Diagnostic::new(
                decl.span,
                "newtype parameter limit exceeded",
            ));
        }
        for name in &decl.parameters {
            budget.charge(name.len(), decl.span)?;
        }
        budget.ty(&decl.inner, decl.inner_span)?;
        derivations(&decl.derives, decl.span, budget)?;
    }
    Ok(())
}

impl Budget {
    /// Charge caller-owned label text before cloning or validating a signature interface.
    fn label(&mut self, label: &Option<ast::ArgumentLabel>) -> Checked<()> {
        if let Some(label) = label {
            self.charge(label.name.len(), label.span)?;
            if !crate::parse::valid_label(&label.name) || label.span.start > label.span.end {
                return Err(Diagnostic::new(label.span, "invalid argument label"));
            }
        }
        Ok(())
    }

    /// Bound written arguments and pipe metadata even in inactive caller-created source ASTs.
    fn argument_metadata(&mut self, expr: &ast::Expr) -> Checked<()> {
        let args = match &expr.kind {
            ast::ExprKind::Pipe { args, label, .. }
            | ast::ExprKind::GlobalPipe { args, label, .. } => {
                self.label(label)?;
                args
            }
            ast::ExprKind::Call { args, .. }
            | ast::ExprKind::GlobalCall { args, .. }
            | ast::ExprKind::Apply { args, .. } => args,
            _ => return Ok(()),
        };
        if args.len() > MAX_PARAMETERS {
            return Err(Diagnostic::new(expr.span, "call argument limit exceeded"));
        }
        for arg in args {
            self.label(&arg.label)?;
        }
        Ok(())
    }
}

/// Bound source trait metadata before any nominal registry clones it.
fn derivations(derives: &[ast::Derivation], span: Span, budget: &mut Budget) -> Checked<()> {
    if derives.len() > 32 {
        return Err(Diagnostic::new(
            span,
            "derive trait count limit exceeded (32)",
        ));
    }
    for derive in derives {
        budget.charge(derive.name.len(), derive.span)?;
    }
    Ok(())
}
