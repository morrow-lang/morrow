//! Deterministic serialization of the typed native module to QBE IL.
use super::*;

/// Render data before functions so descriptor addresses remain visible to both backends.
pub(super) fn program(program: &Program) -> String {
    let mut output = String::new();
    for data in &program.data {
        let mut values = Vec::new();
        for value in &data.values {
            match value {
                DataValue::Bytes(bytes) => byte_values(bytes, &mut values),
                DataValue::Zero(bytes) => values.push(format!("z {bytes}")),
                DataValue::Word(value) => values.push(format!("l {}", operand(value))),
            }
        }
        output.push_str(&format!(
            "data ${} = {{ {} }}\n",
            bare(&data.name),
            values.join(", ")
        ));
    }
    for function in &program.functions {
        function_text(function, &mut output);
    }
    output
}
/// Serialize one complete function signature and its already lowered control flow.
fn function_text(function: &Function, output: &mut String) {
    let export = if function.export { "export " } else { "" };
    let result = function
        .result
        .map(|ty| format!("{} ", ty.qbe()))
        .unwrap_or_default();
    let params = function
        .params
        .iter()
        .map(|(ty, name)| format!("{} %{}", ty.qbe(), bare(name)))
        .collect::<Vec<_>>()
        .join(", ");
    output.push_str(&format!(
        "{export}function {result}${}({params}) {{\n",
        bare(&function.name)
    ));
    for statement in &function.body {
        statement_text(statement, output);
    }
    output.push_str("}\n\n");
}
/// Serialize one typed constant or symbol with QBE's presentation sigil.
fn operand(value: &Operand) -> String {
    match value {
        Operand::Temp(name) => format!("%{}", bare(name)),
        Operand::Symbol(name) => format!("${}", bare(name)),
        Operand::Int(value) => value.to_string(),
        // Decimal NaN spelling discards sign, payload and signaling bits. QBE
        // integer constants carry raw bits even when consumed as F64 values.
        Operand::Float(bits) if f64::from_bits(*bits).is_nan() => {
            i64::from_ne_bytes(bits.to_ne_bytes()).to_string()
        }
        Operand::Float(bits) => format!("d_{:.17e}", f64::from_bits(*bits)),
    }
}
/// Serialize one statement without changing control flow or introducing values.
fn statement_text(statement: &Statement, output: &mut String) {
    let text = match statement {
        Statement::Label(label) => {
            output.push_str(&format!("@{}\n", bare(label)));
            return;
        }
        Statement::Assign {
            destination,
            ty,
            operation,
        } => format!(
            "%{} ={} {}",
            bare(destination),
            ty.qbe(),
            operation_text(operation)
        ),
        Statement::Effect(operation) => operation_text(operation),
        Statement::Store {
            kind,
            value,
            address,
        } => format!(
            "store{} {}, {}",
            store_width(*kind),
            operand(value),
            operand(address)
        ),
        Statement::Jump(label) => format!("jmp @{}", bare(label)),
        Statement::Branch {
            condition,
            then_label,
            else_label,
        } => format!(
            "jnz {}, @{}, @{}",
            operand(condition),
            bare(then_label),
            bare(else_label)
        ),
        Statement::Return(value) => value
            .as_ref()
            .map(|value| format!("ret {}", operand(value)))
            .unwrap_or_else(|| "ret".into()),
        Statement::Trap => "hlt".into(),
    };
    output.push_str(&format!("    {text}\n"));
}
/// Map a typed operation to its bounded QBE operand list.
fn operation_text(operation: &Operation) -> String {
    match operation {
        Operation::Unary(op, value) => format!("{} {}", unary(*op), operand(value)),
        Operation::Binary(op, left, right) => {
            format!("{} {}, {}", binary(*op), operand(left), operand(right))
        }
        Operation::Load(kind, address) => format!("{} {}", load(*kind), operand(address)),
        Operation::StackAlloc { bytes, align } => format!("alloc{align} {bytes}"),
        Operation::Call {
            callee,
            args,
            variadic,
        } => {
            let mut values: Vec<_> = args
                .iter()
                .map(|(ty, value)| format!("{} {}", ty.qbe(), operand(value)))
                .collect();
            if let Some(index) = variadic {
                values.insert(*index, "...".into());
            }
            format!("call {}({})", operand(callee), values.join(", "))
        }
        Operation::Phi(incoming) => format!(
            "phi {}",
            incoming
                .iter()
                .map(|(label, value)| format!("@{} {}", bare(label), operand(value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}
/// Spell a representation-preserving unary operation.
fn unary(op: UnaryOp) -> &'static str {
    match op {
        UnaryOp::Copy => "copy",
        UnaryOp::Neg => "neg",
        UnaryOp::Cast => "cast",
        UnaryOp::ExtUw => "extuw",
        UnaryOp::ExtSw => "extsw",
    }
}
/// Spell integer/float arithmetic and explicitly typed comparisons.
fn binary(op: BinaryOp) -> String {
    match op {
        BinaryOp::Compare(comparison, ty) => {
            let comparison = match comparison {
                Comparison::Eq => "eq",
                Comparison::Ne => "ne",
                Comparison::SLt | Comparison::ULt if ty == Scalar::F64 => "lt",
                Comparison::SLe | Comparison::ULe if ty == Scalar::F64 => "le",
                Comparison::SGt | Comparison::UGt if ty == Scalar::F64 => "gt",
                Comparison::SGe | Comparison::UGe if ty == Scalar::F64 => "ge",
                Comparison::SLt => "slt",
                Comparison::SLe => "sle",
                Comparison::SGt => "sgt",
                Comparison::SGe => "sge",
                Comparison::ULt => "ult",
                Comparison::ULe => "ule",
                Comparison::UGt => "ugt",
                Comparison::UGe => "uge",
            };
            format!("c{comparison}{}", ty.qbe())
        }
        _ => match op {
            BinaryOp::Add => "add",
            BinaryOp::Sub => "sub",
            BinaryOp::Mul => "mul",
            BinaryOp::Div => "div",
            BinaryOp::UDiv => "udiv",
            BinaryOp::Rem => "rem",
            BinaryOp::And => "and",
            BinaryOp::Or => "or",
            BinaryOp::Xor => "xor",
            BinaryOp::Shl => "shl",
            BinaryOp::Shr => "shr",
            BinaryOp::Sar => "sar",
            BinaryOp::Compare(..) => unreachable!(),
        }
        .into(),
    }
}
/// Preserve explicit sign extension for narrow memory reads.
fn load(kind: LoadKind) -> &'static str {
    match kind {
        LoadKind::I64 => "loadl",
        LoadKind::I32 => "loadw",
        LoadKind::I8Unsigned => "loadub",
        LoadKind::I8Signed => "loadsb",
        LoadKind::F64 => "loadd",
    }
}
/// Narrow stores use only their physical width, without extension.
fn store_width(kind: LoadKind) -> char {
    match kind {
        LoadKind::I64 => 'l',
        LoadKind::I32 => 'w',
        LoadKind::I8Unsigned | LoadKind::I8Signed => 'b',
        LoadKind::F64 => 'd',
    }
}

/// Preserve existing ASCII-run output while escaping arbitrary data as exact bytes.
fn byte_values(bytes: &[u8], values: &mut Vec<String>) {
    let mut index = 0;
    while index < bytes.len() {
        let start = index;
        while index < bytes.len()
            && index - start < 512
            && (32..=126).contains(&bytes[index])
            && !matches!(bytes[index], b'"' | b'\\')
        {
            index += 1;
        }
        if index > start {
            let text = std::str::from_utf8(&bytes[start..index]).expect("ASCII data run");
            values.push(format!("b \"{text}\""));
        } else {
            values.push(format!("b {}", bytes[index]));
            index += 1;
        }
    }
}
