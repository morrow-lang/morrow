//! Typed calls into audited native APIs, including explicit representation bridges.
use super::*;
use crate::runtime::{self, Operation as RuntimeOperation, Signature, ValueAbi};

/// Unify a signature scheme with concrete arguments, without accepting untyped fallbacks.
fn bind_type(
    template: &Type,
    actual: &Type,
    bindings: &mut HashMap<String, Type>,
    span: Span,
    depth: usize,
) -> Lowering<()> {
    if depth > MAX_DEPTH {
        return Err(invalid(span, "runtime signature nesting limit exceeded"));
    }
    match (template, actual) {
        (Type::Generic(name), actual) => {
            if let Some(expected) = bindings.get(name) {
                expect_type(actual.clone(), expected.clone(), span)
            } else {
                bindings.insert(name.clone(), actual.clone());
                Ok(())
            }
        }
        (Type::List(a), Type::List(b)) | (Type::Option(a), Type::Option(b)) => {
            bind_type(a, b, bindings, span, depth + 1)
        }
        (Type::Result(a, e), Type::Result(b, f)) => {
            bind_type(a, b, bindings, span, depth + 1)?;
            bind_type(e, f, bindings, span, depth + 1)
        }
        _ => expect_type(actual.clone(), template.clone(), span),
    }
}

/// Resolve the return scheme solely from argument substitutions and checked types.
fn return_type(template: &Type, bindings: &HashMap<String, Type>, span: Span) -> Lowering<Type> {
    match template {
        Type::Generic(name) => bindings
            .get(name)
            .cloned()
            .ok_or_else(|| invalid(span, "unresolved runtime type parameter")),
        Type::List(item) => Ok(Type::List(Box::new(return_type(item, bindings, span)?))),
        Type::Option(item) => Ok(Type::Option(Box::new(return_type(item, bindings, span)?))),
        Type::Result(ok, err) => Ok(Type::Result(
            Box::new(return_type(ok, bindings, span)?),
            Box::new(return_type(err, bindings, span)?),
        )),
        _ => Ok(template.clone()),
    }
}

impl Emitter<'_> {
    /// Resolve an audited runtime ID and lower its concrete typed arguments exactly once.
    pub(super) fn runtime_call(
        &mut self,
        id: usize,
        args: &[Expr],
        span: Span,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        let (signature, ty) = self.checked_runtime_signature(id, args, span)?;
        match signature.symbol {
            "fern_list_get" | "fern_list_head" => {
                return self.list_access(
                    args,
                    signature.symbol == "fern_list_head",
                    span,
                    locals,
                    depth,
                );
            }
            "fern_str_repeat" => return self.repeat_string(args, span, locals, depth),
            "fern_str_slice" => return self.slice_string(args, span, locals, depth),
            _ => {}
        }
        if signature.operation == RuntimeOperation::ScalarContains {
            return self.scalar_contains(args, span, locals, depth);
        }
        let symbol = self.test_runtime_symbol(runtime_symbol(&signature, args, span)?);
        if signature.operation == RuntimeOperation::JsonObject {
            let map = self.expr(&args[0], locals, depth)?;
            let value = self.assign(
                locals,
                ty.clone(),
                NativeOperation::Call {
                    callee: native_operand("$fern_rs_json_object"),
                    args: vec![(Scalar::I64, native_operand(&(map)))],
                    variadic: None,
                },
            );
            return Ok((ty, value));
        }
        let mut values = Vec::new();
        for (arg, abi) in args.iter().zip(&signature.parameter_abi) {
            let value = self.expr(arg, locals, depth)?;
            values.push(self.runtime_argument(value, &arg.ty, *abi, span, locals)?);
        }
        self.runtime_operation(signature.operation, &mut values, span, locals)?;
        if matches!(
            symbol,
            "fern_json_value_null" | "fern_json_value_from_bool" | "fern_json_value_from_int"
        ) {
            let value = self.assign(
                locals,
                ty.clone(),
                NativeOperation::Call {
                    callee: native_operand(&format!("${symbol}")),
                    args: values,
                    variadic: None,
                },
            );
            self.output
                .statement(Statement::Effect(NativeOperation::Call {
                    callee: native_operand("$fern_json_codec_scope_check"),
                    args: vec![(Scalar::I64, native_operand("%fault"))],
                    variadic: None,
                }));
            self.guard_fault(locals);
            return Ok((ty, value));
        }
        if matches!(symbol, "fern_regex_replace" | "fern_regex_replace_all") {
            values.insert(0, (Scalar::I64, native_operand("%fault")));
            let value = self.assign(
                locals,
                ty.clone(),
                NativeOperation::Call {
                    callee: native_operand(&format!("${symbol}_checked")),
                    args: values,
                    variadic: None,
                },
            );
            self.guard_fault(locals);
            return Ok((ty, value));
        }
        if symbol == "fern_str_split" {
            self.split_guard(&values, locals);
        }
        if signature.return_abi == ValueAbi::PackedOption {
            return Ok((ty, self.packed_option(symbol, &values, span, locals)?));
        }
        let terminal = [
            "fern_style_",
            "fern_panel_",
            "fern_table_",
            "fern_progress_",
            "fern_spinner_",
            "fern_tree_",
            "fern_status_",
            "fern_log_",
        ]
        .iter()
        .any(|prefix| symbol.starts_with(prefix));
        let terminal_scope = terminal.then(|| {
            self.assign(
                locals,
                Type::Int,
                NativeOperation::Call {
                    callee: native_operand("$fern_tui_fault_enter"),
                    args: vec![(Scalar::I64, native_operand("%fault"))],
                    variadic: None,
                },
            )
        });
        let instruction = NativeOperation::Call {
            callee: native_operand(&format!("${}", symbol)),
            args: values.clone(),
            variadic: None,
        };
        let raw = match signature.return_abi {
            ValueAbi::Void => {
                self.output.statement(Statement::Effect(instruction));
                "0".into()
            }
            ValueAbi::Word32 => self.assign(locals, Type::Bool, instruction),
            _ => self.assign(locals, Type::Int, instruction),
        };
        if let Some(previous) = terminal_scope {
            self.output
                .statement(Statement::Effect(NativeOperation::Call {
                    callee: native_operand("$fern_tui_fault_leave"),
                    args: vec![(Scalar::I64, native_operand(&previous))],
                    variadic: None,
                }));
            self.guard_fault(locals);
        }
        let mut value = self.runtime_result(raw, &ty, signature.return_abi, locals)?;
        if signature.operation == RuntimeOperation::InvertBool {
            value = self.assign(
                locals,
                Type::Bool,
                NativeOperation::Binary(
                    MachineBinary::Compare(Comparison::Eq, Scalar::I32),
                    native_operand(&(value).to_string()),
                    native_operand("0"),
                ),
            );
        }
        Ok((ty, value))
    }

    /// Validate the complete public IR signature before evaluating argument effects.
    fn checked_runtime_signature(
        &self,
        id: usize,
        args: &[Expr],
        span: Span,
    ) -> Lowering<(Signature, Type)> {
        let signature =
            runtime::signature(id).ok_or_else(|| invalid(span, "unknown runtime identity"))?;
        if signature.parameters.len() != args.len() {
            return Err(invalid(
                span,
                "runtime argument count differs from signature",
            ));
        }
        if signature.return_abi == ValueAbi::NullableStringList {
            return Err(invalid(
                span,
                "nullable directory-list result needs an explicit error contract before native lowering",
            ));
        }
        let mut bindings = HashMap::new();
        for (template, arg) in signature.parameters.iter().zip(args) {
            nominal::resolved(&arg.ty, &self.layouts, arg.span, 0)?;
            bind_type(template, &arg.ty, &mut bindings, arg.span, 0)?;
        }
        let ty = return_type(&signature.return_type, &bindings, span)?;
        Ok((signature, ty))
    }

    /// Apply source/native arity and enum differences after evaluating arguments once.
    fn runtime_operation(
        &mut self,
        operation: RuntimeOperation,
        values: &mut Vec<(Scalar, Operand)>,
        span: Span,
        locals: &mut Locals,
    ) -> Lowering<()> {
        match operation {
            RuntimeOperation::DecimalPredicate => self.decimal_guard(values, locals),
            RuntimeOperation::UniformPadding => values.push(values[1].clone()),
            RuntimeOperation::TableBorder => {
                if values[1].0 != Scalar::I64 {
                    return Err(invalid(span, "border name requires String ABI"));
                }
                let name = values[1].1.clone();
                let value = self.border_style(name, span, locals)?;
                values[1] = (Scalar::I64, native_operand(&value));
            }
            _ => {}
        }
        Ok(())
    }

    /// Map known style names to C enum values; -1 preserves the object's existing style.
    fn border_style(&mut self, name: Operand, span: Span, locals: &mut Locals) -> Lowering<String> {
        let merge = locals.label();
        let mut incoming = Vec::new();
        for (index, style) in ["rounded", "square", "double", "heavy", "ascii", "none"]
            .iter()
            .enumerate()
        {
            let literal = self.string(style, span)?;
            let equal = self.assign(
                locals,
                Type::Int,
                NativeOperation::Call {
                    callee: native_operand("$fern_str_eq"),
                    args: vec![
                        (Scalar::I64, name.clone()),
                        (Scalar::I64, native_operand(&(literal))),
                    ],
                    variadic: None,
                },
            );
            let found = locals.label();
            let next = locals.label();
            self.output.statement(Statement::Branch {
                condition: native_operand(&(equal)),
                then_label: (found).to_string(),
                else_label: (next).to_string(),
            });
            self.start_block(locals, &found);
            incoming.push((found.clone(), Operand::Int(index as i64)));
            self.output.statement(Statement::Jump((merge).to_string()));
            self.start_block(locals, &next);
        }
        incoming.push((locals.current.clone(), Operand::Int(-1)));
        self.output.statement(Statement::Jump((merge).to_string()));
        self.start_block(locals, &merge);
        Ok(self.assign(locals, Type::Int, NativeOperation::Phi(incoming)))
    }

    /// Convert a checked Fern argument to its documented native representation.
    fn runtime_argument(
        &mut self,
        value: String,
        ty: &Type,
        abi: ValueAbi,
        span: Span,
        locals: &mut Locals,
    ) -> Lowering<(Scalar, Operand)> {
        match abi {
            ValueAbi::Double64 => {
                expect_type(ty.clone(), Type::Float, span)?;
                Ok((Scalar::F64, native_operand(&value)))
            }
            ValueAbi::StringList => Ok((
                Scalar::I64,
                native_operand(&self.emit_string_list_argument(&value, locals)),
            )),
            ValueAbi::Word64 | ValueAbi::HeapOption | ValueAbi::HeapResult => Ok((
                Scalar::I64,
                native_operand(&self.payload(locals, ty, value)),
            )),
            ValueAbi::Word32 => Ok((Scalar::I32, native_operand(&value))),
            _ => Err(invalid(span, "unsupported native argument representation")),
        }
    }

    /// Convert native results, retaining signed C int returns and semantic payload widths.
    fn runtime_result(
        &mut self,
        raw: String,
        ty: &Type,
        abi: ValueAbi,
        locals: &mut Locals,
    ) -> Lowering<String> {
        Ok(match abi {
            ValueAbi::Void => "0".into(),
            ValueAbi::Word32 if *ty == Type::Int => self.assign(
                locals,
                Type::Int,
                NativeOperation::Unary(MachineUnary::ExtSw, native_operand(&(raw).to_string())),
            ),
            ValueAbi::Word32 => raw,
            ValueAbi::StringList => self.emit_string_list_result(&raw, locals),
            ValueAbi::HeapJsonMembers => self.assign(
                locals,
                ty.clone(),
                NativeOperation::Call {
                    callee: native_operand("$fern_rs_json_members"),
                    args: vec![(Scalar::I64, native_operand(&(raw).to_string()))],
                    variadic: None,
                },
            ),
            ValueAbi::HeapStringListResult => self.native_adapted_result(&raw, false, locals),
            ValueAbi::HeapExecResult => self.native_adapted_result(&raw, true, locals),
            ValueAbi::ExecResult => self.native_tuple(&raw, 3, &[1, 2], locals),
            ValueAbi::TermSize => self.native_tuple(&raw, 2, &[], locals),
            ValueAbi::RegexMatch => self.native_match(&raw, locals),
            ValueAbi::RegexCaptures => self.native_captures(&raw, locals),
            ValueAbi::Word64 | ValueAbi::HeapOption | ValueAbi::HeapResult => {
                self.unpack(locals, ty, raw)
            }
            _ => {
                return Err(invalid(
                    Span::default(),
                    "unsupported native return representation",
                ));
            }
        })
    }
}

/// Select equality specialization only after the generic element type is known.
fn runtime_symbol(signature: &Signature, args: &[Expr], span: Span) -> Lowering<&'static str> {
    if signature.operation == RuntimeOperation::ScalarContains {
        match args.get(1).map(|arg| &arg.ty) {
            Some(Type::String) => Ok("fern_list_contains_str"),
            Some(Type::Int | Type::Bool) => Ok("fern_list_contains"),
            _ => Err(invalid(
                span,
                "List.contains requires Int, Bool, or String elements",
            )),
        }
    } else {
        Ok(signature.symbol)
    }
}

impl Emitter<'_> {
    /// Bridge only audited packed producers; substring indexes bypass truncating C packing.
    fn packed_option(
        &mut self,
        symbol: &str,
        arguments: &[(Scalar, Operand)],
        span: Span,
        locals: &mut Locals,
    ) -> Lowering<String> {
        let joined = arguments.to_vec();
        match symbol {
            "fern_str_index_of" => {
                let base = arguments
                    .first()
                    .filter(|arg| arg.0 == Scalar::I64)
                    .map(|arg| arg.1.clone())
                    .ok_or_else(|| invalid(span, "index_of requires pointer arguments"))?;
                let found = self.assign(
                    locals,
                    Type::Int,
                    NativeOperation::Call {
                        callee: native_operand("$strstr"),
                        args: joined,
                        variadic: None,
                    },
                );
                let present = self.assign(
                    locals,
                    Type::Bool,
                    NativeOperation::Binary(
                        MachineBinary::Compare(Comparison::Ne, Scalar::I64),
                        native_operand(&(found)),
                        native_operand("0"),
                    ),
                );
                Ok(self.heap_option(
                    &present,
                    NativeOperation::Binary(MachineBinary::Sub, native_operand(&(found)), base),
                    locals,
                ))
            }
            "fern_str_char_at" => {
                // This C helper only packs bytes 0..255; all payload bits survive.
                let packed = self.assign(
                    locals,
                    Type::Int,
                    NativeOperation::Call {
                        callee: native_operand("$fern_str_char_at"),
                        args: joined,
                        variadic: None,
                    },
                );
                let tag = self.assign(
                    locals,
                    Type::Bool,
                    NativeOperation::Unary(MachineUnary::Copy, native_operand(&(packed))),
                );
                let present = self.assign(
                    locals,
                    Type::Bool,
                    NativeOperation::Binary(
                        MachineBinary::Compare(Comparison::Eq, Scalar::I32),
                        native_operand(&(tag)),
                        native_operand("1"),
                    ),
                );
                Ok(self.heap_option(
                    &present,
                    NativeOperation::Binary(
                        MachineBinary::Sar,
                        native_operand(&(packed)),
                        native_operand("32"),
                    ),
                    locals,
                ))
            }
            _ => Err(invalid(
                span,
                "packed Option producer has no proven lossless adapter",
            )),
        }
    }

    /// Create a heap Option with payload computation restricted to the Some branch.
    fn heap_option(
        &mut self,
        present: &str,
        payload_instruction: NativeOperation,
        locals: &mut Locals,
    ) -> String {
        let some = locals.label();
        let none = locals.label();
        let merge = locals.label();
        self.output.statement(Statement::Branch {
            condition: native_operand(present),
            then_label: (some).to_string(),
            else_label: (none).to_string(),
        });
        self.start_block(locals, &some);
        let payload = self.assign(locals, Type::Int, payload_instruction);
        let value = self.assign(
            locals,
            Type::Int,
            NativeOperation::Call {
                callee: native_operand("$fern_result_ok"),
                args: vec![(Scalar::I64, native_operand(&(payload)))],
                variadic: None,
            },
        );
        self.output.statement(Statement::Jump((merge).to_string()));
        self.start_block(locals, &none);
        let empty = self.assign(
            locals,
            Type::Int,
            NativeOperation::Call {
                callee: native_operand("$fern_result_err"),
                args: vec![(Scalar::I64, native_operand("0"))],
                variadic: None,
            },
        );
        self.output.statement(Statement::Jump((merge).to_string()));
        self.start_block(locals, &merge);
        self.assign(
            locals,
            Type::Int,
            NativeOperation::Phi(vec![
                ((some).to_string(), native_operand(&(value))),
                ((none).to_string(), native_operand(&(empty))),
            ]),
        )
    }
}

/// Only these two audited list representations participate in runtime copies.
#[derive(Clone, Copy)]
enum CopyEndpoint<'a> {
    FernList(&'a str),
    StringData(&'a str),
}

impl Emitter<'_> {
    /// Build an independent FernStringList header/data pair from a checked List(String).
    fn emit_string_list_argument(&mut self, value: &str, locals: &mut Locals) -> String {
        let length = self.assign(
            locals,
            Type::Int,
            NativeOperation::Call {
                callee: native_operand("$fern_list_len"),
                args: vec![(Scalar::I64, native_operand(value))],
                variadic: None,
            },
        );
        let capacity = self.assign(
            locals,
            Type::Int,
            NativeOperation::Binary(
                MachineBinary::Add,
                native_operand(&(length).to_string()),
                native_operand("1"),
            ),
        );
        let bytes = self.assign(
            locals,
            Type::Int,
            NativeOperation::Binary(
                MachineBinary::Mul,
                native_operand(&(capacity).to_string()),
                native_operand("8"),
            ),
        );
        let header = self.assign(
            locals,
            Type::Int,
            NativeOperation::Call {
                callee: native_operand("$fern_alloc"),
                args: vec![(Scalar::I64, native_operand("24"))],
                variadic: None,
            },
        );
        let data = self.assign(
            locals,
            Type::Int,
            NativeOperation::Call {
                callee: native_operand("$fern_alloc"),
                args: vec![(Scalar::I64, native_operand(&(bytes)))],
                variadic: None,
            },
        );
        // runtime/fern_runtime.h: FernStringList {char** data; i64 len; i64 cap}.
        self.output.statement(Statement::Store {
            kind: LoadKind::I64,
            value: native_operand(&(data).to_string()),
            address: native_operand(&(header).to_string()),
        });
        self.store_header(&header, 8, &length, locals);
        self.store_header(&header, 16, &capacity, locals);
        self.copy_strings(
            CopyEndpoint::FernList(value),
            CopyEndpoint::StringData(&data),
            &length,
            locals,
        );
        header
    }

    /// Copy a non-null native FernStringList into an ordinary immutable FernList.
    fn emit_string_list_result(&mut self, value: &str, locals: &mut Locals) -> String {
        let data = self.assign(
            locals,
            Type::Int,
            NativeOperation::Load(LoadKind::I64, native_operand(value)),
        );
        let length_pointer = self.assign(
            locals,
            Type::Int,
            NativeOperation::Binary(
                MachineBinary::Add,
                native_operand(value),
                native_operand("8"),
            ),
        );
        let length = self.assign(
            locals,
            Type::Int,
            NativeOperation::Load(LoadKind::I64, native_operand(&(length_pointer))),
        );
        let capacity = self.assign(
            locals,
            Type::Int,
            NativeOperation::Binary(
                MachineBinary::Add,
                native_operand(&(length).to_string()),
                native_operand("1"),
            ),
        );
        let list = self.assign(
            locals,
            Type::Int,
            NativeOperation::Call {
                callee: native_operand("$fern_list_with_capacity"),
                args: vec![(Scalar::I64, native_operand(&(capacity)))],
                variadic: None,
            },
        );
        self.copy_strings(
            CopyEndpoint::StringData(&data),
            CopyEndpoint::FernList(&list),
            &length,
            locals,
        );
        list
    }

    /// Store a full-width runtime header field at its audited byte offset.
    fn store_header(&mut self, header: &str, offset: usize, value: &str, locals: &mut Locals) {
        let address = self.assign(
            locals,
            Type::Int,
            NativeOperation::Binary(
                MachineBinary::Add,
                native_operand(header),
                native_operand(&(offset).to_string()),
            ),
        );
        self.output.statement(Statement::Store {
            kind: LoadKind::I64,
            value: native_operand(value),
            address: native_operand(&(address)),
        });
    }

    /// Copy exactly the source length, preserving 64-bit String pointers and fresh ownership.
    fn copy_strings(
        &mut self,
        source: CopyEndpoint<'_>,
        destination: CopyEndpoint<'_>,
        length: &str,
        locals: &mut Locals,
    ) {
        let before = locals.current.clone();
        let test = locals.label();
        let body = locals.label();
        let done = locals.label();
        let index = locals.temporary();
        let next = locals.temporary();
        self.output.statement(Statement::Jump((test).to_string()));
        self.start_block(locals, &test);
        self.output.statement(Statement::Assign {
            destination: (index).to_string(),
            ty: Scalar::I64,
            operation: NativeOperation::Phi(vec![
                ((before), native_operand("0")),
                ((body).to_string(), native_operand(&(next))),
            ]),
        });
        let more = self.assign(
            locals,
            Type::Bool,
            NativeOperation::Binary(
                MachineBinary::Compare(Comparison::SLt, Scalar::I64),
                native_operand(&(index).to_string()),
                native_operand(length),
            ),
        );
        self.output.statement(Statement::Branch {
            condition: native_operand(&(more)),
            then_label: (body).to_string(),
            else_label: (done).to_string(),
        });
        self.start_block(locals, &body);
        let value = match source {
            CopyEndpoint::FernList(list) => self.assign(
                locals,
                Type::Int,
                NativeOperation::Call {
                    callee: native_operand("$fern_list_get"),
                    args: vec![
                        (Scalar::I64, native_operand(list)),
                        (Scalar::I64, native_operand(&(index).to_string())),
                    ],
                    variadic: None,
                },
            ),
            CopyEndpoint::StringData(data) => {
                let address = self.string_slot(data, &index, locals);
                self.assign(
                    locals,
                    Type::Int,
                    NativeOperation::Load(LoadKind::I64, native_operand(&(address))),
                )
            }
        };
        match destination {
            CopyEndpoint::FernList(list) => {
                self.output
                    .statement(Statement::Effect(NativeOperation::Call {
                        callee: native_operand("$fern_list_push_mut"),
                        args: vec![
                            (Scalar::I64, native_operand(list)),
                            (Scalar::I64, native_operand(&(value))),
                        ],
                        variadic: None,
                    }))
            }
            CopyEndpoint::StringData(data) => {
                let address = self.string_slot(data, &index, locals);
                self.output.statement(Statement::Store {
                    kind: LoadKind::I64,
                    value: native_operand(&(value)),
                    address: native_operand(&(address)),
                });
            }
        }
        self.output.statement(Statement::Assign {
            destination: (next),
            ty: Scalar::I64,
            operation: NativeOperation::Binary(
                MachineBinary::Add,
                native_operand(&(index).to_string()),
                native_operand("1"),
            ),
        });
        self.output.statement(Statement::Jump((test).to_string()));
        self.start_block(locals, &done);
    }

    /// Address one element in a native array of 64-bit String pointers.
    fn string_slot(&mut self, data: &str, index: &str, locals: &mut Locals) -> String {
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
                native_operand(&(offset)),
            ),
        )
    }
}

impl Emitter<'_> {
    /// Reject unexpected allocation failures before accessing a native result pointer.
    fn require_native_pointer(&mut self, pointer: &str, locals: &mut Locals) {
        let valid = locals.label();
        let failure = locals.label();
        let exists = self.assign(
            locals,
            Type::Bool,
            NativeOperation::Binary(
                MachineBinary::Compare(Comparison::Ne, Scalar::I64),
                native_operand(pointer),
                native_operand("0"),
            ),
        );
        self.output.statement(Statement::Branch {
            condition: native_operand(&(exists)),
            then_label: (valid).to_string(),
            else_label: (failure).to_string(),
        });
        self.start_block(locals, &failure);
        self.output.statement(Statement::Trap);
        self.start_block(locals, &valid);
    }

    /// Read one audited 64-bit C field; callers must first establish a valid object.
    fn native_field(&mut self, object: &str, offset: usize, locals: &mut Locals) -> String {
        let address = self.assign(
            locals,
            Type::Int,
            NativeOperation::Binary(
                MachineBinary::Add,
                native_operand(object),
                native_operand(&(offset).to_string()),
            ),
        );
        self.assign(
            locals,
            Type::Int,
            NativeOperation::Load(LoadKind::I64, native_operand(&(address))),
        )
    }

    /// Copy a C record with contiguous 64-bit fields into a separate tagged Rust tuple.
    fn native_tuple(
        &mut self,
        object: &str,
        fields: usize,
        strings: &[usize],
        locals: &mut Locals,
    ) -> String {
        self.require_native_pointer(object, locals);
        let result = self.assign(
            locals,
            Type::Int,
            NativeOperation::Call {
                callee: native_operand("$fern_alloc"),
                args: vec![(Scalar::I64, native_operand(&((fields + 1) * 8).to_string()))],
                variadic: None,
            },
        );
        self.output.statement(Statement::Store {
            kind: LoadKind::I64,
            value: native_operand("0"),
            address: native_operand(&(result).to_string()),
        });
        for index in 0..fields {
            let value = self.native_field(object, index * 8, locals);
            if strings.contains(&index) {
                self.require_native_pointer(&value, locals);
            }
            self.store_header(&result, (index + 1) * 8, &value, locals);
        }
        result
    }

    /// Native Regex.find uses start=-1 for invalid patterns and absence; no text exists then.
    fn native_match(&mut self, object: &str, locals: &mut Locals) -> String {
        self.require_native_pointer(object, locals);
        let start = self.native_field(object, 0, locals);
        let present = self.assign(
            locals,
            Type::Bool,
            NativeOperation::Binary(
                MachineBinary::Compare(Comparison::SGe, Scalar::I64),
                native_operand(&(start)),
                native_operand("0"),
            ),
        );
        let some = locals.label();
        let none = locals.label();
        let merge = locals.label();
        self.output.statement(Statement::Branch {
            condition: native_operand(&(present)),
            then_label: (some).to_string(),
            else_label: (none).to_string(),
        });
        self.start_block(locals, &some);
        let tuple = self.native_tuple(object, 3, &[2], locals);
        let value = self.assign(
            locals,
            Type::Int,
            NativeOperation::Call {
                callee: native_operand("$fern_result_ok"),
                args: vec![(Scalar::I64, native_operand(&(tuple)))],
                variadic: None,
            },
        );
        let some_end = locals.current.clone();
        self.output.statement(Statement::Jump((merge).to_string()));
        self.start_block(locals, &none);
        let absent = self.assign(
            locals,
            Type::Int,
            NativeOperation::Call {
                callee: native_operand("$fern_result_err"),
                args: vec![(Scalar::I64, native_operand("0"))],
                variadic: None,
            },
        );
        self.output.statement(Statement::Jump((merge).to_string()));
        self.start_block(locals, &merge);
        self.assign(
            locals,
            Type::Int,
            NativeOperation::Phi(vec![
                ((some_end), native_operand(&(value))),
                ((none).to_string(), native_operand(&(absent))),
            ]),
        )
    }

    /// Copy the native capture array without loading its nullable data pointer on empty results.
    fn native_captures(&mut self, object: &str, locals: &mut Locals) -> String {
        self.require_native_pointer(object, locals);
        let count = self.native_field(object, 0, locals);
        let list = self.assign(
            locals,
            Type::Int,
            NativeOperation::Call {
                callee: native_operand("$fern_list_with_capacity"),
                args: vec![(Scalar::I64, native_operand("10"))],
                variadic: None,
            },
        );
        let before = locals.current.clone();
        let test = locals.label();
        let body = locals.label();
        let latch = locals.label();
        let done = locals.label();
        let index = locals.temporary();
        let next = locals.temporary();
        self.output.statement(Statement::Jump((test).to_string()));
        self.start_block(locals, &test);
        self.output.statement(Statement::Assign {
            destination: (index).to_string(),
            ty: Scalar::I64,
            operation: NativeOperation::Phi(vec![
                ((before), native_operand("0")),
                ((latch).to_string(), native_operand(&(next))),
            ]),
        });
        let more = self.assign(
            locals,
            Type::Bool,
            NativeOperation::Binary(
                MachineBinary::Compare(Comparison::SLt, Scalar::I64),
                native_operand(&(index)),
                native_operand(&(count)),
            ),
        );
        self.output.statement(Statement::Branch {
            condition: native_operand(&(more)),
            then_label: (body).to_string(),
            else_label: (done).to_string(),
        });
        self.start_block(locals, &body);
        let data = self.native_field(object, 8, locals);
        self.require_native_pointer(&data, locals);
        let offset = self.assign(
            locals,
            Type::Int,
            NativeOperation::Binary(
                MachineBinary::Mul,
                native_operand(&(index)),
                native_operand("24"),
            ),
        );
        let record = self.assign(
            locals,
            Type::Int,
            NativeOperation::Binary(
                MachineBinary::Add,
                native_operand(&(data).to_string()),
                native_operand(&(offset)),
            ),
        );
        let tuple = self.native_tuple(&record, 3, &[2], locals);
        self.output
            .statement(Statement::Effect(NativeOperation::Call {
                callee: native_operand("$fern_list_push_mut"),
                args: vec![
                    (Scalar::I64, native_operand(&(list))),
                    (Scalar::I64, native_operand(&(tuple))),
                ],
                variadic: None,
            }));
        self.output.statement(Statement::Jump((latch).to_string()));
        self.start_block(locals, &latch);
        self.output.statement(Statement::Assign {
            destination: (next),
            ty: Scalar::I64,
            operation: NativeOperation::Binary(
                MachineBinary::Add,
                native_operand(&(index)),
                native_operand("1"),
            ),
        });
        self.output.statement(Statement::Jump((test).to_string()));
        self.start_block(locals, &done);
        list
    }
}

impl Emitter<'_> {
    /// Preserve native errors and translate only successful list or process tuple payloads.
    fn native_adapted_result(
        &mut self,
        result: &str,
        process: bool,
        locals: &mut Locals,
    ) -> String {
        self.require_native_pointer(result, locals);
        let success = self.assign(
            locals,
            Type::Int,
            NativeOperation::Call {
                callee: native_operand("$fern_result_is_ok"),
                args: vec![(Scalar::I64, native_operand(result))],
                variadic: None,
            },
        );
        let ok = locals.label();
        let err = locals.label();
        let merge = locals.label();
        self.output.statement(Statement::Branch {
            condition: native_operand(&(success)),
            then_label: (ok).to_string(),
            else_label: (err).to_string(),
        });
        self.start_block(locals, &ok);
        let native = self.assign(
            locals,
            Type::Int,
            NativeOperation::Call {
                callee: native_operand("$fern_result_unwrap"),
                args: vec![(Scalar::I64, native_operand(result))],
                variadic: None,
            },
        );
        self.require_native_pointer(&native, locals);
        let payload = if process {
            self.native_tuple(&native, 3, &[1, 2], locals)
        } else {
            self.emit_string_list_result(&native, locals)
        };
        let wrapped = self.assign(
            locals,
            Type::Int,
            NativeOperation::Call {
                callee: native_operand("$fern_result_ok"),
                args: vec![(Scalar::I64, native_operand(&(payload)))],
                variadic: None,
            },
        );
        let ok_end = locals.current.clone();
        self.output.statement(Statement::Jump((merge).to_string()));
        self.start_block(locals, &err);
        self.output.statement(Statement::Jump((merge).to_string()));
        self.start_block(locals, &merge);
        self.assign(
            locals,
            Type::Int,
            NativeOperation::Phi(vec![
                ((ok_end), native_operand(&(wrapped))),
                ((err).to_string(), native_operand(result)),
            ]),
        )
    }
}
