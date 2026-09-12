//! Persistent typed-IR evaluation for interactive language exploration.
use crate::{Type, ast, check, ir, parse, runtime};
use std::{collections::HashMap, io::BufRead, rc::Rc};

#[derive(Clone, Debug, PartialEq)]
enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(Rc<String>),
    Unit,
    Json(json::Json),
    JsonError(json::Error),
    Range(i64, i64, bool),
    List(Rc<Vec<Value>>),
    Map(Rc<Vec<(Value, Value)>>),
    Sum(usize, Rc<Vec<Value>>),
    Closure(Rc<ClosureValue>),
    Union(Rc<UnionValue>),
}
/// Keep the active semantic member because raw newtype payloads cannot identify their type.
#[derive(Clone, Debug, PartialEq)]
struct UnionValue {
    member: Type,
    value: Value,
}
/// Closures retain their originating program because later entries can renumber functions.
#[derive(Clone, Debug)]
struct ClosureValue {
    program: Rc<ir::Program>,
    function: ir::FunctionId,
    captures: Vec<Value>,
}
impl PartialEq for ClosureValue {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}
#[derive(Debug)]
enum Failure {
    Message(String),
    Return(Value),
    Break,
    Continue,
}
type Eval<T> = Result<T, Failure>;
fn fault(message: impl Into<String>) -> Failure {
    Failure::Message(message.into())
}

/// An isolated session retaining successful definitions and values, never replaying effects.
#[derive(Default)]
pub struct Session {
    definitions: String,
    bindings: Vec<String>,
    values: HashMap<usize, Value>,
    statements: usize,
}
impl Session {
    /// Inspect an expression using the checker without evaluating it or retained effects.
    pub fn expression_type(&self, source: &str) -> Result<String, String> {
        let mut name = "fern_repl_type_query".to_owned();
        while self.definitions.contains(&name) {
            name.push('_');
        }
        let body = format!("{}\n{source}", self.bindings.join("\n"));
        let indented = parse::indent_code(&body, "    ").map_err(|error| error.message)?;
        let program = format!("{}\nfn {name}():\n{indented}", self.definitions);
        let syntax = parse::parse(&program).map_err(|error| error.message)?;
        let schemes = check::editor::function_schemes(&syntax).map_err(|error| error.message)?;
        let scheme = schemes
            .iter()
            .find(|scheme| scheme.name == name)
            .ok_or("missing type query")?;
        crate::presentation::render_type_with_names(
            &scheme.result,
            &scheme.generics,
            crate::presentation::Limits::default(),
        )
        .map_err(|error| error.message)
    }

    /// Check an entry before evaluating it; failed entries never replace prior bindings.
    pub fn evaluate(&mut self, source: &str) -> Result<String, String> {
        let declaration = declaration_source(source);
        let binding = source.trim_start().starts_with("let ");
        let definitions = if declaration {
            format!("{}\n{source}\n", self.definitions)
        } else {
            self.definitions.clone()
        };
        let mut body = self.bindings.join("\n");
        if !declaration {
            body.push('\n');
            body.push_str(source);
        }
        if body.trim().is_empty() {
            body.push_str("()");
        }
        let indented = parse::indent_code(&body, "    ").map_err(|error| error.message)?;
        let program = format!("{definitions}\nfn main():\n{indented}");
        let syntax = parse::parse(&program).map_err(|e| e.message)?;
        let typed = Rc::new(check::check(&syntax).map_err(|e| e.message)?);
        crate::actors::reject_interactive(&syntax)?;
        let main = typed
            .functions
            .iter()
            .find(|f| f.name == "main")
            .ok_or("missing REPL entry")?;
        let ir::ExprKind::Block(statements) = &main.body.kind else {
            return Err("invalid REPL entry block".into());
        };
        if declaration {
            self.definitions = definitions;
            return Ok(String::new());
        }
        let mut machine = Machine::new(typed.clone(), self.values.clone());
        let result = machine.statements(&statements[self.statements..]);
        let result = machine.finish(result);
        let value = match result {
            Ok(value) => value,
            Err(Failure::Message(message)) => return Err(message),
            Err(Failure::Return(_)) => return Err("return outside an interactive function".into()),
            Err(Failure::Break | Failure::Continue) => {
                return Err("loop control outside a loop".into());
            }
        };
        graph_budget(machine.locals.values())?;
        if binding {
            self.bindings.push(source.into());
            self.values = machine.locals;
            self.statements = statements.len();
        } else if !matches!(statements.last(), Some(ir::Stmt::Expr(expr)) if expr.ty == Type::Unit)
        {
            let ty = match statements.last() {
                Some(ir::Stmt::Expr(expr)) => &expr.ty,
                _ => &Type::Unit,
            };
            machine.output.push_str(&format!(
                "{} : {}\n",
                display_typed(&value, ty, &syntax, &typed.types, &mut 4096),
                type_name(ty)
            ));
        }
        Ok(machine.output)
    }
}
struct Machine {
    program: Rc<ir::Program>,
    locals: HashMap<usize, Value>,
    output: String,
    steps: usize,
    depth: usize,
    defers: Vec<Value>,
    cleanup_depth: usize,
    cleanup_steps: usize,
    json_limits: json::Limits,
    json_cleanup: json::Limits,
}
impl Machine {
    /// Initialize one evaluation entry with fresh work and cleanup budgets.
    fn new(program: Rc<ir::Program>, locals: HashMap<usize, Value>) -> Self {
        Self {
            program,
            locals,
            output: String::new(),
            steps: 0,
            depth: 0,
            defers: Vec::new(),
            cleanup_depth: 0,
            cleanup_steps: 0,
            json_limits: json::Limits::new(64 * 1024 * 1024),
            json_cleanup: json::Limits::new(8 * 1024 * 1024),
        }
    }
    /// Bound evaluator recursion and work independently of compiler syntax limits.
    fn expression(&mut self, expr: &ir::Expr) -> Eval<Value> {
        self.charge_step()?;
        self.depth += 1;
        let result = self.node(expr);
        self.depth -= 1;
        result
    }
    /// Evaluate one typed node using resolved identities and semantic values.
    fn node(&mut self, expr: &ir::Expr) -> Eval<Value> {
        use ir::ExprKind::*;
        match &expr.kind {
            Actor(_) => Err(Failure::Message(
                "managed actors are not supported in the REPL yet".into(),
            )),
            JsonCodec { .. } | JsonCodecTemplate { .. } | EditorHole { .. } | Probe { .. } => {
                self.codec_node(expr)
            }
            UnionInject { value } => {
                let member = value.ty.clone();
                let value = self.expression(value)?;
                Ok(Value::Union(Rc::new(UnionValue { member, value })))
            }
            UnionWiden { value } => self.expression(value),
            Wrap(value) | Unwrap(value) => self.expression(value),
            Return(value) => Err(Failure::Return(self.expression(value)?)),
            Break => Err(Failure::Break),
            Continue => Err(Failure::Continue),
            Range {
                start,
                end,
                inclusive,
            } => self.range(start, end, *inclusive),
            For {
                pattern,
                iterable,
                body,
            } => self.for_each(pattern, iterable, body),
            With {
                steps,
                body,
                handlers,
            } => self.with_block(steps, body, handlers),
            Defer(value) => self.defer(value),
            Closure { function, captures } => self.closure(*function, captures),
            Invoke { callee, args } => self.apply_expression(callee, args),
            Lambda { .. } | FunctionValue { .. } => Err(fault("unfinalized interactive closure")),
            Interpolate(parts) => self.interpolate(parts),
            Int(n) => Ok(Value::Int(*n)),
            Float(n) => Ok(Value::Float(*n)),
            Bool(v) => Ok(Value::Bool(*v)),
            String(s) => Ok(Value::String(Rc::new(s.clone()))),
            Unit => Ok(Value::Unit),
            Local(id) => self.local(*id),
            List(values) => Ok(Value::List(Rc::new(self.arguments(values)?))),
            Map(entries) => self.map_literal(entries),
            Tuple(values) => Ok(Value::Sum(0, Rc::new(self.arguments(values)?))),
            CustomConstruct { tag, fields } => {
                Ok(Value::Sum(*tag, Rc::new(self.arguments(fields)?)))
            }
            Field { value, index } => self.field(value, *index),
            Construct { constructor, value } => self.construct(*constructor, value.as_deref()),
            Try(value) => self.try_value(value),
            Unary { op, value } => unary(*op, self.expression(value)?),
            Binary { op, left, right } => self.binary(*op, left, right),
            Call { target, args } => {
                let args = self.arguments(args)?;
                self.call(*target, args)
            }
            If {
                condition,
                then_branch,
                else_branch,
            } => self.conditional(condition, then_branch, else_branch.as_deref()),
            Match { value, arms } => self.match_expression(value, arms),
            Block(statements) => self.lexical_block(statements),
        }
    }
    /// Execute concrete codecs and reject every private checking artifact.
    fn codec_node(&mut self, expr: &ir::Expr) -> Eval<Value> {
        use ir::ExprKind::*;
        match &expr.kind {
            JsonCodec {
                direction,
                input,
                plan,
            } => self.json_codec(*direction, input, plan),
            JsonCodecTemplate { .. } => {
                Err(fault("JSON codec template cannot enter executable IR"))
            }
            EditorHole { .. } => Err(fault("editor hole cannot enter executable IR")),
            Probe { .. } => Err(fault("inference probe cannot enter executable IR")),
            _ => Err(fault("invalid interactive codec node")),
        }
    }
    /// Preserve early Result propagation without changing the surrounding function's cleanup path.
    fn try_value(&mut self, value: &ir::Expr) -> Eval<Value> {
        match self.expression(value)? {
            Value::Sum(0, fields) if fields.len() == 1 => Ok(fields[0].clone()),
            value @ Value::Sum(1, _) => Err(Failure::Return(value)),
            _ => Err(fault("invalid Result")),
        }
    }
    /// Read the current lexical slot without exposing absent evaluator state.
    fn local(&self, id: ir::LocalId) -> Eval<Value> {
        self.locals
            .get(&id.0)
            .cloned()
            .ok_or_else(|| fault("missing interactive local"))
    }
    /// Evaluate a callable before its arguments, preserving abrupt exits from either.
    fn apply_expression(&mut self, callee: &ir::Expr, args: &[ir::Expr]) -> Eval<Value> {
        let callee = self.expression(callee)?;
        let args = self.arguments(args)?;
        self.invoke(&callee, args)
    }
    /// Evaluate the scrutinee once before examining any pattern or guard.
    fn match_expression(&mut self, value: &ir::Expr, arms: &[ir::MatchArm]) -> Eval<Value> {
        let value = self.expression(value)?;
        self.matching(&value, arms)
    }
    /// Evaluate each capture once and retain the code that assigned its function identity.
    fn closure(&mut self, function: ir::FunctionId, captures: &[ir::Expr]) -> Eval<Value> {
        let captures = self.arguments(captures)?;
        Ok(Value::Closure(Rc::new(ClosureValue {
            program: self.program.clone(),
            function,
            captures,
        })))
    }
    /// Read a checked structural or nominal field from immutable storage.
    fn field(&mut self, value: &ir::Expr, index: usize) -> Eval<Value> {
        match self.expression(value)? {
            Value::Sum(_, fields) => fields
                .get(index)
                .cloned()
                .ok_or_else(|| fault("invalid field")),
            _ => Err(fault("invalid record")),
        }
    }
    /// Preserve semantic sum tags without depending on the native transport encoding.
    fn construct(
        &mut self,
        constructor: crate::Constructor,
        value: Option<&ir::Expr>,
    ) -> Eval<Value> {
        let tag = usize::from(matches!(
            constructor,
            crate::Constructor::None | crate::Constructor::Err
        ));
        let fields = match value {
            Some(value) => vec![self.expression(value)?],
            None => Vec::new(),
        };
        Ok(Value::Sum(tag, Rc::new(fields)))
    }
    /// Convert scalar interpolation parts in order while bounding the resulting string.
    fn interpolate(&mut self, parts: &[ir::Expr]) -> Eval<Value> {
        let mut text = String::new();
        for part in parts {
            let value = self.expression(part)?;
            let next = match value {
                Value::String(s) => s.to_string(),
                other => display(&other),
            };
            if text.len() + next.len() > 1024 * 1024 {
                return Err(fault("interactive string limit exceeded"));
            }
            text.push_str(&next);
        }
        Ok(Value::String(Rc::new(text)))
    }
    /// Discard the true branch value for a conditional with no else, preserving effects.
    fn conditional(
        &mut self,
        condition: &ir::Expr,
        yes: &ir::Expr,
        no: Option<&ir::Expr>,
    ) -> Eval<Value> {
        match self.expression(condition)? {
            Value::Bool(true) => {
                let value = self.expression(yes)?;
                Ok(if no.is_some() { value } else { Value::Unit })
            }
            Value::Bool(false) => no.map_or(Ok(Value::Unit), |expr| self.expression(expr)),
            _ => Err(fault("invalid condition")),
        }
    }
    /// Evaluate arguments once from left to right.
    fn arguments(&mut self, args: &[ir::Expr]) -> Eval<Vec<Value>> {
        args.iter().map(|arg| self.expression(arg)).collect()
    }
    /// Evaluate an incremental statement suffix, keeping its top-level bindings.
    fn statements(&mut self, statements: &[ir::Stmt]) -> Eval<Value> {
        let mut result = Value::Unit;
        for statement in statements {
            result = match statement {
                ir::Stmt::Let { id, value } => {
                    let value = self.expression(value)?;
                    self.locals.insert(id.0, value);
                    Value::Unit
                }
                ir::Stmt::LetElse {
                    pattern,
                    value,
                    else_branch,
                } => self.let_else(pattern, value, else_branch)?,
                ir::Stmt::Expr(value) => self.expression(value)?,
            };
        }
        Ok(result)
    }
    /// Respect short-circuit control flow before evaluating a binary right operand.
    fn binary(&mut self, op: ast::BinaryOp, left: &ir::Expr, right: &ir::Expr) -> Eval<Value> {
        let left = self.expression(left)?;
        if (op == ast::BinaryOp::And && left == Value::Bool(false))
            || (op == ast::BinaryOp::Or && left == Value::Bool(true))
        {
            return Ok(left);
        }
        let right = self.expression(right)?;
        if op == ast::BinaryOp::In {
            return self.builtin(ir::Builtin::ListContains, vec![right, left]);
        }
        binary(op, left, right)
    }
    /// Run a resolved function in its own locals and catch Result propagation at its boundary.
    fn call(&mut self, target: ir::CallTarget, args: Vec<Value>) -> Eval<Value> {
        match target {
            ir::CallTarget::Builtin(builtin) => self.builtin(builtin, args),
            ir::CallTarget::Runtime(id) => self.runtime(id, args),
            ir::CallTarget::Function(id) => self.function(self.program.clone(), id, &[], args),
        }
    }
    /// Invoke captured code from its immutable originating program, never a rebuilt ID table.
    fn invoke(&mut self, value: &Value, args: Vec<Value>) -> Eval<Value> {
        let Value::Closure(closure) = value else {
            return Err(fault("value is not callable"));
        };
        self.function(
            closure.program.clone(),
            closure.function,
            &closure.captures,
            args,
        )
    }
    /// Restore caller code and locals on success, error, or Result propagation.
    fn function(
        &mut self,
        program: Rc<ir::Program>,
        id: ir::FunctionId,
        captures: &[Value],
        args: Vec<Value>,
    ) -> Eval<Value> {
        let function = program
            .functions
            .iter()
            .find(|f| f.id == id)
            .ok_or_else(|| fault("unknown function"))?;
        if function.params.len() != args.len() || function.captures.len() != captures.len() {
            return Err(fault("invalid function environment or argument count"));
        }
        let previous_program = std::mem::replace(&mut self.program, program.clone());
        let previous = std::mem::take(&mut self.locals);
        let previous_defers = std::mem::take(&mut self.defers);
        self.locals
            .extend(function.params.iter().zip(args).map(|(p, v)| (p.id.0, v)));
        self.locals.extend(
            function
                .captures
                .iter()
                .zip(captures)
                .map(|(p, v)| (p.id.0, v.clone())),
        );
        let result = self.expression(&function.body);
        let result = self.finish(result);
        self.defers = previous_defers;
        self.locals = previous;
        self.program = previous_program;
        match result {
            Err(Failure::Return(value)) => Ok(value),
            Err(Failure::Break | Failure::Continue) => Err(fault("loop control outside a loop")),
            other => other,
        }
    }
    /// Match nested tags before binding their fields; failed guards restore the arm scope.
    fn matching(&mut self, value: &Value, arms: &[ir::MatchArm]) -> Eval<Value> {
        for arm in arms {
            let previous = self.locals.clone();
            let matched = self.pattern(&arm.pattern, value)?;
            let guard = if matched {
                arm.guard
                    .as_ref()
                    .map_or(Ok(Value::Bool(true)), |g| self.expression(g))
            } else {
                Ok(Value::Bool(false))
            };
            let result = match guard {
                Ok(Value::Bool(true)) => Some(self.expression(&arm.body)),
                Ok(_) => None,
                Err(e) => Some(Err(e)),
            };
            self.locals = previous;
            if let Some(result) = result {
                return result;
            }
        }
        Err(fault("nonexhaustive interactive match"))
    }
}

/// Evaluate scalar unary operators with Fern's numeric domains.
fn unary(op: ast::UnaryOp, value: Value) -> Eval<Value> {
    match (op, value) {
        (ast::UnaryOp::Negate, Value::Int(n)) => Ok(Value::Int(n.wrapping_neg())),
        (ast::UnaryOp::Negate, Value::Float(n)) => Ok(Value::Float(-n)),
        (ast::UnaryOp::Not, Value::Bool(v)) => Ok(Value::Bool(!v)),
        (ast::UnaryOp::BitNot, Value::Int(n)) => Ok(Value::Int(!n)),
        _ => Err(fault("invalid unary operands")),
    }
}
/// Evaluate scalar arithmetic; integer zero division is an interactive diagnostic.
fn binary(op: ast::BinaryOp, left: Value, right: Value) -> Eval<Value> {
    use ast::BinaryOp::*;
    if matches!(op, Eq | Ne) {
        let equal = left == right;
        return Ok(Value::Bool(if op == Eq { equal } else { !equal }));
    }
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Ok(match op {
            Add => Value::Int(a.wrapping_add(b)),
            Subtract => Value::Int(a.wrapping_sub(b)),
            Multiply => Value::Int(a.wrapping_mul(b)),
            Power => Value::Int(integer_power(a, b)?),
            BitAnd => Value::Int(a & b),
            BitOr => Value::Int(a | b),
            BitXor => Value::Int(a ^ b),
            ShiftLeft => Value::Int(a.wrapping_shl((b & 63) as u32)),
            ShiftRight => Value::Int(a.wrapping_shr((b & 63) as u32)),
            Divide | Remainder if b == 0 => return Err(fault("integer division by zero")),
            Divide => Value::Int(a.wrapping_div(b)),
            Remainder => Value::Int(a.wrapping_rem(b)),
            Lt => Value::Bool(a < b),
            Le => Value::Bool(a <= b),
            Gt => Value::Bool(a > b),
            Ge => Value::Bool(a >= b),
            _ => return Err(fault("invalid integer operation")),
        }),
        (Value::Float(a), Value::Float(b)) => Ok(match op {
            Add => Value::Float(a + b),
            Subtract => Value::Float(a - b),
            Multiply => Value::Float(a * b),
            Power => Value::Float(a.powf(b)),
            Divide => Value::Float(a / b),
            Lt => Value::Bool(a < b),
            Le => Value::Bool(a <= b),
            Gt => Value::Bool(a > b),
            Ge => Value::Bool(a >= b),
            _ => return Err(fault("invalid Float operation")),
        }),
        (Value::String(a), Value::String(b)) if op == Add => {
            builtins::concat_strings(a.as_str(), b.as_str())
        }
        (Value::Bool(a), Value::Bool(b)) => {
            Ok(Value::Bool(if op == And { a && b } else { a || b }))
        }
        _ => Err(fault("invalid binary operands")),
    }
}
/// Use at most 63 squaring steps for nonnegative full-width integer exponents.
fn integer_power(mut base: i64, exponent: i64) -> Eval<i64> {
    if exponent < 0 {
        return Err(fault("negative integer exponent"));
    }
    let mut power = exponent as u64;
    let mut result = 1i64;
    while power != 0 {
        if power & 1 != 0 {
            result = result.wrapping_mul(base);
        }
        power >>= 1;
        base = base.wrapping_mul(base);
    }
    Ok(result)
}
/// Render interactive values without exposing internal pointers.
fn display(value: &Value) -> String {
    match value {
        Value::Union(value) => display(&value.value),
        Value::Int(n) => n.to_string(),
        Value::Float(n) => float_text(*n),
        Value::Bool(v) => v.to_string(),
        Value::String(s) => format!("{s:?}"),
        Value::Unit => "()".into(),
        Value::Json(_) => "<json.Value>".into(),
        Value::JsonError(_) => "<json.Error>".into(),
        Value::Range(start, end, inclusive) => {
            format!("{start}..{}{end}", if *inclusive { "=" } else { "" })
        }
        Value::Closure(_) => "<function>".into(),
        Value::Map(_) => "<map>".into(),
        Value::List(values) => format!(
            "[{}]",
            values.iter().map(display).collect::<Vec<_>>().join(", ")
        ),
        Value::Sum(tag, fields) => format!(
            "variant {tag}({})",
            fields.iter().map(display).collect::<Vec<_>>().join(", ")
        ),
    }
}
/// Spell common semantic types for interactive results.
fn type_name(ty: &Type) -> String {
    match ty {
        Type::Union(members) => members
            .iter()
            .map(type_name)
            .collect::<Vec<_>>()
            .join(" | "),
        Type::Function(args, result) => format!(
            "({}) -> {}",
            args.iter().map(type_name).collect::<Vec<_>>().join(", "),
            type_name(result)
        ),
        Type::List(a) => format!("List({})", type_name(a)),
        Type::Map(key, value) => format!("Map({}, {})", type_name(key), type_name(value)),
        Type::Option(a) => format!("Option({})", type_name(a)),
        Type::Result(a, b) => format!("Result({}, {})", type_name(a), type_name(b)),
        Type::Tuple(args) => format!(
            "({}{})",
            args.iter().map(type_name).collect::<Vec<_>>().join(", "),
            if args.len() == 1 { "," } else { "" }
        ),
        Type::Named(name, args) => {
            if args.is_empty() {
                name.clone()
            } else {
                format!(
                    "{name}({})",
                    args.iter().map(type_name).collect::<Vec<_>>().join(", ")
                )
            }
        }
        Type::Native(native) => native.name().into(),
        _ => format!("{ty:?}"),
    }
}

#[path = "repl/builtins.rs"]
mod builtins;
#[path = "repl/control.rs"]
mod control;
#[path = "repl/functions.rs"]
mod functions;
#[path = "repl/iteration.rs"]
mod iteration;
mod json;
#[path = "repl/maps.rs"]
mod maps;
mod patterns;
#[path = "repl/storage.rs"]
mod storage;
#[path = "repl/with.rs"]
mod with;

/// Bound retained immutable graphs using unique Rc identities, including shared subtrees.
fn graph_budget<'a>(values: impl Iterator<Item = &'a Value>) -> Result<(), String> {
    let mut pending: Vec<_> = values.collect();
    let mut seen = std::collections::HashSet::new();
    let mut programs = std::collections::HashSet::new();
    let mut json = json::Storage::default();
    let mut bytes = 0usize;
    let mut count = 0usize;
    while let Some(value) = pending.pop() {
        count += 1;
        match value {
            Value::Union(value) => {
                if seen.insert(Rc::as_ptr(value) as usize) {
                    let (type_bytes, type_nodes) = storage::type_size(&value.member)?;
                    bytes = bytes.saturating_add(std::mem::size_of::<UnionValue>() + type_bytes);
                    count = count.saturating_add(type_nodes);
                    pending.push(&value.value);
                }
            }
            Value::Json(value) => json.add(value, &mut bytes, &mut count)?,
            Value::JsonError(error) => {
                if let Some(path) = &error.path
                    && seen.insert(Rc::as_ptr(path) as usize)
                {
                    bytes = bytes.saturating_add(path.capacity() + 40);
                }
            }
            Value::String(s) => {
                if seen.insert(Rc::as_ptr(s) as usize) {
                    bytes = bytes.saturating_add(s.len());
                }
            }
            Value::List(xs) | Value::Sum(_, xs) => {
                if seen.insert(Rc::as_ptr(xs) as usize) {
                    bytes = bytes.saturating_add(xs.len() * std::mem::size_of::<Value>());
                    pending.extend(xs.iter());
                }
            }
            Value::Map(entries) => {
                if seen.insert(Rc::as_ptr(entries) as usize) {
                    bytes =
                        bytes.saturating_add(entries.len() * std::mem::size_of::<(Value, Value)>());
                    pending.extend(entries.iter().flat_map(|(key, value)| [key, value]));
                }
            }
            Value::Closure(closure) => {
                if seen.insert(Rc::as_ptr(closure) as usize) {
                    bytes =
                        bytes.saturating_add(closure.captures.len() * std::mem::size_of::<Value>());
                    pending.extend(closure.captures.iter());
                }
                if programs.insert(Rc::as_ptr(&closure.program) as usize) {
                    let (code_bytes, code_nodes) = storage::program_size(&closure.program)?;
                    bytes = bytes.saturating_add(code_bytes);
                    count = count.saturating_add(code_nodes);
                }
            }
            _ => {}
        }
        if bytes > 16 * 1024 * 1024 || count > 200_000 {
            return Err("interactive value storage limit exceeded".into());
        }
    }
    Ok(())
}
/// Render semantic constructors and bounded previews of compound interactive values.
fn display_typed(
    value: &Value,
    ty: &Type,
    syntax: &ast::Program,
    layouts: &[ir::TypeLayout],
    budget: &mut usize,
) -> String {
    if *budget == 0 {
        return "…".into();
    }
    *budget -= 1;
    if let Some(layout) = layouts
        .iter()
        .find(|layout| layout.ty == *ty && layout.storage == ir::LayoutStorage::Unboxed)
        && let Type::Named(name, _) = ty
    {
        let constructor = syntax
            .newtypes
            .iter()
            .find(|d| d.name == *name)
            .map_or(name.as_str(), |d| d.constructor.as_str());
        return format!(
            "{constructor}({})",
            display_typed(value, &layout.variants[0][0], syntax, layouts, budget)
        );
    }
    match (value, ty) {
        (Value::Map(entries), Type::Map(key, value)) => {
            display_map(entries, key, value, syntax, layouts, budget)
        }
        (Value::Union(value), Type::Union(_)) => {
            display_typed(&value.value, &value.member, syntax, layouts, budget)
        }
        (Value::List(values), Type::List(ty)) => {
            let mut shown = values
                .iter()
                .take(64)
                .map(|v| display_typed(v, ty, syntax, layouts, budget))
                .collect::<Vec<_>>();
            if values.len() > 64 {
                shown.push("…".into());
            }
            format!("[{}]", shown.join(", "))
        }
        (Value::Sum(tag, fields), _) => {
            let (name, types) = constructor_types(*tag, ty, syntax, layouts);
            let shown = fields
                .iter()
                .zip(types)
                .take(64)
                .map(|(v, t)| display_typed(v, &t, syntax, layouts, budget))
                .collect::<Vec<_>>()
                .join(", ");
            if matches!(ty, Type::Tuple(_)) {
                format!("({shown}{})", if fields.len() == 1 { "," } else { "" })
            } else if fields.is_empty() {
                name
            } else {
                format!("{name}({shown})")
            }
        }
        _ => display(value),
    }
}
/// Render an ordered map with bounded entry and recursive-value previews.
fn display_map(
    entries: &[(Value, Value)],
    key: &Type,
    value: &Type,
    syntax: &ast::Program,
    layouts: &[ir::TypeLayout],
    budget: &mut usize,
) -> String {
    let mut shown = entries
        .iter()
        .take(64)
        .map(|(k, v)| {
            format!(
                "{}: {}",
                display_typed(k, key, syntax, layouts, budget),
                display_typed(v, value, syntax, layouts, budget)
            )
        })
        .collect::<Vec<_>>();
    if entries.len() > 64 {
        shown.push("…".into());
    }
    format!("%{{{}}}", shown.join(", "))
}
/// Pair original constructor spellings with finalized semantic fields, including expanded aliases.
fn constructor_types(
    tag: usize,
    ty: &Type,
    syntax: &ast::Program,
    layouts: &[ir::TypeLayout],
) -> (String, Vec<Type>) {
    match ty {
        Type::Option(a) => {
            if tag == 0 {
                ("Some".into(), vec![*a.clone()])
            } else {
                ("None".into(), vec![])
            }
        }
        Type::Result(a, b) => {
            if tag == 0 {
                ("Ok".into(), vec![*a.clone()])
            } else {
                ("Err".into(), vec![*b.clone()])
            }
        }
        Type::Tuple(types) => (String::new(), types.clone()),
        Type::Named(name, _) => {
            let variant = syntax
                .types
                .iter()
                .find(|d| &d.name == name)
                .and_then(|d| d.variants.get(tag));
            let fields = layouts
                .iter()
                .find(|layout| &layout.ty == ty)
                .and_then(|layout| layout.variants.get(tag));
            match (variant, fields) {
                (Some(variant), Some(fields)) => (variant.name.clone(), fields.clone()),
                _ => (name.clone(), vec![]),
            }
        }
        _ => (type_name(ty), vec![]),
    }
}

/// Match the native %.17g double rendering using bounded, safe Rust formatting.
fn float_text(value: f64) -> String {
    if value.is_nan() {
        return "nan".into();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-inf"
        } else {
            "inf"
        }
        .into();
    }
    let scientific = format!("{value:.16e}");
    let (mantissa, exponent) = scientific.split_once('e').unwrap();
    let exponent: i32 = exponent.parse().unwrap();
    if !(-4..17).contains(&exponent) {
        let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
        format!(
            "{mantissa}e{}{number:02}",
            if exponent < 0 { "-" } else { "+" },
            number = exponent.unsigned_abs()
        )
    } else {
        let places = (16 - exponent).max(0) as usize;
        let text = format!("{value:.places$}");
        if text.contains('.') {
            text.trim_end_matches('0').trim_end_matches('.').into()
        } else {
            text
        }
    }
}

/// Run bounded entries; paste mode submits an entire declaration group atomically.
/// Prompts appear only for a terminal, keeping piped sessions deterministic.
pub fn serve(
    mut input: impl std::io::BufRead,
    output: impl std::io::Write,
    interactive: bool,
) -> Result<(), String> {
    serve_lines(output, interactive, |ready, output| {
        if interactive {
            repl_prompt(output, ready)?;
        }
        let mut line = String::new();
        let read = std::io::Read::take(input.by_ref(), 1024 * 1024 + 1)
            .read_line(&mut line)
            .map_err(|error| error.to_string())?;
        Ok(if read == 0 {
            Input::End
        } else {
            Input::Line(line)
        })
    })
}

mod terminal;

/// Run the terminal editor while sharing evaluation and multiline state with piped sessions.
pub fn serve_terminal(quiet: bool) -> Result<(), String> {
    terminal::serve(quiet)
}

enum Input {
    Line(String),
    Interrupted,
    End,
}

/// One entry state machine serves both readline terminals and bounded buffered input.
fn serve_lines<W: std::io::Write>(
    mut output: W,
    banner: bool,
    mut read_line: impl FnMut(bool, &mut W) -> Result<Input, String>,
) -> Result<(), String> {
    let mut session = Session::default();
    let mut pending = String::new();
    let mut pasting = false;
    if banner {
        writeln!(
            output,
            "Fern interactive session. :help for commands; blank line submits a block."
        )
        .map_err(|e| e.to_string())?;
    }
    loop {
        output.flush().map_err(|error| error.to_string())?;
        let (line, read) = match read_line(pending.is_empty() && !pasting, &mut output)? {
            Input::Line(line) => (line, 1),
            Input::End => (String::new(), 0),
            Input::Interrupted => {
                pending.clear();
                pasting = false;
                continue;
            }
        };
        if line.len() + pending.len() > 1024 * 1024 {
            return Err("interactive input limit exceeded".into());
        }
        let entry = line.trim_end_matches(['\r', '\n']);
        if pasting {
            if read == 0 {
                writeln!(output, "error: unfinished paste; use :end to submit")
                    .map_err(|e| e.to_string())?;
                break;
            }
            if entry != ":end" {
                pending.push_str(entry);
                pending.push('\n');
                continue;
            }
            pasting = false;
        } else if pending.is_empty() && read != 0 && entry.starts_with(':') {
            if !repl_command(entry, &mut session, &mut pasting, &mut output)? {
                break;
            }
            continue;
        } else if !pending.is_empty() || parse::line_continues(entry) {
            if !entry.trim().is_empty() {
                pending.push_str(entry);
                pending.push('\n');
                continue;
            }
        } else {
            pending.push_str(entry);
        }
        if !pending.trim().is_empty() {
            match session.evaluate(&pending) {
                Ok(text) => write!(output, "{text}").map_err(|e| e.to_string())?,
                Err(message) => writeln!(output, "error: {message}").map_err(|e| e.to_string())?,
            }
        }
        pending.clear();
        if read == 0 {
            break;
        }
    }
    Ok(())
}

/// Handle commands only between entries so source text retains its ordinary meaning.
fn repl_command(
    entry: &str,
    session: &mut Session,
    pasting: &mut bool,
    output: &mut impl std::io::Write,
) -> Result<bool, String> {
    let (command, expression) = entry.split_once(char::is_whitespace).unwrap_or((entry, ""));
    if matches!(command, ":type" | ":t") {
        let result = if expression.trim().is_empty() {
            "usage: :type <expression>".into()
        } else {
            session
                .expression_type(expression.trim())
                .unwrap_or_else(|error| format!("error: {error}"))
        };
        writeln!(output, "{result}").map_err(|error| error.to_string())?;
        return Ok(true);
    }
    match entry {
        ":quit" | ":q" => return Ok(false),
        ":reset" => *session = Session::default(),
        ":paste" => *pasting = true,
        ":clear" => write!(output, "\x1b[H\x1b[2J").map_err(|error| error.to_string())?,
        ":help" | ":h" => writeln!(output, "Enter expressions, let bindings, or typed functions. Commands: :help (:h), :type (:t) <expression>, :clear, :reset, :quit (:q). Use :paste then :end to submit multiple function clauses together. Native-only APIs report a diagnostic here.").map_err(|e| e.to_string())?,
        _ => writeln!(output, "error: unknown interactive command").map_err(|e| e.to_string())?,
    }
    Ok(true)
}

/// Flush terminal prompts without changing piped-session output.
fn repl_prompt(output: &mut impl std::io::Write, ready: bool) -> Result<(), String> {
    let prompt = if ready { "fern> " } else { "...   " };
    write!(output, "{prompt}")
        .and_then(|()| output.flush())
        .map_err(|e| e.to_string())
}

/// Classify retained declarations using the same explicit source keywords as the parser.
fn declaration_source(source: &str) -> bool {
    [
        "fn ",
        "type ",
        "newtype ",
        "pub fn ",
        "pub type ",
        "pub newtype ",
        "@doc ",
    ]
    .iter()
    .any(|prefix| source.trim_start().starts_with(prefix))
}

impl Machine {
    /// Restore lexical bindings after either a block value or an abrupt control-flow exit.
    fn lexical_block(&mut self, statements: &[ir::Stmt]) -> Eval<Value> {
        let previous = self.locals.clone();
        let result = self.statements(statements);
        self.locals = previous;
        result
    }
}
