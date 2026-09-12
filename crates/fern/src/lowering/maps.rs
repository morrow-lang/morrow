//! Immutable maps preserve insertion order in GC-managed full-width key/value pairs.
use super::*;

/// Identify the compiler-owned map operations.
pub(super) fn is_map(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::MapNew
            | Builtin::MapGet
            | Builtin::MapPut
            | Builtin::MapDelete
            | Builtin::MapLen
            | Builtin::MapIsEmpty
            | Builtin::MapContains
            | Builtin::MapKeys
            | Builtin::MapValues
    )
}

/// Require a concrete map and key types with defined semantic equality.
pub(super) fn types<'a>(
    ty: &'a Type,
    layouts: &HashMap<Type, &ir::TypeLayout>,
    span: Span,
) -> Lowering<(&'a Type, &'a Type)> {
    let Type::Map(key, value) = ty else {
        return Err(invalid(span, "Map operation requires Map type"));
    };
    if !matches!(
        newtypes::representation(key, layouts, span)?,
        Type::Int | Type::Bool | Type::String
    ) {
        return Err(invalid(span, "Map keys require Int, Bool, or String"));
    }
    Ok((key, value))
}

/// Derive all source signatures independently from the public typed IR.
fn signature(
    builtin: Builtin,
    args: &[Expr],
    result: &Type,
    layouts: &HashMap<Type, &ir::TypeLayout>,
    span: Span,
) -> Lowering<Type> {
    if builtin == Builtin::MapNew {
        types(result, layouts, span)?;
        if !args.is_empty() {
            return Err(invalid(span, "Map.new takes no arguments"));
        }
        return Ok(result.clone());
    }
    let first = args
        .first()
        .ok_or_else(|| invalid(span, "Map operation requires argument"))?;
    let (key, value) = types(&first.ty, layouts, span)?;
    let mut params = vec![first.ty.clone()];
    let output = match builtin {
        Builtin::MapGet => {
            params.push(key.clone());
            Type::Option(Box::new(value.clone()))
        }
        Builtin::MapPut => {
            params.extend([key.clone(), value.clone()]);
            first.ty.clone()
        }
        Builtin::MapDelete => {
            params.push(key.clone());
            first.ty.clone()
        }
        Builtin::MapContains => {
            params.push(key.clone());
            Type::Bool
        }
        Builtin::MapLen => Type::Int,
        Builtin::MapIsEmpty => Type::Bool,
        Builtin::MapKeys => Type::List(Box::new(key.clone())),
        Builtin::MapValues => Type::List(Box::new(value.clone())),
        _ => return Err(invalid(span, "expected Map builtin identity")),
    };
    if args.len() != params.len() {
        return Err(invalid(span, "Map argument count differs from signature"));
    }
    for (arg, param) in args.iter().zip(params) {
        expect_type(arg.ty.clone(), param, arg.span)?;
    }
    expect_type(output.clone(), result.clone(), span)?;
    Ok(output)
}

impl Emitter<'_> {
    /// Evaluate each key/value pair once and replace duplicates only in this private literal.
    pub(super) fn map_literal(
        &mut self,
        entries: &[(Expr, Expr)],
        ty: &Type,
        span: Span,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        let (key_type, value_type) = types(ty, &self.layouts, span)?;
        if entries.len() > MAX_NODES {
            return Err(invalid(span, "Map literal limit exceeded"));
        }
        self.maps_used = true;
        let map = self.assign(
            locals,
            ty.clone(),
            NativeOperation::Call {
                callee: native_operand("$fern_list_with_capacity"),
                args: vec![(
                    Scalar::I64,
                    native_operand(&(entries.len().max(1)).to_string()),
                )],
                variadic: None,
            },
        );
        for (key, value) in entries {
            expect_type(key.ty.clone(), key_type.clone(), key.span)?;
            expect_type(value.ty.clone(), value_type.clone(), value.span)?;
            let key = self.expr(key, locals, depth)?;
            let key = self.payload(locals, key_type, key);
            let value = self.expr(value, locals, depth)?;
            let value = self.payload(locals, value_type, value);
            let index = self.map_index(&map, &key, key_type, locals);
            self.output
                .statement(Statement::Effect(NativeOperation::Call {
                    callee: native_operand("$fern_rs_map_literal_put"),
                    args: vec![
                        (Scalar::I64, native_operand(&(map).to_string())),
                        (Scalar::I64, native_operand(&(key).to_string())),
                        (Scalar::I64, native_operand(&(value))),
                        (Scalar::I64, native_operand(&(index))),
                    ],
                    variadic: None,
                }));
        }
        Ok((ty.clone(), map))
    }

    /// Evaluate resolved map arguments in source order before calling immutable helpers.
    pub(super) fn map_call(
        &mut self,
        builtin: Builtin,
        args: &[Expr],
        result: &Type,
        span: Span,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        let output = signature(builtin, args, result, &self.layouts, span)?;
        self.maps_used = true;
        if builtin == Builtin::MapNew {
            return Ok((
                output.clone(),
                self.assign(
                    locals,
                    output,
                    NativeOperation::Call {
                        callee: native_operand("$fern_list_with_capacity"),
                        args: vec![(Scalar::I64, native_operand("1"))],
                        variadic: None,
                    },
                ),
            ));
        }
        let mut values = Vec::new();
        for arg in args {
            let value = self.expr(arg, locals, depth)?;
            values.push(self.payload(locals, &arg.ty, value));
        }
        let (key, _) = types(&args[0].ty, &self.layouts, span)?;
        let map = &values[0];
        let instruction = match builtin {
            Builtin::MapLen => NativeOperation::Call {
                callee: native_operand("$fern_list_len"),
                args: vec![(Scalar::I64, native_operand(&(map).to_string()))],
                variadic: None,
            },
            Builtin::MapIsEmpty => {
                let len = self.assign(
                    locals,
                    Type::Int,
                    NativeOperation::Call {
                        callee: native_operand("$fern_list_len"),
                        args: vec![(Scalar::I64, native_operand(&(map).to_string()))],
                        variadic: None,
                    },
                );
                NativeOperation::Binary(
                    MachineBinary::Compare(Comparison::Eq, Scalar::I64),
                    native_operand(&(len)),
                    native_operand("0"),
                )
            }
            Builtin::MapKeys | Builtin::MapValues => NativeOperation::Call {
                callee: native_operand("$fern_rs_map_project"),
                args: vec![
                    (Scalar::I64, native_operand(&(map).to_string())),
                    (
                        Scalar::I64,
                        Operand::Int(if builtin == Builtin::MapKeys { 0 } else { 8 }),
                    ),
                ],
                variadic: None,
            },
            _ => self.map_keyed_call(builtin, &values, key, locals),
        };
        Ok((output.clone(), self.assign(locals, output, instruction)))
    }

    /// Select a key-aware lookup and preserve full-width stored payloads.
    fn map_keyed_call(
        &mut self,
        builtin: Builtin,
        values: &[String],
        key_type: &Type,
        locals: &mut Locals,
    ) -> NativeOperation {
        let map = &values[0];
        let key = &values[1];
        let index = self.map_index(map, key, key_type, locals);
        match builtin {
            Builtin::MapGet => NativeOperation::Call {
                callee: native_operand("$fern_rs_map_get"),
                args: vec![
                    (Scalar::I64, native_operand(&(map).to_string())),
                    (Scalar::I64, native_operand(&(index))),
                ],
                variadic: None,
            },
            Builtin::MapPut => NativeOperation::Call {
                callee: native_operand("$fern_rs_map_put"),
                args: vec![
                    (Scalar::I64, native_operand(&(map).to_string())),
                    (Scalar::I64, native_operand(&(key).to_string())),
                    (Scalar::I64, native_operand(&(values[2]).to_string())),
                    (Scalar::I64, native_operand(&(index))),
                ],
                variadic: None,
            },
            Builtin::MapDelete => NativeOperation::Call {
                callee: native_operand("$fern_rs_map_delete"),
                args: vec![
                    (Scalar::I64, native_operand(&(map).to_string())),
                    (Scalar::I64, native_operand(&(index))),
                ],
                variadic: None,
            },
            Builtin::MapContains => NativeOperation::Binary(
                MachineBinary::Compare(Comparison::SGe, Scalar::I64),
                native_operand(&(index)),
                native_operand("0"),
            ),
            _ => unreachable!("map signature validated"),
        }
    }

    /// Use full-word equality for Int/Bool and byte-content equality for String keys.
    fn map_index(&mut self, map: &str, key: &str, key_type: &Type, locals: &mut Locals) -> String {
        let suffix = if *self.representation(key_type) == Type::String {
            "string"
        } else {
            "word"
        };
        self.assign(
            locals,
            Type::Int,
            NativeOperation::Call {
                callee: native_operand(&format!("$fern_rs_map_index_{}", suffix)),
                args: vec![
                    (Scalar::I64, native_operand(map)),
                    (Scalar::I64, native_operand(key)),
                ],
                variadic: None,
            },
        )
    }
}
