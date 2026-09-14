//! Source syntax produced by the bounded Fern parser.
use crate::{Constructor, Span, Type};

#[derive(Clone, Debug, Default)]
pub struct Program {
    pub traits: Vec<TraitDecl>,
    pub implementations: Vec<Implementation>,
    pub docs: Vec<DocComment>,
    pub functions: Vec<Function>,
    pub types: Vec<TypeDecl>,
    pub aliases: Vec<TypeAlias>,
    pub newtypes: Vec<NewtypeDecl>,
    pub module: Option<String>,
    pub imports: Vec<Import>,
    pub exports: Vec<String>,
}

/// Literal documentation associated with one immediately following declaration.
#[derive(Clone, Debug)]
pub struct DocComment {
    pub target: String,
    pub text: String,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Import {
    pub module: String,
    pub alias: Option<String>,
    pub items: Option<Vec<String>>,
    pub public: bool,
    pub span: Span,
}

/// A transparent source type name; it introduces no value constructor.
#[derive(Clone, Debug)]
pub struct TypeAlias {
    /// Source visibility belongs to this type declaration, independently of same-named values.
    pub public: bool,
    pub name: String,
    pub parameters: Vec<String>,
    pub target: Type,
    pub span: Span,
}

/// A distinct one-payload nominal type whose constructor adds no runtime allocation.
#[derive(Clone, Debug)]
pub struct NewtypeDecl {
    pub derives: Vec<Derivation>,
    /// Source visibility belongs to this type declaration, independently of same-named values.
    pub public: bool,
    pub name: String,
    pub parameters: Vec<String>,
    pub constructor: String,
    pub inner: Type,
    pub span: Span,
    pub constructor_span: Span,
    pub inner_span: Span,
}

/// An explicit source trait derivation; unsupported traits remain located diagnostics.
#[derive(Clone, Debug)]
pub struct Derivation {
    pub name: String,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct TypeDecl {
    pub derives: Vec<Derivation>,
    /// Source visibility belongs to this type declaration, independently of same-named values.
    pub public: bool,
    pub name: String,
    pub parameters: Vec<String>,
    pub variants: Vec<Variant>,
    pub record: bool,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Variant {
    pub name: String,
    pub fields: Vec<Field>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Field {
    pub name: Option<String>,
    pub ty: Type,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub struct Function {
    pub constraints: Vec<TraitBound>,
    pub guard: Option<Expr>,
    pub group_start: usize,
    pub syntax: FunctionSyntax,
    /// Original declaration visibility, retained when module exports are flattened.
    pub public: bool,
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<Type>,
    pub body: Expr,
    pub span: Span,
}
/// Preserve whether an arrow introduces a result type or the function body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FunctionSyntax {
    /// Trusted C declaration wrapper; its synthesized body retains the exact foreign ABI.
    Foreign,
    /// An abstract trait method; resolved statically before executable IR is emitted.
    Trait,
    /// A zero-argument compile-time value, referenced without call parentheses.
    Constant,
    Colon,
    Arrow,
}

/// A single-parameter static trait contract and its method declaration identities.
#[derive(Clone, Debug)]
pub struct TraitDecl {
    pub public: bool,
    pub name: String,
    pub parameter: String,
    pub parents: Vec<TraitBound>,
    pub methods: Vec<TraitMethod>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct TraitMethod {
    pub name: String,
    pub function: String,
    pub default: Option<String>,
}

#[derive(Clone, Debug)]
pub struct TraitBound {
    pub name: String,
    pub ty: Type,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Implementation {
    pub bound: TraitBound,
    pub constraints: Vec<TraitBound>,
    pub methods: Vec<(String, String)>,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub struct Param {
    /// Optional explicit external label; a simple binder supplies its default label.
    pub label: Option<ArgumentLabel>,
    pub pattern: Pattern,
    pub annotation: Option<Type>,
    pub span: Span,
}
/// Anonymous parameter with optional context-inferred type annotation.
#[derive(Clone, Debug)]
pub struct LambdaParam {
    pub name: String,
    pub annotation: Option<Type>,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}
#[derive(Clone, Debug)]
pub enum ExprKind {
    /// Only foreign declarations synthesize this native-only operation.
    ForeignCall {
        declaration: crate::ffi::Declaration,
        args: Vec<Expr>,
    },
    Receive {
        arms: Vec<MatchArm>,
        timeout: Option<(Box<Expr>, Box<Expr>)>,
    },
    /// A compile-time decoder target, legal only in a canonically resolved static argument slot.
    TypeTarget(Type),
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Interpolate(Vec<StringPart>),
    MultilineString(Vec<StringPart>),
    Name(String),
    /// Source spelling plus a loader-resolved declaration identity, independent of local roots.
    GlobalName {
        name: String,
        resolved: String,
    },
    GlobalCall {
        name: String,
        resolved: String,
        args: Vec<Argument>,
    },
    GlobalPipe {
        value: Box<Expr>,
        name: String,
        resolved: String,
        args: Vec<Argument>,
        position: usize,
        label: Option<ArgumentLabel>,
    },
    Unit,
    List(Vec<Expr>),
    Map(Vec<(Expr, Expr)>),
    RecordUpdate {
        value: Box<Expr>,
        fields: Vec<RecordField>,
    },
    Tuple(Vec<Expr>),
    Try(Box<Expr>),
    Return(Box<Expr>),
    Defer(Box<Expr>),
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
        bindings: Vec<WithBinding>,
        body: Box<Expr>,
        arms: Option<Vec<MatchArm>>,
    },
    PostfixIf {
        value: Box<Expr>,
        condition: Box<Expr>,
    },
    ConditionMatch(Vec<ConditionArm>),
    Pipe {
        value: Box<Expr>,
        name: String,
        args: Vec<Argument>,
        position: usize,
        label: Option<ArgumentLabel>,
    },
    Field {
        value: Box<Expr>,
        name: String,
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
    Lambda {
        params: Vec<LambdaParam>,
        body: Box<Expr>,
    },
    Apply {
        callee: Box<Expr>,
        args: Vec<Argument>,
    },
    Call {
        name: String,
        args: Vec<Argument>,
    },
    If {
        condition: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Option<Box<Expr>>,
    },
    Block(Vec<Stmt>),
}

/// One source argument label, independent of variable or module name resolution.
#[derive(Clone, Debug)]
pub struct ArgumentLabel {
    pub name: String,
    pub span: Span,
}

/// Preserve the written order and complete source anchor of one call argument.
#[derive(Clone, Debug)]
pub struct Argument {
    pub label: Option<ArgumentLabel>,
    pub value: Expr,
    pub span: Span,
}

impl Argument {
    /// Wrap an existing expression as an unlabeled argument without copying it.
    pub fn positional(value: Expr) -> Self {
        Self {
            span: value.span,
            value,
            label: None,
        }
    }
}
impl std::ops::Deref for Argument {
    type Target = Expr;
    /// Borrow the value for expression-only visitors; label-aware consumers inspect metadata explicitly.
    fn deref(&self) -> &Expr {
        &self.value
    }
}
impl std::ops::DerefMut for Argument {
    /// Mutate a value without replacing its source argument label or written position.
    fn deref_mut(&mut self) -> &mut Expr {
        &mut self.value
    }
}
impl From<Expr> for Argument {
    /// Construct an explicitly positional argument from an expression.
    fn from(value: Expr) -> Self {
        Self::positional(value)
    }
}

/// One immutable record replacement, retained in source evaluation order.
#[derive(Clone, Debug)]
pub struct RecordField {
    pub name: String,
    pub value: Expr,
    pub span: Span,
}

/// Preserve literal segments separately from embedded expressions for formatting.
#[derive(Clone, Debug)]
pub enum StringPart {
    Text(String),
    Value(Expr),
}

#[derive(Clone, Debug)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub guard: Option<Expr>,
    pub body: Expr,
    pub span: Span,
}

/// A condition branch retains its expression; None denotes the final wildcard arm.
#[derive(Clone, Debug)]
pub struct ConditionArm {
    pub condition: Option<Expr>,
    pub body: Expr,
    pub span: Span,
}

/// One sequential Result binding; later bindings can use earlier successful payloads.
#[derive(Clone, Debug)]
pub struct WithBinding {
    pub pattern: Pattern,
    pub value: Expr,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Pattern {
    pub kind: PatternKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum PatternKind {
    Typed {
        pattern: Box<Pattern>,
        annotation: Type,
    },
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
    Bind(String),
    Int(i64),
    Bool(bool),
    String(String),
    NamedConstructor {
        name: String,
        fields: Vec<Pattern>,
    },
    Constructor {
        constructor: Constructor,
        binding: Option<String>,
    },
}
#[derive(Clone, Debug)]
pub enum Stmt {
    LetElse {
        pattern: Pattern,
        annotation: Option<Type>,
        value: Expr,
        else_branch: Expr,
        span: Span,
    },
    LetPattern {
        pattern: Pattern,
        annotation: Option<Type>,
        value: Expr,
        span: Span,
    },
    Let {
        name: String,
        annotation: Option<Type>,
        value: Expr,
        span: Span,
    },
    Expr(Expr),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    BitNot,
    Negate,
    Not,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOp {
    In,
    Power,
    BitAnd,
    BitOr,
    BitXor,
    ShiftLeft,
    ShiftRight,
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}
