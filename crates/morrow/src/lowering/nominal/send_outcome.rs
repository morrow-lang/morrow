//! Elide only nonescaping enqueue Results after ordinary match validation.
use super::*;

pub(super) enum Scrutinee {
    Boxed(String),
    SendOutcome(String),
}

fn scalar_patterns(patterns: &[Pattern]) -> bool {
    patterns.iter().all(|pattern| match pattern {
        Pattern::Wildcard => true,
        Pattern::Variant { tag, fields } if *tag <= 1 && fields.len() == 1 => {
            matches!(fields[0], Pattern::Wildcard | Pattern::Bind(_))
                || (*tag == 1 && matches!(fields[0], Pattern::Int(_)))
        }
        // A whole-Result binding can escape through a guard or arm. Keep its
        // ordinary owned representation even when that particular arm is rare.
        _ => false,
    })
}

impl Emitter<'_> {
    pub(super) fn match_scrutinee(
        &mut self,
        value: &Expr,
        patterns: &[Pattern],
        partial: bool,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<Scrutinee> {
        if !partial
            && value.ty == Type::Result(Box::new(Type::Unit), Box::new(Type::Int))
            && scalar_patterns(patterns)
            && let ExprKind::Actor(ir::ActorExpr::Send { pid, message }) = &value.kind
        {
            // Match coverage, source Result duties and public IR contracts have
            // already run. Preserve expr's own validation/work charge and child
            // depths too: selecting a representation grants no trusted-IR bypass.
            self.validate_expr(value, depth)?;
            self.strict_termination(value, locals, depth + 1)?;
            let (_, outcome) = self.send_call(
                pid,
                message,
                value.span,
                actor_backend::SendResult::Outcome,
                locals,
                depth + 1,
            )?;
            Ok(Scrutinee::SendOutcome(outcome))
        } else {
            self.expr(value, locals, depth).map(Scrutinee::Boxed)
        }
    }

    pub(super) fn match_pattern(
        &mut self,
        pattern: &Pattern,
        ty: &Type,
        value: &Scrutinee,
        state: &mut PatternState<'_>,
        locals: &mut Locals,
    ) -> Lowering<()> {
        let outcome = match value {
            Scrutinee::SendOutcome(outcome) => outcome,
            Scrutinee::Boxed(value) => {
                return self.pattern_branch(pattern, ty, value, state, locals);
            }
        };
        self.pattern_work(state)?;
        let Pattern::Variant { tag, fields } = pattern else {
            return if matches!(pattern, Pattern::Wildcard) {
                Ok(())
            } else {
                Err(invalid(state.span, "invalid scalar send pattern"))
            };
        };
        let test = self.assign(
            locals,
            Type::Bool,
            NativeOperation::Binary(
                MachineBinary::Compare(
                    if *tag == 0 {
                        Comparison::Eq
                    } else {
                        Comparison::Ne
                    },
                    Scalar::I64,
                ),
                native_operand(outcome),
                native_operand("0"),
            ),
        );
        self.require_pattern(&test, state.failure, locals);
        if !matches!(fields[0], Pattern::Wildcard) {
            let (payload_type, payload) = if *tag == 0 {
                (Type::Unit, "0")
            } else {
                (Type::Int, outcome.as_str())
            };
            state.depth += 1;
            self.pattern_branch(&fields[0], &payload_type, payload, state, locals)?;
            state.depth -= 1;
        }
        Ok(())
    }
}
