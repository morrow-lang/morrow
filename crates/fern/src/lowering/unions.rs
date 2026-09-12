//! Structural unions use immutable full-width tagged envelopes and exact semantic tag identities.
use super::*;

/// Reject hostile noncanonical alternatives before ordering, hashing or selecting an ABI.
pub(super) fn canonical(members: &[Type], span: Span) -> Lowering<()> {
    if !(2..=128).contains(&members.len()) {
        return Err(invalid(span, "union requires 2 to 128 canonical members"));
    }
    let mut work = 0usize;
    for member in members {
        if matches!(member, Type::Union(_)) {
            return Err(invalid(span, "nested union must be flattened"));
        }
        work = work.saturating_add(crate::unions::cost(member, span)?);
        if work > 400_000 {
            return Err(invalid(span, "union representation work limit exceeded"));
        }
    }
    if members.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(invalid(span, "union members must be sorted and distinct"));
    }
    Ok(())
}

impl Emitter<'_> {
    /// Keep conversion dispatch separate while retaining exact nominal/union type validation.
    pub(super) fn conversion(
        &mut self,
        expr: &Expr,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        match &expr.kind {
            ExprKind::Wrap(value) => self.newtype_expr(expr, value, true, locals, depth),
            ExprKind::Unwrap(value) => self.newtype_expr(expr, value, false, locals, depth),
            ExprKind::UnionInject { value } => self.union_inject(expr, value, locals, depth),
            ExprKind::UnionWiden { value } => self.union_widen(expr, value, locals, depth),
            _ => Err(invalid(expr.span, "expected representation conversion")),
        }
    }

    /// Evaluate one exact member once, then allocate and store all payload bits without truncation.
    fn union_inject(
        &mut self,
        expr: &Expr,
        value: &Expr,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        let Type::Union(members) = &expr.ty else {
            return Err(invalid(expr.span, "union injection requires Union target"));
        };
        let tag = members
            .iter()
            .position(|ty| *ty == value.ty)
            .ok_or_else(|| invalid(value.span, "injected type is not an exact union member"))?;
        let raw = self.expr(value, locals, depth)?;
        let payload = self.payload(locals, &value.ty, raw);
        let result = self.union_envelope(&tag.to_string(), &payload, locals);
        Ok((expr.ty.clone(), result))
    }

    /// A widening conversion derives every mapping from exact checked source/target members.
    fn union_widen(
        &mut self,
        expr: &Expr,
        value: &Expr,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        nominal::resolved(&value.ty, &self.layouts, value.span, 0)?;
        if !matches!(value.ty, Type::Union(_))
            || !matches!(expr.ty, Type::Union(_))
            || !crate::unions::subset(&value.ty, &expr.ty)
        {
            return Err(invalid(
                expr.span,
                "union widening requires a canonical subset",
            ));
        }
        let raw = self.expr(value, locals, depth)?;
        let result = self.union_rebox(&value.ty, &expr.ty, &raw, locals)?;
        Ok((expr.ty.clone(), result))
    }

    /// Allocate an immutable tag/payload pair; both operands are already full-width values.
    pub(super) fn union_envelope(
        &mut self,
        tag: &str,
        payload: &str,
        locals: &mut Locals,
    ) -> String {
        let value = self.assign(
            locals,
            Type::Int,
            NativeOperation::Call {
                callee: native_operand("$fern_alloc"),
                args: vec![(Scalar::I64, native_operand("16"))],
                variadic: None,
            },
        );
        let address = self.assign(
            locals,
            Type::Int,
            NativeOperation::Binary(
                MachineBinary::Add,
                native_operand(&(value)),
                native_operand("8"),
            ),
        );
        self.output.statement(Statement::Store {
            kind: LoadKind::I64,
            value: native_operand(tag),
            address: native_operand(&(value)),
        });
        self.output.statement(Statement::Store {
            kind: LoadKind::I64,
            value: native_operand(payload),
            address: native_operand(&(address)),
        });
        value
    }

    /// Read payload bits only after the caller established a matching union member tag.
    pub(super) fn union_payload(&mut self, value: &str, locals: &mut Locals) -> String {
        let address = self.assign(
            locals,
            Type::Int,
            NativeOperation::Binary(
                MachineBinary::Add,
                native_operand(value),
                native_operand("8"),
            ),
        );
        self.assign(
            locals,
            Type::Int,
            NativeOperation::Load(LoadKind::I64, native_operand(&(address))),
        )
    }

    /// Preserve aliases while remapping to a checked target subset or superset; identical types alias.
    pub(super) fn union_rebox(
        &mut self,
        source: &Type,
        target: &Type,
        value: &str,
        locals: &mut Locals,
    ) -> Lowering<String> {
        if source == target {
            return Ok(value.to_owned());
        }
        let actual = self.assign(
            locals,
            Type::Int,
            NativeOperation::Load(LoadKind::I64, native_operand(value)),
        );
        let merged = locals.label();
        let mut incoming = vec![];
        for (old, member) in crate::unions::members(source).iter().enumerate() {
            let Some(new) = crate::unions::members(target)
                .iter()
                .position(|ty| ty == member)
            else {
                continue;
            };
            let yes = locals.label();
            let no = locals.label();
            let test = self.assign(
                locals,
                Type::Bool,
                NativeOperation::Binary(
                    MachineBinary::Compare(Comparison::Eq, Scalar::I64),
                    native_operand(&(actual).to_string()),
                    native_operand(&(old).to_string()),
                ),
            );
            self.output.statement(Statement::Branch {
                condition: native_operand(&(test)),
                then_label: (yes).to_string(),
                else_label: (no).to_string(),
            });
            self.start_block(locals, &yes);
            self.incoming(Ok(new.to_string()), &mut incoming, &merged, locals)?;
            self.start_block(locals, &no);
        }
        // Safe IR only constructs validated member tags; subset callers have already tested them.
        self.output.statement(Statement::Trap);
        let (_, tag) = self.join(Type::Int, incoming, &merged, locals)?;
        let payload = self.union_payload(value, locals);
        Ok(self.union_envelope(&tag, &payload, locals))
    }
}
