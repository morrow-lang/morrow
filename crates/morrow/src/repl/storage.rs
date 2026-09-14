//! Aggregate limits for retained closures, including their originating compiled programs.
use super::*;

/// Count retained code and types iteratively so shared program identities can be charged once.
pub(super) fn program_size(program: &ir::Program) -> Result<(usize, usize), String> {
    let mut budget = CodeBudget {
        pending: Vec::new(),
        codec_plans: std::collections::HashSet::new(),
        bytes: 0,
        count: 0,
    };
    for function in &program.functions {
        budget.bytes += function.name.len() + std::mem::size_of::<ir::Function>();
        budget.pending.push(Part::Expr(&function.body));
        budget.pending.push(Part::Type(&function.return_type));
        for parameter in function.params.iter().chain(&function.captures) {
            budget.pending.push(Part::Type(&parameter.ty));
        }
    }
    for layout in &program.types {
        budget.pending.push(Part::Type(&layout.ty));
        budget.bytes += std::mem::size_of::<ir::TypeLayout>()
            + layout.variants.len() * std::mem::size_of::<Vec<Type>>()
            + layout.fields.len() * std::mem::size_of::<String>()
            + layout.fields.iter().map(String::len).sum::<usize>()
            + layout.variant_names.len() * std::mem::size_of::<String>()
            + layout.variant_names.iter().map(String::len).sum::<usize>();
        budget
            .pending
            .extend(layout.variants.iter().flatten().map(Part::Type));
    }
    while let Some(part) = budget.pending.pop() {
        budget.count += 1;
        match part {
            Part::Expr(expr) => budget.expression(expr)?,
            Part::Type(ty) => budget.ty(ty),
            Part::Pattern(pattern) => budget.pattern(pattern),
        }
        if budget.bytes > 16 * 1024 * 1024 || budget.count + budget.pending.len() > 200_000 {
            return Err("interactive value storage limit exceeded".into());
        }
    }
    Ok((budget.bytes, budget.count))
}
enum Part<'a> {
    Expr(&'a ir::Expr),
    Type(&'a Type),
    Pattern(&'a ir::Pattern),
}
struct CodeBudget<'a> {
    codec_plans: std::collections::HashSet<usize>,
    pending: Vec<Part<'a>>,
    bytes: usize,
    count: usize,
}
impl<'a> CodeBudget<'a> {
    /// Charge concrete codec tables retained by executable closures before queueing their types.
    fn codec(&mut self, plan: &'a Rc<crate::json_codec::Plan>) -> Result<(), String> {
        if plan.entries.len() > 4096 {
            return Err("interactive value storage limit exceeded".into());
        }
        if !self.codec_plans.insert(Rc::as_ptr(plan) as usize) {
            return Ok(());
        }
        self.count += plan.entries.len();
        self.bytes += std::mem::size_of_val(plan.entries.as_slice());
        for entry in &plan.entries {
            self.pending.push(Part::Type(&entry.ty));
            match &entry.kind {
                crate::json_codec::Kind::Sum(variants) => {
                    self.bytes += std::mem::size_of_val(variants.as_slice());
                    for variant in variants {
                        self.bytes += variant.wire_tag.len()
                            + std::mem::size_of_val(variant.fields.as_slice());
                    }
                }
                crate::json_codec::Kind::Tuple(ids) | crate::json_codec::Kind::Union(ids) => {
                    self.bytes += std::mem::size_of_val(ids.as_slice())
                }
                crate::json_codec::Kind::Record(fields) => {
                    self.bytes += std::mem::size_of_val(fields.as_slice());
                    for field in fields {
                        self.bytes += field.name.len();
                    }
                }
                _ => {}
            }
        }
        if self.bytes > 16 * 1024 * 1024 || self.count + self.pending.len() > 200_000 {
            return Err("interactive value storage limit exceeded".into());
        }
        Ok(())
    }
    /// Include expression-owned strings, child storage and all retained type information.
    fn expression(&mut self, expr: &'a ir::Expr) -> Result<(), String> {
        use ir::ExprKind::*;
        self.bytes += std::mem::size_of::<ir::Expr>();
        self.pending.push(Part::Type(&expr.ty));
        match &expr.kind {
            ForeignCall { declaration, args } => {
                declaration.validate(expr.span).map_err(|e| e.message)?;
                self.bytes +=
                    declaration.symbol.len() + declaration.library.as_ref().map_or(0, |s| s.len());
                self.expressions(args);
            }
            Actor(actor) => {
                self.pending
                    .extend(crate::actors::children(actor).into_iter().map(Part::Expr));
                match actor {
                    ir::ActorExpr::Receive { arms, .. }
                    | ir::ActorExpr::Lowered(crate::actors::Lowered {
                        operation: crate::actors::Operation::Select { arms, .. },
                    }) => self
                        .pending
                        .extend(arms.iter().map(|a| Part::Pattern(&a.pattern))),
                    _ => {}
                }
            }
            JsonCodecTemplate { .. } => {
                return Err("JSON codec template cannot enter executable IR".into());
            }
            EditorHole { .. } => return Err("editor hole cannot enter executable IR".into()),
            Probe { .. } => return Err("inference probe cannot enter executable IR".into()),
            JsonCodec { input, plan, .. } => {
                self.pending.push(Part::Expr(input));
                self.codec(plan)?;
            }
            String(text) => self.bytes += text.len(),
            Range { start, end, .. } => self.pending.extend([Part::Expr(start), Part::Expr(end)]),
            For {
                pattern,
                iterable,
                body,
            } => self.pending.extend([
                Part::Pattern(pattern),
                Part::Expr(iterable),
                Part::Expr(body),
            ]),
            With {
                steps,
                body,
                handlers,
            } => self.with_block(steps, body, handlers),
            Map(pairs) => self.map_pairs(pairs),
            List(xs)
            | Tuple(xs)
            | Interpolate(xs)
            | Closure { captures: xs, .. }
            | CustomConstruct { fields: xs, .. }
            | Call { args: xs, .. } => self.expressions(xs),
            Invoke { callee, args } => self.invocation(callee, args),
            UnionInject { value }
            | UnionWiden { value }
            | Wrap(value)
            | Unwrap(value)
            | Return(value)
            | Defer(value)
            | Try(value)
            | Field { value, .. }
            | Unary { value, .. } => self.pending.push(Part::Expr(value)),
            Construct { value, .. } => self.pending.extend(value.iter().map(|v| Part::Expr(v))),
            Binary { left, right, .. } => {
                self.pending.extend([Part::Expr(left), Part::Expr(right)]);
            }
            If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.conditional(condition, then_branch, else_branch.as_deref());
            }
            Match { value, arms } => self.match_values(value, arms),
            Block(statements) => self.statements(statements),
            Lambda { .. } | FunctionValue { .. } => {
                return Err("unfinalized interactive closure".into());
            }
            Int(_) | Float(_) | Bool(_) | Local(_) | Unit | Break | Continue => {}
        }
        Ok(())
    }
    /// Account callee and arguments without losing captured closure storage.
    fn invocation(&mut self, callee: &'a ir::Expr, args: &'a [ir::Expr]) {
        self.pending.push(Part::Expr(callee));
        self.expressions(args);
    }
    /// Retain every branch even when its condition is statically false.
    fn conditional(
        &mut self,
        condition: &'a ir::Expr,
        then_branch: &'a ir::Expr,
        else_branch: Option<&'a ir::Expr>,
    ) {
        self.pending
            .extend([Part::Expr(condition), Part::Expr(then_branch)]);
        self.pending.extend(else_branch.map(Part::Expr));
    }
    /// Retain match subjects, pattern payloads, guards and branch bodies in the code budget.
    fn match_values(&mut self, value: &'a ir::Expr, arms: &'a [ir::MatchArm]) {
        self.pending.push(Part::Expr(value));
        for arm in arms {
            self.pending
                .extend([Part::Pattern(&arm.pattern), Part::Expr(&arm.body)]);
            self.pending.extend(arm.guard.iter().map(Part::Expr));
        }
    }

    /// Visit every key and value retained by a map literal.
    fn map_pairs(&mut self, pairs: &'a [(ir::Expr, ir::Expr)]) {
        self.pending.extend(
            pairs
                .iter()
                .flat_map(|(key, value)| [Part::Expr(key), Part::Expr(value)]),
        );
    }
    /// Include each specialized error handler once and all sequential binding patterns.
    fn with_block(
        &mut self,
        steps: &'a [ir::WithStep],
        body: &'a ir::Expr,
        handlers: &'a [ir::WithHandler],
    ) {
        self.bytes += std::mem::size_of_val(steps) + std::mem::size_of_val(handlers);
        self.pending.push(Part::Expr(body));
        for step in steps {
            self.pending
                .extend([Part::Pattern(&step.pattern), Part::Expr(&step.value)]);
        }
        for handler in handlers {
            self.pending
                .extend([Part::Type(&handler.error.ty), Part::Expr(&handler.body)]);
        }
    }
    /// Charge every statement and all scopes retained by a let-else fallback.
    fn statements(&mut self, statements: &'a [ir::Stmt]) {
        self.bytes += std::mem::size_of_val(statements);
        for stmt in statements {
            let value = match stmt {
                ir::Stmt::Let { value, .. } | ir::Stmt::Expr(value) => value,
                ir::Stmt::LetElse {
                    pattern,
                    value,
                    else_branch,
                } => {
                    self.pending
                        .extend([Part::Pattern(pattern), Part::Expr(else_branch)]);
                    value
                }
            };
            self.pending.push(Part::Expr(value));
        }
    }
    fn expressions(&mut self, values: &'a [ir::Expr]) {
        self.pending.extend(values.iter().map(Part::Expr));
    }
    /// Traverse function and nominal arguments without expanding recursive nominal layouts.
    fn ty(&mut self, ty: &'a Type) {
        self.bytes += std::mem::size_of::<Type>();
        match ty {
            Type::List(a) | Type::Option(a) => self.pending.push(Part::Type(a)),
            Type::ActorFunction(a, b) | Type::Result(a, b) | Type::Map(a, b) => {
                self.pending.extend([Part::Type(a), Part::Type(b)])
            }
            Type::Function(args, result) => {
                self.pending.extend(args.iter().map(Part::Type));
                self.pending.push(Part::Type(result));
            }
            Type::Named(name, args) => {
                self.bytes += name.len();
                self.pending.extend(args.iter().map(Part::Type));
            }
            Type::Union(args) | Type::Tuple(args) => {
                self.pending.extend(args.iter().map(Part::Type))
            }
            Type::Generic(name) => self.bytes += name.len(),
            _ => {}
        }
    }
    /// Retained pattern strings and nested structures are part of closure code storage.
    fn pattern(&mut self, pattern: &'a ir::Pattern) {
        self.bytes += std::mem::size_of::<ir::Pattern>();
        match pattern {
            ir::Pattern::UnionSelect { narrowed, binding } => {
                self.pending.push(Part::Type(narrowed));
                if let Some(binding) = binding {
                    self.pending.push(Part::Type(&binding.ty));
                }
            }
            ir::Pattern::Newtype(inner) => self.pending.push(Part::Pattern(inner)),
            ir::Pattern::List { prefix, rest } => {
                self.pending.extend(prefix.iter().map(Part::Pattern));
                if let Some(rest) = rest {
                    self.pending.push(Part::Pattern(rest));
                }
            }
            ir::Pattern::TupleRest { prefix, rest } => {
                self.pending.extend(prefix.iter().map(Part::Pattern));
                self.pending.push(Part::Pattern(rest));
            }
            ir::Pattern::Tuple(xs) | ir::Pattern::Variant { fields: xs, .. } => {
                self.pending.extend(xs.iter().map(Part::Pattern));
            }
            ir::Pattern::String(text) => self.bytes += text.len(),
            _ => {}
        }
    }
}

/// Include semantic union member trees retained by their immutable runtime carrier.
pub(super) fn type_size(ty: &Type) -> Result<(usize, usize), String> {
    let mut budget = CodeBudget {
        pending: vec![Part::Type(ty)],
        codec_plans: std::collections::HashSet::new(),
        bytes: 0,
        count: 0,
    };
    while let Some(part) = budget.pending.pop() {
        let Part::Type(ty) = part else {
            unreachable!("type walker enqueues only types");
        };
        budget.count += 1;
        budget.ty(ty);
        if budget.bytes > 16 * 1024 * 1024 || budget.count + budget.pending.len() > 200_000 {
            return Err("interactive value storage limit exceeded".into());
        }
    }
    Ok((budget.bytes, budget.count))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retained_codec_tables_charge_shared_identity_once() {
        let plan = Rc::new(crate::json_codec::Plan {
            root: 0,
            entries: vec![crate::json_codec::Entry {
                ty: Type::Int,
                kind: crate::json_codec::Kind::Int,
            }],
        });
        let mut budget = CodeBudget {
            codec_plans: std::collections::HashSet::new(),
            pending: Vec::new(),
            bytes: 0,
            count: 0,
        };
        budget.codec(&plan).unwrap();
        let first = (budget.bytes, budget.count, budget.pending.len());
        budget.codec(&plan).unwrap();
        assert_eq!(first, (budget.bytes, budget.count, budget.pending.len()));
        let distinct = Rc::new((*plan).clone());
        budget.codec(&distinct).unwrap();
        assert_eq!(first.0 * 2, budget.bytes);
        assert_eq!(first.1 * 2, budget.count);
    }

    fn closure(program: Rc<ir::Program>) -> Value {
        Value::Closure(Rc::new(ClosureValue {
            actor_entries: Rc::default(),
            program,
            function: ir::FunctionId(0),
            captures: Vec::new(),
        }))
    }
    fn program() -> Rc<ir::Program> {
        Rc::new(ir::Program {
            types: Vec::new(),
            functions: vec![ir::Function {
                mailbox: None,
                id: ir::FunctionId(0),
                name: "retained".into(),
                params: Vec::new(),
                captures: Vec::new(),
                return_type: Type::String,
                local_count: 0,
                body: ir::Expr {
                    kind: ir::ExprKind::String("x".repeat(1_000_000)),
                    ty: Type::String,
                    span: crate::Span::default(),
                },
            }],
        })
    }
    #[test]
    fn retained_programs_reject_editor_holes() {
        let mut source = (*program()).clone();
        let receiver = source.functions[0].body.clone();
        let mut hole = receiver.clone();
        hole.kind = ir::ExprKind::EditorHole {
            token: ir::EditorHoleToken::new(),
            receiver: Box::new(receiver.clone()),
        };
        let returning = ir::Expr {
            kind: ir::ExprKind::Return(Box::new(receiver)),
            ty: Type::Never,
            span: crate::Span::default(),
        };
        source.functions[0].body.kind =
            ir::ExprKind::Block(vec![ir::Stmt::Expr(returning), ir::Stmt::Expr(hole)]);
        assert!(program_size(&source).unwrap_err().contains("editor hole"));
    }
    #[test]
    fn template_execution_and_retained_inactive_programs_are_rejected() {
        let mut source = (*program()).clone();
        let input = source.functions[0].body.clone();
        let template = ir::Expr {
            span: input.span,
            ty: Type::Result(
                Box::new(Type::String),
                Box::new(Type::Native(crate::runtime::NativeType::JsonError)),
            ),
            kind: ir::ExprKind::JsonCodecTemplate {
                direction: crate::json_codec::Direction::Encode,
                target: Type::String,
                input: Box::new(input.clone()),
                token: ir::CodecTemplateToken::new(),
            },
        };
        let mut machine = Machine::new(Rc::new(source.clone()), HashMap::new());
        assert!(
            matches!(machine.expression(&template),Err(Failure::Message(s)) if s.contains("codec template"))
        );
        source.functions[0].body.kind = ir::ExprKind::Block(vec![
            ir::Stmt::Expr(ir::Expr {
                span: input.span,
                ty: Type::Never,
                kind: ir::ExprKind::Return(Box::new(input)),
            }),
            ir::Stmt::Expr(template),
        ]);
        assert!(
            program_size(&source)
                .unwrap_err()
                .contains("codec template")
        );
    }
    #[test]
    fn retained_programs_reject_inference_probes() {
        let mut source = (*program()).clone();
        source.functions[0].body.kind = ir::ExprKind::Probe {
            token: ir::ProbeToken::new(0),
            children: vec![],
            bindings: vec![],
        };
        assert!(
            program_size(&source)
                .unwrap_err()
                .contains("inference probe")
        );
    }
    #[test]
    fn unique_programs_count_toward_storage_but_shared_programs_count_once() {
        let independent: Vec<_> = (0..18).map(|_| closure(program())).collect();
        assert!(
            graph_budget(independent.iter())
                .unwrap_err()
                .contains("storage limit")
        );
        let shared = program();
        let aliases: Vec<_> = (0..100).map(|_| closure(shared.clone())).collect();
        assert!(graph_budget(aliases.iter()).is_ok());
    }
}
