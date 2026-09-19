//! Work-list monomorphization: concrete IR is the only input to lowering.
use super::{Checked, Checker, Inference, MAX_FUNCTIONS, Signature, nominal};
use crate::{Diagnostic, Span, Type, ast, ir};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Context { Plain, Root, Actor(Type) }

struct Instance {
    context: Context,
    template: usize,
    arguments: Vec<Type>,
}
struct Driver<'a> {
    registry: &'a nominal::Registry,
    source: &'a ast::Program,
    signatures: &'a HashMap<String, Signature>,
    instances: Vec<Instance>,
    seen: HashMap<(usize, Vec<Type>, Context), ir::FunctionId>,
    context: Context,
    codec_plans: HashMap<usize, std::rc::Rc<crate::json_codec::Plan>>,
}

/// Check nongeneric definitions and every demanded generic specialization.
pub(super) fn run(
    program: &ast::Program,
    registry: &nominal::Registry,
    signatures: &HashMap<String, Signature>,
) -> Checked<ir::Program> {
    let mut driver = Driver {
        registry,
        source: program,
        signatures,
        instances: Vec::new(),
        seen: HashMap::new(),
        context: Context::Root,
        codec_plans: HashMap::new(),
    };
    for (index, function) in program.functions.iter().enumerate() {
        if function.syntax == ast::FunctionSyntax::Trait {
            continue;
        }
        if function.name.starts_with("$traits_") {
            continue;
        }
        if signatures[&function.name].generics.is_empty() {
            let context = signatures[&function.name].mailbox.clone().map(Context::Actor).unwrap_or(Context::Root);
            driver.enqueue(index, Vec::new(), context)?;
        }
    }
    let mut functions = Vec::new();
    let mut index = 0;
    while index < driver.instances.len() {
        let instance = &driver.instances[index];
        let original = &program.functions[instance.template];
        let substitutions = signatures[&original.name]
            .generics
            .iter()
            .cloned()
            .zip(instance.arguments.iter().cloned())
            .collect();
        let function = source_instance(original, &substitutions)?;
        let mailbox = match &instance.context { Context::Actor(mailbox) => Some(mailbox.clone()), _ => None };
        let mut checked = Checker {
            mailbox,
            editor: None,
            recovery: None,
            signatures,
            registry,
            scopes: vec![HashMap::new()],
            local_count: 0,
            expr_count: 0,
            inference: Inference {
                codec_substitutions: substitutions,
                ..Inference::default()
            },
            function_return: Type::Unit,
            deferred: false,
            loop_depth: 0,
        }
        .function(&function)?;
        checked.id = ir::FunctionId(index);
        driver.context = checked.mailbox.clone().map(Context::Actor).unwrap_or(Context::Root);
        driver.rewrite(&mut checked.body)?;
        functions.push(checked);
        index += 1;
    }
    super::lift::run(&mut functions)?;
    let types = registry.layouts(&functions)?;
    Ok(ir::Program { functions, types })
}

impl Driver<'_> {
    /// Deduplicate an instance before its body is checked so recursion terminates.
    fn enqueue(&mut self, template: usize, arguments: Vec<Type>, context: Context) -> Checked<ir::FunctionId> {
        let context = if self.signatures[&self.source.functions[template].name].contextual { context } else { Context::Plain };
        let key = (template, arguments.clone(), context.clone());
        if let Some(id) = self.seen.get(&key) {
            return Ok(*id);
        }
        if self.instances.len() >= MAX_FUNCTIONS {
            return Err(Diagnostic::new(
                Span::default(),
                "generic specialization limit exceeded",
            ));
        }
        let id = ir::FunctionId(self.instances.len());
        self.seen.insert(key, id);
        self.instances.push(Instance {
            context,
            template,
            arguments,
        });
        Ok(id)
    }

    /// Infer the demanded concrete instance from this already checked call signature.
    fn target(
        &mut self,
        template: usize,
        args: &[ir::Expr],
        result: &Type,
    ) -> Checked<ir::FunctionId> {
        self.target_types(
            template,
            &args.iter().map(|a| a.ty.clone()).collect::<Vec<_>>(),
            result,
            None,
            self.context.clone(),
        )
    }

    fn target_types(
        &mut self,
        template: usize,
        args: &[Type],
        result: &Type,
        mailbox: Option<&Type>,
        context: Context,
    ) -> Checked<ir::FunctionId> {
        let function = &self.source.functions[template];
        let signature = &self.signatures[&function.name];
        if let Some(target) =
            self.registry
                .traits
                .target(&function.name, signature, args, result, function.span)?
        {
            let target = self
                .signatures
                .get(&target)
                .ok_or_else(|| {
                    Diagnostic::new(function.span, "missing trait implementation signature")
                })?
                .id
                .0;
            return self.target_types(target, args, result, mailbox, context);
        }
        let mut values = HashMap::new();
        let pairs = signature
            .params
            .iter()
            .zip(args)
            .chain([(&signature.result, result)])
            .chain(signature.mailbox.as_ref().zip(mailbox))
            .collect::<Vec<_>>();
        super::unions::capture_pairs(&pairs, &mut values)?;
        let arguments = signature
            .generics
            .iter()
            .map(|n| {
                values.get(n).cloned().ok_or_else(|| {
                    Diagnostic::new(Span::default(), format!("cannot infer generic type '{n}'"))
                })
            })
            .collect::<Checked<Vec<_>>>()?;
        self.enqueue(template, arguments, context)
    }

    /// Replace source-template call IDs and recursively queue all reachable instances.
    fn rewrite(&mut self, expr: &mut ir::Expr) -> Checked<()> {
        if let ir::ExprKind::Lambda { captures, body, .. } = &mut expr.kind {
            for capture in captures { self.rewrite(&mut capture.value)?; }
            let context = match &expr.ty { Type::ActorFunction(mailbox, _) => Context::Actor(*mailbox.clone()), _ => Context::Root };
            let previous = std::mem::replace(&mut self.context, context);
            self.rewrite(body)?;
            self.context = previous;
        } else {
            for child in super::lift::children_mut(expr) { self.rewrite(child)?; }
        }
        if let ir::ExprKind::JsonCodec { plan, .. } = &mut expr.kind {
            let identity = std::rc::Rc::as_ptr(plan) as usize;
            if let Some(existing) = self.codec_plans.get(&identity) {
                *plan = existing.clone();
            } else if plan
                .entries
                .iter()
                .any(|e| matches!(e.kind, crate::json_codec::Kind::Custom { .. }))
            {
                let mut concrete = (**plan).clone();
                for entry in &mut concrete.entries {
                    if let crate::json_codec::Kind::Custom { encode, decode } = &mut entry.kind {
                        for (callback, input, output) in [
                            (
                                encode,
                                entry.ty.clone(),
                                Type::Native(crate::runtime::NativeType::JsonValue),
                            ),
                            (
                                decode,
                                Type::Native(crate::runtime::NativeType::JsonValue),
                                entry.ty.clone(),
                            ),
                        ] {
                            if let crate::json_codec::Callback::Source(name) = callback {
                                let signature = self.signatures.get(name).ok_or_else(|| {
                                    Diagnostic::new(expr.span, "missing custom Json callback")
                                })?;
                                let result = Type::Result(
                                    Box::new(output),
                                    Box::new(Type::Native(crate::runtime::NativeType::JsonError)),
                                );
                                *callback = crate::json_codec::Callback::Function(
                                    self.target_types(signature.id.0, &[input], &result, None, Context::Root)?,
                                );
                            }
                        }
                    }
                }
                *plan = std::rc::Rc::new(concrete);
                self.codec_plans.insert(identity, plan.clone());
            }
        }
        match &mut expr.kind {
            ir::ExprKind::Actor(ir::ActorExpr::Call {
                function,
                args,
                mailbox,
            }) => {
                let types: Vec<_> = args.iter().map(|a| a.ty.clone()).collect();
                *function = self.target_types(function.0, &types, &expr.ty, Some(mailbox), Context::Actor(mailbox.clone()))?;
            }
            ir::ExprKind::Call {
                target: ir::CallTarget::Function(id),
                args,
            } => *id = self.target(id.0, args, &expr.ty)?,
            ir::ExprKind::FunctionValue {
                target: ir::CallTarget::Function(id),
            } => {
                let Some((mailbox, params, result)) = crate::actors::function(&expr.ty) else {
                    return Err(Diagnostic::new(expr.span, "invalid function value type"));
                };
                *id = self.target_types(id.0, params, result, mailbox, mailbox.cloned().map(Context::Actor).unwrap_or(Context::Root))?;
            }
            _ => {}
        }
        Ok(())
    }
}

/// Substitute a template's API and local annotations without changing source names.
fn source_instance(
    source: &ast::Function,
    values: &HashMap<String, Type>,
) -> Checked<ast::Function> {
    let mut source = source.clone();
    for param in &mut source.params {
        param.annotation = Some(nominal::substitute(
            super::clauses::parameter_type(param),
            values,
        )?);
    }
    source.return_type = source
        .return_type
        .as_ref()
        .map(|t| nominal::substitute(t, values))
        .transpose()?;
    substitute_expr(&mut source.body, values)?;
    Ok(source)
}

/// Substitute the bounded expression tree's explicit local annotations and guards.
pub(super) fn substitute_expr(expr: &mut ast::Expr, values: &HashMap<String, Type>) -> Checked<()> {
    match &mut expr.kind {
        ast::ExprKind::Receive { arms, timeout, .. } => substitute_receive(arms, timeout, values)?,
        ast::ExprKind::TypeTarget(ty) => *ty = nominal::substitute(ty, values)?,
        ast::ExprKind::Range { .. } | ast::ExprKind::For { .. } | ast::ExprKind::With { .. } => {
            substitute_iteration(expr, values)?
        }
        ast::ExprKind::Map(entries) => {
            for (key, value) in entries {
                substitute_expr(key, values)?;
                substitute_expr(value, values)?;
            }
        }
        ast::ExprKind::RecordUpdate { value, fields } => substitute_update(value, fields, values)?,
        ast::ExprKind::Lambda { params, body } => substitute_lambda(params, body, values)?,
        ast::ExprKind::Apply { callee, args } => substitute_apply(callee, args, values)?,
        ast::ExprKind::Return(value)
        | ast::ExprKind::Defer(value)
        | ast::ExprKind::Unary { value, .. }
        | ast::ExprKind::Try(value)
        | ast::ExprKind::Field { value, .. } => substitute_expr(value, values)?,
        ast::ExprKind::PostfixIf {
            value: left,
            condition: right,
        }
        | ast::ExprKind::Binary { left, right, .. } => {
            substitute_expr(left, values)?;
            substitute_expr(right, values)?;
        }
        ast::ExprKind::Pipe { value, args, .. } | ast::ExprKind::GlobalPipe { value, args, .. } => {
            substitute_expr(value, values)?;
            for arg in args {
                substitute_expr(arg, values)?;
            }
        }
        ast::ExprKind::Interpolate(parts) | ast::ExprKind::MultilineString(parts) => {
            substitute_string(parts, values)?;
        }
        ast::ExprKind::Call { args, .. } | ast::ExprKind::GlobalCall { args, .. } => {
            for arg in args {
                substitute_expr(arg, values)?;
            }
        }
        ast::ExprKind::Tuple(args) | ast::ExprKind::List(args) => {
            for arg in args {
                substitute_expr(arg, values)?;
            }
        }
        ast::ExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            substitute_expr(condition, values)?;
            substitute_expr(then_branch, values)?;
            if let Some(value) = else_branch {
                substitute_expr(value, values)?;
            }
        }
        ast::ExprKind::Match { value, arms } => substitute_match(value, arms, values)?,
        ast::ExprKind::ConditionMatch(arms) => substitute_conditions(arms, values)?,
        ast::ExprKind::Block(stmts) => substitute_block(stmts, values)?,
        _ => {}
    }
    Ok(())
}

/// Substitute block annotations while preserving initializer and statement traversal order.
fn substitute_block(stmts: &mut [ast::Stmt], values: &HashMap<String, Type>) -> Checked<()> {
    for stmt in stmts {
        if let ast::Stmt::LetElse { else_branch, .. } = stmt {
            substitute_expr(else_branch, values)?;
        }
        match stmt {
            ast::Stmt::LetElse {
                annotation, value, ..
            }
            | ast::Stmt::Let {
                annotation, value, ..
            }
            | ast::Stmt::LetPattern {
                annotation, value, ..
            } => {
                *annotation = annotation
                    .as_ref()
                    .map(|t| nominal::substitute(t, values))
                    .transpose()?;
                substitute_expr(value, values)?;
            }
            ast::Stmt::Expr(value) => substitute_expr(value, values)?,
        }
    }
    Ok(())
}

/// Substitute lambda annotations before its recursively checked body.
fn substitute_lambda(
    params: &mut [ast::LambdaParam],
    body: &mut ast::Expr,
    values: &HashMap<String, Type>,
) -> Checked<()> {
    for param in params {
        param.annotation = param
            .annotation
            .as_ref()
            .map(|ty| nominal::substitute(ty, values))
            .transpose()?;
    }
    substitute_expr(body, values)
}

/// Substitute update initializers so generic callback annotations remain concrete.
fn substitute_update(
    base: &mut ast::Expr,
    fields: &mut [ast::RecordField],
    values: &HashMap<String, Type>,
) -> Checked<()> {
    substitute_expr(base, values)?;
    for field in fields {
        substitute_expr(&mut field.value, values)?;
    }
    Ok(())
}

/// Substitute match guards and bodies without conflating their lexical patterns.
fn substitute_match(
    value: &mut ast::Expr,
    arms: &mut [ast::MatchArm],
    values: &HashMap<String, Type>,
) -> Checked<()> {
    substitute_expr(value, values)?;
    for arm in arms {
        substitute_pattern(&mut arm.pattern, values)?;
        if let Some(guard) = &mut arm.guard {
            substitute_expr(guard, values)?;
        }
        substitute_expr(&mut arm.body, values)?;
    }
    Ok(())
}
/// Substitute source conditional arms before their bounded lazy lowering.
fn substitute_conditions(
    arms: &mut [ast::ConditionArm],
    values: &HashMap<String, Type>,
) -> Checked<()> {
    for arm in arms {
        if let Some(condition) = &mut arm.condition {
            substitute_expr(condition, values)?;
        }
        substitute_expr(&mut arm.body, values)?;
    }
    Ok(())
}

/// Substitute annotations nested inside flat loop and with control flow.
fn substitute_iteration(expr: &mut ast::Expr, values: &HashMap<String, Type>) -> Checked<()> {
    match &mut expr.kind {
        ast::ExprKind::Range { start, end, .. } => {
            substitute_expr(start, values)?;
            substitute_expr(end, values)?;
        }
        ast::ExprKind::For { iterable, body, .. } => {
            substitute_expr(iterable, values)?;
            substitute_expr(body, values)?;
        }
        ast::ExprKind::With {
            bindings,
            body,
            arms,
        } => {
            for binding in bindings {
                substitute_expr(&mut binding.value, values)?;
            }
            substitute_expr(body, values)?;
            for arm in arms.iter_mut().flatten() {
                if let Some(guard) = &mut arm.guard {
                    substitute_expr(guard, values)?;
                }
                substitute_expr(&mut arm.body, values)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Substitute embedded string expressions without modifying their literal text segments.
fn substitute_string(parts: &mut [ast::StringPart], values: &HashMap<String, Type>) -> Checked<()> {
    for part in parts {
        if let ast::StringPart::Value(value) = part {
            substitute_expr(value, values)?;
        }
    }
    Ok(())
}

/// Substitute every source typed narrowing before concrete dispatch is reconstructed.
fn substitute_pattern(pattern: &mut ast::Pattern, values: &HashMap<String, Type>) -> Checked<()> {
    match &mut pattern.kind {
        ast::PatternKind::Typed {
            pattern: inner,
            annotation,
        } => {
            *annotation = nominal::substitute(annotation, values)?;
            substitute_pattern(inner, values)?;
        }
        ast::PatternKind::Tuple(fields) | ast::PatternKind::NamedConstructor { fields, .. } => {
            for field in fields {
                substitute_pattern(field, values)?;
            }
        }
        ast::PatternKind::List { prefix, rest } => {
            for field in prefix {
                substitute_pattern(field, values)?;
            }
            if let Some(rest) = rest {
                substitute_pattern(rest, values)?;
            }
        }
        ast::PatternKind::TupleRest { prefix, rest } => {
            for field in prefix {
                substitute_pattern(field, values)?;
            }
            substitute_pattern(rest, values)?;
        }
        _ => {}
    }
    Ok(())
}

/// Substitute receive guards, bodies, and deadlines while preserving their source order.
fn substitute_receive(
    arms: &mut [ast::MatchArm],
    timeout: &mut Option<(Box<ast::Expr>, Box<ast::Expr>)>,
    values: &HashMap<String, Type>,
) -> Checked<()> {
    for arm in arms {
        if let Some(guard) = &mut arm.guard {
            substitute_expr(guard, values)?;
        }
        substitute_expr(&mut arm.body, values)?;
    }
    if let Some((duration, body)) = timeout {
        substitute_expr(duration, values)?;
        substitute_expr(body, values)?;
    }
    Ok(())
}

/// Preserve callee-before-arguments traversal while substituting all explicit annotations.
fn substitute_apply(
    callee: &mut ast::Expr,
    args: &mut [ast::Argument],
    values: &HashMap<String, Type>,
) -> Checked<()> {
    substitute_expr(callee, values)?;
    for arg in args {
        substitute_expr(arg, values)?;
    }
    Ok(())
}
