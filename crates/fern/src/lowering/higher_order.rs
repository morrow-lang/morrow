//! Higher-order collection operations invoke typed heap closures, never C callbacks.
use super::*;

/// Identify the compiler-owned callback operations.
pub(super) fn is_higher_order(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::ListMap
            | Builtin::ListFold
            | Builtin::ListFilter
            | Builtin::ListFind
            | Builtin::ListAny
            | Builtin::ListAll
            | Builtin::OptionMap
            | Builtin::ResultMap
            | Builtin::ResultAndThen
            | Builtin::ResultUnwrapOrElse
    )
}

/// Validate generic callback signatures independently of the source checker.
pub(super) fn signature(builtin: Builtin, args: &[Expr], span: Span) -> Lowering<Type> {
    let arity = if builtin == Builtin::ListFold { 3 } else { 2 };
    if args.len() != arity {
        return Err(invalid(
            span,
            "higher-order argument count differs from signature",
        ));
    }
    let Type::Function(params, returned) = &args[arity - 1].ty else {
        return Err(invalid(
            span,
            "higher-order callback requires function value",
        ));
    };
    let (expected, output) = match (&args[0].ty, builtin) {
        (Type::List(item), Builtin::ListMap) => (vec![*item.clone()], Type::List(returned.clone())),
        (Type::List(item), Builtin::ListFold) => {
            expect_type(*returned.clone(), args[1].ty.clone(), span)?;
            (vec![args[1].ty.clone(), *item.clone()], args[1].ty.clone())
        }
        (
            Type::List(item),
            Builtin::ListFilter | Builtin::ListFind | Builtin::ListAny | Builtin::ListAll,
        ) => {
            expect_type(*returned.clone(), Type::Bool, span)?;
            let output = match builtin {
                Builtin::ListFilter => args[0].ty.clone(),
                Builtin::ListFind => Type::Option(item.clone()),
                _ => Type::Bool,
            };
            (vec![*item.clone()], output)
        }
        (Type::Option(item), Builtin::OptionMap) => {
            (vec![*item.clone()], Type::Option(returned.clone()))
        }
        (Type::Result(item, error), Builtin::ResultMap) => (
            vec![*item.clone()],
            Type::Result(returned.clone(), error.clone()),
        ),
        (Type::Result(item, error), Builtin::ResultAndThen) => {
            let Type::Result(_, callback_error) = &**returned else {
                return Err(invalid(span, "Result.and_then callback must return Result"));
            };
            expect_type(*callback_error.clone(), *error.clone(), span)?;
            (vec![*item.clone()], *returned.clone())
        }
        (Type::Result(item, error), Builtin::ResultUnwrapOrElse) => {
            expect_type(*returned.clone(), *item.clone(), span)?;
            (vec![*error.clone()], *item.clone())
        }
        _ => {
            return Err(invalid(
                span,
                "higher-order operation has incompatible collection type",
            ));
        }
    };
    if *params != expected {
        return Err(invalid(
            span,
            "higher-order callback parameter types differ from signature",
        ));
    }
    Ok(output)
}

struct Loop {
    entry: String,
    head: String,
    body: String,
    step: String,
    exhausted: String,
    found: String,
    merge: String,
    index: String,
    next: String,
    accumulator: String,
    accumulated: String,
}

impl Loop {
    /// Reserve loop identities before emitting forward-referenced phi operands.
    fn new(locals: &mut Locals) -> Self {
        Self {
            entry: locals.current.clone(),
            head: locals.label(),
            body: locals.label(),
            step: locals.label(),
            exhausted: locals.label(),
            found: locals.label(),
            merge: locals.label(),
            index: locals.temporary(),
            next: locals.temporary(),
            accumulator: locals.temporary(),
            accumulated: locals.temporary(),
        }
    }
}

impl Emitter<'_> {
    /// Evaluate collection, initial accumulator, and callback once in source order.
    pub(super) fn higher_order(
        &mut self,
        builtin: Builtin,
        args: &[Expr],
        span: Span,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        let result = signature(builtin, args, span)?;
        let mut values = Vec::new();
        for arg in args {
            values.push(self.expr(arg, locals, depth)?);
        }
        let value = if matches!(args[0].ty, Type::List(_)) {
            self.higher_list(builtin, args, &values, &result, locals)
        } else {
            self.higher_sum(builtin, args, &values, &result, locals)
        };
        Ok((result, value))
    }

    /// Lower list callbacks with a bounded index, preserving callback evaluation order.
    fn higher_list(
        &mut self,
        builtin: Builtin,
        args: &[Expr],
        values: &[String],
        result: &Type,
        locals: &mut Locals,
    ) -> String {
        let Type::List(item) = &args[0].ty else {
            unreachable!("signature validated")
        };
        let Type::Function(_, callback_result) = &args.last().unwrap().ty else {
            unreachable!("signature validated")
        };
        let collection = &values[0];
        let callback = values.last().unwrap();
        let length = self.assign(
            locals,
            Type::Int,
            NativeOperation::Call {
                callee: native_operand("$fern_list_len"),
                args: vec![(Scalar::I64, native_operand(&(collection).to_string()))],
                variadic: None,
            },
        );
        let output = if matches!(builtin, Builtin::ListMap | Builtin::ListFilter) {
            self.higher_list_output(&length, result, locals)
        } else {
            "0".into()
        };
        let flow = Loop::new(locals);
        self.loop_header(&flow, &length, builtin, result, values, locals);
        let raw = self.assign(
            locals,
            Type::Int,
            NativeOperation::Call {
                callee: native_operand("$fern_list_get"),
                args: vec![
                    (Scalar::I64, native_operand(&(collection).to_string())),
                    (Scalar::I64, native_operand(&(flow.index).to_string())),
                ],
                variadic: None,
            },
        );
        let element = self.unpack(locals, item, raw.clone());
        let mut callback_args = Vec::new();
        if builtin == Builtin::ListFold {
            callback_args.push((result.clone(), flow.accumulator.clone()));
        }
        callback_args.push((*item.clone(), element));
        let mapped = self.invoke_values(callback, &callback_args, callback_result, locals);
        self.loop_action(
            &flow,
            builtin,
            (&output, &raw, &mapped),
            callback_result,
            locals,
        );
        self.start_block(locals, &flow.step);
        self.output.statement(Statement::Assign {
            destination: (flow.next).to_string(),
            ty: Scalar::I64,
            operation: NativeOperation::Binary(
                MachineBinary::Add,
                native_operand(&(flow.index).to_string()),
                native_operand("1"),
            ),
        });
        self.output
            .statement(Statement::Jump((flow.head).to_string()));
        self.start_block(locals, &flow.exhausted);
        match builtin {
            Builtin::ListMap | Builtin::ListFilter => output,
            Builtin::ListFold => flow.accumulator,
            _ => self.loop_search_result(&flow, builtin, result, &raw, locals),
        }
    }

    /// Runtime list capacity must be positive even when mapping an empty collection.
    fn higher_list_output(&mut self, length: &str, result: &Type, locals: &mut Locals) -> String {
        let empty = self.assign(
            locals,
            Type::Bool,
            NativeOperation::Binary(
                MachineBinary::Compare(Comparison::Eq, Scalar::I64),
                native_operand(length),
                native_operand("0"),
            ),
        );
        let extra = self.payload(locals, &Type::Bool, empty);
        let capacity = self.assign(
            locals,
            Type::Int,
            NativeOperation::Binary(
                MachineBinary::Add,
                native_operand(length),
                native_operand(&(extra)),
            ),
        );
        self.assign(
            locals,
            result.clone(),
            NativeOperation::Call {
                callee: native_operand("$fern_list_with_capacity"),
                args: vec![(Scalar::I64, native_operand(&(capacity)))],
                variadic: None,
            },
        )
    }

    /// Define the induction/accumulator phis and guard each indexed runtime read.
    fn loop_header(
        &mut self,
        flow: &Loop,
        length: &str,
        builtin: Builtin,
        result: &Type,
        values: &[String],
        locals: &mut Locals,
    ) {
        self.output
            .statement(Statement::Jump((flow.head).to_string()));
        self.start_block(locals, &flow.head);
        self.output.statement(Statement::Assign {
            destination: (flow.index).to_string(),
            ty: Scalar::I64,
            operation: NativeOperation::Phi(vec![
                ((flow.entry).to_string(), native_operand("0")),
                (
                    (flow.step).to_string(),
                    native_operand(&(flow.next).to_string()),
                ),
            ]),
        });
        if builtin == Builtin::ListFold {
            self.output.statement(Statement::Assign {
                destination: (flow.accumulator).to_string(),
                ty: machine_width(self.width(result.clone())),
                operation: NativeOperation::Phi(vec![
                    (
                        (flow.entry).to_string(),
                        native_operand(&(values[1]).to_string()),
                    ),
                    (
                        (flow.step).to_string(),
                        native_operand(&(flow.accumulated).to_string()),
                    ),
                ]),
            });
        }
        let available = self.assign(
            locals,
            Type::Bool,
            NativeOperation::Binary(
                MachineBinary::Compare(Comparison::SLt, Scalar::I64),
                native_operand(&(flow.index).to_string()),
                native_operand(length),
            ),
        );
        self.output.statement(Statement::Branch {
            condition: native_operand(&(available)),
            then_label: (flow.body).to_string(),
            else_label: (flow.exhausted).to_string(),
        });
        self.start_block(locals, &flow.body);
    }

    /// Apply a callback result, routing predicate failures directly to the next item.
    fn loop_action(
        &mut self,
        flow: &Loop,
        builtin: Builtin,
        values: (&str, &str, &str),
        callback_result: &Type,
        locals: &mut Locals,
    ) {
        let (output, raw, mapped) = values;
        match builtin {
            Builtin::ListMap => {
                let payload = self.payload(locals, callback_result, mapped.into());
                self.output
                    .statement(Statement::Effect(NativeOperation::Call {
                        callee: native_operand("$fern_list_push_mut"),
                        args: vec![
                            (Scalar::I64, native_operand(output)),
                            (Scalar::I64, native_operand(&(payload))),
                        ],
                        variadic: None,
                    }));
                self.output
                    .statement(Statement::Jump((flow.step).to_string()));
            }
            Builtin::ListFold => {
                self.output.statement(Statement::Assign {
                    destination: (flow.accumulated).to_string(),
                    ty: machine_width(self.width(callback_result.clone())),
                    operation: NativeOperation::Unary(MachineUnary::Copy, native_operand(mapped)),
                });
                self.output
                    .statement(Statement::Jump((flow.step).to_string()));
            }
            Builtin::ListFilter => {
                let retain = locals.label();
                self.output.statement(Statement::Branch {
                    condition: native_operand(mapped),
                    then_label: (retain).to_string(),
                    else_label: (flow.step).to_string(),
                });
                self.start_block(locals, &retain);
                self.output
                    .statement(Statement::Effect(NativeOperation::Call {
                        callee: native_operand("$fern_list_push_mut"),
                        args: vec![
                            (Scalar::I64, native_operand(output)),
                            (Scalar::I64, native_operand(raw)),
                        ],
                        variadic: None,
                    }));
                self.output
                    .statement(Statement::Jump((flow.step).to_string()));
            }
            Builtin::ListAll => self.output.statement(Statement::Branch {
                condition: native_operand(mapped),
                then_label: (flow.step).to_string(),
                else_label: (flow.found).to_string(),
            }),
            _ => self.output.statement(Statement::Branch {
                condition: native_operand(mapped),
                then_label: (flow.found).to_string(),
                else_label: (flow.step).to_string(),
            }),
        }
    }

    /// Merge an exhausted list and an early predicate result without invoking more items.
    fn loop_search_result(
        &mut self,
        flow: &Loop,
        builtin: Builtin,
        result: &Type,
        raw: &str,
        locals: &mut Locals,
    ) -> String {
        let empty = if builtin == Builtin::ListFind {
            self.assign(
                locals,
                result.clone(),
                NativeOperation::Call {
                    callee: native_operand("$fern_result_err"),
                    args: vec![(Scalar::I64, native_operand("0"))],
                    variadic: None,
                },
            )
        } else {
            u8::from(builtin == Builtin::ListAll).to_string()
        };
        self.output
            .statement(Statement::Jump((flow.merge).to_string()));
        self.start_block(locals, &flow.found);
        let found = if builtin == Builtin::ListFind {
            self.assign(
                locals,
                result.clone(),
                NativeOperation::Call {
                    callee: native_operand("$fern_result_ok"),
                    args: vec![(Scalar::I64, native_operand(raw))],
                    variadic: None,
                },
            )
        } else {
            u8::from(builtin == Builtin::ListAny).to_string()
        };
        self.output
            .statement(Statement::Jump((flow.merge).to_string()));
        self.start_block(locals, &flow.merge);
        self.assign(
            locals,
            result.clone(),
            NativeOperation::Phi(vec![
                ((flow.exhausted).to_string(), native_operand(&(empty))),
                ((flow.found).to_string(), native_operand(&(found))),
            ]),
        )
    }

    /// Transform an active success payload, retaining its checked callback ABI.
    fn higher_sum_success(
        &mut self,
        builtin: Builtin,
        args: &[Expr],
        values: &[String],
        result: &Type,
        locals: &mut Locals,
    ) -> String {
        let original = &values[0];
        let callback = &values[1];
        let Type::Function(params, callback_result) = &args[1].ty else {
            unreachable!("signature validated")
        };
        let raw = self.assign(
            locals,
            Type::Int,
            NativeOperation::Call {
                callee: native_operand("$fern_result_unwrap"),
                args: vec![(Scalar::I64, native_operand(&(original).to_string()))],
                variadic: None,
            },
        );
        if builtin == Builtin::ResultUnwrapOrElse {
            self.unpack(locals, result, raw)
        } else {
            let payload = self.unpack(locals, &params[0], raw);
            let mapped = self.invoke_values(
                callback,
                &[(params[0].clone(), payload)],
                callback_result,
                locals,
            );
            if builtin == Builtin::ResultAndThen {
                mapped
            } else {
                let packed = self.payload(locals, callback_result, mapped);
                self.assign(
                    locals,
                    result.clone(),
                    NativeOperation::Call {
                        callee: native_operand("$fern_result_ok"),
                        args: vec![(Scalar::I64, native_operand(&(packed)))],
                        variadic: None,
                    },
                )
            }
        }
    }

    /// Inspect the tag before unwrapping and skip callbacks on the inactive variant.
    fn higher_sum(
        &mut self,
        builtin: Builtin,
        args: &[Expr],
        values: &[String],
        result: &Type,
        locals: &mut Locals,
    ) -> String {
        let original = &values[0];
        let callback = &values[1];
        let Type::Function(params, callback_result) = &args[1].ty else {
            unreachable!("signature validated")
        };
        let tag = self.assign(
            locals,
            Type::Bool,
            NativeOperation::Call {
                callee: native_operand("$fern_result_is_ok"),
                args: vec![(Scalar::I64, native_operand(&(original).to_string()))],
                variadic: None,
            },
        );
        let success = locals.label();
        let failure = locals.label();
        let merge = locals.label();
        self.output.statement(Statement::Branch {
            condition: native_operand(&(tag)),
            then_label: (success).to_string(),
            else_label: (failure).to_string(),
        });
        self.start_block(locals, &success);
        let ok = self.higher_sum_success(builtin, args, values, result, locals);
        let ok_end = locals.current.clone();
        self.output.statement(Statement::Jump((merge).to_string()));
        self.start_block(locals, &failure);
        let err = if builtin == Builtin::ResultUnwrapOrElse {
            let raw = self.assign(
                locals,
                Type::Int,
                NativeOperation::Call {
                    callee: native_operand("$fern_result_unwrap"),
                    args: vec![(Scalar::I64, native_operand(&(original).to_string()))],
                    variadic: None,
                },
            );
            let payload = self.unpack(locals, &params[0], raw);
            self.invoke_values(
                callback,
                &[(params[0].clone(), payload)],
                callback_result,
                locals,
            )
        } else {
            original.clone()
        };
        let err_end = locals.current.clone();
        self.output.statement(Statement::Jump((merge).to_string()));
        self.start_block(locals, &merge);
        if *result == Type::Unit {
            "0".into()
        } else {
            self.assign(
                locals,
                result.clone(),
                NativeOperation::Phi(vec![
                    ((ok_end), native_operand(&(ok))),
                    ((err_end), native_operand(&(err))),
                ]),
            )
        }
    }
}
