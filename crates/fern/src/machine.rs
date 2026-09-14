//! Bounded typed native operations shared by QBE and direct object generation.
//! Names are compiler-owned identities; no instruction text is parsed here.
#[path = "machine/dominance.rs"]
mod dominance;
#[path = "machine/render.rs"]
mod render;
#[path = "machine/types.rs"]
mod types;
#[path = "machine/validate.rs"]
mod validate;

/// Supported native scalar register representations on 64-bit hosts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Scalar {
    I32,
    I64,
    F64,
}
impl Scalar {
    /// Return the corresponding QBE width spelling.
    pub fn qbe(self) -> char {
        match self {
            Self::I32 => 'w',
            Self::I64 => 'l',
            Self::F64 => 'd',
        }
    }
    /// Convert a compiler-selected width, rejecting unsupported representations.
    pub fn from_width(width: char) -> Self {
        match width {
            'w' => Self::I32,
            'l' => Self::I64,
            'd' => Self::F64,
            _ => unreachable!("unsupported compiler-owned machine scalar width"),
        }
    }
}

/// A scalar constant, local SSA identity or relocatable symbol address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Operand {
    Temp(String),
    Symbol(String),
    Int(i64),
    Float(u64),
}
impl Operand {
    /// Convert one compiler-owned value token; this never accepts instructions.
    pub fn named(value: &str) -> Self {
        if let Some(name) = value.strip_prefix('%') {
            Self::Temp(name.into())
        } else if let Some(name) = value.strip_prefix('$') {
            Self::Symbol(name.into())
        } else if let Some(value) = value.strip_prefix("d_") {
            Self::Float(
                value
                    .parse::<f64>()
                    .expect("compiler-owned float constant")
                    .to_bits(),
            )
        } else {
            Self::Int(value.parse().expect("compiler-owned integer constant"))
        }
    }
}

/// Representation conversions and single-operand arithmetic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    Copy,
    Neg,
    Cast,
    ExtUw,
    ExtSw,
}
/// Integer signedness is explicit; floating comparisons use the accompanying F64 type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Comparison {
    Eq,
    Ne,
    SLt,
    SLe,
    SGt,
    SGe,
    ULt,
    ULe,
    UGt,
    UGe,
}
/// Arithmetic operations never infer a source-language operation from text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    UDiv,
    Rem,
    And,
    Or,
    Xor,
    Shl,
    Shr,
    Sar,
    Compare(Comparison, Scalar),
}
/// Memory access width and extension semantics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadKind {
    I64,
    I32,
    I8Unsigned,
    I8Signed,
    F64,
}
/// A typed instruction; call result consumption is separate from the callee ABI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Operation {
    Unary(UnaryOp, Operand),
    Binary(BinaryOp, Operand, Operand),
    Load(LoadKind, Operand),
    StackAlloc {
        bytes: u32,
        align: u32,
    },
    Call {
        callee: Operand,
        args: Vec<(Scalar, Operand)>,
        variadic: Option<usize>,
    },
    /// A trusted foreign call with exact physical C widths independent of Fern registers.
    ForeignCall {
        declaration: crate::ffi::Declaration,
        args: Vec<(Scalar, Operand)>,
    },
    Phi(Vec<(String, Operand)>),
}
/// Control-flow and effect stream within one function.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Statement {
    Label(String),
    Assign {
        destination: String,
        ty: Scalar,
        operation: Operation,
    },
    Effect(Operation),
    Store {
        kind: LoadKind,
        value: Operand,
        address: Operand,
    },
    Jump(String),
    Branch {
        condition: Operand,
        then_label: String,
        else_label: String,
    },
    Return(Option<Operand>),
    Trap,
}
/// Static bytes, padding or a full-width integer/address relocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DataValue {
    Bytes(Vec<u8>),
    Zero(u32),
    Word(Operand),
}
/// Immutable compiler-owned data with native symbol identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Data {
    pub name: String,
    pub values: Vec<DataValue>,
}
/// Fully lowered native function with explicit physical parameters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Function {
    pub name: String,
    pub result: Option<Scalar>,
    pub params: Vec<(Scalar, String)>,
    pub export: bool,
    pub body: Vec<Statement>,
}
/// Validatable lowered module; both backends consume these identical values.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Program {
    pub data: Vec<Data>,
    pub functions: Vec<Function>,
}
impl Program {
    /// Check bounded identities, control flow and operand references before code generation.
    pub fn validate(&self) -> Result<(), String> {
        validate::program(self)
    }
    /// Render deterministic QBE text from typed operations, without source-language decisions.
    pub fn to_text(&self) -> String {
        render::program(self)
    }
}
/// A lowering event, permitting fixed stack storage to be inserted at the entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Begin {
        name: String,
        result: Option<Scalar>,
        params: Vec<(Scalar, String)>,
        export: bool,
    },
    End,
    Statement(Statement),
    Data(Data),
}
/// Typed recording stream used while semantic lowering discovers helpers and stack slots.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Buffer {
    pub items: Vec<Item>,
}
impl Buffer {
    /// Start an empty typed recording stream.
    pub fn new() -> Self {
        Self::default()
    }
    /// Append one already structured lowering event.
    pub fn push(&mut self, item: Item) {
        self.items.push(item);
    }
    /// Append an operation or control event to the current function.
    pub fn statement(&mut self, statement: Statement) {
        self.push(Item::Statement(statement));
    }
    /// Start a function with a complete physical signature.
    pub fn begin(
        &mut self,
        name: &str,
        result: Option<Scalar>,
        params: Vec<(Scalar, String)>,
        export: bool,
    ) {
        self.push(Item::Begin {
            name: name.into(),
            result,
            params,
            export,
        });
    }
    /// Finish the current function recording.
    pub fn end(&mut self) {
        self.push(Item::End);
    }
    /// Record an immutable data definition, including relocation operands.
    pub fn data(&mut self, name: &str, values: Vec<DataValue>) {
        self.push(Item::Data(Data {
            name: name.into(),
            values,
        }));
    }
    /// Append another typed stream without interpreting its serialized form.
    pub fn extend(&mut self, other: &Self) {
        self.items.extend_from_slice(&other.items);
    }
    /// Return the insertion cursor in typed events, never byte offsets.
    pub fn len(&self) -> usize {
        self.items.len()
    }
    /// Return whether the recording stream is empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    /// Hoist typed entry allocations before a saved event cursor.
    pub fn insert(&mut self, index: usize, other: &Self) {
        self.items.splice(index..index, other.items.iter().cloned());
    }
    /// Assemble and validate complete function/data definitions.
    pub fn finish(&self) -> Result<Program, String> {
        let mut program = Program::default();
        let mut active: Option<Function> = None;
        for item in &self.items {
            match item {
                Item::Begin {
                    name,
                    result,
                    params,
                    export,
                } => {
                    if active.is_some() {
                        return Err("nested machine function".into());
                    }
                    active = Some(Function {
                        name: name.clone(),
                        result: *result,
                        params: params.clone(),
                        export: *export,
                        body: vec![],
                    });
                }
                Item::End => program
                    .functions
                    .push(active.take().ok_or("machine function end without begin")?),
                Item::Statement(statement) => active
                    .as_mut()
                    .ok_or("machine statement outside function")?
                    .body
                    .push(statement.clone()),
                Item::Data(data) => program.data.push(data.clone()),
            }
        }
        if active.is_some() {
            return Err("unterminated machine function".into());
        }
        program.validate()?;
        Ok(program)
    }
    /// Render complete typed events; callers needing diagnostics should call finish first.
    pub fn render(&self) -> String {
        self.finish()
            .expect("validated machine recording")
            .to_text()
    }
    /// Inspect structured direct calls when selecting optional compiler helpers.
    pub fn calls(&self, name: &str) -> bool {
        self.items.iter().any(|item| matches!(item,
            Item::Statement(Statement::Assign { operation: Operation::Call { callee:Operand::Symbol(symbol),.. },.. }
            | Statement::Effect(Operation::Call { callee:Operand::Symbol(symbol),.. })) if bare(symbol)==bare(name)))
    }
}
/// Strip an optional QBE display sigil without changing identity text.
pub fn bare(name: &str) -> &str {
    name.strip_prefix(['%', '$', '@']).unwrap_or(name)
}

#[cfg(test)]
#[path = "machine/tests.rs"]
mod tests;
