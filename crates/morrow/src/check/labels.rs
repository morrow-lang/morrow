//! Source-call label interfaces and effect-preserving parameter permutation.
use super::*;

/// Return source parameter names without exposing normalized dispatch bindings.
pub(super) fn parameters(function: &ast::Function) -> Vec<Option<ast::ArgumentLabel>> {
    function
        .params
        .iter()
        .map(|param| {
            param.label.clone().or_else(|| {
                if let ast::PatternKind::Bind(name) = &param.pattern.kind
                    && !name.starts_with('$')
                {
                    return Some(ast::ArgumentLabel {
                        name: name.clone(),
                        span: param.pattern.span,
                    });
                }
                None
            })
        })
        .collect()
}

/// Derive one stable external interface from every clause's explicit or simple binding name.
pub(super) fn group(group: &[ast::Function]) -> Checked<Vec<Option<ast::ArgumentLabel>>> {
    let mut result: Vec<Option<ast::ArgumentLabel>> = vec![None; group[0].params.len()];
    for function in group {
        for (slot, label) in result.iter_mut().zip(parameters(function)) {
            if let Some(label) = label {
                if slot.as_ref().is_some_and(|prior| prior.name != label.name) {
                    return Err(Diagnostic::new(
                        label.span,
                        "function clauses must use consistent external labels",
                    ));
                }
                *slot = Some(label);
            }
        }
    }
    let mut names = HashSet::new();
    for label in result.iter().flatten() {
        if !names.insert(&label.name) {
            return Err(Diagnostic::new(
                label.span,
                "duplicate parameter: duplicate external label",
            ));
        }
    }
    Ok(result)
}

/// Reject source-only labels when a callable has a structural or runtime interface.
pub(super) fn positional(args: &[ast::Argument]) -> Checked<()> {
    if let Some(label) = args.iter().find_map(|arg| arg.label.as_ref()) {
        return Err(Diagnostic::new(
            label.span,
            "this callable uses positional arguments",
        ));
    }
    Ok(())
}

/// Map written arguments to slots, validating even caller-created AST argument order.
pub(super) fn order(
    args: &[ast::Argument],
    labels: &[Option<ast::ArgumentLabel>],
    span: Span,
) -> Checked<Vec<usize>> {
    let names: HashMap<_, _> = labels
        .iter()
        .enumerate()
        .filter_map(|(i, l)| l.as_ref().map(|l| (l.name.as_str(), i)))
        .collect();
    let mut used = vec![false; labels.len()];
    let mut supplied = HashSet::new();
    let mut labeled = false;
    let mut result = Vec::new();
    for (index, arg) in args.iter().enumerate() {
        let slot = if let Some(label) = &arg.label {
            labeled = true;
            if !supplied.insert(&label.name) {
                return Err(Diagnostic::new(label.span, "duplicate argument label"));
            }
            *names.get(label.name.as_str()).ok_or_else(|| {
                Diagnostic::new(label.span, format!("unknown argument label {}", label.name))
            })?
        } else {
            if labeled {
                return Err(Diagnostic::new(
                    arg.span,
                    "positional arguments must precede labeled arguments",
                ));
            }
            index
        };
        let occupied = used
            .get_mut(slot)
            .ok_or_else(|| Diagnostic::new(arg.span, "too many arguments"))?;
        if *occupied {
            return Err(Diagnostic::new(
                arg.span,
                "argument position already supplied",
            ));
        }
        *occupied = true;
        result.push(slot);
    }
    if used.iter().any(|used| !used) {
        return Err(Diagnostic::new(span, "missing argument"));
    }
    Ok(result)
}

impl Checker<'_> {
    /// Store reordered values once in written order before reading parameter-order locals.
    pub(super) fn ordered_call(
        &mut self,
        target: ir::CallTarget,
        args: Vec<ir::Expr>,
        order: &[usize],
        result: Type,
        span: Span,
    ) -> TypedKind {
        if order.iter().enumerate().all(|(index, slot)| index == *slot) {
            return (ir::ExprKind::Call { target, args }, result);
        }
        if let Some(index) = args.iter().position(|arg| arg.ty == Type::Never) {
            let statements = args
                .into_iter()
                .take(index + 1)
                .map(ir::Stmt::Expr)
                .collect();
            return (ir::ExprKind::Block(statements), Type::Never);
        }
        let mut statements = Vec::new();
        let mut values = vec![None; args.len()];
        for (value, slot) in args.into_iter().zip(order) {
            let id = ir::LocalId(self.local_count);
            self.local_count += 1;
            values[*slot] = Some(ir::Expr {
                kind: ir::ExprKind::Local(id),
                ty: value.ty.clone(),
                span: value.span,
            });
            statements.push(ir::Stmt::Let { id, value });
        }
        let args = values
            .into_iter()
            .map(|value| value.expect("validated label permutation"))
            .collect();
        statements.push(ir::Stmt::Expr(ir::Expr {
            kind: ir::ExprKind::Call { target, args },
            ty: result.clone(),
            span,
        }));
        (ir::ExprKind::Block(statements), result)
    }
}

/// Publish call-site obligations only after all declared schemes have finished inference.
pub(super) fn finalize(
    program: &ast::Program,
    signatures: &mut HashMap<String, Signature>,
) -> Checked<()> {
    let mut work = 0usize;
    for function in &program.functions {
        let signature = signatures
            .get_mut(&function.name)
            .ok_or_else(|| Diagnostic::new(function.span, "missing source label signature"))?;
        let mut counts = HashMap::new();
        for ty in &signature.params {
            work = work.saturating_add(crate::unions::cost(ty, function.span)?);
            if work > 400_000 {
                return Err(Diagnostic::new(
                    function.span,
                    "argument-label scheme work limit exceeded",
                ));
            }
            *counts.entry(ty).or_insert(0usize) += 1;
        }
        let required = signature
            .params
            .iter()
            .map(|ty| *ty == Type::Bool || counts[ty] > 1)
            .collect::<Vec<_>>();
        for (index, needed) in required.iter().enumerate() {
            if *needed
                && signature
                    .labels
                    .get(index)
                    .and_then(Option::as_ref)
                    .is_none()
            {
                return Err(Diagnostic::new(
                    function.params[index].span,
                    format!("parameter {} requires a stable external label", index + 1),
                ));
            }
        }
        signature.required_labels = required;
    }
    Ok(())
}

/// Enforce the original scheme's interface without deriving obligations from concrete callers.
pub(super) fn required(
    args: &[ast::Argument],
    signature: &Signature,
    order: &[usize],
) -> Checked<()> {
    for (arg, slot) in args.iter().zip(order) {
        if arg.label.is_none() && signature.required_labels.get(*slot) == Some(&true) {
            let label = signature.labels[*slot]
                .as_ref()
                .expect("finalized required label");
            return Err(Diagnostic::new(
                arg.span,
                format!("argument requires label '{}:'", label.name),
            ));
        }
    }
    Ok(())
}
