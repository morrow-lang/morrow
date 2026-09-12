//! First-class ranges and bounded collection iteration with lexical loop exits.
use super::*;

pub(super) struct LoopTargets {
    next: String,
    done: String,
}

struct Iteration {
    slot: String,
    head: String,
    body: String,
    next: String,
    increment: String,
    done: String,
    end: String,
    inclusive: String,
}

impl Emitter<'_> {
    /// Capture both endpoints once; range iteration remains lazy even at i64 limits.
    pub(super) fn range(
        &mut self,
        start: &Expr,
        end: &Expr,
        inclusive: bool,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        expect_type(start.ty.clone(), Type::Int, start.span)?;
        expect_type(end.ty.clone(), Type::Int, end.span)?;
        let start = self.expr(start, locals, depth)?;
        let end = self.expr(end, locals, depth)?;
        let value = self.assign(
            locals,
            Type::Range,
            NativeOperation::Call {
                callee: native_operand("$fern_alloc"),
                args: vec![(Scalar::I64, native_operand("24"))],
                variadic: None,
            },
        );
        self.store_field(&value, 0, &start, locals);
        self.store_field(&value, 8, &end, locals);
        self.store_field(&value, 16, &u8::from(inclusive).to_string(), locals);
        Ok((Type::Range, value))
    }

    /// Write a full-width internal field without inventing a source-level field access.
    pub(super) fn store_field(
        &mut self,
        value: &str,
        offset: usize,
        raw: &str,
        locals: &mut Locals,
    ) {
        let address = self.assign(
            locals,
            Type::Int,
            NativeOperation::Binary(
                MachineBinary::Add,
                native_operand(value),
                native_operand(&(offset).to_string()),
            ),
        );
        self.output.statement(Statement::Store {
            kind: LoadKind::I64,
            value: native_operand(raw),
            address: native_operand(&(address)),
        });
    }

    /// Read an audited internal layout field as raw bits for later typed unpacking.
    pub(super) fn raw_field(&mut self, value: &str, offset: usize, locals: &mut Locals) -> String {
        let address = self.assign(
            locals,
            Type::Int,
            NativeOperation::Binary(
                MachineBinary::Add,
                native_operand(value),
                native_operand(&(offset).to_string()),
            ),
        );
        self.assign(
            locals,
            Type::Int,
            NativeOperation::Load(LoadKind::I64, native_operand(&(address))),
        )
    }

    /// Jump to the innermost loop without draining this function's deferred callbacks.
    pub(super) fn loop_exit(
        &mut self,
        next: bool,
        span: Span,
        locals: &Locals,
    ) -> Lowering<(Type, String)> {
        let targets = locals
            .loops
            .last()
            .ok_or_else(|| invalid(span, "loop control outside loop"))?;
        let target = if next { &targets.next } else { &targets.done };
        self.output.statement(Statement::Jump((target).to_string()));
        Err(Exit::Terminated)
    }

    /// Iterate a captured collection, restoring the outer lexical scope on every exit.
    pub(super) fn iteration(
        &mut self,
        pattern: &Pattern,
        iterable: &Expr,
        body: &Expr,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        let item = match &iterable.ty {
            Type::Range => Type::Int,
            Type::List(item) => *item.clone(),
            Type::Map(key, value) => Type::Tuple(vec![*key.clone(), *value.clone()]),
            _ => return Err(invalid(iterable.span, "for requires List, Map, or Range")),
        };
        let collection = self.expr(iterable, locals, depth)?;
        let flow = self.iteration_setup(&collection, &iterable.ty, locals);
        let outer = locals.values.clone();
        let value = self.iteration_item(&flow, &collection, &iterable.ty, &item, locals);
        self.bind_irrefutable(pattern, &item, &value, iterable.span, locals, depth)?;
        locals.loops.push(LoopTargets {
            next: flow.next.clone(),
            done: flow.done.clone(),
        });
        let outcome = self.expr(body, locals, depth);
        locals.loops.pop();
        locals.values = outer;
        match outcome {
            Ok(_) => self
                .output
                .statement(Statement::Jump((flow.next).to_string())),
            Err(Exit::Terminated) => {}
            Err(error) => return Err(error),
        }
        self.iteration_increment(&flow, locals);
        self.start_block(locals, &flow.done);
        Ok((Type::Unit, "0".into()))
    }

    /// Guard the current induction value before any indexed load or body evaluation.
    fn iteration_setup(&mut self, collection: &str, ty: &Type, locals: &mut Locals) -> Iteration {
        let (start, end, inclusive) = if *ty == Type::Range {
            (
                self.raw_field(collection, 0, locals),
                self.raw_field(collection, 8, locals),
                self.raw_field(collection, 16, locals),
            )
        } else {
            (
                "0".into(),
                self.assign(
                    locals,
                    Type::Int,
                    NativeOperation::Call {
                        callee: native_operand("$fern_list_len"),
                        args: vec![(Scalar::I64, native_operand(collection))],
                        variadic: None,
                    },
                ),
                "0".into(),
            )
        };
        let flow = Iteration {
            slot: locals.stack_slot(),
            head: locals.label(),
            body: locals.label(),
            next: locals.label(),
            increment: locals.label(),
            done: locals.label(),
            end,
            inclusive,
        };
        self.output.statement(Statement::Store {
            kind: LoadKind::I64,
            value: native_operand(&(start)),
            address: native_operand(&(flow.slot).to_string()),
        });
        self.output
            .statement(Statement::Jump((flow.head).to_string()));
        self.start_block(locals, &flow.head);
        let index = self.assign(
            locals,
            Type::Int,
            NativeOperation::Load(LoadKind::I64, native_operand(&(flow.slot).to_string())),
        );
        let before = self.assign(
            locals,
            Type::Bool,
            NativeOperation::Binary(
                MachineBinary::Compare(Comparison::SLt, Scalar::I64),
                native_operand(&(index)),
                native_operand(&(flow.end).to_string()),
            ),
        );
        let within = self.assign(
            locals,
            Type::Bool,
            NativeOperation::Binary(
                MachineBinary::Compare(Comparison::SLe, Scalar::I64),
                native_operand(&(index)),
                native_operand(&(flow.end).to_string()),
            ),
        );
        let inclusive = self.assign(
            locals,
            Type::Bool,
            NativeOperation::Binary(
                MachineBinary::And,
                native_operand(&(within)),
                native_operand(&(flow.inclusive).to_string()),
            ),
        );
        let available = self.assign(
            locals,
            Type::Bool,
            NativeOperation::Binary(
                MachineBinary::Or,
                native_operand(&(before)),
                native_operand(&(inclusive)),
            ),
        );
        self.output.statement(Statement::Branch {
            condition: native_operand(&(available)),
            then_label: (flow.body).to_string(),
            else_label: (flow.done).to_string(),
        });
        self.start_block(locals, &flow.body);
        flow
    }

    /// Maps expose fresh structural tuples while lists preserve their typed payloads.
    fn iteration_item(
        &mut self,
        flow: &Iteration,
        collection: &str,
        ty: &Type,
        item: &Type,
        locals: &mut Locals,
    ) -> String {
        let index = self.assign(
            locals,
            Type::Int,
            NativeOperation::Load(LoadKind::I64, native_operand(&(flow.slot).to_string())),
        );
        if *ty == Type::Range {
            return index;
        }
        let raw = self.assign(
            locals,
            Type::Int,
            NativeOperation::Call {
                callee: native_operand("$fern_list_get"),
                args: vec![
                    (Scalar::I64, native_operand(collection)),
                    (Scalar::I64, native_operand(&(index))),
                ],
                variadic: None,
            },
        );
        if matches!(ty, Type::Map(_, _)) {
            let key = self.raw_field(&raw, 0, locals);
            let value = self.raw_field(&raw, 8, locals);
            let tuple = self.assign(
                locals,
                item.clone(),
                NativeOperation::Call {
                    callee: native_operand("$fern_alloc"),
                    args: vec![(Scalar::I64, native_operand("24"))],
                    variadic: None,
                },
            );
            self.store_field(&tuple, 0, "0", locals);
            self.store_field(&tuple, 8, &key, locals);
            self.store_field(&tuple, 16, &value, locals);
            tuple
        } else {
            self.unpack(locals, item, raw)
        }
    }

    /// Test the endpoint before incrementing, so inclusive i64::MAX cannot overflow.
    fn iteration_increment(&mut self, flow: &Iteration, locals: &mut Locals) {
        self.start_block(locals, &flow.next);
        let index = self.assign(
            locals,
            Type::Int,
            NativeOperation::Load(LoadKind::I64, native_operand(&(flow.slot).to_string())),
        );
        let at_end = self.assign(
            locals,
            Type::Bool,
            NativeOperation::Binary(
                MachineBinary::Compare(Comparison::Eq, Scalar::I64),
                native_operand(&(index)),
                native_operand(&(flow.end).to_string()),
            ),
        );
        self.output.statement(Statement::Branch {
            condition: native_operand(&(at_end)),
            then_label: (flow.done).to_string(),
            else_label: (flow.increment).to_string(),
        });
        self.start_block(locals, &flow.increment);
        let next = self.assign(
            locals,
            Type::Int,
            NativeOperation::Binary(
                MachineBinary::Add,
                native_operand(&(index)),
                native_operand("1"),
            ),
        );
        self.output.statement(Statement::Store {
            kind: LoadKind::I64,
            value: native_operand(&(next)),
            address: native_operand(&(flow.slot).to_string()),
        });
        self.output
            .statement(Statement::Jump((flow.head).to_string()));
    }

    /// Materialize index/item tuples using raw payload bits for every element type.
    pub(super) fn enumerate(
        &mut self,
        args: &[Expr],
        span: Span,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        let [arg] = args else {
            return Err(invalid(span, "List.enumerate requires one argument"));
        };
        let Type::List(item) = &arg.ty else {
            return Err(invalid(span, "List.enumerate requires List"));
        };
        let result = Type::List(Box::new(Type::Tuple(vec![Type::Int, *item.clone()])));
        let value = self.expr(arg, locals, depth)?;
        self.enumerate_used = true;
        let value = self.assign(
            locals,
            result.clone(),
            NativeOperation::Call {
                callee: native_operand("$fern_rs_list_enumerate"),
                args: vec![(Scalar::I64, native_operand(&(value)))],
                variadic: None,
            },
        );
        Ok((result, value))
    }
}
