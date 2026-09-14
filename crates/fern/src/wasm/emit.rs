//! Structured scalar control flow with lexical locals and typed branch results.
#[path = "aggregates.rs"]
mod aggregates;
#[path = "callables.rs"]
mod callables;
#[path = "cleanup.rs"]
mod cleanup;
#[path = "collections.rs"]
mod collections;
#[path = "higher_order.rs"]
mod higher_order;
#[path = "iteration.rs"]
mod iteration;
#[path = "maps.rs"]
mod maps;
#[path = "results.rs"]
mod results;
#[path = "text_helpers.rs"]
mod text_helpers;
#[path = "unions.rs"]
mod unions;
use super::{Result, expect, invalid, managed, strings, value_type};
use crate::{
    Span, Type,
    ast::{BinaryOp, UnaryOp},
    ir::{self, CallTarget, Expr, ExprKind, Pattern, Stmt},
};
use std::collections::BTreeMap;
use wasm_encoder::{BlockType, Function, Instruction as I, ValType};

type Functions<'a> = BTreeMap<usize, (u32, &'a ir::Function)>;
type Locals = BTreeMap<usize, (u32, Type)>;

pub(super) fn function(
    function: &ir::Function,
    functions: &Functions<'_>,
    work: &mut usize,
    strings: Option<&strings::Runtime>,
) -> Result<Function> {
    let mut emitter = Emitter {
        instructions: Vec::new(),
        locals: Vec::new(),
        param_count: (function.params.len() + function.captures.len()) as u32,
        functions,
        return_type: &function.return_type,
        work,
        strings,
        root_base: None,
        control_depth: 0,
        loops: Vec::new(),
        exit_target: None,
        return_slot: None,
        defer_head: None,
        defer_root: None,
    };
    if strings.is_some() {
        let base = emitter.temp(ValType::I32, function.body.span)?;
        emitter.root_base = Some(base);
        emitter.emit(I::GlobalGet(0));
        emitter.emit(I::LocalSet(base));
    }
    let return_slot = emitter.temp(
        value_type(&function.return_type, function.body.span)?,
        function.body.span,
    )?;
    emitter.return_slot = Some(return_slot);
    emitter.emit(I::Block(BlockType::Empty));
    emitter.exit_target = Some(emitter.control_depth);
    let mut pending = vec![&function.body];
    let mut has_defers = false;
    while let Some(expr) = pending.pop() {
        has_defers |= matches!(expr.kind, ExprKind::Defer(_));
        pending.extend(ir::children(expr));
    }
    if has_defers && strings.is_some() {
        let head = emitter.temp(ValType::I32, function.body.span)?;
        let root = emitter.temp(ValType::I32, function.body.span)?;
        emitter.defer_head = Some(head);
        emitter.defer_root = Some(root);
        emitter.emit(I::GlobalGet(0));
        emitter.emit(I::LocalSet(root));
        emitter.emit(I::I32Const(0));
        emitter.root_string(function.body.span)?;
        emitter.emit(I::Drop);
    }
    let mut locals = Locals::new();
    for (index, param) in function.captures.iter().chain(&function.params).enumerate() {
        if locals
            .insert(param.id.0, (index as u32, param.ty.clone()))
            .is_some()
        {
            return Err(invalid(function.body.span, "duplicate parameter identity"));
        }
        if managed(&param.ty) {
            emitter.emit(I::LocalGet(index as u32));
            emitter.root_string(function.body.span)?;
            emitter.emit(I::Drop);
        }
    }
    let actual = emitter.expr(&function.body, &mut locals, 0)?;
    expect(&actual, &function.return_type, function.body.span)?;
    if actual != Type::Never {
        emitter.emit(I::LocalSet(return_slot));
    }
    emitter.emit(I::End);
    emitter.exit_target = None;
    if has_defers && managed(&function.return_type) {
        // Reserve return rooting before running allocating callbacks. In normal code
        // expr already rooted the result; failure paths leave its initialized zero.
        emitter.emit(I::LocalGet(return_slot));
        emitter.root_string(function.body.span)?;
        emitter.emit(I::Drop);
    }
    emitter.cleanups(function.body.span)?;
    emitter.restore_roots();
    emitter.emit(I::LocalGet(return_slot));
    emitter.emit(I::End);
    let mut encoded = Function::new(emitter.locals.into_iter().map(|ty| (1, ty)));
    for instruction in emitter.instructions {
        encoded.instruction(&instruction);
    }
    Ok(encoded)
}

pub(super) struct Emitter<'a, 'b> {
    instructions: Vec<I<'static>>,
    locals: Vec<ValType>,
    param_count: u32,
    functions: &'a Functions<'a>,
    return_type: &'a Type,
    work: &'b mut usize,
    strings: Option<&'a strings::Runtime>,
    root_base: Option<u32>,
    control_depth: u32,
    loops: Vec<(u32, u32)>,
    exit_target: Option<u32>,
    return_slot: Option<u32>,
    defer_head: Option<u32>,
    defer_root: Option<u32>,
}

impl Emitter<'_, '_> {
    fn restore_roots(&mut self) {
        if let Some(base) = self.root_base {
            self.emit(I::LocalGet(base));
            self.emit(I::GlobalSet(0));
        }
    }

    fn root_string(&mut self, span: Span) -> Result<()> {
        let runtime = self
            .strings
            .ok_or_else(|| invalid(span, "missing string heap"))?;
        self.emit(I::Call(runtime.first + strings::PUSH));
        Ok(())
    }
    pub(super) fn emit(&mut self, instruction: I<'static>) {
        if matches!(instruction, I::Unreachable)
            && self.strings.is_some()
            && let Some(target) = self.exit_target
        {
            self.instructions.extend([
                I::I32Const(1),
                I::GlobalSet(2),
                I::Br(self.control_depth - target),
            ]);
            return;
        }
        let check_fault = matches!(instruction, I::Call(_))
            && self.strings.is_some()
            && self.exit_target.is_some();
        match instruction {
            I::Block(_) | I::Loop(_) | I::If(_) => self.control_depth += 1,
            I::End => self.control_depth = self.control_depth.saturating_sub(1),
            _ => {}
        }
        self.instructions.push(instruction);
        if check_fault {
            self.instructions.extend([
                I::GlobalGet(2),
                I::If(BlockType::Empty),
                I::Br(self.control_depth + 1 - self.exit_target.unwrap()),
                I::End,
            ]);
        }
    }

    pub(super) fn temp(&mut self, ty: ValType, span: Span) -> Result<u32> {
        if self.locals.len() + self.param_count as usize >= 65_536 {
            return Err(invalid(span, "generated local limit exceeded"));
        }
        let index = self.param_count + self.locals.len() as u32;
        self.locals.push(ty);
        Ok(index)
    }

    fn bind(&mut self, locals: &mut Locals, id: ir::LocalId, ty: &Type, span: Span) -> Result<u32> {
        if locals.contains_key(&id.0) {
            return Err(invalid(span, "duplicate local identity"));
        }
        let index = self.temp(value_type(ty, span)?, span)?;
        locals.insert(id.0, (index, ty.clone()));
        Ok(index)
    }

    fn expr(&mut self, expr: &Expr, locals: &mut Locals, depth: usize) -> Result<Type> {
        *self.work += 1;
        if *self.work > 100_000 || depth >= 128 {
            return Err(invalid(expr.span, "expression complexity limit exceeded"));
        }
        if expr.ty != Type::Never {
            value_type(&expr.ty, expr.span)?;
        }
        let depth = depth + 1;
        let actual = match &expr.kind {
            ExprKind::Int(n) => {
                self.emit(I::I64Const(*n));
                Type::Int
            }
            ExprKind::Float(n) => {
                self.emit(I::F64Const((*n).into()));
                Type::Float
            }
            ExprKind::Bool(value) => {
                self.emit(I::I32Const(i32::from(*value)));
                Type::Bool
            }
            ExprKind::Unit => {
                self.emit(I::I32Const(0));
                Type::Unit
            }
            ExprKind::String(value) => {
                let pointer = self
                    .strings
                    .and_then(|runtime| runtime.literals.get(value))
                    .ok_or_else(|| invalid(expr.span, "missing string literal allocation"))?;
                self.emit(I::I32Const(*pointer));
                Type::String
            }
            ExprKind::Interpolate(parts) => {
                let runtime = self
                    .strings
                    .ok_or_else(|| invalid(expr.span, "missing string heap"))?;
                let first = runtime.first;
                self.emit(I::I32Const(runtime.literals[""]));
                for part in parts {
                    let ty = self.expr(part, locals, depth)?;
                    match ty {
                        Type::String => {}
                        Type::Int => self.emit(I::Call(first + strings::INT_TEXT)),
                        Type::Bool => {
                            self.emit(I::If(BlockType::Result(ValType::I32)));
                            self.emit(I::I32Const(runtime.literals["true"]));
                            self.emit(I::Else);
                            self.emit(I::I32Const(runtime.literals["false"]));
                            self.emit(I::End);
                        }
                        Type::Unit => {
                            self.emit(I::Drop);
                            self.emit(I::I32Const(runtime.literals["()"]));
                        }
                        _ => {
                            return Err(invalid(
                                part.span,
                                "interpolation of this value type is unavailable",
                            ));
                        }
                    }
                    self.root_string(part.span)?;
                    self.emit(I::Call(first + strings::CONCAT));
                    self.root_string(part.span)?;
                }
                Type::String
            }
            ExprKind::Local(id) => {
                let (index, ty) = locals
                    .get(&id.0)
                    .ok_or_else(|| invalid(expr.span, "local is not in lexical scope"))?;
                self.emit(I::LocalGet(*index));
                ty.clone()
            }
            ExprKind::Closure { function, captures } => {
                self.closure(*function, captures, &expr.ty, locals, depth, expr.span)?
            }
            ExprKind::Invoke { callee, args } => {
                let actual = self.expr(callee, locals, depth)?;
                let slot = self.temp(ValType::I32, expr.span)?;
                self.emit(I::LocalSet(slot));
                let mut values = Vec::new();
                for arg in args {
                    let ty = self.expr(arg, locals, depth)?;
                    let value = self.temp(value_type(&ty, arg.span)?, arg.span)?;
                    self.emit(I::LocalSet(value));
                    values.push((value, ty));
                }
                self.invoke_values(slot, &actual, &values, expr.span)?
            }
            ExprKind::Range {
                start,
                end,
                inclusive,
            } => self.range(start, end, *inclusive, locals, depth, expr.span)?,
            ExprKind::For {
                pattern,
                iterable,
                body,
            } => self.iteration(pattern, iterable, body, locals, depth, expr.span)?,
            ExprKind::Break | ExprKind::Continue => {
                let (done, next) = *self
                    .loops
                    .last()
                    .ok_or_else(|| invalid(expr.span, "loop control outside loop"))?;
                let target = if matches!(expr.kind, ExprKind::Break) {
                    done
                } else {
                    next
                };
                self.emit(I::Br(self.control_depth - target));
                Type::Never
            }
            ExprKind::Try(value) => self.try_result(value, locals, depth, expr.span)?,
            ExprKind::With { .. } => self.with_result(expr, locals, depth)?,
            ExprKind::Defer(value) => self.defer(value, locals, depth, expr.span)?,
            ExprKind::Return(value) => {
                let actual = self.expr(value, locals, depth)?;
                expect(&actual, self.return_type, value.span)?;
                self.emit(I::LocalSet(self.return_slot.unwrap()));
                self.emit(I::Br(self.control_depth - self.exit_target.unwrap()));
                Type::Never
            }
            ExprKind::Unary { op, value } => {
                let ty = self.expr(value, locals, depth)?;
                match (op, &ty) {
                    (UnaryOp::Negate, Type::Int) => {
                        self.emit(I::I64Const(-1));
                        self.emit(I::I64Mul);
                    }
                    (UnaryOp::Negate, Type::Float) => self.emit(I::F64Neg),
                    (UnaryOp::Not, Type::Bool) => self.emit(I::I32Eqz),
                    (UnaryOp::BitNot, Type::Int) => {
                        self.emit(I::I64Const(-1));
                        self.emit(I::I64Xor);
                    }
                    (_, Type::Never) => {}
                    _ => return Err(invalid(expr.span, "invalid unary operand type")),
                }
                ty
            }
            ExprKind::Binary { op, left, right } => {
                self.binary(*op, left, right, locals, depth, expr.span)?
            }
            ExprKind::UnionInject { value } | ExprKind::UnionWiden { value } => {
                self.union_conversion(expr, value, locals, depth)?
            }
            ExprKind::Map(entries) => {
                self.map_literal(entries, &expr.ty, locals, depth, expr.span)?
            }
            ExprKind::ForeignCall { .. } => {
                return Err(invalid(expr.span, "foreign calls require a native runtime"));
            }
            ExprKind::Call {
                target: CallTarget::Builtin(builtin),
                args,
            } if matches!(
                builtin,
                ir::Builtin::MapNew
                    | ir::Builtin::MapGet
                    | ir::Builtin::MapPut
                    | ir::Builtin::MapDelete
                    | ir::Builtin::MapLen
                    | ir::Builtin::MapIsEmpty
                    | ir::Builtin::MapContains
                    | ir::Builtin::MapKeys
                    | ir::Builtin::MapValues
            ) =>
            {
                self.map_call(*builtin, args, &expr.ty, locals, depth, expr.span)?
            }
            ExprKind::Call {
                target: CallTarget::Builtin(builtin),
                args,
            } if matches!(
                builtin,
                ir::Builtin::ListMap
                    | ir::Builtin::ListFilter
                    | ir::Builtin::ListFold
                    | ir::Builtin::ListFind
                    | ir::Builtin::ListAny
                    | ir::Builtin::ListAll
                    | ir::Builtin::ListContains
                    | ir::Builtin::ListEnumerate
                    | ir::Builtin::OptionMap
                    | ir::Builtin::ResultMap
                    | ir::Builtin::ResultAndThen
                    | ir::Builtin::ResultUnwrapOrElse
            ) =>
            {
                self.higher_order(*builtin, args, &expr.ty, locals, depth, expr.span)?
            }
            ExprKind::Call { target, args } => {
                let CallTarget::Function(id) = target else {
                    let actual = self.string_call(*target, args, locals, depth, expr.span)?;
                    if actual != expr.ty {
                        return Err(invalid(expr.span, "string call result type mismatch"));
                    }
                    if managed(&actual) {
                        self.root_string(expr.span)?;
                    }
                    return Ok(actual);
                };
                let (index, function) = self
                    .functions
                    .get(&id.0)
                    .ok_or_else(|| invalid(expr.span, "unknown function identity"))?;
                if !function.captures.is_empty() {
                    return Err(invalid(
                        expr.span,
                        "captured function requires closure invocation",
                    ));
                }
                if args.len() != function.params.len() {
                    return Err(invalid(expr.span, "function argument count mismatch"));
                }
                let mut diverges = false;
                for (arg, param) in args.iter().zip(&function.params) {
                    let actual = self.expr(arg, locals, depth)?;
                    expect(&actual, &param.ty, arg.span)?;
                    diverges |= actual == Type::Never;
                }
                self.emit(I::Call(*index));
                if diverges {
                    Type::Never
                } else {
                    function.return_type.clone()
                }
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let condition_type = self.expr(condition, locals, depth)?;
                expect(&condition_type, &Type::Bool, condition.span)?;
                self.emit(I::If(block_type(&expr.ty, expr.span)?));
                let then_ty = self.expr(then_branch, &mut locals.clone(), depth)?;
                self.emit(I::Else);
                let else_ty = if let Some(branch) = else_branch {
                    self.expr(branch, &mut locals.clone(), depth)?
                } else {
                    self.emit(I::I32Const(0));
                    Type::Unit
                };
                self.emit(I::End);
                if condition_type == Type::Never {
                    Type::Never
                } else {
                    expect(&then_ty, &expr.ty, then_branch.span)?;
                    expect(&else_ty, &expr.ty, expr.span)?;
                    expr.ty.clone()
                }
            }
            ExprKind::Block(statements) => {
                self.block(statements, &mut locals.clone(), depth, &expr.ty)?
            }
            ExprKind::Match { value, arms } => {
                if arms.is_empty() || arms.len() > 1024 {
                    return Err(invalid(
                        expr.span,
                        "match arm count is empty or exceeds limit",
                    ));
                }
                let value_ty = self.expr(value, locals, depth)?;
                if value_ty == Type::Never {
                    return Err(invalid(
                        expr.span,
                        "match on a nonreturning value is not supported",
                    ));
                }
                let temp = self.temp(value_type(&value_ty, value.span)?, value.span)?;
                self.emit(I::LocalSet(temp));
                self.emit(I::Block(block_type(&expr.ty, expr.span)?));
                for arm in arms {
                    let mut scoped = locals.clone();
                    self.pattern(&arm.pattern, temp, &value_ty, &mut scoped, arm.span)?;
                    self.emit(I::If(BlockType::Result(ValType::I32)));
                    if let Some(guard) = &arm.guard {
                        let guard_type = self.expr(guard, &mut scoped, depth)?;
                        expect(&guard_type, &Type::Bool, guard.span)?;
                    } else {
                        self.emit(I::I32Const(1));
                    }
                    self.emit(I::Else);
                    self.emit(I::I32Const(0));
                    self.emit(I::End);
                    self.emit(I::If(BlockType::Empty));
                    let actual = self.expr(&arm.body, &mut scoped, depth)?;
                    expect(&actual, &expr.ty, arm.body.span)?;
                    self.emit(I::Br(1));
                    self.emit(I::End);
                }
                self.emit(I::Unreachable); // A malformed non-exhaustive IR cannot fall through.
                self.emit(I::End);
                expr.ty.clone()
            }
            ExprKind::Tuple(_)
            | ExprKind::List(_)
            | ExprKind::CustomConstruct { .. }
            | ExprKind::Construct { .. }
            | ExprKind::Field { .. }
            | ExprKind::Wrap(_)
            | ExprKind::Unwrap(_) => self.aggregate(expr, locals, depth)?,
            _ => return Err(invalid(expr.span, unsupported(&expr.kind))),
        };
        if actual != expr.ty {
            return Err(invalid(
                expr.span,
                format!(
                    "expression type mismatch: declared {:?}, produced {actual:?}",
                    expr.ty
                ),
            ));
        }
        if managed(&actual) {
            self.root_string(expr.span)?;
        }
        Ok(actual)
    }

    fn string_call(
        &mut self,
        target: CallTarget,
        args: &[Expr],
        locals: &mut Locals,
        depth: usize,
        span: Span,
    ) -> Result<Type> {
        if let CallTarget::Runtime(id) = target {
            match crate::runtime::signature(id).map(|signature| signature.symbol) {
                Some("fern_str_compare") => return self.compare_text(args, locals, depth, span),
                Some("fern_str_join") => return self.join_text(args, locals, depth, span),
                _ => {}
            }
        }
        use ir::Builtin;
        if let CallTarget::Builtin(builtin) = target
            && matches!(
                builtin,
                Builtin::ListLen
                    | Builtin::ListGet
                    | Builtin::ListHead
                    | Builtin::ListTail
                    | Builtin::ListIsEmpty
                    | Builtin::ListPush
                    | Builtin::ListConcat
                    | Builtin::ListReverse
                    | Builtin::OptionIsSome
                    | Builtin::OptionIsNone
                    | Builtin::OptionUnwrapOr
                    | Builtin::ResultIsOk
                    | Builtin::ResultIsErr
                    | Builtin::ResultUnwrapOr
            )
        {
            return self.collection_call(builtin, args, locals, depth, span);
        }
        let operation = match target {
            CallTarget::Builtin(Builtin::StringLen) => Some((strings::LEN, 1, Type::Int)),
            CallTarget::Builtin(Builtin::StringConcat) => Some((strings::CONCAT, 2, Type::String)),
            CallTarget::Builtin(Builtin::StringEq) => Some((strings::EQ, 2, Type::Bool)),
            CallTarget::Runtime(id) => {
                crate::runtime::signature(id).and_then(|signature| match signature.symbol {
                    "fern_str_len" => Some((strings::LEN, 1, Type::Int)),
                    "fern_str_concat" => Some((strings::CONCAT, 2, Type::String)),
                    "fern_str_eq" => Some((strings::EQ, 2, Type::Bool)),
                    "fern_str_quote" => Some((strings::QUOTE, 1, Type::String)),
                    _ => None,
                })
            }
            _ => None,
        };
        let (operation, arity, result) = operation.ok_or_else(|| {
            invalid(
                span,
                format!("host capability {target:?} is unavailable in the portable browser target"),
            )
        })?;
        let runtime = self
            .strings
            .ok_or_else(|| invalid(span, "missing string heap"))?;
        if args.len() != arity {
            return Err(invalid(span, "string call argument count mismatch"));
        }
        let mut diverges = false;
        for arg in args {
            let actual = self.expr(arg, locals, depth)?;
            expect(&actual, &Type::String, arg.span)?;
            diverges |= actual == Type::Never;
        }
        self.emit(I::Call(runtime.first + operation));
        Ok(if diverges { Type::Never } else { result })
    }

    fn block(
        &mut self,
        statements: &[Stmt],
        locals: &mut Locals,
        depth: usize,
        result: &Type,
    ) -> Result<Type> {
        let mut last = Type::Unit;
        let mut diverges = false;
        for (position, statement) in statements.iter().enumerate() {
            let is_last = position + 1 == statements.len();
            let span = match statement {
                Stmt::Let { value, .. } | Stmt::Expr(value) | Stmt::LetElse { value, .. } => {
                    value.span
                }
            };
            let root_mark = if self.strings.is_some() {
                let mark = self.temp(ValType::I32, span)?;
                self.emit(I::GlobalGet(0));
                self.emit(I::LocalSet(mark));
                Some(mark)
            } else {
                None
            };
            let mut retained_bindings = Vec::new();
            match statement {
                Stmt::Let { id, value } => {
                    let ty = self.expr(value, locals, depth)?;
                    if ty != Type::Never {
                        let index = self.bind(locals, *id, &ty, value.span)?;
                        self.emit(I::LocalSet(index));
                        if managed(&ty) {
                            retained_bindings.push(index);
                        }
                    }
                    diverges |= ty == Type::Never;
                    last = Type::Unit;
                }
                Stmt::Expr(value) => {
                    last = self.expr(value, locals, depth)?;
                    diverges |= last == Type::Never;
                    if !is_last && last != Type::Never {
                        self.emit(I::Drop);
                    }
                }
                Stmt::LetElse {
                    pattern,
                    value,
                    else_branch,
                } => {
                    let ty = self.expr(value, locals, depth)?;
                    let temp = self.temp(value_type(&ty, value.span)?, value.span)?;
                    self.emit(I::LocalSet(temp));
                    let previous = locals
                        .keys()
                        .copied()
                        .collect::<std::collections::BTreeSet<_>>();
                    self.pattern(pattern, temp, &ty, locals, value.span)?;
                    for (id, (slot, ty)) in locals.iter() {
                        if !previous.contains(id) && managed(ty) {
                            retained_bindings.push(*slot);
                        }
                    }
                    self.emit(I::I32Eqz);
                    self.emit(I::If(BlockType::Empty));
                    let actual = self.expr(else_branch, &mut locals.clone(), depth)?;
                    expect(&actual, &Type::Never, else_branch.span)?;
                    self.emit(I::End);
                    last = Type::Unit;
                }
            }
            if let Some(mark) = root_mark {
                // The statement's operands are dead. Keep only its escaping binding or
                // block result, with no allocating operation between reset and re-root.
                self.emit(I::LocalGet(mark));
                self.emit(I::GlobalSet(0));
                for binding in retained_bindings {
                    self.emit(I::LocalGet(binding));
                    self.root_string(span)?;
                    self.emit(I::Drop);
                }
                if is_last && managed(&last) {
                    self.root_string(span)?;
                }
            }
        }
        if statements.is_empty() || !matches!(statements.last(), Some(Stmt::Expr(_))) {
            self.emit(I::I32Const(0));
        }
        if diverges {
            self.emit(I::Unreachable);
            Ok(Type::Never)
        } else {
            expect(&last, result, Span::default())?;
            Ok(last)
        }
    }

    fn pattern(
        &mut self,
        pattern: &Pattern,
        value: u32,
        ty: &Type,
        locals: &mut Locals,
        span: Span,
    ) -> Result<()> {
        match pattern {
            Pattern::Wildcard => self.emit(I::I32Const(1)),
            Pattern::Bind(id) => {
                let binding = self.bind(locals, *id, ty, span)?;
                self.emit(I::LocalGet(value));
                self.emit(I::LocalSet(binding));
                self.emit(I::I32Const(1));
            }
            Pattern::Int(n) if *ty == Type::Int => {
                self.emit(I::LocalGet(value));
                self.emit(I::I64Const(*n));
                self.emit(I::I64Eq);
            }
            Pattern::Bool(value_pattern) if *ty == Type::Bool => {
                self.emit(I::LocalGet(value));
                self.emit(I::I32Const(i32::from(*value_pattern)));
                self.emit(I::I32Eq);
            }
            _ => return self.aggregate_pattern(pattern, value, ty, locals, span),
        }
        Ok(())
    }

    fn binary(
        &mut self,
        op: BinaryOp,
        left: &Expr,
        right: &Expr,
        locals: &mut Locals,
        depth: usize,
        span: Span,
    ) -> Result<Type> {
        let lhs = self.expr(left, locals, depth)?;
        if matches!(op, BinaryOp::And | BinaryOp::Or) {
            expect(&lhs, &Type::Bool, left.span)?;
            self.emit(I::If(BlockType::Result(ValType::I32)));
            let rhs;
            if op == BinaryOp::And {
                rhs = self.expr(right, &mut locals.clone(), depth)?;
                self.emit(I::Else);
                self.emit(I::I32Const(0));
            } else {
                self.emit(I::I32Const(1));
                self.emit(I::Else);
                rhs = self.expr(right, &mut locals.clone(), depth)?;
            }
            self.emit(I::End);
            expect(&rhs, &Type::Bool, right.span)?;
            return Ok(if lhs == Type::Never {
                Type::Never
            } else {
                Type::Bool
            });
        }
        let rhs = self.expr(right, locals, depth)?;
        let operand = if lhs == Type::Never { &rhs } else { &lhs };
        expect(&rhs, operand, right.span)?;
        if lhs == Type::Never || rhs == Type::Never {
            self.emit(I::Unreachable);
            return Ok(Type::Never);
        }
        if *operand == Type::String {
            let runtime = self
                .strings
                .ok_or_else(|| invalid(span, "missing string heap"))?;
            match op {
                BinaryOp::Add => {
                    self.emit(I::Call(runtime.first + strings::CONCAT));
                    Ok(Type::String)
                }
                BinaryOp::Eq | BinaryOp::Ne => {
                    self.emit(I::Call(runtime.first + strings::EQ));
                    if op == BinaryOp::Ne {
                        self.emit(I::I32Eqz);
                    }
                    Ok(Type::Bool)
                }
                _ => Err(invalid(span, "unsupported string operator")),
            }
        } else {
            self.numeric(op, operand, span)
        }
    }
}

fn block_type(ty: &Type, span: Span) -> Result<BlockType> {
    if *ty == Type::Never {
        Ok(BlockType::Empty)
    } else {
        Ok(BlockType::Result(value_type(ty, span)?))
    }
}

fn unsupported(kind: &ExprKind) -> &'static str {
    match kind {
        ExprKind::Actor(_) => "actors are a native server capability",
        ExprKind::FunctionValue { .. } | ExprKind::Lambda { .. } => {
            "unlifted callable cannot enter executable IR"
        }
        _ => "operation requires an unsupported browser representation or host capability",
    }
}
