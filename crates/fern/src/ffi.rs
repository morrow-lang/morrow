//! Explicit trusted C declarations retain physical ABI separately from Fern values.
use crate::{Diagnostic, Span, Type};

/// Supported C scalar/pointer transports on Fern's 64-bit native targets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AbiType {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    Bool,
    Void,
    /// The nominal phantom argument describes the pointee; values remain opaque.
    Pointer(Type),
}
/// One explicit foreign symbol, including all ABI information required by native emission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Declaration {
    pub symbol: String,
    /// A logical linker library name, never a path, shell command or linker option.
    pub library: Option<String>,
    pub params: Vec<AbiType>,
    pub result: AbiType,
}
impl AbiType {
    /// Semantic value transported by this ABI; narrow scalars are distinct checked newtypes.
    pub fn source_type(&self) -> Type {
        match self {
            Self::I64 => Type::Int,
            Self::F64 => Type::Float,
            Self::Bool => Type::Bool,
            Self::Void => Type::Unit,
            Self::Pointer(item) => Type::Named("Ptr".into(), vec![item.clone()]),
            Self::I8 => named("CInt8"),
            Self::I16 => named("CInt16"),
            Self::I32 => named("CInt32"),
            Self::U8 => named("CUInt8"),
            Self::U16 => named("CUInt16"),
            Self::U32 => named("CUInt32"),
            Self::U64 => named("CUInt64"),
            Self::F32 => named("CFloat32"),
        }
    }
    /// Physical pointer values are passed as full-width addresses after checked handle extraction.
    pub fn machine_scalar(&self) -> Option<crate::machine::Scalar> {
        use crate::machine::Scalar;
        match self {
            Self::Void => None,
            Self::F32 | Self::F64 => Some(Scalar::F64),
            Self::Bool => Some(Scalar::I32),
            _ => Some(Scalar::I64),
        }
    }
}
fn named(name: &str) -> Type {
    Type::Named(name.into(), vec![])
}
impl Declaration {
    /// Reject malformed or compiler-owned symbols and unbounded signatures before code generation.
    pub fn validate(&self, span: Span) -> Result<(), Diagnostic> {
        let identifier = |s: &str| {
            !s.is_empty()
                && s.len() <= 255
                && s.bytes()
                    .next()
                    .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
                && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        };
        if !identifier(&self.symbol) || self.symbol.starts_with("fern_") || self.params.len() > 64 {
            return Err(Diagnostic::new(
                span,
                "foreign symbol must be a bounded non-Fern C identifier (at most 64 arguments)",
            ));
        }
        if self.library.as_ref().is_some_and(|s| !identifier(s)) {
            return Err(Diagnostic::new(
                span,
                "foreign library must be a logical ASCII name, not a path or linker option",
            ));
        }
        let mut pending = Vec::new();
        for ty in self.params.iter().chain([&self.result]) {
            if let AbiType::Pointer(item) = ty {
                pending.push((item, 0usize));
            }
        }
        let mut nodes = 0usize;
        while let Some((ty, depth)) = pending.pop() {
            nodes += 1;
            if nodes > 4096 || depth >= 128 {
                return Err(Diagnostic::new(
                    span,
                    "foreign pointee type nesting or size limit exceeded",
                ));
            }
            match ty {
                Type::Named(name, children) => {
                    if name.len() > 255 {
                        return Err(Diagnostic::new(span, "foreign pointee name exceeds limit"));
                    }
                    pending.extend(children.iter().map(|ty| (ty, depth + 1)));
                }
                Type::Union(children) | Type::Tuple(children) => {
                    pending.extend(children.iter().map(|ty| (ty, depth + 1)))
                }
                Type::Function(children, result) => {
                    pending.extend(children.iter().map(|ty| (ty, depth + 1)));
                    pending.push((result, depth + 1));
                }
                Type::Pid(inner) | Type::List(inner) | Type::Option(inner) => {
                    pending.push((inner, depth + 1))
                }
                Type::ActorFunction(a, b) | Type::Map(a, b) | Type::Result(a, b) => {
                    pending.extend([(a.as_ref(), depth + 1), (b.as_ref(), depth + 1)])
                }
                Type::Generic(_) | Type::Infer(_) => {
                    return Err(Diagnostic::new(
                        span,
                        "foreign pointer types must be concrete",
                    ));
                }
                _ => {}
            }
        }
        if self.params.contains(&AbiType::Void) {
            return Err(Diagnostic::new(
                span,
                "foreign Unit/void is only valid as a return type",
            ));
        }
        Ok(())
    }
}

/// Interpret only explicitly supported source ABI types; ordinary aggregates never cross C calls.
pub fn abi_type(ty: &Type, span: Span) -> Result<AbiType, Diagnostic> {
    Ok(match ty {
        Type::Int => AbiType::I64,
        Type::Float => AbiType::F64,
        Type::Bool => AbiType::Bool,
        Type::Unit => AbiType::Void,
        Type::Named(name, args) if name == "Ptr" && args.len() == 1 => {
            AbiType::Pointer(args[0].clone())
        }
        Type::Named(name, args) if args.is_empty() => match name.as_str() {
            "CInt8" => AbiType::I8,
            "CInt16" => AbiType::I16,
            "CInt32" => AbiType::I32,
            "CUInt8" => AbiType::U8,
            "CUInt16" => AbiType::U16,
            "CUInt32" => AbiType::U32,
            "CUInt64" => AbiType::U64,
            "CFloat32" => AbiType::F32,
            _ => {
                return Err(Diagnostic::new(
                    span,
                    "foreign ABI requires Int, Float, Bool, a C scalar, Ptr(a), or Unit return",
                ));
            }
        },
        _ => {
            return Err(Diagnostic::new(
                span,
                "foreign ABI requires Int, Float, Bool, a C scalar, Ptr(a), or Unit return",
            ));
        }
    })
}

/// Types owned by the trusted foreign boundary cannot be redeclared by source modules.
pub fn reserved_type(name: &str) -> bool {
    matches!(
        name,
        "Ptr"
            | "CInt8"
            | "CInt16"
            | "CInt32"
            | "CUInt8"
            | "CUInt16"
            | "CUInt32"
            | "CUInt64"
            | "CFloat32"
    )
}

/// Compiler-owned safe conversions and sealed pointer operations.
pub fn is_api(name: &str) -> bool {
    matches!(
        name,
        "Ptr.null" | "Ptr.is_null" | "Ptr.equal" | "Ptr.to_string" | "String.as_ptr"
    ) || name.split_once('.').is_some_and(|(owner, method)| {
        owner != "Ptr"
            && reserved_type(owner)
            && matches!(method, "from_int" | "to_int" | "from_float" | "to_float")
            && ((owner == "CFloat32") == matches!(method, "from_float" | "to_float"))
    })
}

/// Collect only explicit logical libraries from validated native call metadata.
pub fn libraries(program: &crate::machine::Program) -> Result<Vec<String>, String> {
    use crate::machine::{Operation, Statement};
    let mut libraries = std::collections::BTreeSet::new();
    for function in &program.functions {
        for statement in &function.body {
            if let Statement::Assign {
                operation: Operation::ForeignCall { declaration, .. },
                ..
            }
            | Statement::Effect(Operation::ForeignCall { declaration, .. }) = statement
            {
                declaration
                    .validate(Span::default())
                    .map_err(|e| e.message)?;
                if let Some(library) = &declaration.library {
                    libraries.insert(library.clone());
                }
                if libraries.len() > 64 {
                    return Err("foreign library limit exceeded (64)".into());
                }
            }
        }
    }
    Ok(libraries.into_iter().collect())
}
