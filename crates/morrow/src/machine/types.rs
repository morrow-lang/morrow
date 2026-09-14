//! Physical operand and canonical ABI checks before a machine builder sees public input.
use super::*;
use crate::runtime_abi;
use std::collections::{BTreeMap, BTreeSet};

/// Validate physical widths and every symbol against generated/native declarations.
pub(super) fn program(program: &Program) -> Result<(), String> {
    let functions: BTreeMap<_, _> = program
        .functions
        .iter()
        .map(|f| (bare(&f.name), f))
        .collect();
    let data: BTreeSet<_> = program.data.iter().map(|d| bare(&d.name)).collect();
    let mut foreign = BTreeMap::new();
    for function in &program.functions {
        for statement in &function.body {
            if let Statement::Assign {
                operation: Operation::ForeignCall { declaration, .. },
                ..
            }
            | Statement::Effect(Operation::ForeignCall { declaration, .. }) = statement
                && let Some(previous) = foreign.insert(&declaration.symbol, declaration)
                && previous != declaration
            {
                return Err(format!(
                    "conflicting foreign declarations for {}",
                    declaration.symbol
                ));
            }
        }
    }
    let context = Context { functions, data };
    for data in &program.data {
        for value in &data.values {
            if let DataValue::Word(Operand::Symbol(symbol)) = value {
                context.symbol(symbol)?;
            }
        }
    }
    for function in &program.functions {
        context.function(function)?;
    }
    Ok(())
}
struct Context<'a> {
    functions: BTreeMap<&'a str, &'a Function>,
    data: BTreeSet<&'a str>,
}
impl Context<'_> {
    /// Only known definitions or audited runtime functions may appear as addresses.
    fn symbol(&self, name: &str) -> Result<(), String> {
        let name = bare(name);
        if self.functions.contains_key(name)
            || self.data.contains(name)
            || runtime_abi::signature(name).is_some()
        {
            Ok(())
        } else {
            Err(format!("unknown machine symbol: {name}"))
        }
    }
    /// Establish local physical types independently of statement order.
    fn function(&self, function: &Function) -> Result<(), String> {
        let mut values: BTreeMap<_, _> = function
            .params
            .iter()
            .map(|(ty, name)| (bare(name), *ty))
            .collect();
        for statement in &function.body {
            if let Statement::Assign {
                destination, ty, ..
            } = statement
            {
                values.insert(bare(destination), *ty);
            }
        }
        for statement in &function.body {
            match statement {
                Statement::Assign { ty, operation, .. } => {
                    self.operation(operation, Some(*ty), &values)?
                }
                Statement::Effect(operation) => self.operation(operation, None, &values)?,
                Statement::Store {
                    kind,
                    value,
                    address,
                } => {
                    self.operand(address, Scalar::I64, &values)?;
                    self.operand(
                        value,
                        if *kind == LoadKind::F64 {
                            Scalar::F64
                        } else {
                            Scalar::I64
                        },
                        &values,
                    )?;
                }
                Statement::Branch { condition, .. } => {
                    self.operand(condition, Scalar::I64, &values)?
                }
                Statement::Return(Some(value)) => self.operand(
                    value,
                    function.result.ok_or("value returned from void function")?,
                    &values,
                )?,
                _ => {}
            }
        }
        Ok(())
    }
    /// Permit QBE's explicit integer register narrowing but never cross float/integer classes.
    fn operand(
        &self,
        value: &Operand,
        expected: Scalar,
        values: &BTreeMap<&str, Scalar>,
    ) -> Result<(), String> {
        if let Operand::Symbol(name) = value {
            self.symbol(name)?;
        }
        let actual = scalar(value, values)?;
        if compatible(actual, expected) {
            Ok(())
        } else {
            Err(format!(
                "incompatible machine operand type: {actual:?} for {expected:?}"
            ))
        }
    }
    /// Validate every operation family before its destination can reach a native builder.
    fn operation(
        &self,
        op: &Operation,
        result: Option<Scalar>,
        values: &BTreeMap<&str, Scalar>,
    ) -> Result<(), String> {
        if let Operation::Call {
            callee,
            args,
            variadic,
        } = op
        {
            return self.call(callee, args, *variadic, result, values);
        }
        if let Operation::ForeignCall { declaration, args } = op {
            declaration
                .validate(crate::Span::default())
                .map_err(|error| error.message)?;
            if self.functions.contains_key(declaration.symbol.as_str())
                || self.data.contains(declaration.symbol.as_str())
                || runtime_abi::signature(&declaration.symbol).is_some()
            {
                return Err(format!(
                    "foreign symbol conflicts with compiler/runtime symbol: {}",
                    declaration.symbol
                ));
            }
            if args.len() != declaration.params.len() {
                return Err("foreign argument count mismatch".into());
            }
            for ((provided, arg), expected) in args.iter().zip(&declaration.params) {
                if Some(*provided) != expected.machine_scalar() {
                    return Err("foreign logical argument type mismatch".into());
                }
                self.operand(arg, *provided, values)?;
                if matches!(arg, Operand::Temp(_)) && scalar(arg, values)? != *provided {
                    return Err(
                        "foreign operand must have its declared logical register width".into(),
                    );
                }
            }
            if result.is_some() && result != declaration.result.machine_scalar() {
                return Err("foreign result type mismatch".into());
            }
            return Ok(());
        }
        let result = result.ok_or("non-call machine operation used as effect")?;
        match op {
            Operation::Unary(op, value) => self.unary(*op, value, result, values)?,
            Operation::Binary(op, left, right) => self.binary(*op, left, right, result, values)?,
            Operation::Load(kind, address) => {
                self.operand(address, Scalar::I64, values)?;
                if (*kind == LoadKind::F64) != (result == Scalar::F64) {
                    return Err("incompatible machine load type".into());
                }
            }
            Operation::StackAlloc { .. } => {
                if result != Scalar::I64 {
                    return Err("machine stack address requires I64".into());
                }
            }
            Operation::Phi(incoming) => {
                for (_, value) in incoming {
                    self.operand(value, result, values)?;
                }
            }
            Operation::Call { .. } | Operation::ForeignCall { .. } => {
                unreachable!("call validated before destination handling")
            }
        }
        Ok(())
    }
    /// Bitcasts preserve width; extension explicitly consumes a 32-bit integer value.
    fn unary(
        &self,
        op: UnaryOp,
        value: &Operand,
        result: Scalar,
        values: &BTreeMap<&str, Scalar>,
    ) -> Result<(), String> {
        match op {
            UnaryOp::Cast => {
                let actual = scalar(value, values)?;
                if !matches!(
                    (actual, result),
                    (Scalar::I64, Scalar::F64) | (Scalar::F64, Scalar::I64)
                ) {
                    return Err(
                        "machine bitcast requires opposite 64-bit scalar representations".into(),
                    );
                }
                if let Operand::Symbol(name) = value {
                    self.symbol(name)?;
                }
            }
            UnaryOp::ExtUw | UnaryOp::ExtSw => {
                if result != Scalar::I64 {
                    return Err("machine extension requires I64 result".into());
                }
                self.operand(value, Scalar::I32, values)?;
            }
            UnaryOp::Copy | UnaryOp::Neg => self.operand(value, result, values)?,
        }
        Ok(())
    }
    /// Comparisons produce integer conditions; bit operations cannot consume Float values.
    fn binary(
        &self,
        op: BinaryOp,
        left: &Operand,
        right: &Operand,
        result: Scalar,
        values: &BTreeMap<&str, Scalar>,
    ) -> Result<(), String> {
        let input = if let BinaryOp::Compare(cmp, input) = op {
            if result == Scalar::F64 {
                return Err("machine comparison requires integer result".into());
            }
            if input == Scalar::F64
                && matches!(
                    cmp,
                    Comparison::ULt | Comparison::ULe | Comparison::UGt | Comparison::UGe
                )
            {
                return Err("unsigned machine comparison cannot consume Float".into());
            }
            input
        } else {
            if result == Scalar::F64
                && !matches!(
                    op,
                    BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div
                )
            {
                return Err("integer-only machine operation on Float".into());
            }
            result
        };
        self.operand(left, input, values)?;
        self.operand(right, input, values)
    }
    /// Direct calls use canonical declarations, while explicit indirect arguments retain their widths.
    fn call(
        &self,
        callee: &Operand,
        args: &[(Scalar, Operand)],
        variadic: Option<usize>,
        result: Option<Scalar>,
        values: &BTreeMap<&str, Scalar>,
    ) -> Result<(), String> {
        self.operand(callee, Scalar::I64, values)?;
        for (ty, arg) in args {
            self.operand(arg, *ty, values)?;
        }
        let Operand::Symbol(name) = callee else {
            return Ok(());
        };
        let name = bare(name);
        let (params, actual, expected_variadic) = if let Some(function) = self.functions.get(name) {
            (
                function.params.iter().map(|(ty, _)| *ty).collect(),
                function.result,
                None,
            )
        } else {
            let signature = runtime_abi::signature(name)
                .ok_or_else(|| format!("machine call target is not a function: {name}"))?;
            (signature.params, signature.result, signature.variadic)
        };
        if variadic != expected_variadic {
            return Err(format!("machine variadic signature mismatch: {name}"));
        }
        if (expected_variadic.is_none() && args.len() != params.len()) || args.len() < params.len()
        {
            return Err(format!("machine call arity mismatch: {name}"));
        }
        for ((provided, _), expected) in args.iter().zip(params) {
            if !compatible(*provided, expected) {
                return Err(format!("machine call parameter type mismatch: {name}"));
            }
        }
        if let Some(result) = result
            && !actual.is_some_and(|actual| compatible(actual, result))
        {
            return Err(format!("machine call result type mismatch: {name}"));
        }
        Ok(())
    }
}
/// Determine actual SSA representation; immediates remain width-polymorphic integers.
fn scalar(value: &Operand, values: &BTreeMap<&str, Scalar>) -> Result<Scalar, String> {
    match value {
        Operand::Temp(name) => values
            .get(bare(name))
            .copied()
            .ok_or_else(|| format!("unknown temporary: {name}")),
        Operand::Float(_) => Ok(Scalar::F64),
        Operand::Symbol(_) | Operand::Int(_) => Ok(Scalar::I64),
    }
}
/// Both native backends permit integer truncation/extension at explicit width boundaries.
fn compatible(actual: Scalar, expected: Scalar) -> bool {
    (actual == Scalar::F64) == (expected == Scalar::F64)
}
