//! Structural checks performed once before either native backend consumes a module.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
const MAX_ITEMS: usize = 2_000_000;
const MAX_DATA: usize = 64 * 1024 * 1024;

/// Validate the whole module before publishing machine code or invoking external tools.
pub(super) fn program(program: &Program) -> Result<(), String> {
    limits(program)?;
    let mut names = BTreeSet::new();
    let mut bytes = 0usize;
    let mut items = program.data.len().saturating_add(program.functions.len());
    for data in &program.data {
        identity(&data.name)?;
        if !names.insert(bare(&data.name)) {
            return Err("duplicate machine symbol".into());
        }
        for value in &data.values {
            bytes = bytes.saturating_add(match value {
                DataValue::Bytes(value) => value.len(),
                DataValue::Zero(bytes) => *bytes as usize,
                DataValue::Word(_) => 8,
            });
            if let DataValue::Word(value) = value {
                match value {
                    Operand::Int(_) | Operand::Float(_) => {}
                    Operand::Symbol(name) => identity(name)?,
                    Operand::Temp(_) => return Err("temporary in static machine data".into()),
                }
            }
        }
    }
    for function in &program.functions {
        identity(&function.name)?;
        if !names.insert(bare(&function.name)) {
            return Err("duplicate machine symbol".into());
        }
        items = items
            .saturating_add(function.body.len())
            .saturating_add(function.params.len());
    }
    if items > MAX_ITEMS || bytes > MAX_DATA {
        return Err("machine module limit exceeded".into());
    }
    let mut dominance_work = 0;
    for function in &program.functions {
        validate_function(function)?;
        super::dominance::function(function, &mut dominance_work)?;
    }
    super::types::program(program)?;
    Ok(())
}
/// Reject malformed names before they reach serialized or object symbol namespaces.
fn identity(name: &str) -> Result<(), String> {
    let name = bare(name);
    if name.is_empty()
        || name.len() > 1024
        || name.as_bytes().first().is_some_and(u8::is_ascii_digit)
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.'))
    {
        return Err(format!(
            "invalid machine identity: {:?}",
            name.chars().take(128).collect::<String>()
        ));
    }
    Ok(())
}
/// Collect the complete SSA namespace, including loop-backedge definitions.
fn definitions(function: &Function) -> Result<(BTreeMap<&str, Scalar>, BTreeSet<&str>), String> {
    let mut values = BTreeMap::new();
    let mut labels = BTreeSet::new();
    for (ty, name) in &function.params {
        identity(name)?;
        if values.insert(bare(name), *ty).is_some() {
            return Err("duplicate machine temporary".into());
        }
    }
    for statement in &function.body {
        match statement {
            Statement::Label(label) => {
                identity(label)?;
                if !labels.insert(bare(label)) {
                    return Err("duplicate machine block".into());
                }
            }
            Statement::Assign {
                destination, ty, ..
            } => {
                identity(destination)?;
                if values.insert(bare(destination), *ty).is_some() {
                    return Err("duplicate machine temporary".into());
                }
            }
            _ => {}
        }
    }
    Ok((values, labels))
}
/// Check explicit block termination and every named value/target reference.
fn validate_function(function: &Function) -> Result<(), String> {
    let (values, labels) = definitions(function)?;
    let mut active = false;
    let mut terminated = false;
    for statement in &function.body {
        if let Statement::Label(_) = statement {
            if active && !terminated {
                return Err("machine block missing terminator".into());
            }
            active = true;
            terminated = false;
            continue;
        }
        if !active {
            return Err("machine instruction before entry block".into());
        }
        if terminated {
            return Err("machine instruction follows terminator".into());
        }
        match statement {
            Statement::Assign { operation, .. } | Statement::Effect(operation) => {
                operation_refs(operation, &values, &labels)?
            }
            Statement::Store { value, address, .. } => {
                operand(value, &values)?;
                operand(address, &values)?;
            }
            Statement::Jump(label) => {
                target(label, &labels)?;
                terminated = true;
            }
            Statement::Branch {
                condition,
                then_label,
                else_label,
            } => {
                operand(condition, &values)?;
                target(then_label, &labels)?;
                target(else_label, &labels)?;
                terminated = true;
            }
            Statement::Return(value) => {
                if let Some(value) = value {
                    operand(value, &values)?;
                }
                if value.is_some() != function.result.is_some() {
                    return Err("machine return representation mismatch".into());
                }
                terminated = true;
            }
            Statement::Trap => terminated = true,
            Statement::Label(_) => unreachable!(),
        }
    }
    if !active || !terminated {
        return Err("machine function missing terminator".into());
    }
    validate_phis(function)?;
    Ok(())
}
/// Check references within one typed operation without inferring native ABI declarations.
fn operation_refs(
    operation: &Operation,
    values: &BTreeMap<&str, Scalar>,
    labels: &BTreeSet<&str>,
) -> Result<(), String> {
    match operation {
        Operation::Unary(_, value) | Operation::Load(_, value) => operand(value, values)?,
        Operation::Binary(_, left, right) => {
            operand(left, values)?;
            operand(right, values)?;
        }
        Operation::StackAlloc { bytes, align } => {
            if *bytes == 0 || !matches!(align, 4 | 8 | 16) {
                return Err("invalid machine stack allocation".into());
            }
        }
        Operation::Call {
            callee,
            args,
            variadic,
        } => {
            operand(callee, values)?;
            if !matches!(callee, Operand::Temp(_) | Operand::Symbol(_)) {
                return Err("invalid machine callee".into());
            }
            if variadic.is_some_and(|index| index > args.len()) {
                return Err("invalid machine variadic boundary".into());
            }
            for (_, value) in args {
                operand(value, values)?;
            }
        }
        Operation::Phi(incoming) => {
            if incoming.is_empty() {
                return Err("empty machine phi".into());
            }
            let mut seen = BTreeSet::new();
            for (label, value) in incoming {
                target(label, labels)?;
                operand(value, values)?;
                if !seen.insert(bare(label)) {
                    return Err("duplicate machine phi predecessor".into());
                }
            }
        }
    }
    Ok(())
}
/// A forward SSA reference is allowed, but an absent identity is never allowed.
fn operand(value: &Operand, values: &BTreeMap<&str, Scalar>) -> Result<(), String> {
    match value {
        Operand::Temp(name) => {
            identity(name)?;
            if !values.contains_key(bare(name)) {
                return Err(format!("unknown temporary {}", bare(name)));
            }
        }
        Operand::Symbol(name) => identity(name)?,
        _ => {}
    }
    Ok(())
}
/// Validate a control edge against the function-local namespace.
fn target(label: &str, labels: &BTreeSet<&str>) -> Result<(), String> {
    if !labels.contains(bare(label)) {
        return Err(format!("unknown machine target {}", bare(label)));
    }
    Ok(())
}

/// Establish real incoming edges before accepting phi labels or their position.
fn validate_phis(function: &Function) -> Result<(), String> {
    let mut predecessors: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    let mut current = "";
    for statement in &function.body {
        match statement {
            Statement::Label(label) => current = bare(label),
            Statement::Jump(label) => {
                predecessors.entry(bare(label)).or_default().insert(current);
            }
            Statement::Branch {
                then_label,
                else_label,
                ..
            } => {
                predecessors
                    .entry(bare(then_label))
                    .or_default()
                    .insert(current);
                predecessors
                    .entry(bare(else_label))
                    .or_default()
                    .insert(current);
            }
            _ => {}
        }
    }
    let mut ordinary = false;
    for statement in &function.body {
        match statement {
            Statement::Label(label) => {
                current = bare(label);
                ordinary = false;
            }
            Statement::Assign {
                operation: Operation::Phi(incoming),
                ..
            } => {
                if ordinary {
                    return Err("machine phi follows ordinary instruction".into());
                }
                let actual: BTreeSet<_> = incoming.iter().map(|(label, _)| bare(label)).collect();
                if predecessors.get(current) != Some(&actual) {
                    return Err("machine phi predecessor set differs from control edges".into());
                }
            }
            _ => ordinary = true,
        }
    }
    Ok(())
}

/// Charge nested vectors before allocating lookup maps or constructing backend operands.
fn limits(program: &Program) -> Result<(), String> {
    let mut count = program.data.len().saturating_add(program.functions.len());
    if count > MAX_ITEMS {
        return Err("machine module limit exceeded".into());
    }
    for data in &program.data {
        count = count.saturating_add(data.values.len());
        if count > MAX_ITEMS {
            return Err("machine data item limit exceeded".into());
        }
    }
    for function in &program.functions {
        count = count
            .saturating_add(function.params.len())
            .saturating_add(function.body.len());
        if count > MAX_ITEMS {
            return Err("machine function item limit exceeded".into());
        }
        let mut stack_bytes = 0usize;
        for statement in &function.body {
            let added = match statement {
                Statement::Assign { operation, .. } | Statement::Effect(operation) => {
                    match operation {
                        Operation::Call { args, .. } => args.len().saturating_add(1),
                        Operation::Phi(incoming) => incoming.len().saturating_mul(2),
                        Operation::StackAlloc { bytes, align } => {
                            stack_bytes = stack_bytes
                                .saturating_add(*bytes as usize)
                                .saturating_add(*align as usize);
                            if stack_bytes > 16 * 1024 * 1024 {
                                return Err("machine stack allocation limit exceeded".into());
                            }
                            2
                        }
                        _ => 2,
                    }
                }
                _ => 2,
            };
            count = count.saturating_add(added);
            if count > MAX_ITEMS {
                return Err("machine operand item limit exceeded".into());
            }
        }
    }
    Ok(())
}
