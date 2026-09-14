//! Checked expressions retain semantic types and resolved identities for lowering.
mod codec_templates;
use crate::{
    Constructor, Span, Type,
    ast::{BinaryOp, UnaryOp},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalId(pub usize);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FunctionId(pub usize);
#[derive(Clone, Debug, Default)]
pub struct Program {
    pub functions: Vec<Function>,
    pub types: Vec<TypeLayout>,
}

/// Physical storage is explicit; an unboxed nominal layout contains exactly one payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutStorage {
    Tagged,
    Unboxed,
}

#[derive(Clone, Debug)]
pub struct TypeLayout {
    pub storage: LayoutStorage,
    pub ty: Type,
    pub variants: Vec<Vec<Type>>,
    /// Original unqualified source constructor names; empty for records and newtypes.
    pub variant_names: Vec<String>,
    pub fields: Vec<String>,
}
#[derive(Clone, Debug)]
pub struct Function {
    pub mailbox: Option<Type>,
    pub id: FunctionId,
    pub name: String,
    pub params: Vec<Param>,
    pub captures: Vec<Param>,
    pub return_type: Type,
    pub body: Expr,
    pub local_count: usize,
}
#[derive(Clone, Debug)]
pub struct Param {
    pub id: LocalId,
    pub ty: Type,
}
#[derive(Clone, Debug)]
pub struct Capture {
    pub param: Param,
    pub value: Expr,
}
#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub ty: Type,
    pub span: Span,
}
/// Opaque identity for a compiler-only inference obligation; never executable IR.
/// External callers cannot construct provisional computation tokens.
/// ```compile_fail
/// let token = fern_compiler::ir::ProbeToken::new(0);
/// ```
#[derive(Clone, Debug)]
pub struct ProbeToken(usize);
impl ProbeToken {
    /// Only compiler passes can mint provisional expression identities.
    pub(crate) fn new(id: usize) -> Self {
        Self(id)
    }
    /// Internal validators can associate a probe with its bounded obligation slot.
    pub(crate) fn id(&self) -> usize {
        self.0
    }
}

/// An opaque editor-only computation identity; safe external code cannot construct it.
/// ```compile_fail
/// let token = fern_compiler::ir::EditorHoleToken::new();
/// ```
#[derive(Clone, Debug)]
pub struct EditorHoleToken(());
impl EditorHoleToken {
    /// Only editor proof checking can mint an incomplete, non-executable operation.
    pub(crate) fn new() -> Self {
        Self(())
    }
}

/// Unforgeable identity for a discarded generic codec proof, never executable IR.
/// ```compile_fail
/// let token = fern_compiler::ir::CodecTemplateToken::new();
/// ```
#[derive(Clone, Debug)]
pub struct CodecTemplateToken(());
impl CodecTemplateToken {
    pub(crate) fn new() -> Self {
        Self(())
    }
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    /// Checked native-only C call; pointer values are sealed Fern handles until native lowering.
    ForeignCall {
        declaration: crate::ffi::Declaration,
        args: Vec<Expr>,
    },
    Actor(ActorExpr),
    JsonCodecTemplate {
        direction: crate::json_codec::Direction,
        input: Box<Expr>,
        target: Type,
        token: CodecTemplateToken,
    },
    /// Concrete codec with exactly one executable input; static targets never enter IR.
    JsonCodec {
        direction: crate::json_codec::Direction,
        input: Box<Expr>,
        plan: std::rc::Rc<crate::json_codec::Plan>,
    },
    UnionInject {
        value: Box<Expr>,
    },
    UnionWiden {
        value: Box<Expr>,
    },
    Wrap(Box<Expr>),
    Unwrap(Box<Expr>),
    /// Incomplete member result used only in isolated editor proof; never executable.
    EditorHole {
        token: EditorHoleToken,
        receiver: Box<Expr>,
    },
    /// Inference-only computation; finalization and executable boundaries must reject it.
    Probe {
        token: ProbeToken,
        children: Vec<Expr>,
        bindings: Vec<Param>,
    },
    Break,
    Continue,
    Range {
        start: Box<Expr>,
        end: Box<Expr>,
        inclusive: bool,
    },
    For {
        pattern: Pattern,
        iterable: Box<Expr>,
        body: Box<Expr>,
    },
    With {
        steps: Vec<WithStep>,
        body: Box<Expr>,
        handlers: Vec<WithHandler>,
    },
    Return(Box<Expr>),
    Defer(Box<Expr>),
    /// Temporary checked lambda; eliminated by specialization/lifting before lowering.
    Lambda {
        params: Vec<Param>,
        captures: Vec<Capture>,
        body: Box<Expr>,
        local_count: usize,
    },
    /// Temporary callable reference; its complete function type determines specialization.
    FunctionValue {
        target: CallTarget,
    },
    Closure {
        function: FunctionId,
        captures: Vec<Expr>,
    },
    Invoke {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Interpolate(Vec<Expr>),
    Local(LocalId),
    Unit,
    List(Vec<Expr>),
    Map(Vec<(Expr, Expr)>),
    Tuple(Vec<Expr>),
    Try(Box<Expr>),
    CustomConstruct {
        tag: usize,
        fields: Vec<Expr>,
    },
    Field {
        value: Box<Expr>,
        index: usize,
    },
    Construct {
        constructor: Constructor,
        value: Option<Box<Expr>>,
    },
    Match {
        value: Box<Expr>,
        arms: Vec<MatchArm>,
    },
    Unary {
        op: UnaryOp,
        value: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Call {
        target: CallTarget,
        args: Vec<Expr>,
    },
    If {
        condition: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Option<Box<Expr>>,
    },
    Block(Vec<Stmt>),
}

#[derive(Clone, Debug)]
pub struct WithStep {
    pub pattern: Pattern,
    pub value: Expr,
    pub error_handler: Option<usize>,
}
#[derive(Clone, Debug)]
pub struct WithHandler {
    pub error: Param,
    pub body: Expr,
}

#[derive(Clone, Debug)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub guard: Option<Expr>,
    pub body: Expr,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum Pattern {
    UnionSelect {
        narrowed: Type,
        binding: Option<Param>,
    },
    Newtype(Box<Pattern>),
    Tuple(Vec<Pattern>),
    List {
        prefix: Vec<Pattern>,
        rest: Option<Box<Pattern>>,
    },
    TupleRest {
        prefix: Vec<Pattern>,
        rest: Box<Pattern>,
    },
    Wildcard,
    Bind(LocalId),
    Int(i64),
    Bool(bool),
    String(String),
    Variant {
        tag: usize,
        fields: Vec<Pattern>,
    },
    Constructor {
        constructor: Constructor,
        binding: Option<LocalId>,
    },
}
// Keep the public IR shape; boxing this field needs a separate allocation/layout audit.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug)]
pub enum Stmt {
    LetElse {
        pattern: Pattern,
        value: Expr,
        else_branch: Expr,
    },
    Let {
        id: LocalId,
        value: Expr,
    },
    Expr(Expr),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallTarget {
    Function(FunctionId),
    Builtin(Builtin),
    Runtime(usize),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Builtin {
    ListEnumerate,
    MapNew,
    MapGet,
    MapPut,
    MapDelete,
    MapLen,
    MapIsEmpty,
    MapContains,
    MapKeys,
    MapValues,
    ListMap,
    ListFold,
    ListFilter,
    ListFind,
    ListAny,
    ListAll,
    OptionMap,
    ResultMap,
    ResultAndThen,
    ResultUnwrapOrElse,
    Print,
    Println,
    StringConcat,
    StringEq,
    StringLen,
    ListLen,
    ListGet,
    ListHead,
    ListTail,
    ListIsEmpty,
    ListPush,
    ListReverse,
    ListConcat,
    ListContains,
    OptionIsSome,
    OptionIsNone,
    OptionUnwrapOr,
    ResultIsOk,
    ResultIsErr,
    ResultUnwrapOr,
}

/// List child expressions, including guards, without traversing type layouts.
pub(crate) fn children(expr: &Expr) -> Vec<&Expr> {
    match &expr.kind {
        ExprKind::Actor(actor) => crate::actors::children(actor),
        ExprKind::Probe { children, .. } => children.iter().collect(),
        ExprKind::EditorHole { receiver, .. } => vec![receiver],
        ExprKind::Range { start, end, .. } => vec![start, end],
        ExprKind::For { iterable, body, .. } => vec![iterable, body],
        ExprKind::With {
            steps,
            body,
            handlers,
        } => steps
            .iter()
            .map(|s| &s.value)
            .chain(std::iter::once(body.as_ref()))
            .chain(handlers.iter().map(|h| &h.body))
            .collect(),
        ExprKind::Lambda { captures, body, .. } => captures
            .iter()
            .map(|c| &c.value)
            .chain(std::iter::once(body.as_ref()))
            .collect(),
        ExprKind::Map(entries) => entries.iter().flat_map(|(k, v)| [k, v]).collect(),
        ExprKind::Closure { captures, .. } => captures.iter().collect(),
        ExprKind::Invoke { callee, args } => std::iter::once(callee.as_ref())
            .chain(args.iter())
            .collect(),
        ExprKind::JsonCodecTemplate { input: value, .. }
        | ExprKind::JsonCodec { input: value, .. }
        | ExprKind::UnionInject { value }
        | ExprKind::UnionWiden { value }
        | ExprKind::Wrap(value)
        | ExprKind::Unwrap(value)
        | ExprKind::Return(value)
        | ExprKind::Defer(value)
        | ExprKind::Unary { value, .. }
        | ExprKind::Try(value)
        | ExprKind::Field { value, .. } => vec![value],
        ExprKind::Binary { left, right, .. } => vec![left, right],
        ExprKind::ForeignCall { args, .. }
        | ExprKind::Call { args, .. }
        | ExprKind::Interpolate(args)
        | ExprKind::Tuple(args)
        | ExprKind::List(args)
        | ExprKind::CustomConstruct { fields: args, .. } => args.iter().collect(),
        ExprKind::Construct {
            value: Some(value), ..
        } => vec![value],
        ExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            let mut values = vec![condition.as_ref(), then_branch.as_ref()];
            values.extend(else_branch.as_deref());
            values
        }
        ExprKind::Match { value, arms } => {
            let mut values = vec![value.as_ref()];
            for arm in arms {
                values.extend(arm.guard.as_ref());
                values.push(&arm.body);
            }
            values
        }
        ExprKind::Block(stmts) => stmts.iter().flat_map(statement_children).collect(),
        _ => Vec::new(),
    }
}

/// Reject inference probes and editor holes, including syntactically unreachable expressions.
pub(crate) fn reject_probes(program: &Program) -> Result<(), crate::Diagnostic> {
    reject_incomplete(program, false, &mut 0)
}

/// Permit only genuine codec template tokens for a private generic effect proof.
pub(crate) fn validate_codec_templates(
    program: &Program,
    work: &mut usize,
) -> Result<(), crate::Diagnostic> {
    reject_incomplete(program, true, work)
}

fn reject_incomplete(
    program: &Program,
    templates: bool,
    type_work: &mut usize,
) -> Result<(), crate::Diagnostic> {
    let mut count = 0;
    for function in &program.functions {
        let mut pending = vec![(&function.body, 0)];
        while let Some((expr, depth)) = pending.pop() {
            count += 1;
            if count > 100_000 || depth >= 128 {
                return Err(crate::Diagnostic::new(
                    expr.span,
                    "typed IR publication complexity limit exceeded",
                ));
            }
            if !templates && matches!(expr.kind, ExprKind::JsonCodecTemplate { .. }) {
                return Err(crate::Diagnostic::new(
                    expr.span,
                    "JSON codec template cannot enter executable IR",
                ));
            }
            if templates {
                codec_templates::validate(expr, type_work)?;
            }
            if matches!(expr.kind, ExprKind::EditorHole { .. }) {
                return Err(crate::Diagnostic::new(
                    expr.span,
                    "editor hole cannot enter executable IR",
                ));
            }
            if let ExprKind::Probe { token, .. } = &expr.kind {
                let _identity = token.id();
                return Err(crate::Diagnostic::new(
                    expr.span,
                    "inference probe cannot enter executable IR",
                ));
            }
            pending.extend(children(expr).into_iter().map(|child| (child, depth + 1)));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;

/// Keep let-else failure branches visible in shared bounded expression traversal.
fn statement_children(stmt: &Stmt) -> Vec<&Expr> {
    match stmt {
        Stmt::LetElse {
            value, else_branch, ..
        } => vec![value, else_branch],
        Stmt::Let { value, .. } | Stmt::Expr(value) => vec![value],
    }
}

/// Actor operations retain semantic mailbox identity through concrete specialization.
#[derive(Clone, Debug)]
pub enum ActorExpr {
    Lowered(crate::actors::Lowered),
    Spawn {
        entry: Box<Expr>,
        mailbox: Type,
        max_restarts: Option<Box<Expr>>,
    },
    SupervisedCurrent {
        pid: Box<Expr>,
    },
    Send {
        pid: Box<Expr>,
        message: Box<Expr>,
    },
    Receive {
        mailbox: Type,
        arms: Vec<MatchArm>,
        timeout: Option<(Box<Expr>, Box<Expr>)>,
    },
    Call {
        function: FunctionId,
        args: Vec<Expr>,
        mailbox: Type,
    },
}
