//! Shared native lowering uses checked types and resolved symbol identities.
//! QBE serialization remains an adapter over the same structured machine program.
use crate::machine::{
    self, BinaryOp as MachineBinary, Buffer, Comparison, DataValue, LoadKind, Operand,
    Operation as NativeOperation, Scalar, Statement, UnaryOp as MachineUnary,
};
use crate::{
    Constructor, Diagnostic, Span, Type,
    ast::{BinaryOp, UnaryOp},
    ir::{self, Builtin, CallTarget, Expr, ExprKind, Function, MatchArm, Pattern, Stmt},
};
#[path = "lowering/helpers.rs"]
mod helpers;
use helpers::{machine_width, native_operand};
use std::collections::{BTreeMap, BTreeSet, HashMap};
#[path = "lowering/boundaries.rs"]
mod boundaries;
#[path = "lowering/callback_inline.rs"]
mod callback_inline;
#[path = "lowering/closures.rs"]
mod closures;
#[path = "lowering/control.rs"]
mod control;
#[path = "lowering/fault.rs"]
mod fault;
#[path = "lowering/foreign.rs"]
mod foreign;
#[path = "lowering/higher_order.rs"]
mod higher_order;
#[path = "lowering/iteration.rs"]
mod iteration;
#[path = "lowering/json_codecs.rs"]
mod json_codecs;
#[path = "lowering/maps.rs"]
mod maps;
#[path = "lowering/newtypes.rs"]
mod newtypes;
#[path = "lowering/nominal.rs"]
mod nominal;
#[path = "lowering/numeric.rs"]
mod numeric;
#[path = "lowering/roots.rs"]
mod roots;
#[path = "lowering/runtime_calls.rs"]
mod runtime_calls;
#[path = "lowering/tail.rs"]
mod tail;
#[path = "lowering/test_entry.rs"]
mod test_entry;
#[path = "lowering/union_validation.rs"]
mod union_validation;
#[path = "lowering/unions.rs"]
mod unions;
#[path = "lowering/with.rs"]
mod with;

#[path = "lowering/actors.rs"]
mod actor_backend;

const MAX_DEPTH: usize = 128;
const MAX_NODES: usize = 200_000;

/// Lower `program` to native-backend IL, rejecting inconsistent public IR.
/// No source-name or AST type inference occurs here. Requires exactly one main.
pub fn emit(program: &ir::Program) -> Result<String, Diagnostic> {
    lower(program).map(|program| program.to_text())
}

/// Lower a checked test entry, failing explicit process-exit calls instead of accepting early success.
/// All ordinary typed-IR validation applies; the caller selects the test's entry beforehand.
pub fn emit_test(program: &ir::Program) -> Result<String, Diagnostic> {
    lower_test(program).map(|program| program.to_text())
}

/// Lower checked semantic IR once for both native backends, preserving all runtime boundaries.
pub fn lower(program: &ir::Program) -> Result<machine::Program, Diagnostic> {
    emit_mode(program, false, false)
}

/// Lower a test entry with explicit process-exit rejection for either native backend.
pub fn lower_test(program: &ir::Program) -> Result<machine::Program, Diagnostic> {
    emit_mode(program, true, false)
}

/// Lower checked library functions without synthesizing a process entry point.
/// Exported host adapters are added separately by `native_library`.
pub fn lower_library(program: &ir::Program) -> Result<machine::Program, Diagnostic> {
    emit_mode(program, false, true)
}

/// Share executable validation while keeping test-only process behavior out of ordinary programs.
fn emit_mode(
    program: &ir::Program,
    test_mode: bool,
    library: bool,
) -> Result<machine::Program, Diagnostic> {
    ir::reject_probes(program)?;
    crate::json_codec::validate_program(program)?;
    let lowered = actor_backend::prepare(program);
    let result = lowered.and_then(|prepared| {
        emit_inner(
            &prepared.program,
            test_mode,
            prepared.plan,
            prepared.layouts,
            library,
        )
    });
    result.map_err(|exit| match exit {
        Exit::Diagnostic(error) => error,
        Exit::Terminated => Diagnostic::new(
            Span::default(),
            "invalid typed IR: unhandled control termination",
        ),
    })
}

/// Termination travels separately from diagnostics and never creates a usable operand.
#[derive(Debug)]
enum Exit {
    Diagnostic(Diagnostic),
    Terminated,
}
type Lowering<T> = Result<T, Exit>;
impl From<Diagnostic> for Exit {
    fn from(error: Diagnostic) -> Self {
        Self::Diagnostic(error)
    }
}

/// Validate signatures and lower complete function bodies through their exit handlers.
fn emit_inner(
    program: &ir::Program,
    test_mode: bool,
    actors: actor_backend::Plan,
    layouts: HashMap<Type, &ir::TypeLayout>,
    library: bool,
) -> Lowering<machine::Program> {
    let mut functions = BTreeMap::new();
    let mut main = None;
    for function in &program.functions {
        if function.params.len() > MAX_NODES
            || function.captures.len() > MAX_NODES.saturating_sub(function.params.len())
        {
            return Err(invalid(
                function.body.span,
                "function signature limit exceeded",
            ));
        }
        nominal::resolved(&function.return_type, &layouts, function.body.span, 0)?;
        for param in function.params.iter().chain(&function.captures) {
            nominal::resolved(&param.ty, &layouts, function.body.span, 0)?;
        }
        if functions.insert(function.id.0, function).is_some() {
            return Err(invalid(function.body.span, "duplicate function identity"));
        }
        if function.name == "main" {
            if main.replace(function).is_some() {
                return Err(invalid(function.body.span, "duplicate main function"));
            }
            if !function.captures.is_empty()
                || !function.params.is_empty()
                || !boundaries::main_type(&function.return_type)
            {
                return Err(invalid(
                    function.body.span,
                    "main must take no arguments and return Int, Unit, or Result(Unit, E)",
                ));
            }
        }
    }
    if !library && main.is_none() {
        return Err(invalid(Span::default(), "missing main function"));
    }
    let mut emitter = new_emitter(functions, layouts, actors, test_mode);
    emitter.actor_descriptors()?;
    if library {
        emitter.actor_library()?;
    }
    for function in &program.functions {
        if !emitter.actors.entries.contains_key(&function.id.0) {
            emitter.function(function)?;
        }
    }
    emitter.support_helpers(if library { None } else { main });
    emitter.data.extend(&emitter.output);
    emitter
        .data
        .finish()
        .map_err(|message| invalid(Span::default(), &message))
}

/// Initialize emission state only after source signatures and nominal layouts are validated.
fn new_emitter<'a>(
    functions: BTreeMap<usize, &'a Function>,
    layouts: HashMap<Type, &'a ir::TypeLayout>,
    actors: actor_backend::Plan,
    test_mode: bool,
) -> Emitter<'a> {
    Emitter {
        actors,
        test_mode,
        codec_tables: HashMap::new(),
        codec_data_bytes: 0,
        functions,
        layouts,
        output: Buffer::new(),
        data: Buffer::new(),
        strings: 0,
        nodes: 0,
        maps_used: false,
        enumerate_used: false,
        numeric_used: false,
        float_contains_used: false,
        list_access_used: false,
        repeat_used: false,
        slice_used: false,
        pattern_tail_used: false,
    }
}

/// Build an IR-boundary diagnostic at `span`; `message` describes the invariant.
fn invalid(span: Span, message: &str) -> Exit {
    Exit::Diagnostic(Diagnostic::new(
        span,
        format!("invalid typed IR: {message}"),
    ))
}

/// Map semantic `ty` to QBE scalar width; pointers and integers remain distinct in IR.
fn scalar_width(ty: Type) -> char {
    match ty {
        Type::Range
        | Type::Int
        | Type::String
        | Type::Map(_, _)
        | Type::List(_)
        | Type::Option(_)
        | Type::Result(_, _)
        | Type::Tuple(_)
        | Type::Union(_)
        | Type::Native(_)
        | Type::Named(_, _)
        | Type::Pid(_)
        | Type::ChildKey(_)
        | Type::RootFunction(_)
        | Type::ActorFunction(_, _)
        | Type::Function(_, _) => 'l',
        Type::Bool | Type::Unit => 'w',
        Type::Float => 'd',
        Type::Never | Type::Infer(_) | Type::Generic(_) => {
            unreachable!("concrete types validated at IR boundary")
        }
    }
}

/// Check `actual` against `expected`, retaining the malformed node's source span.
fn expect_type(actual: Type, expected: Type, span: Span) -> Lowering<()> {
    if actual != Type::Never && actual != expected {
        return Err(invalid(
            span,
            &format!("expected {expected:?}, found {actual:?}"),
        ));
    }
    Ok(())
}

struct Emitter<'a> {
    actors: actor_backend::Plan,
    codec_tables: HashMap<usize, String>,
    codec_data_bytes: usize,
    test_mode: bool,
    functions: BTreeMap<usize, &'a Function>,
    layouts: HashMap<Type, &'a ir::TypeLayout>,
    output: Buffer,
    data: Buffer,
    strings: usize,
    nodes: usize,
    maps_used: bool,
    enumerate_used: bool,
    numeric_used: bool,
    float_contains_used: bool,
    list_access_used: bool,
    repeat_used: bool,
    slice_used: bool,
    pattern_tail_used: bool,
}

struct Locals {
    roots: roots::Frame,
    tail: Option<tail::TailLoop>,
    stack_allocations: Buffer,
    values: BTreeMap<usize, (Type, String)>,
    defined: BTreeSet<usize>,
    count: usize,
    return_type: Type,
    loops: Vec<iteration::LoopTargets>,
    temporary: usize,
    label: usize,
    current: String,
}

impl Locals {
    /// Allocate a unique SSA temporary within this function.
    fn temporary(&mut self) -> String {
        let name = format!("%t{}", self.temporary);
        self.temporary += 1;
        name
    }

    /// Reserve a unique block label within this function.
    fn label(&mut self) -> String {
        let name = format!("@b{}", self.label);
        self.label += 1;
        name
    }

    /// Define `id` once within its owning function, with checked type and SSA value.
    fn define(&mut self, id: usize, ty: Type, value: String, span: Span) -> Lowering<()> {
        if id >= self.count || !self.defined.insert(id) {
            return Err(invalid(
                span,
                "local identity is out of bounds or defined twice",
            ));
        }
        self.values.insert(id, (ty, value));
        Ok(())
    }
}

impl Emitter<'_> {
    /// Append compiler-owned helper definitions once, including only used numeric adapters.
    fn support_helpers(&mut self, main: Option<&Function>) {
        self.test_exit_helper();
        if self.output.calls("morrow_rs_json_object") || self.output.calls("morrow_rs_json_members")
        {
            helpers::json(&mut self.output);
        }
        helpers::control(&mut self.output);
        helpers::fault(&mut self.output);
        if self.actors.active {
            helpers::actor_fault_tail(&mut self.output);
        } else {
            helpers::fault_tail(&mut self.output);
        }
        if self.numeric_used {
            helpers::numeric(&mut self.output);
        }
        if self.float_contains_used {
            helpers::float_contains(&mut self.output);
        }
        if self.enumerate_used {
            helpers::iteration(&mut self.output);
        }
        if self.list_access_used {
            helpers::list_access(&mut self.output);
        }
        if self.repeat_used {
            helpers::repeat(&mut self.output);
        }
        if self.slice_used {
            helpers::slice(&mut self.output);
        }
        if let Some(main) = main {
            self.main_wrapper(main);
        }
        if self.pattern_tail_used {
            helpers::pattern_tail(&mut self.output);
        }
        if self.maps_used {
            helpers::maps(&mut self.output);
        }
    }

    /// Emit `function` using only its resolved signature and typed body.
    fn function(&mut self, function: &Function) -> Lowering<()> {
        let mut locals = Locals {
            roots: roots::Frame::default(),
            tail: None,
            stack_allocations: Buffer::new(),
            values: BTreeMap::new(),
            defined: BTreeSet::new(),
            count: function.local_count,
            return_type: function.return_type.clone(),
            loops: vec![],
            temporary: 0,
            label: 0,
            current: "@start".into(),
        };
        let mut params = vec![
            (Scalar::I64, "%env".to_owned()),
            (Scalar::I64, "%fault".to_owned()),
        ];
        if self.actors.managed.contains(&function.id.0) {
            params.push((Scalar::I64, "%exec".into()));
        }
        for param in &function.params {
            let value = format!("%v{}", param.id.0);
            locals.define(
                param.id.0,
                param.ty.clone(),
                value.clone(),
                function.body.span,
            )?;
            params.push((machine_width(self.width(param.ty.clone())), value));
        }
        self.output.begin(
            &format!("$f{}", function.id.0),
            Some(machine_width(self.width(function.return_type.clone()))),
            params.clone(),
            false,
        );
        self.output.statement(Statement::Label("@start".to_owned()));
        let entry = self.output.len();
        for param in &function.params {
            self.root_value(&mut locals, &param.ty, &format!("%v{}", param.id.0));
        }
        if !function.captures.is_empty() {
            self.root_pointer(&mut locals, "%env");
        }
        self.load_captures(function, &mut locals)?;
        self.output.statement(Statement::Assign {
            destination: "%return_slot".to_owned(),
            ty: Scalar::I64,
            operation: NativeOperation::StackAlloc { bytes: 8, align: 8 },
        });
        self.output.statement(Statement::Assign {
            destination: "%defer_head".to_owned(),
            ty: Scalar::I64,
            operation: NativeOperation::StackAlloc { bytes: 8, align: 8 },
        });
        self.output.statement(Statement::Store {
            kind: LoadKind::I64,
            value: native_operand("0"),
            address: native_operand("%defer_head"),
        });
        if function.body.ty != Type::Never
            && (function.return_type != Type::Unit || function.name != "main")
        {
            expect_type(
                function.body.ty.clone(),
                function.return_type.clone(),
                function.body.span,
            )?;
        }
        self.start_tail(function, &mut locals)?;
        match self.tail_expr(&function.body, &mut locals, 0) {
            Ok(value) => self.save_return(&value, &mut locals),
            Err(Exit::Terminated) => {}
            Err(error) => return Err(error),
        }
        self.finish_tail(&mut locals, function.body.span)?;
        self.finish_function(&mut locals);
        self.root_entry(&mut locals, function.body.span)?;
        self.output.insert(entry, &locals.stack_allocations);
        self.root_phis(entry.saturating_sub(1));
        Ok(())
    }

    /// Bridge the C runtime's 32-bit entry ABI to the resolved Morrow main signature.
    fn main_wrapper(&mut self, main: &Function) {
        if self.actors.active {
            self.actor_main(main);
            return;
        }
        self.output
            .begin("$morrow_main", Some(Scalar::I32), vec![], true);
        self.output.statement(Statement::Label("@start".to_owned()));
        self.output.statement(Statement::Assign {
            destination: "%fault".to_owned(),
            ty: Scalar::I64,
            operation: NativeOperation::StackAlloc { bytes: 8, align: 8 },
        });
        self.output.statement(Statement::Store {
            kind: LoadKind::I64,
            value: native_operand("0"),
            address: native_operand("%fault"),
        });
        self.output.statement(Statement::Assign {
            destination: "%exit".to_owned(),
            ty: machine_width(self.width(main.return_type.clone())),
            operation: NativeOperation::Call {
                callee: native_operand(&format!("$f{}", main.id.0)),
                args: vec![
                    (Scalar::I64, native_operand("0")),
                    (Scalar::I64, native_operand("%fault")),
                ],
                variadic: None,
            },
        });
        self.output.statement(Statement::Assign {
            destination: "%code".to_owned(),
            ty: Scalar::I64,
            operation: NativeOperation::Load(LoadKind::I64, native_operand("%fault")),
        });
        self.output.statement(Statement::Assign {
            destination: "%failed".to_owned(),
            ty: Scalar::I32,
            operation: NativeOperation::Binary(
                MachineBinary::Compare(Comparison::Ne, Scalar::I64),
                native_operand("%code"),
                native_operand("0"),
            ),
        });
        self.output.statement(Statement::Branch {
            condition: native_operand("%failed"),
            then_label: "@failed".to_owned(),
            else_label: "@success".to_owned(),
        });
        self.output
            .statement(Statement::Label("@failed".to_owned()));
        self.output
            .statement(Statement::Effect(NativeOperation::Call {
                callee: native_operand("$morrow_rs_report_fault"),
                args: vec![(Scalar::I64, native_operand("%code"))],
                variadic: None,
            }));
        self.output
            .statement(Statement::Return(Some(native_operand("1"))));
        self.output
            .statement(Statement::Label("@success".to_owned()));
        if main.return_type.clone() == Type::Int {
            self.output.statement(Statement::Assign {
                destination: "%status".to_owned(),
                ty: Scalar::I32,
                operation: NativeOperation::Unary(MachineUnary::Copy, native_operand("%exit")),
            });
            self.output
                .statement(Statement::Return(Some(native_operand("%status"))));
            self.output.end();
        } else if matches!(main.return_type, Type::Result(_, _)) {
            self.result_main_exit();
        } else {
            self.output
                .statement(Statement::Return(Some(native_operand("0"))));
            self.output.end();
        }
    }

    /// Emit `expr` within lexical `locals`, with bounded recursive descent.
    fn expr(&mut self, expr: &Expr, locals: &mut Locals, depth: usize) -> Lowering<String> {
        self.validate_expr(expr, depth)?;
        self.strict_termination(expr, locals, depth + 1)?;
        let (actual, value) = match &expr.kind {
            ExprKind::ForeignCall { declaration, args } => {
                self.foreign_call(declaration, args, &expr.ty, expr.span, locals, depth + 1)?
            }
            ExprKind::Actor(actor) => self.actor_expression(actor, expr.span, locals, depth + 1)?,
            ExprKind::JsonCodec { .. } => self.json_codec(expr, locals, depth + 1)?,
            ExprKind::With { .. }
            | ExprKind::For { .. }
            | ExprKind::Range { .. }
            | ExprKind::Break
            | ExprKind::Continue
            | ExprKind::Return(_)
            | ExprKind::Defer(_)
            | ExprKind::Match { .. }
            | ExprKind::If { .. } => self.flow_expr(expr, locals, depth + 1)?,
            ExprKind::JsonCodecTemplate { .. }
            | ExprKind::EditorHole { .. }
            | ExprKind::Probe { .. } => {
                return Err(private_expression(expr));
            }
            ExprKind::Lambda { .. } | ExprKind::FunctionValue { .. } => {
                return Err(invalid(expr.span, "unlifted callable expression"));
            }
            ExprKind::Closure { function, captures } => {
                self.closure(*function, captures, expr.span, locals, depth + 1)?
            }
            ExprKind::Invoke { callee, args } => {
                self.invoke(callee, args, expr.span, locals, depth + 1)?
            }
            ExprKind::UnionInject { .. }
            | ExprKind::UnionWiden { .. }
            | ExprKind::Wrap(_)
            | ExprKind::Unwrap(_) => self.conversion(expr, locals, depth + 1)?,
            ExprKind::CustomConstruct { tag, fields } => {
                self.custom_construct(*tag, fields, &expr.ty, expr.span, locals, depth + 1)?
            }
            ExprKind::Field { value, index } => {
                self.record_field(value, *index, locals, depth + 1)?
            }
            ExprKind::Try(value) => self.attempt(value, locals, depth + 1)?,
            ExprKind::Interpolate(parts) => self.interpolate(parts, locals, depth + 1)?,
            ExprKind::Unit => (Type::Unit, "0".into()),
            ExprKind::Tuple(items) => self.tuple(items, &expr.ty, expr.span, locals, depth + 1)?,
            ExprKind::Map(entries) => {
                self.map_literal(entries, &expr.ty, expr.span, locals, depth + 1)?
            }
            ExprKind::List(items) => self.list(items, &expr.ty, expr.span, locals, depth + 1)?,
            ExprKind::Construct { constructor, value } => {
                self.construct_expr(expr, *constructor, value.as_deref(), locals, depth + 1)?
            }
            ExprKind::Int(value) => (Type::Int, value.to_string()),
            ExprKind::Float(value) => self.float_literal(*value, locals),
            ExprKind::Bool(value) => (Type::Bool, u8::from(*value).to_string()),
            ExprKind::String(value) => (Type::String, self.string(value, expr.span)?),
            ExprKind::Local(id) => Self::local(*id, expr.span, locals)?,
            ExprKind::Unary { op, value } => self.unary(*op, value, locals, depth + 1)?,
            ExprKind::Binary { op, left, right } => {
                self.binary(*op, left, right, locals, depth + 1)?
            }
            ExprKind::Call { target, args } => {
                self.call(*target, args, &expr.ty, expr.span, locals, depth + 1)?
            }
            ExprKind::Block(stmts) => self.block(stmts, locals, depth + 1, false)?,
        };
        expect_type(actual, expr.ty.clone(), expr.span)?;
        self.root_value(locals, &expr.ty, &value);
        Ok(value)
    }

    /// Lower a built-in sum node using its annotated type and optional child payload.
    fn construct_expr(
        &mut self,
        expr: &Expr,
        constructor: Constructor,
        value: Option<&Expr>,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        self.construct(constructor, value, &expr.ty, expr.span, locals, depth)
    }

    /// Lower structured control independently from scalar and aggregate value dispatch.
    fn flow_expr(
        &mut self,
        expr: &Expr,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        match &expr.kind {
            ExprKind::With {
                steps,
                body,
                handlers,
            } => self.with(steps, body, handlers, locals, depth, false),
            ExprKind::For {
                pattern,
                iterable,
                body,
            } => self.iteration(pattern, iterable, body, locals, depth),
            ExprKind::Range {
                start,
                end,
                inclusive,
            } => self.range(start, end, *inclusive, locals, depth),
            ExprKind::Break => self.loop_exit(false, expr.span, locals),
            ExprKind::Continue => self.loop_exit(true, expr.span, locals),
            ExprKind::Return(value) => self.returned(value, locals, depth),
            ExprKind::Defer(value) => self.defer(value, locals, depth),
            ExprKind::Match { value, arms } => self.matching(value, arms, locals, depth, false),
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => self.conditional(
                condition,
                then_branch,
                else_branch.as_deref(),
                locals,
                depth,
                false,
            ),
            _ => Err(invalid(expr.span, "expected control expression")),
        }
    }

    /// Validate a node's concrete type and bound recursive lowering before emitting it.
    fn validate_expr(&mut self, expr: &Expr, depth: usize) -> Lowering<()> {
        if matches!(
            expr.kind,
            ExprKind::Return(_) | ExprKind::Break | ExprKind::Continue
        ) && expr.ty != Type::Never
        {
            return Err(invalid(
                expr.span,
                "control exit expression requires Never type",
            ));
        }
        if expr.ty != Type::Never {
            nominal::resolved(&expr.ty, &self.layouts, expr.span, 0)?;
        }
        self.nodes += 1;
        if depth > MAX_DEPTH || self.nodes > MAX_NODES {
            return Err(invalid(expr.span, "lowering complexity limit exceeded"));
        }
        Ok(())
    }

    /// Resolve a lexical identity without reconstructing source names or types.
    fn local(id: ir::LocalId, span: Span, locals: &Locals) -> Lowering<(Type, String)> {
        locals
            .values
            .get(&id.0)
            .cloned()
            .ok_or_else(|| invalid(span, "unknown or out-of-scope local identity"))
    }

    /// Validate structural tuple shape before allocating its tag and typed fields.
    fn tuple(
        &mut self,
        items: &[Expr],
        ty: &Type,
        span: Span,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        if !matches!(ty, Type::Tuple(fields) if !fields.is_empty()) {
            return Err(invalid(
                span,
                "tuple expression requires a nonempty structural tuple type",
            ));
        }
        self.custom_construct(0, items, ty, span, locals, depth)
    }

    /// Preserve every literal Float bit without a locale-dependent decimal conversion.
    fn float_literal(&mut self, value: f64, locals: &mut Locals) -> (Type, String) {
        (
            Type::Float,
            self.assign(
                locals,
                Type::Float,
                NativeOperation::Unary(
                    MachineUnary::Cast,
                    native_operand(&(value.to_bits() as i64).to_string()),
                ),
            ),
        )
    }

    /// Emit UTF-8 `value` as bounded ASCII runs and exact numeric bytes.
    fn string(&mut self, value: &str, span: Span) -> Lowering<String> {
        if value.as_bytes().contains(&0) {
            return Err(invalid(
                span,
                "embedded NUL is unsupported by the C string runtime",
            ));
        }
        let name = format!("$str{}", self.strings);
        self.strings += 1;
        let mut bytes = value.as_bytes().to_vec();
        bytes.push(0);
        self.data.data(&name, vec![DataValue::Bytes(bytes)]);
        Ok(name)
    }

    /// Emit a typed unary operation on `value`, returning its semantic result type.
    fn unary(
        &mut self,
        op: UnaryOp,
        value: &Expr,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        let expected = match op {
            UnaryOp::Negate if value.ty == Type::Float => Type::Float,
            UnaryOp::Negate | UnaryOp::BitNot => Type::Int,
            UnaryOp::Not => Type::Bool,
        };
        expect_type(value.ty.clone(), expected.clone(), value.span)?;
        let value = self.expr(value, locals, depth)?;
        let instruction = match op {
            UnaryOp::Negate if expected == Type::Float => {
                NativeOperation::Unary(MachineUnary::Neg, native_operand(&(value)))
            }
            UnaryOp::Negate => NativeOperation::Binary(
                MachineBinary::Sub,
                native_operand("0"),
                native_operand(&(value)),
            ),
            UnaryOp::Not => NativeOperation::Binary(
                MachineBinary::Compare(Comparison::Eq, Scalar::I32),
                native_operand(&(value)),
                native_operand("0"),
            ),
            UnaryOp::BitNot => NativeOperation::Binary(
                MachineBinary::Xor,
                native_operand(&(value)),
                native_operand("-1"),
            ),
        };
        Ok((expected.clone(), self.assign(locals, expected, instruction)))
    }

    /// Emit a scalar instruction `instruction`, assigning a fresh SSA value of `ty`.
    fn assign(&mut self, locals: &mut Locals, ty: Type, instruction: NativeOperation) -> String {
        let result = locals.temporary();
        let allocation = matches!(&instruction, NativeOperation::Call { callee: Operand::Symbol(name), .. } if name == "morrow_alloc");
        self.output.statement(Statement::Assign {
            destination: result.clone(),
            ty: machine_width(self.width(ty.clone())),
            operation: instruction,
        });
        if allocation {
            self.root_pointer(locals, &result);
        } else {
            self.root_value(locals, &ty, &result);
        }
        result
    }

    /// Emit the block `label` and record its actual predecessor identity.
    fn start_block(&mut self, locals: &mut Locals, label: &str) {
        self.output.statement(Statement::Label((label).to_string()));
        locals.current = label.to_owned();
    }

    /// Emit a final-value block and remove its local bindings at lexical scope exit.
    fn block(
        &mut self,
        stmts: &[Stmt],
        locals: &mut Locals,
        depth: usize,
        tail: bool,
    ) -> Lowering<(Type, String)> {
        let outer = locals.values.clone();
        let result = self.block_statements(stmts, locals, depth, tail);
        locals.values = outer;
        result
    }

    /// Evaluate a block's statements until normal completion or a propagated exit.
    fn block_statements(
        &mut self,
        stmts: &[Stmt],
        locals: &mut Locals,
        depth: usize,
        tail: bool,
    ) -> Lowering<(Type, String)> {
        let mut result = (Type::Unit, "0".into());
        for (index, stmt) in stmts.iter().enumerate() {
            match stmt {
                Stmt::Let { id, value } => {
                    let lowered = self.expr(value, locals, depth)?;
                    locals.define(id.0, value.ty.clone(), lowered, value.span)?;
                    result = (Type::Unit, "0".into());
                }
                Stmt::LetElse {
                    pattern,
                    value,
                    else_branch,
                } => {
                    self.let_else(pattern, value, else_branch, locals, depth)?;
                    result = (Type::Unit, "0".into());
                }
                Stmt::Expr(value) => {
                    result = (
                        value.ty.clone(),
                        self.position_expr(value, locals, depth, tail && index + 1 == stmts.len())?,
                    )
                }
            }
        }
        Ok(result)
    }

    /// Emit a resolved call after checking the signature and each argument's type.
    fn call(
        &mut self,
        target: CallTarget,
        args: &[Expr],
        result: &Type,
        span: Span,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        if let Some(outcome) = self.special_call(target, args, result, span, locals, depth) {
            return outcome;
        }
        let (symbol, params, result) = self.signature(target, args, span)?;
        if args.len() != params.len() {
            return Err(invalid(
                span,
                "call argument count differs from resolved signature",
            ));
        }
        let mut arguments = if matches!(target, CallTarget::Function(_)) {
            vec![
                (Scalar::I64, Operand::Int(0)),
                (Scalar::I64, native_operand("%fault")),
            ]
        } else {
            Vec::new()
        };
        if let CallTarget::Function(id) = target
            && self.actors.managed.contains(&id.0)
        {
            arguments.push((Scalar::I64, native_operand("%exec")));
        }
        for (arg, expected) in args.iter().zip(params) {
            expect_type(arg.ty.clone(), expected.clone(), arg.span)?;
            let mut value = self.expr(arg, locals, depth)?;
            let mut abi = self.width(expected.clone());
            if matches!(
                target,
                CallTarget::Builtin(Builtin::Print | Builtin::Println)
            ) && expected == Type::Bool
            {
                value = self.assign(
                    locals,
                    Type::Int,
                    NativeOperation::Unary(
                        MachineUnary::ExtUw,
                        native_operand(&(value).to_string()),
                    ),
                );
                abi = 'l';
            }
            arguments.push((machine_width(abi), native_operand(&value)));
        }
        let instruction = NativeOperation::Call {
            callee: native_operand(&format!("${}", symbol)),
            args: arguments.clone(),
            variadic: None,
        };
        let value = if result == Type::Unit {
            self.output.statement(Statement::Effect(instruction));
            "0".into()
        } else {
            self.assign(locals, result.clone(), instruction)
        };
        if matches!(target, CallTarget::Function(_)) {
            self.guard_fault(locals);
        }
        Ok((result, value))
    }

    /// Dispatch calls whose checked payloads require a compiler-owned adapter.
    fn special_call(
        &mut self,
        target: CallTarget,
        args: &[Expr],
        result: &Type,
        span: Span,
        locals: &mut Locals,
        depth: usize,
    ) -> Option<Lowering<(Type, String)>> {
        match target {
            CallTarget::Runtime(id) => Some(self.runtime_call(id, args, span, locals, depth)),
            CallTarget::Builtin(Builtin::ListGet | Builtin::ListHead) => Some(self.list_access(
                args,
                target == CallTarget::Builtin(Builtin::ListHead),
                span,
                locals,
                depth,
            )),
            CallTarget::Builtin(Builtin::ListContains) => {
                Some(self.scalar_contains(args, span, locals, depth))
            }
            CallTarget::Builtin(Builtin::ListEnumerate) => {
                Some(self.enumerate(args, span, locals, depth))
            }
            CallTarget::Builtin(builtin) if maps::is_map(builtin) => {
                Some(self.map_call(builtin, args, result, span, locals, depth))
            }
            CallTarget::Builtin(builtin) if higher_order::is_higher_order(builtin) => {
                Some(self.higher_order(builtin, args, span, locals, depth))
            }
            _ if compound_builtin(target) => {
                Some(self.compound_call(target, args, span, locals, depth))
            }
            _ => None,
        }
    }

    /// Resolve backend symbols from `target` IDs or builtin identities, never source text.
    fn signature(
        &self,
        target: CallTarget,
        args: &[Expr],
        span: Span,
    ) -> Lowering<(String, Vec<Type>, Type)> {
        let builtin = match target {
            CallTarget::Runtime(_) => {
                return Err(invalid(span, "runtime call requires registry lowering"));
            }
            CallTarget::Function(id) => {
                let function = self
                    .functions
                    .get(&id.0)
                    .ok_or_else(|| invalid(span, "unknown function identity"))?;
                if !function.captures.is_empty() {
                    return Err(invalid(
                        span,
                        "direct call cannot supply captured environment",
                    ));
                }
                return Ok((
                    format!("f{}", id.0),
                    function.params.iter().map(|p| p.ty.clone()).collect(),
                    function.return_type.clone(),
                ));
            }
            CallTarget::Builtin(builtin) => builtin,
        };
        let (symbol, params, result) = match builtin {
            Builtin::StringConcat => (
                "morrow_str_concat".into(),
                vec![Type::String, Type::String],
                Type::String,
            ),
            Builtin::StringEq => (
                "morrow_str_eq".into(),
                vec![Type::String, Type::String],
                Type::Bool,
            ),
            Builtin::StringLen => ("morrow_str_len".into(), vec![Type::String], Type::Int),
            Builtin::Print | Builtin::Println => {
                let arg = args
                    .first()
                    .ok_or_else(|| invalid(span, "print requires one argument"))?;
                let suffix = match arg.ty.clone() {
                    Type::Int => "int",
                    Type::Float => "float",
                    Type::Bool => "bool",
                    Type::String => "str",
                    _ => return Err(invalid(arg.span, "cannot print compound or Unit value")),
                };
                let name = if builtin == Builtin::Print {
                    "print"
                } else {
                    "println"
                };
                (
                    format!("morrow_{name}_{suffix}"),
                    vec![arg.ty.clone()],
                    Type::Unit,
                )
            }
            _ => return compound_signature(builtin, args, span),
        };
        Ok((symbol, params, result))
    }

    /// Emit typed arithmetic/comparisons; logical operators require control flow.
    fn binary(
        &mut self,
        op: BinaryOp,
        left: &Expr,
        right: &Expr,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        if matches!(op, BinaryOp::And | BinaryOp::Or) {
            return self.logical(op, left, right, locals, depth);
        }
        if op == BinaryOp::In {
            return self.scalar_contains_ordered(right, left, locals, depth, true);
        }
        expect_type(right.ty.clone(), left.ty.clone(), right.span)?;
        let result_type = numeric::binary_type(op, &left.ty, left.span)?;
        let lhs = self.expr(left, locals, depth)?;
        let rhs = self.expr(right, locals, depth)?;
        if left.ty == Type::String {
            return Ok(self.string_binary(op, &lhs, &rhs, locals));
        }
        let value = self.numeric_value(op, &left.ty, &result_type, &lhs, &rhs, locals);
        Ok((result_type, value))
    }

    /// Emit the three checked String operators through content-aware runtime helpers.
    fn string_binary(
        &mut self,
        op: BinaryOp,
        lhs: &str,
        rhs: &str,
        locals: &mut Locals,
    ) -> (Type, String) {
        if op == BinaryOp::Add {
            let value = self.assign(
                locals,
                Type::String,
                NativeOperation::Call {
                    callee: native_operand("$morrow_str_concat"),
                    args: vec![
                        (Scalar::I64, native_operand(lhs)),
                        (Scalar::I64, native_operand(rhs)),
                    ],
                    variadic: None,
                },
            );
            return (Type::String, value);
        }
        let value = self.assign(
            locals,
            Type::Bool,
            NativeOperation::Call {
                callee: native_operand("$morrow_str_eq"),
                args: vec![
                    (Scalar::I64, native_operand(lhs)),
                    (Scalar::I64, native_operand(rhs)),
                ],
                variadic: None,
            },
        );
        let value = if op == BinaryOp::Ne {
            self.assign(
                locals,
                Type::Bool,
                NativeOperation::Binary(
                    MachineBinary::Compare(Comparison::Eq, Scalar::I32),
                    native_operand(&(value)),
                    native_operand("0"),
                ),
            )
        } else {
            value
        };
        (Type::Bool, value)
    }

    /// Emit true short-circuit evaluation, evaluating `right` only in its RHS block.
    fn logical(
        &mut self,
        op: BinaryOp,
        left: &Expr,
        right: &Expr,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        expect_type(left.ty.clone(), Type::Bool, left.span)?;
        expect_type(right.ty.clone(), Type::Bool, right.span)?;
        let lhs = self.expr(left, locals, depth)?;
        let before = locals.current.clone();
        let rhs_label = locals.label();
        let merge = locals.label();
        let (on_true, on_false, shortcut) = if op == BinaryOp::And {
            (&rhs_label, &merge, 0)
        } else {
            (&merge, &rhs_label, 1)
        };
        self.output.statement(Statement::Branch {
            condition: native_operand(&(lhs)),
            then_label: (on_true).to_string(),
            else_label: (on_false).to_string(),
        });
        self.start_block(locals, &rhs_label);
        let mut incoming = vec![(before, shortcut.to_string())];
        let rhs = self.expr(right, locals, depth);
        self.incoming(rhs, &mut incoming, &merge, locals)?;
        self.join(Type::Bool, incoming, &merge, locals)
    }

    /// Emit `if` with phi incoming labels captured after nested branch expressions.
    fn conditional(
        &mut self,
        condition: &Expr,
        then_branch: &Expr,
        else_branch: Option<&Expr>,
        locals: &mut Locals,
        depth: usize,
        tail: bool,
    ) -> Lowering<(Type, String)> {
        expect_type(condition.ty.clone(), Type::Bool, condition.span)?;
        let result_type = if let Some(other) = else_branch {
            control::joined(&then_branch.ty, &other.ty, other.span)?
        } else {
            Type::Unit
        };
        let test = self.expr(condition, locals, depth)?;
        let then_label = locals.label();
        let else_label = locals.label();
        let merge = locals.label();
        let mut incoming = Vec::new();
        self.output.statement(Statement::Branch {
            condition: native_operand(&(test)),
            then_label: (then_label).to_string(),
            else_label: (else_label).to_string(),
        });
        self.start_block(locals, &then_label);
        let then_value = self.position_expr(then_branch, locals, depth, tail);
        self.incoming(then_value, &mut incoming, &merge, locals)?;
        self.start_block(locals, &else_label);
        let else_value = if let Some(other) = else_branch {
            self.position_expr(other, locals, depth, tail)
        } else {
            Ok("0".into())
        };
        self.incoming(else_value, &mut incoming, &merge, locals)?;
        self.join(result_type, incoming, &merge, locals)
    }
}

/// Select the scalar opcode from a checked operator and operand type.
fn binary_instruction(op: BinaryOp, operand: Type) -> MachineBinary {
    let cmp = match op {
        BinaryOp::Add => return MachineBinary::Add,
        BinaryOp::Subtract => return MachineBinary::Sub,
        BinaryOp::Multiply => return MachineBinary::Mul,
        BinaryOp::Divide => return MachineBinary::Div,
        BinaryOp::Remainder => return MachineBinary::Rem,
        BinaryOp::BitAnd => return MachineBinary::And,
        BinaryOp::BitOr => return MachineBinary::Or,
        BinaryOp::BitXor => return MachineBinary::Xor,
        BinaryOp::ShiftLeft => return MachineBinary::Shl,
        BinaryOp::ShiftRight => return MachineBinary::Sar,
        BinaryOp::Eq => Comparison::Eq,
        BinaryOp::Ne => Comparison::Ne,
        BinaryOp::Lt => Comparison::SLt,
        BinaryOp::Le => Comparison::SLe,
        BinaryOp::Gt => Comparison::SGt,
        BinaryOp::Ge => Comparison::SGe,
        BinaryOp::In | BinaryOp::Power | BinaryOp::And | BinaryOp::Or => {
            unreachable!("operator requires dedicated lowering")
        }
    };
    MachineBinary::Compare(cmp, machine_width(scalar_width(operand)))
}

/// Reject unresolved or excessively nested types before choosing any ABI layout.
fn concrete(ty: &Type, span: Span, depth: usize) -> Lowering<()> {
    if depth > MAX_DEPTH {
        return Err(invalid(span, "type nesting limit exceeded"));
    }
    match ty {
        Type::Never | Type::Infer(_) | Type::Generic(_) => Err(invalid(
            span,
            "unresolved inference variable or generic type",
        )),
        Type::Function(args, result) => {
            for arg in args {
                concrete(arg, span, depth + 1)?;
            }
            concrete(result, span, depth + 1)
        }
        Type::Union(members) => {
            unions::canonical(members, span)?;
            for member in members {
                concrete(member, span, depth + 1)?;
            }
            Ok(())
        }
        Type::Tuple(args) | Type::Named(_, args) => {
            for arg in args {
                concrete(arg, span, depth + 1)?;
            }
            Ok(())
        }
        Type::Pid(item) | Type::ChildKey(item) | Type::RootFunction(item) | Type::List(item) | Type::Option(item) => concrete(item, span, depth + 1),
        Type::ActorFunction(ok, err) | Type::Result(ok, err) | Type::Map(ok, err) => {
            concrete(ok, span, depth + 1)?;
            concrete(err, span, depth + 1)
        }
        Type::Range
        | Type::Native(_)
        | Type::Int
        | Type::Float
        | Type::Bool
        | Type::String
        | Type::Unit => Ok(()),
    }
}

/// Determine the payload type for a constructor, rejecting tags from another sum type.
fn payload_type(constructor: Constructor, ty: &Type, span: Span) -> Lowering<Option<&Type>> {
    match (constructor, ty) {
        (Constructor::Some, Type::Option(item)) => Ok(Some(item)),
        (Constructor::None, Type::Option(_)) => Ok(None),
        (Constructor::Ok, Type::Result(ok, _)) => Ok(Some(ok)),
        (Constructor::Err, Type::Result(_, err)) => Ok(Some(err)),
        _ => Err(invalid(
            span,
            "constructor does not belong to the checked sum type",
        )),
    }
}

/// Identify builtins whose C runtime ABI transports every scalar as a full word.
fn compound_builtin(target: CallTarget) -> bool {
    matches!(target, CallTarget::Builtin(builtin) if !matches!(builtin,
        Builtin::Print | Builtin::Println | Builtin::StringConcat | Builtin::StringEq | Builtin::StringLen))
}

/// Resolve generic runtime operations against the first argument's concrete type.
fn compound_signature(
    builtin: Builtin,
    args: &[Expr],
    span: Span,
) -> Lowering<(String, Vec<Type>, Type)> {
    let first = args
        .first()
        .ok_or_else(|| invalid(span, "builtin requires an argument"))?;
    match builtin {
        Builtin::ListLen
        | Builtin::ListGet
        | Builtin::ListHead
        | Builtin::ListTail
        | Builtin::ListIsEmpty
        | Builtin::ListPush
        | Builtin::ListReverse
        | Builtin::ListConcat
        | Builtin::ListContains => list_signature(builtin, &first.ty, span),
        Builtin::OptionIsSome | Builtin::OptionIsNone | Builtin::OptionUnwrapOr => {
            let Type::Option(item) = &first.ty else {
                return Err(invalid(span, "Option builtin requires Option"));
            };
            sum_signature(builtin, &first.ty, item)
        }
        Builtin::ResultIsOk | Builtin::ResultIsErr | Builtin::ResultUnwrapOr => {
            let Type::Result(ok, _) = &first.ty else {
                return Err(invalid(span, "Result builtin requires Result"));
            };
            sum_signature(builtin, &first.ty, ok)
        }
        _ => Err(invalid(span, "expected a compound builtin identity")),
    }
}

/// Derive list ABI symbols and semantic signatures, avoiding compound pointer equality.
fn list_signature(builtin: Builtin, ty: &Type, span: Span) -> Lowering<(String, Vec<Type>, Type)> {
    let Type::List(item) = ty else {
        return Err(invalid(span, "List builtin requires List"));
    };
    let mut params = vec![ty.clone()];
    let (symbol, result) = match builtin {
        Builtin::ListLen => ("morrow_list_len", Type::Int),
        Builtin::ListIsEmpty => ("morrow_list_is_empty", Type::Bool),
        Builtin::ListHead => ("morrow_list_head", *item.clone()),
        Builtin::ListGet => {
            params.push(Type::Int);
            ("morrow_list_get", *item.clone())
        }
        Builtin::ListTail => ("morrow_list_tail", ty.clone()),
        Builtin::ListReverse => ("morrow_list_reverse", ty.clone()),
        Builtin::ListPush => {
            params.push(*item.clone());
            ("morrow_list_push", ty.clone())
        }
        Builtin::ListConcat => {
            params.push(ty.clone());
            ("morrow_list_concat", ty.clone())
        }
        Builtin::ListContains => {
            if !matches!(**item, Type::Int | Type::Bool | Type::String) {
                return Err(invalid(
                    span,
                    "List contains requires Int, Bool, or String elements",
                ));
            }
            params.push(*item.clone());
            (
                if **item == Type::String {
                    "morrow_list_contains_str"
                } else {
                    "morrow_list_contains"
                },
                Type::Bool,
            )
        }
        _ => return Err(invalid(span, "expected a List builtin identity")),
    };
    Ok((symbol.into(), params, result))
}

/// Use heap Result helpers for both built-in sum types, with eager default arguments.
fn sum_signature(
    builtin: Builtin,
    ty: &Type,
    payload: &Type,
) -> Lowering<(String, Vec<Type>, Type)> {
    if matches!(builtin, Builtin::OptionUnwrapOr | Builtin::ResultUnwrapOr) {
        Ok((
            "morrow_result_unwrap_or".into(),
            vec![ty.clone(), payload.clone()],
            payload.clone(),
        ))
    } else {
        Ok(("morrow_result_is_ok".into(), vec![ty.clone()], Type::Bool))
    }
}

impl Emitter<'_> {
    /// Widen Boolean/Unit values before storing them in runtime payload slots.
    fn payload(&mut self, locals: &mut Locals, ty: &Type, value: String) -> String {
        if matches!(self.representation(ty), Type::Bool | Type::Unit) {
            self.assign(
                locals,
                Type::Int,
                NativeOperation::Unary(MachineUnary::ExtUw, native_operand(&(value))),
            )
        } else if *self.representation(ty) == Type::Float {
            self.assign(
                locals,
                Type::Int,
                NativeOperation::Unary(MachineUnary::Cast, native_operand(&(value))),
            )
        } else {
            value
        }
    }

    /// Narrow payload loads to the checked scalar width without changing pointer values.
    fn unpack(&mut self, locals: &mut Locals, ty: &Type, value: String) -> String {
        if matches!(self.representation(ty), Type::Bool | Type::Unit) {
            self.assign(
                locals,
                ty.clone(),
                NativeOperation::Unary(MachineUnary::Copy, native_operand(&(value))),
            )
        } else if *self.representation(ty) == Type::Float {
            self.assign(
                locals,
                Type::Float,
                NativeOperation::Unary(MachineUnary::Cast, native_operand(&(value))),
            )
        } else {
            self.root_value(locals, ty, &value);
            value
        }
    }

    /// Allocate a private list and populate it once before exposing its immutable value.
    fn list(
        &mut self,
        items: &[Expr],
        ty: &Type,
        span: Span,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        let Type::List(item_type) = ty else {
            return Err(invalid(span, "list literal requires List type"));
        };
        let list = self.assign(
            locals,
            ty.clone(),
            NativeOperation::Call {
                callee: native_operand("$morrow_list_with_capacity"),
                args: vec![(
                    Scalar::I64,
                    native_operand(&(items.len().max(1)).to_string()),
                )],
                variadic: None,
            },
        );
        for item in items {
            expect_type(item.ty.clone(), *item_type.clone(), item.span)?;
            let value = self.expr(item, locals, depth)?;
            let value = self.payload(locals, &item.ty, value);
            self.output
                .statement(Statement::Effect(NativeOperation::Call {
                    callee: native_operand("$morrow_list_push_mut"),
                    args: vec![
                        (Scalar::I64, native_operand(&(list).to_string())),
                        (Scalar::I64, native_operand(&(value))),
                    ],
                    variadic: None,
                }));
        }
        Ok((ty.clone(), list))
    }

    /// Construct heap-backed Options/Results using the checked payload, including full i64s.
    fn construct(
        &mut self,
        constructor: Constructor,
        value: Option<&Expr>,
        ty: &Type,
        span: Span,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        let expected = payload_type(constructor, ty, span)?;
        let payload = match (value, expected) {
            (Some(value), Some(expected)) => {
                expect_type(value.ty.clone(), expected.clone(), value.span)?;
                let lowered = self.expr(value, locals, depth)?;
                self.payload(locals, expected, lowered)
            }
            (None, None) => "0".into(),
            _ => {
                return Err(invalid(
                    span,
                    "constructor payload presence differs from its signature",
                ));
            }
        };
        let symbol = if matches!(constructor, Constructor::Some | Constructor::Ok) {
            "morrow_result_ok"
        } else {
            "morrow_result_err"
        };
        Ok((
            ty.clone(),
            self.assign(
                locals,
                ty.clone(),
                NativeOperation::Call {
                    callee: native_operand(&format!("${}", symbol)),
                    args: vec![(Scalar::I64, native_operand(&(payload)))],
                    variadic: None,
                },
            ),
        ))
    }

    /// Emit a runtime call with explicit full-width transport and typed result conversion.
    fn compound_call(
        &mut self,
        target: CallTarget,
        args: &[Expr],
        span: Span,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        let (symbol, params, result) = self.signature(target, args, span)?;
        if args.len() != params.len() {
            return Err(invalid(
                span,
                "builtin argument count differs from signature",
            ));
        }
        let mut arguments = Vec::new();
        for (arg, expected) in args.iter().zip(params) {
            expect_type(arg.ty.clone(), expected, arg.span)?;
            let value = self.expr(arg, locals, depth)?;
            let payload = self.payload(locals, &arg.ty, value);
            arguments.push((Scalar::I64, native_operand(&payload)));
        }
        let raw = self.assign(
            locals,
            Type::Int,
            NativeOperation::Call {
                callee: native_operand(&format!("${}", symbol)),
                args: arguments.clone(),
                variadic: None,
            },
        );
        let mut value = self.unpack(locals, &result, raw);
        if matches!(
            target,
            CallTarget::Builtin(Builtin::OptionIsNone | Builtin::ResultIsErr)
        ) {
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
        Ok((result, value))
    }
}

impl Emitter<'_> {
    /// Propagate Err unchanged, then expose an Ok payload in the continuing block.
    fn attempt(
        &mut self,
        value: &Expr,
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        let Type::Result(ok, error) = &value.ty else {
            return Err(invalid(value.span, "? requires Result operand"));
        };
        let Type::Result(_, returned_error) = &locals.return_type else {
            return Err(invalid(
                value.span,
                "? requires enclosing Result return type",
            ));
        };
        expect_type(*error.clone(), *returned_error.clone(), value.span)?;
        let result = self.expr(value, locals, depth)?;
        let tag = self.assign(
            locals,
            Type::Bool,
            NativeOperation::Call {
                callee: native_operand("$morrow_result_is_ok"),
                args: vec![(Scalar::I64, native_operand(&(result).to_string()))],
                variadic: None,
            },
        );
        let success = locals.label();
        let failure = locals.label();
        self.output.statement(Statement::Branch {
            condition: native_operand(&(tag)),
            then_label: (success).to_string(),
            else_label: (failure).to_string(),
        });
        self.start_block(locals, &failure);
        self.save_return(&result, locals);
        self.start_block(locals, &success);
        let payload = self.assign(
            locals,
            Type::Int,
            NativeOperation::Call {
                callee: native_operand("$morrow_result_unwrap"),
                args: vec![(Scalar::I64, native_operand(&(result).to_string()))],
                variadic: None,
            },
        );
        let value = self.unpack(locals, ok, payload);
        Ok((*ok.clone(), value))
    }
}

impl Emitter<'_> {
    /// Convert scalar parts once and concatenate in source order using the runtime ABI.
    fn interpolate(
        &mut self,
        parts: &[Expr],
        locals: &mut Locals,
        depth: usize,
    ) -> Lowering<(Type, String)> {
        let mut text = self.string("", Span::default())?;
        for part in parts {
            if !matches!(part.ty, Type::Int | Type::Float | Type::Bool | Type::String) {
                return Err(invalid(part.span, "interpolation requires scalar values"));
            }
            let value = self.expr(part, locals, depth)?;
            let value = match part.ty {
                Type::String => value,
                Type::Int => self.assign(
                    locals,
                    Type::String,
                    NativeOperation::Call {
                        callee: native_operand("$morrow_int_to_str"),
                        args: vec![(Scalar::I64, native_operand(&(value)))],
                        variadic: None,
                    },
                ),
                Type::Bool => {
                    let value = self.assign(
                        locals,
                        Type::Int,
                        NativeOperation::Unary(MachineUnary::ExtUw, native_operand(&(value))),
                    );
                    self.assign(
                        locals,
                        Type::String,
                        NativeOperation::Call {
                            callee: native_operand("$morrow_bool_to_str"),
                            args: vec![(Scalar::I64, native_operand(&(value)))],
                            variadic: None,
                        },
                    )
                }
                Type::Float => self.float_string(value, locals, part.span)?,
                _ => return Err(invalid(part.span, "interpolation requires scalar values")),
            };
            text = self.assign(
                locals,
                Type::String,
                NativeOperation::Call {
                    callee: native_operand("$morrow_str_concat"),
                    args: vec![
                        (Scalar::I64, native_operand(&(text).to_string())),
                        (Scalar::I64, native_operand(&(value))),
                    ],
                    variadic: None,
                },
            );
        }
        Ok((Type::String, text))
    }

    /// Format an IEEE double into a bounded GC allocation with round-trip precision.
    fn float_string(&mut self, value: String, locals: &mut Locals, span: Span) -> Lowering<String> {
        let _ = span;
        Ok(self.assign(
            locals,
            Type::String,
            NativeOperation::Call {
                callee: native_operand("$morrow_float_to_str"),
                args: vec![(Scalar::F64, native_operand(&value))],
                variadic: None,
            },
        ))
    }
}

/// Keep diagnostics for private proof nodes consistent before any executable lowering.
fn private_expression(expr: &Expr) -> Exit {
    let message = match expr.kind {
        ExprKind::JsonCodecTemplate { .. } => "JSON codec template cannot enter executable IR",
        ExprKind::EditorHole { .. } => "editor hole cannot enter executable IR",
        ExprKind::Probe { .. } => "inference probe cannot enter executable IR",
        _ => "invalid private expression",
    };
    invalid(expr.span, message)
}

/// Reuse validated native continuation shapes for deterministic interactive scheduling.
pub(crate) fn prepare_interactive_actors(
    program: &ir::Program,
) -> Result<(ir::Program, BTreeMap<usize, usize>), Diagnostic> {
    ir::reject_probes(program)?;
    crate::json_codec::validate_program(program)?;
    let prepared = actor_backend::prepare(program).map_err(|exit| match exit {
        Exit::Diagnostic(error) => error,
        Exit::Terminated => {
            Diagnostic::new(Span::default(), "actor preparation terminated unexpectedly")
        }
    })?;
    let mut entries = prepared.plan.entries;
    entries.extend(prepared.plan.helpers);
    Ok((prepared.program.into_owned(), entries))
}

#[cfg(test)]
mod machine_lowering_tests {
    use super::*;

    #[test]
    fn shared_lowering_preserves_qbe_and_typed_entry() {
        let program = ir::Program {
            types: vec![],
            functions: vec![Function {
                root_context: false,
                mailbox: None,
                captures: vec![],
                id: ir::FunctionId(0),
                name: "main".into(),
                return_type: Type::Int,
                body: Expr {
                    kind: ExprKind::Int(42),
                    ty: Type::Int,
                    span: Span::default(),
                },
                params: vec![],
                local_count: 0,
            }],
        };
        let machine = lower(&program).unwrap();
        assert_eq!(machine.to_text(), emit(&program).unwrap());
        let main = machine
            .functions
            .iter()
            .find(|function| function.name == "$f0")
            .unwrap();
        assert_eq!(main.result, Some(Scalar::I64));
        assert_eq!(
            main.params,
            vec![(Scalar::I64, "%env".into()), (Scalar::I64, "%fault".into())]
        );
        assert!(main.body.iter().any(|statement| matches!(statement,
            Statement::Store { kind: LoadKind::I64, value: Operand::Int(42), address: Operand::Temp(name) } if name == "return_slot"
        )));
        assert!(machine.functions.iter().any(|function| function.export
            && function.name == "$morrow_main"
            && function.result == Some(Scalar::I32)));
    }
}
