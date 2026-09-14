//! Inline small, statically identified callbacks without changing control scopes.
use super::*;

// No recursive call expansion: eligible bodies contain no calls or closures.
// The normal emitter node/root limits additionally bound total generated work.
const MAX_INLINE_NODES: usize = 64;

/// The whitelist excludes function-scoped exits, cleanup, suspension and loops.
/// Arithmetic/aggregate lowering can still fault or allocate; it keeps the
/// caller's fault handler and precise roots exactly as ordinary expressions do.
fn eligible(expr: &Expr, remaining: &mut usize) -> bool {
    let Some(next) = remaining.checked_sub(1) else {
        return false;
    };
    *remaining = next;
    match &expr.kind {
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Bool(_)
        | ExprKind::String(_)
        | ExprKind::Local(_)
        | ExprKind::Unit => true,
        ExprKind::Field { value, .. } | ExprKind::Unary { value, .. } => eligible(value, remaining),
        ExprKind::Binary { left, right, .. } => {
            eligible(left, remaining) && eligible(right, remaining)
        }
        ExprKind::CustomConstruct { fields, .. } | ExprKind::Tuple(fields) => {
            fields.iter().all(|field| eligible(field, remaining))
        }
        ExprKind::Construct { value, .. } => value
            .as_ref()
            .is_none_or(|value| eligible(value, remaining)),
        ExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            eligible(condition, remaining)
                && eligible(then_branch, remaining)
                && else_branch
                    .as_ref()
                    .is_none_or(|branch| eligible(branch, remaining))
        }
        ExprKind::Block(statements) => statements.iter().all(|statement| match statement {
            Stmt::Let { value, .. } | Stmt::Expr(value) => eligible(value, remaining),
            Stmt::LetElse { .. } => false,
        }),
        _ => false,
    }
}

impl Emitter<'_> {
    /// Reuse the already evaluated environment so capture evaluation and source
    /// argument order never move into the loop. Unknown/effectful functions keep
    /// their ordinary callback ABI, cleanup activation and suspension behavior.
    pub(super) fn invoke_callback_values(
        &mut self,
        expression: &Expr,
        closure: &str,
        args: &[(Type, String)],
        result: &Type,
        locals: &mut Locals,
    ) -> Lowering<String> {
        let function = match &expression.kind {
            ExprKind::Closure { function, .. } => self.functions.get(&function.0).copied(),
            _ => None,
        };
        let Some(function) = function.filter(|function| {
            let mut remaining = MAX_INLINE_NODES;
            function.mailbox.is_none()
                && function.captures.len() <= MAX_INLINE_NODES
                && function.params.len() <= MAX_INLINE_NODES
                && !self.actors.entries.contains_key(&function.id.0)
                && !self.actors.managed.contains(&function.id.0)
                && eligible(&function.body, &mut remaining)
        }) else {
            return Ok(self.invoke_values(closure, args, result, locals));
        };
        if function.params.len() != args.len() {
            return Err(invalid(
                expression.span,
                "inlined callback argument count differs from signature",
            ));
        }
        expect_type(
            function.return_type.clone(),
            result.clone(),
            expression.span,
        )?;

        // Local identities belong to each lifted function. Only the lexical
        // identity tables change: SSA names, control blocks, fault handler and
        // root slots remain owned by the enclosing physical function.
        let values = std::mem::take(&mut locals.values);
        let defined = std::mem::take(&mut locals.defined);
        let count = std::mem::replace(&mut locals.count, function.local_count);
        let outcome = (|| {
            for (param, (ty, value)) in function.params.iter().zip(args) {
                expect_type(ty.clone(), param.ty.clone(), expression.span)?;
                locals.define(param.id.0, ty.clone(), value.clone(), expression.span)?;
            }
            for (index, capture) in function.captures.iter().enumerate() {
                let address = self.assign(
                    locals,
                    Type::Int,
                    NativeOperation::Binary(
                        MachineBinary::Add,
                        native_operand(closure),
                        Operand::Int((8 * (index + 1)) as i64),
                    ),
                );
                let raw = self.assign(
                    locals,
                    Type::Int,
                    NativeOperation::Load(LoadKind::I64, native_operand(&address)),
                );
                let value = self.unpack(locals, &capture.ty, raw);
                self.root_value(locals, &capture.ty, &value);
                locals.define(capture.id.0, capture.ty.clone(), value, expression.span)?;
            }
            self.expr(&function.body, locals, 0)
        })();
        locals.values = values;
        locals.defined = defined;
        locals.count = count;
        outcome
    }
}
