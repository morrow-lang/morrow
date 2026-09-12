//! Length-checked sequence projections and delayed immutable suffix materialization.
use super::*;

/// A suffix stays unallocated until every structural test of its arm succeeds.
pub(super) struct Pending {
    id: ir::LocalId,
    owner: Type,
    value: String,
    offset: usize,
}

/// A suffix may bind or ignore, but cannot hide another refutable pattern.
fn checked_rest(rest: &Pattern, span: Span) -> Lowering<()> {
    if matches!(rest, Pattern::Bind(_) | Pattern::Wildcard) {
        Ok(())
    } else {
        Err(invalid(
            span,
            "sequence rest must bind or ignore its suffix",
        ))
    }
}

impl Emitter<'_> {
    /// Validate homogeneous prefix elements and the sole optional suffix binder.
    pub(super) fn checked_list_pattern(
        &self,
        prefix: &[Pattern],
        rest: Option<&Pattern>,
        ty: &Type,
        span: Span,
        depth: usize,
    ) -> Lowering<Pattern> {
        let Type::List(item) = ty else {
            return Err(invalid(span, "list pattern requires List subject"));
        };
        if prefix.len() > MAX_NODES {
            return Err(invalid(span, "sequence pattern length limit exceeded"));
        }
        if let Some(rest) = rest {
            checked_rest(rest, span)?;
        }
        let prefix = prefix
            .iter()
            .map(|p| self.checked_pattern(p, item, span, depth + 1))
            .collect::<Lowering<_>>()?;
        Ok(Pattern::List {
            prefix,
            rest: rest.cloned().map(Box::new),
        })
    }

    /// Validate a tuple prefix against known arity, preserving singleton suffix types.
    pub(super) fn checked_tuple_rest(
        &self,
        prefix: &[Pattern],
        rest: &Pattern,
        ty: &Type,
        span: Span,
        depth: usize,
    ) -> Lowering<Pattern> {
        let fields = tuple_fields(ty, span)?;
        if prefix.len() > fields.len() || prefix.len() > MAX_NODES {
            return Err(invalid(span, "tuple rest prefix exceeds subject arity"));
        }
        checked_rest(rest, span)?;
        let prefix = prefix
            .iter()
            .zip(fields)
            .map(|(p, ty)| self.checked_pattern(p, ty, span, depth + 1))
            .collect::<Lowering<_>>()?;
        Ok(Pattern::TupleRest {
            prefix,
            rest: Box::new(rest.clone()),
        })
    }

    /// Check length before any demanded prefix access; failed arms never copy a suffix.
    pub(super) fn list_pattern(
        &mut self,
        prefix: &[Pattern],
        rest: Option<&Pattern>,
        ty: &Type,
        value: &str,
        state: &mut PatternState<'_>,
        locals: &mut Locals,
    ) -> Lowering<()> {
        let Type::List(item) = ty else {
            return Err(invalid(state.span, "list pattern requires List subject"));
        };
        let length = self.assign(
            locals,
            Type::Int,
            NativeOperation::Call {
                callee: native_operand("$fern_list_len"),
                args: vec![(Scalar::I64, native_operand(value))],
                variadic: None,
            },
        );
        let op = if rest.is_some() {
            Comparison::SGe
        } else {
            Comparison::Eq
        };
        let test = self.assign(
            locals,
            Type::Bool,
            NativeOperation::Binary(
                MachineBinary::Compare(op, Scalar::I64),
                native_operand(&length),
                Operand::Int(prefix.len() as i64),
            ),
        );
        self.require_pattern(&test, state.failure, locals);
        for (index, pattern) in prefix.iter().enumerate() {
            if matches!(pattern, Pattern::Wildcard) {
                continue;
            }
            let raw = self.assign(
                locals,
                Type::Int,
                NativeOperation::Call {
                    callee: native_operand("$fern_list_get"),
                    args: vec![
                        (Scalar::I64, native_operand(value)),
                        (Scalar::I64, native_operand(&(index).to_string())),
                    ],
                    variadic: None,
                },
            );
            let field = self.unpack(locals, item, raw);
            state.depth += 1;
            self.pattern_branch(pattern, item, &field, state, locals)?;
            state.depth -= 1;
        }
        queue_rest(rest, ty, value, prefix.len(), state);
        Ok(())
    }

    /// Read a typed tuple's demanded prefix, deferring its suffix copy with sibling tails.
    pub(super) fn tuple_rest_pattern(
        &mut self,
        prefix: &[Pattern],
        rest: &Pattern,
        ty: &Type,
        value: &str,
        state: &mut PatternState<'_>,
        locals: &mut Locals,
    ) -> Lowering<()> {
        let fields = tuple_fields(ty, state.span)?;
        for (index, (pattern, field_ty)) in prefix.iter().zip(fields).enumerate() {
            if matches!(pattern, Pattern::Wildcard) {
                continue;
            }
            let field = self.custom_field(value, index, field_ty, locals);
            state.depth += 1;
            self.pattern_branch(pattern, field_ty, &field, state, locals)?;
            state.depth -= 1;
        }
        queue_rest(Some(rest), ty, value, prefix.len(), state);
        Ok(())
    }

    /// Introduce all suffix bindings only on the fully matched predecessor before its guard.
    pub(super) fn materialize_rests(
        &mut self,
        state: &mut PatternState<'_>,
        locals: &mut Locals,
    ) -> Lowering<()> {
        for pending in std::mem::take(&mut state.pending) {
            let (ty, value) = if matches!(pending.owner, Type::List(_)) {
                let value = if pending.offset == 0 {
                    pending.value
                } else {
                    self.pattern_tail_used = true;
                    self.assign(
                        locals,
                        pending.owner.clone(),
                        NativeOperation::Call {
                            callee: native_operand("$fern_rs_pattern_tail"),
                            args: vec![
                                (Scalar::I64, native_operand(&(pending.value).to_string())),
                                (Scalar::I64, native_operand(&(pending.offset).to_string())),
                            ],
                            variadic: None,
                        },
                    )
                };
                (pending.owner, value)
            } else {
                self.tuple_suffix(&pending, state.span, locals)?
            };
            locals.define(pending.id.0, ty, value, state.span)?;
            state.bindings.push(pending.id.0);
        }
        self.materialize_unions(state, locals)
    }

    /// Copy exact raw tuple words without narrowing Float, integer or captured values.
    fn tuple_suffix(
        &mut self,
        pending: &Pending,
        span: Span,
        locals: &mut Locals,
    ) -> Lowering<(Type, String)> {
        let fields = tuple_fields(&pending.owner, span)?;
        let suffix = &fields[pending.offset..];
        if suffix.is_empty() {
            return Ok((Type::Unit, "0".into()));
        }
        let ty = Type::Tuple(suffix.to_vec());
        if pending.offset == 0 {
            return Ok((ty, pending.value.clone()));
        }
        let value = self.assign(
            locals,
            ty.clone(),
            NativeOperation::Call {
                callee: native_operand("$fern_alloc"),
                args: vec![(
                    Scalar::I64,
                    native_operand(&(8 * (suffix.len() + 1)).to_string()),
                )],
                variadic: None,
            },
        );
        self.output.statement(Statement::Store {
            kind: LoadKind::I64,
            value: native_operand("0"),
            address: native_operand(&(value)),
        });
        for index in 0..suffix.len() {
            let source = self.assign(
                locals,
                Type::Int,
                NativeOperation::Binary(
                    MachineBinary::Add,
                    native_operand(&(pending.value).to_string()),
                    native_operand(&(8 * (pending.offset + index + 1)).to_string()),
                ),
            );
            let raw = self.assign(
                locals,
                Type::Int,
                NativeOperation::Load(LoadKind::I64, native_operand(&(source))),
            );
            let dest = self.assign(
                locals,
                Type::Int,
                NativeOperation::Binary(
                    MachineBinary::Add,
                    native_operand(&(value).to_string()),
                    native_operand(&(8 * (index + 1)).to_string()),
                ),
            );
            self.output.statement(Statement::Store {
                kind: LoadKind::I64,
                value: native_operand(&(raw)),
                address: native_operand(&(dest)),
            });
        }
        Ok((ty, value))
    }
}

/// Unit is the zero-element product; every nonempty suffix remains a structural tuple.
fn tuple_fields(ty: &Type, span: Span) -> Lowering<&[Type]> {
    match ty {
        Type::Tuple(fields) => Ok(fields),
        Type::Unit => Ok(&[]),
        _ => Err(invalid(span, "tuple rest pattern requires tuple subject")),
    }
}

/// Store only named suffixes; wildcard tails perform neither projections nor allocation.
fn queue_rest(
    rest: Option<&Pattern>,
    ty: &Type,
    value: &str,
    offset: usize,
    state: &mut PatternState<'_>,
) {
    if let Some(Pattern::Bind(id)) = rest {
        state.pending.push(Pending {
            id: *id,
            owner: ty.clone(),
            value: value.into(),
            offset,
        });
    }
}

/// Model empty/cons constructors for coverage without changing the runtime list layout.
pub(super) fn specialize_list(
    prefix: &[Pattern],
    rest: Option<&Pattern>,
    tag: usize,
) -> Option<Vec<Pattern>> {
    if prefix.is_empty() {
        if rest.is_some() {
            return Some(vec![Pattern::Wildcard; if tag == 0 { 0 } else { 2 }]);
        }
        return (tag == 0).then(Vec::new);
    }
    (tag == 1).then(|| {
        vec![
            prefix[0].clone(),
            Pattern::List {
                prefix: prefix[1..].to_vec(),
                rest: rest.cloned().map(Box::new),
            },
        ]
    })
}
