//! Shared actor syntax and type views; identities and effects are never guessed by the backend.
use crate::{Type, ast, ir};
#[path = "actors/contracts.rs"]
pub(crate) mod contracts;
#[path = "actors/positions.rs"]
mod positions;

/// Peel only the explicit actor-effect wrapper, retaining the ordinary source signature.
pub(crate) fn function(ty: &Type) -> Option<(Option<&Type>, &[Type], &Type)> {
    match ty {
        Type::Function(args, result) => Some((None, args, result)),
        Type::ActorFunction(mailbox, function) => {
            let Type::Function(args, result) = function.as_ref() else {
                return None;
            };
            Some((Some(mailbox), args, result))
        }
        _ => None,
    }
}

/// Enumerate direct syntax children exactly once; callers own depth and aggregate limits.
pub(crate) fn source_children(expr: &ast::Expr) -> Vec<&ast::Expr> {
    use ast::ExprKind::*;
    match &expr.kind {
        Receive { .. } | With { .. } => scoped_children(expr),
        Match { value, arms } => {
            let mut out = vec![value.as_ref()];
            out.extend(arm_children(arms));
            out
        }
        If {
            condition,
            then_branch,
            else_branch,
        } => {
            let mut out = vec![condition.as_ref(), then_branch.as_ref()];
            out.extend(else_branch.as_deref());
            out
        }
        Lambda { body, .. }
        | Try(body)
        | Return(body)
        | Defer(body)
        | Unary { value: body, .. }
        | Field { value: body, .. } => vec![body],
        Binary { left, right, .. }
        | Range {
            start: left,
            end: right,
            ..
        }
        | PostfixIf {
            value: left,
            condition: right,
        } => vec![left, right],
        Tuple(values) | List(values) => values.iter().collect(),
        Map(entries) => entries.iter().flat_map(|(k, v)| [k, v]).collect(),
        RecordUpdate { value, fields } => std::iter::once(value.as_ref())
            .chain(fields.iter().map(|f| &f.value))
            .collect(),
        Call { args, .. } | GlobalCall { args, .. } => args.iter().map(|a| &a.value).collect(),
        Apply { callee, args } => std::iter::once(callee.as_ref())
            .chain(args.iter().map(|a| &a.value))
            .collect(),
        Pipe { value, args, .. } | GlobalPipe { value, args, .. } => {
            std::iter::once(value.as_ref())
                .chain(args.iter().map(|a| &a.value))
                .collect()
        }
        Block(stmts) => stmts.iter().flat_map(statement_children).collect(),
        Interpolate(parts) | MultilineString(parts) => parts
            .iter()
            .filter_map(|part| match part {
                ast::StringPart::Value(v) => Some(v),
                _ => None,
            })
            .collect(),
        For { iterable, body, .. } => vec![iterable, body],
        ConditionMatch(arms) => arms
            .iter()
            .flat_map(|a| a.condition.iter().chain([&a.body]))
            .collect(),
        _ => Vec::new(),
    }
}

/// Preserve initializer-before-failure order for statement-owned children.
fn statement_children(stmt: &ast::Stmt) -> Vec<&ast::Expr> {
    match stmt {
        ast::Stmt::Let { value, .. }
        | ast::Stmt::LetPattern { value, .. }
        | ast::Stmt::Expr(value) => vec![value],
        ast::Stmt::LetElse {
            value, else_branch, ..
        } => vec![value, else_branch],
    }
}

/// Guards belong to the same receive/match arm and precede its body.
fn arm_children(arms: &[ast::MatchArm]) -> Vec<&ast::Expr> {
    arms.iter()
        .flat_map(|a| a.guard.iter().chain([&a.body]))
        .collect()
}

/// Actor children remain visible to publication checks, capture discovery and resource accounting.
pub(crate) fn children(actor: &ir::ActorExpr) -> Vec<&ir::Expr> {
    match actor {
        ir::ActorExpr::Lowered(value) => value.children(),
        ir::ActorExpr::Spawn { entry, .. } => vec![entry],
        ir::ActorExpr::Send { pid, message } => vec![pid, message],
        ir::ActorExpr::Call { args, .. } => args.iter().collect(),
        ir::ActorExpr::Receive { arms, timeout, .. } => {
            let mut out: Vec<_> = arms
                .iter()
                .flat_map(|a| a.guard.iter().chain([&a.body]))
                .collect();
            if let Some((duration, body)) = timeout {
                out.extend([duration.as_ref(), body.as_ref()]);
            }
            out
        }
    }
}

/// Mutable actor children allow ordinary substitution and closure lifting without replay.
pub(crate) fn children_mut(actor: &mut ir::ActorExpr) -> Vec<&mut ir::Expr> {
    match actor {
        ir::ActorExpr::Lowered(value) => value.children_mut(),
        ir::ActorExpr::Spawn { entry, .. } => vec![entry],
        ir::ActorExpr::Send { pid, message } => vec![pid, message],
        ir::ActorExpr::Call { args, .. } => args.iter_mut().collect(),
        ir::ActorExpr::Receive { arms, timeout, .. } => {
            let mut out: Vec<_> = arms
                .iter_mut()
                .flat_map(|a| a.guard.iter_mut().chain([&mut a.body]))
                .collect();
            if let Some((duration, body)) = timeout {
                out.extend([duration.as_mut(), body.as_mut()]);
            }
            out
        }
    }
}

/// Opaque compiler continuation operation; source/public IR cannot mint executable steps.
///
/// ```compile_fail
/// let forged = fern_compiler::actors::Lowered {};
/// ```
#[derive(Clone, Debug)]
pub struct Lowered {
    pub(crate) operation: Operation,
}
#[derive(Clone, Debug)]
pub(crate) enum Operation {
    Continue(Box<ir::Expr>),
    Pointer(Box<ir::Expr>),
    Register {
        selector: Box<ir::Expr>,
        timeout: Option<Box<ir::Expr>>,
        duration: Box<ir::Expr>,
    },
    Select {
        value: Box<ir::Expr>,
        arms: Vec<ir::MatchArm>,
    },
}
impl Lowered {
    /// Expose every owned child to bounded compiler publication walkers.
    fn children(&self) -> Vec<&ir::Expr> {
        match &self.operation {
            Operation::Continue(value) | Operation::Pointer(value) => vec![value],
            Operation::Register {
                selector,
                timeout,
                duration,
            } => std::iter::once(selector.as_ref())
                .chain(timeout.as_deref())
                .chain([duration.as_ref()])
                .collect(),
            Operation::Select { value, arms } => std::iter::once(value.as_ref())
                .chain(arms.iter().flat_map(|a| a.guard.iter().chain([&a.body])))
                .collect(),
        }
    }
    /// Preserve shared substitution visibility without exposing constructors to callers.
    fn children_mut(&mut self) -> Vec<&mut ir::Expr> {
        match &mut self.operation {
            Operation::Continue(value) | Operation::Pointer(value) => vec![value],
            Operation::Register {
                selector,
                timeout,
                duration,
            } => std::iter::once(selector.as_mut())
                .chain(timeout.as_deref_mut())
                .chain([duration.as_mut()])
                .collect(),
            Operation::Select { value, arms } => std::iter::once(value.as_mut())
                .chain(
                    arms.iter_mut()
                        .flat_map(|a| a.guard.iter_mut().chain([&mut a.body])),
                )
                .collect(),
        }
    }
}

/// Reject unsupported actor entries before committing definitions or performing any interactive effect.
pub(crate) fn reject_interactive(program: &ast::Program) -> Result<(), String> {
    let mut pending: Vec<_> = program.functions.iter().map(|f| &f.body).collect();
    let mut work = 0;
    while let Some(expr) = pending.pop() {
        work += 1;
        if work > 200_000 {
            return Err("interactive actor preflight limit exceeded".into());
        }
        if matches!(&expr.kind, ast::ExprKind::Receive { .. })
            || matches!(&expr.kind, ast::ExprKind::Call { name, .. } if name == "spawn" || name == "send")
        {
            return Err("managed actors are unsupported in the REPL; use native build/run".into());
        }
        pending.extend(source_children(expr));
    }
    Ok(())
}

/// Preserve arm and binding evaluation order for syntax owning multiple independent scopes.
fn scoped_children(expr: &ast::Expr) -> Vec<&ast::Expr> {
    use ast::ExprKind::*;
    match &expr.kind {
        Receive { arms, timeout } => {
            let mut out = arm_children(arms);
            if let Some((duration, body)) = timeout {
                out.extend([duration.as_ref(), body.as_ref()]);
            }
            out
        }
        With {
            bindings,
            body,
            arms,
        } => {
            let mut out: Vec<_> = bindings.iter().map(|b| &b.value).collect();
            out.push(body);
            if let Some(arms) = arms {
                out.extend(arm_children(arms));
            }
            out
        }
        _ => Vec::new(),
    }
}
