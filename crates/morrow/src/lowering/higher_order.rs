//! Higher-order collection operations invoke typed heap closures, never C callbacks.
use super::*;

/// Identify the compiler-owned callback operations.
pub(super) fn is_higher_order(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::ListMap
            | Builtin::ListSortBy
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
        (Type::List(item), Builtin::ListSortBy) => {
            if !matches!(&**returned, Type::Named(name, params) if name == "Ordering" && params.is_empty())
            {
                return Err(invalid(
                    span,
                    "List.sort_by comparator must return Ordering",
                ));
            }
            (vec![*item.clone(), *item.clone()], args[0].ty.clone())
        }
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

/// A fresh, unpublished list whose capacity covers the entire input traversal.
/// Only this loop may write its backing storage and initialized-prefix length.
struct ListOutput {
    header: String,
    data: String,
    length_slot: String,
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
        let value = if builtin == Builtin::ListSortBy {
            self.sort_by(args, &values, locals)?
        } else if matches!(args[0].ty, Type::List(_)) {
            self.higher_list(builtin, args, &values, &result, locals)?
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
    ) -> Lowering<String> {
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
                callee: native_operand("$morrow_list_len"),
                args: vec![(Scalar::I64, native_operand(&(collection).to_string()))],
                variadic: None,
            },
        );
        // Validation above establishes a nonnegative length within the backing
        // allocation. The source header is immutable, so callbacks cannot change
        // its data or bounds. Explicit roots keep both allocations alive across
        // callbacks; the native collector never relocates them.
        let input_data = self.higher_list_data(collection, locals);
        let output = if matches!(builtin, Builtin::ListMap | Builtin::ListFilter) {
            Some(self.higher_list_output(&length, result, locals))
        } else {
            None
        };
        let flow = Loop::new(locals);
        self.loop_header(&flow, &length, builtin, result, values, locals);
        let address = self.higher_list_slot(&input_data, &flow.index, locals);
        let raw = self.assign(
            locals,
            Type::Int,
            NativeOperation::Load(LoadKind::I64, native_operand(&address)),
        );
        let element = self.unpack(locals, item, raw.clone());
        let mut callback_args = Vec::new();
        if builtin == Builtin::ListFold {
            callback_args.push((result.clone(), flow.accumulator.clone()));
        }
        callback_args.push((*item.clone(), element));
        let mapped = self.invoke_callback_values(
            args.last().unwrap(),
            callback,
            &callback_args,
            callback_result,
            locals,
        )?;
        self.loop_action(
            &flow,
            builtin,
            output.as_ref(),
            (&raw, &mapped),
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
        Ok(match builtin {
            Builtin::ListMap | Builtin::ListFilter => output.unwrap().header,
            Builtin::ListFold => flow.accumulator,
            _ => self.loop_search_result(&flow, builtin, result, &raw, locals),
        })
    }

    /// Stable merge sort driven by the runtime state machine: compiled code performs
    /// every comparison through the typed closure, the runtime only chooses which
    /// positions to compare and materializes the final permutation.
    fn sort_by(
        &mut self,
        args: &[Expr],
        values: &[String],
        locals: &mut Locals,
    ) -> Lowering<String> {
        let Type::List(item) = &args[0].ty else {
            unreachable!("signature validated")
        };
        let Type::Function(_, ordering) = &args[1].ty else {
            unreachable!("signature validated")
        };
        let layout = self
            .layouts
            .get(ordering)
            .ok_or_else(|| invalid(args[1].span, "Ordering requires a concrete nominal layout"))?;
        if layout.variants.len() != 3 || layout.variants.iter().any(|fields| !fields.is_empty()) {
            return Err(invalid(
                args[1].span,
                "Ordering layout must have exactly the payload-free Less, Equal, Greater variants",
            ));
        }
        let collection = &values[0];
        let callback = &values[1];
        let call = |callee: &str, args: Vec<Operand>| NativeOperation::Call {
            callee: native_operand(callee),
            args: args.into_iter().map(|arg| (Scalar::I64, arg)).collect(),
            variadic: None,
        };
        let length = self.assign(
            locals,
            Type::Int,
            call("$morrow_list_len", vec![native_operand(collection)]),
        );
        // The immutable source stays rooted for the whole loop; positions come from
        // the runtime and are always below `length`, so slot reads stay in bounds.
        let data = self.higher_list_data(collection, locals);
        let state = self.assign(
            locals,
            Type::Int,
            call("$morrow_sort_begin", vec![native_operand(&length)]),
        );
        // The state is a collector-managed object: keep it alive across callbacks.
        self.root_pointer(locals, &state);
        let head = locals.label();
        let body = locals.label();
        let done = locals.label();
        self.output.statement(Statement::Jump(head.clone()));
        self.start_block(locals, &head);
        let pair = self.assign(
            locals,
            Type::Int,
            call("$morrow_sort_next", vec![native_operand(&state)]),
        );
        let pending = self.assign(
            locals,
            Type::Bool,
            NativeOperation::Binary(
                MachineBinary::Compare(Comparison::SGe, Scalar::I64),
                native_operand(&pair),
                native_operand("0"),
            ),
        );
        self.output.statement(Statement::Branch {
            condition: native_operand(&pending),
            then_label: body.clone(),
            else_label: done.clone(),
        });
        self.start_block(locals, &body);
        let left = self.assign(
            locals,
            Type::Int,
            NativeOperation::Binary(
                MachineBinary::Shr,
                native_operand(&pair),
                native_operand("32"),
            ),
        );
        let right = self.assign(
            locals,
            Type::Int,
            NativeOperation::Binary(
                MachineBinary::And,
                native_operand(&pair),
                native_operand("4294967295"),
            ),
        );
        let mut elements = Vec::new();
        for index in [&left, &right] {
            let address = self.higher_list_slot(&data, index, locals);
            let raw = self.assign(
                locals,
                Type::Int,
                NativeOperation::Load(LoadKind::I64, native_operand(&address)),
            );
            elements.push((*item.clone(), self.unpack(locals, item, raw)));
        }
        let outcome =
            self.invoke_callback_values(&args[1], callback, &elements, ordering, locals)?;
        // Ordering values are payload-free variants: the tag word orders Less < Equal < Greater.
        let tag = self.assign(
            locals,
            Type::Int,
            NativeOperation::Load(LoadKind::I64, native_operand(&outcome)),
        );
        let signum = self.assign(
            locals,
            Type::Int,
            NativeOperation::Binary(
                MachineBinary::Sub,
                native_operand(&tag),
                native_operand("1"),
            ),
        );
        self.output.statement(Statement::Effect(call(
            "$morrow_sort_report",
            vec![native_operand(&state), native_operand(&signum)],
        )));
        self.output.statement(Statement::Jump(head));
        self.start_block(locals, &done);
        Ok(self.assign(
            locals,
            args[0].ty.clone(),
            call(
                "$morrow_sort_finish",
                vec![native_operand(&state), native_operand(collection)],
            ),
        ))
    }

    /// Runtime list capacity must be positive even when mapping an empty collection.
    fn higher_list_output(
        &mut self,
        length: &str,
        result: &Type,
        locals: &mut Locals,
    ) -> ListOutput {
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
        let header = self.assign(
            locals,
            result.clone(),
            NativeOperation::Call {
                callee: native_operand("$morrow_list_with_capacity"),
                args: vec![(Scalar::I64, native_operand(&(capacity)))],
                variadic: None,
            },
        );
        let data = self.higher_list_data(&header, locals);
        let length_slot = self.assign(
            locals,
            Type::Int,
            NativeOperation::Binary(
                MachineBinary::Add,
                native_operand(&header),
                native_operand("8"),
            ),
        );
        ListOutput {
            header,
            data,
            length_slot,
        }
    }

    /// Load the audited List ABI's first field and retain its nonmoving storage.
    fn higher_list_data(&mut self, header: &str, locals: &mut Locals) -> String {
        let data = self.assign(
            locals,
            Type::Int,
            NativeOperation::Load(LoadKind::I64, native_operand(header)),
        );
        self.root_pointer(locals, &data);
        data
    }

    /// The loop guard and valid allocation bound prove index * 8 cannot overflow
    /// and addresses a full native payload word. No safepoint occurs before use.
    fn higher_list_slot(&mut self, data: &str, index: &str, locals: &mut Locals) -> String {
        let offset = self.assign(
            locals,
            Type::Int,
            NativeOperation::Binary(
                MachineBinary::Mul,
                native_operand(index),
                native_operand("8"),
            ),
        );
        self.assign(
            locals,
            Type::Int,
            NativeOperation::Binary(
                MachineBinary::Add,
                native_operand(data),
                native_operand(&offset),
            ),
        )
    }

    /// Append only to this loop's fresh builder. Map writes one output per input;
    /// filter writes at most one, so neither can exceed the reserved capacity.
    /// Zeroed backing storage traces completed pointers across later callbacks.
    fn higher_list_append(
        &mut self,
        output: &ListOutput,
        index: Option<&str>,
        payload: &str,
        locals: &mut Locals,
    ) {
        let index = index.map(str::to_owned).unwrap_or_else(|| {
            self.assign(
                locals,
                Type::Int,
                NativeOperation::Load(LoadKind::I64, native_operand(&output.length_slot)),
            )
        });
        let address = self.higher_list_slot(&output.data, &index, locals);
        self.output.statement(Statement::Store {
            kind: LoadKind::I64,
            value: native_operand(payload),
            address: native_operand(&address),
        });
        let next = self.assign(
            locals,
            Type::Int,
            NativeOperation::Binary(
                MachineBinary::Add,
                native_operand(&index),
                native_operand("1"),
            ),
        );
        self.output.statement(Statement::Store {
            kind: LoadKind::I64,
            value: native_operand(&next),
            address: native_operand(&output.length_slot),
        });
    }

    /// Define the induction/accumulator phis and guard each indexed native read.
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
        output: Option<&ListOutput>,
        values: (&str, &str),
        callback_result: &Type,
        locals: &mut Locals,
    ) {
        let (raw, mapped) = values;
        match builtin {
            Builtin::ListMap => {
                let payload = self.payload(locals, callback_result, mapped.into());
                self.higher_list_append(output.unwrap(), Some(&flow.index), &payload, locals);
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
                self.higher_list_append(output.unwrap(), None, raw, locals);
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
                    callee: native_operand("$morrow_result_err"),
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
                    callee: native_operand("$morrow_result_ok"),
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
                callee: native_operand("$morrow_result_unwrap"),
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
                        callee: native_operand("$morrow_result_ok"),
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
                callee: native_operand("$morrow_result_is_ok"),
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
                    callee: native_operand("$morrow_result_unwrap"),
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
