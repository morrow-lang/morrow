//! Translate validated machine CFGs without repeating language semantics.
use super::*;
use cranelift_codegen::ir::{
    Block, InstBuilder, MemFlagsData, StackSlotData, StackSlotKind, Value,
    condcodes::{FloatCC, IntCC},
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};

type Phis = Vec<(String, Scalar, Vec<(String, Operand)>)>;
struct Lower<'a, 'b> {
    backend: &'a mut Backend,
    builder: FunctionBuilder<'b>,
    variables: BTreeMap<String, (Variable, Scalar)>,
    blocks: BTreeMap<String, Block>,
    phis: BTreeMap<String, Phis>,
    current: String,
    result: Option<Scalar>,
}
impl Backend {
    /// Translate one validated CFG and let the backend verifier check its native representation.
    pub(super) fn define_function(&mut self, function: &Function) -> Result<(), String> {
        let name = machine::bare(&function.name);
        let (id, signature) = self
            .functions
            .get(name)
            .cloned()
            .ok_or("missing function identity")?;
        let mut context = self.module.make_context();
        context.func.signature = self.signature(&signature);
        let mut frontend = FunctionBuilderContext::new();
        let builder = FunctionBuilder::new(&mut context.func, &mut frontend);
        let mut lower = Lower {
            backend: self,
            builder,
            variables: BTreeMap::new(),
            blocks: BTreeMap::new(),
            phis: BTreeMap::new(),
            current: String::new(),
            result: function.result,
        };
        lower.prepare(function)?;
        for statement in &function.body {
            lower.statement(statement)?;
        }
        lower.builder.seal_all_blocks();
        let config = lower.backend.module.target_config();
        lower.builder.finalize(config);
        self.module
            .define_function(id, &mut context)
            .map_err(|e| format!("Cranelift function {name}: {e}"))
    }
}
impl Lower<'_, '_> {
    /// Reserve blocks, SSA variables and phi parameters before resolving forward edges.
    fn prepare(&mut self, function: &Function) -> Result<(), String> {
        for (ty, name) in &function.params {
            self.variable(name, *ty);
        }
        let mut label = String::new();
        for statement in &function.body {
            match statement {
                Statement::Label(name) => {
                    label = machine::bare(name).into();
                    let block = self.builder.create_block();
                    self.blocks.insert(label.clone(), block);
                }
                Statement::Assign {
                    destination,
                    ty,
                    operation,
                } => {
                    self.variable(destination, *ty);
                    if let Operation::Phi(incoming) = operation {
                        self.phis.entry(label.clone()).or_default().push((
                            machine::bare(destination).into(),
                            *ty,
                            incoming.clone(),
                        ));
                    }
                }
                _ => {}
            }
        }
        let first = function
            .body
            .iter()
            .find_map(|s| {
                if let Statement::Label(name) = s {
                    Some(machine::bare(name))
                } else {
                    None
                }
            })
            .ok_or("missing entry block")?;
        let entry = *self.blocks.get(first).ok_or("missing entry identity")?;
        self.builder.append_block_params_for_function_params(entry);
        for (label, phis) in &self.phis {
            let block = *self.blocks.get(label).ok_or("missing phi block")?;
            if block == entry {
                return Err("entry block cannot contain phi inputs".into());
            }
            for (_, ty, _) in phis {
                self.builder.append_block_param(block, native_type(*ty));
            }
        }
        self.builder.switch_to_block(entry);
        for (index, (_, name)) in function.params.iter().enumerate() {
            let value = self.builder.block_params(entry)[index];
            self.define(name, value)?;
        }
        self.current = first.into();
        Ok(())
    }
    /// Allocate a native SSA variable for an already validated unique machine identity.
    fn variable(&mut self, name: &str, ty: Scalar) {
        let variable = self.builder.declare_var(native_type(ty));
        self.variables
            .insert(machine::bare(name).into(), (variable, ty));
    }
    /// Bind a machine destination to its native value without repeating semantic inference.
    fn define(&mut self, name: &str, value: Value) -> Result<(), String> {
        let variable = self
            .variables
            .get(machine::bare(name))
            .ok_or("missing SSA destination")?
            .0;
        self.builder.def_var(variable, value);
        Ok(())
    }
    /// Preserve low bits when narrowing and apply the requested extension for wider integers.
    fn coerce(&mut self, value: Value, ty: ir::Type, signed: bool) -> Result<Value, String> {
        let actual = self.builder.func.dfg.value_type(value);
        if actual == ty {
            return Ok(value);
        }
        if actual.is_int() && ty.is_int() {
            return Ok(if actual.bits() > ty.bits() {
                self.builder.ins().ireduce(ty, value)
            } else if signed {
                self.builder.ins().sextend(ty, value)
            } else {
                self.builder.ins().uextend(ty, value)
            });
        }
        Err(format!("invalid machine conversion {actual} to {ty}"))
    }
    /// Materialize a constant, symbol or SSA use at the explicit machine operand width.
    fn operand(&mut self, operand: &Operand, ty: Scalar) -> Result<Value, String> {
        let target = native_type(ty);
        let value = match operand {
            Operand::Temp(name) => {
                let variable = self
                    .variables
                    .get(machine::bare(name))
                    .ok_or_else(|| format!("unknown SSA value {name}"))?
                    .0;
                self.builder.use_var(variable)
            }
            Operand::Int(value) if ty != Scalar::F64 => self.builder.ins().iconst(target, *value),
            Operand::Float(bits) if ty == Scalar::F64 => self
                .builder
                .ins()
                .f64const(ir::immediates::Ieee64::with_bits(*bits)),
            Operand::Symbol(name) => {
                let name = machine::bare(name);
                if let Some(id) = self.backend.data.get(name) {
                    let reference = self
                        .backend
                        .module
                        .declare_data_in_func(*id, self.builder.func);
                    self.builder.ins().symbol_value(types::I64, reference)
                } else {
                    self.backend.external(name)?;
                    let id = self
                        .backend
                        .functions
                        .get(name)
                        .ok_or("missing symbol identity")?
                        .0;
                    let reference = self
                        .backend
                        .module
                        .declare_func_in_func(id, self.builder.func);
                    self.builder.ins().func_addr(types::I64, reference)
                }
            }
            _ => return Err("constant has incompatible machine type".into()),
        };
        self.coerce(value, target, false)
    }
    /// Recover physical width for operations whose input class differs from their result.
    fn actual_type(&self, operand: &Operand) -> Result<Scalar, String> {
        match operand {
            Operand::Temp(name) => self
                .variables
                .get(machine::bare(name))
                .map(|(_, ty)| *ty)
                .ok_or_else(|| format!("unknown SSA value {name}")),
            Operand::Float(_) => Ok(Scalar::F64),
            _ => Ok(Scalar::I64),
        }
    }
    /// Supply phi arguments in their predecessor context, preserving parallel assignment.
    fn edge(&mut self, label: &str) -> Result<(Block, Vec<ir::BlockArg>), String> {
        let name = machine::bare(label);
        let block = *self
            .blocks
            .get(name)
            .ok_or_else(|| format!("unknown block {name}"))?;
        let mut arguments = Vec::new();
        for (_, ty, incoming) in self.phis.get(name).cloned().unwrap_or_default() {
            let value = incoming
                .iter()
                .find(|(from, _)| machine::bare(from) == self.current)
                .ok_or_else(|| format!("missing phi edge {} -> {name}", self.current))?;
            arguments.push(self.operand(&value.1, ty)?.into());
        }
        Ok((block, arguments))
    }
    /// Lower control and effects in order; return and call signatures stay independently checked.
    fn statement(&mut self, statement: &Statement) -> Result<(), String> {
        match statement {
            Statement::Label(name) => {
                let name = machine::bare(name);
                let block = *self.blocks.get(name).ok_or("unknown block")?;
                if self.current != name {
                    self.builder.switch_to_block(block);
                }
                self.current = name.into();
                for (index, (destination, _, _)) in self
                    .phis
                    .get(name)
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .enumerate()
                {
                    self.define(destination, self.builder.block_params(block)[index])?;
                }
            }
            Statement::Assign {
                destination,
                ty,
                operation,
            } => {
                if !matches!(operation, Operation::Phi(_)) {
                    let value = self
                        .operation(operation, Some(*ty))?
                        .ok_or("void operation assigned to a value")?;
                    let value = self.coerce(value, native_type(*ty), false)?;
                    self.define(destination, value)?;
                }
            }
            Statement::Effect(operation) => {
                self.operation(operation, None)?;
            }
            Statement::Store {
                kind,
                value,
                address,
            } => {
                let address = self.operand(address, Scalar::I64)?;
                let ty = load_type(*kind);
                let value = self.operand(
                    value,
                    if *kind == LoadKind::F64 {
                        Scalar::F64
                    } else {
                        Scalar::I64
                    },
                )?;
                let value = self.coerce(value, ty, false)?;
                self.builder
                    .ins()
                    .store(MemFlagsData::new(), value, address, 0);
            }
            Statement::Jump(label) => {
                let (block, args) = self.edge(label)?;
                self.builder.ins().jump(block, &args);
            }
            Statement::Branch {
                condition,
                then_label,
                else_label,
            } => {
                let condition = self.operand(condition, self.actual_type(condition)?)?;
                let condition = self.builder.ins().icmp_imm_s(IntCC::NotEqual, condition, 0);
                let (yes, yes_args) = self.edge(then_label)?;
                let (no, no_args) = self.edge(else_label)?;
                self.builder
                    .ins()
                    .brif(condition, yes, &yes_args, no, &no_args);
            }
            Statement::Return(value) => {
                let values = match (value, self.result) {
                    (Some(value), Some(ty)) => vec![self.operand(value, ty)?],
                    (None, None) => vec![],
                    _ => return Err("return does not match native function signature".into()),
                };
                self.builder.ins().return_(&values);
            }
            Statement::Trap => {
                self.builder.ins().trap(ir::TrapCode::unwrap_user(1));
            }
        }
        Ok(())
    }
    /// Translate a value-producing instruction or a call with an intentionally discarded result.
    fn operation(
        &mut self,
        operation: &Operation,
        requested: Option<Scalar>,
    ) -> Result<Option<Value>, String> {
        if let Operation::Call {
            callee,
            args,
            variadic,
        } = operation
        {
            return self.call(callee, args, *variadic, requested);
        }
        if let Operation::ForeignCall { declaration, args } = operation {
            return self.foreign_call(declaration, args, requested);
        }
        let ty = requested.ok_or("non-call instruction used as effect")?;
        let value = match operation {
            Operation::Unary(op, operand) => self.unary(*op, operand, ty)?,
            Operation::Binary(op, left, right) => self.binary(*op, left, right, ty)?,
            Operation::Load(kind, address) => {
                let address = self.operand(address, Scalar::I64)?;
                let loaded =
                    self.builder
                        .ins()
                        .load(load_type(*kind), MemFlagsData::new(), address, 0);
                self.coerce(
                    loaded,
                    native_type(ty),
                    matches!(kind, LoadKind::I8Signed | LoadKind::I32),
                )?
            }
            Operation::StackAlloc { bytes, align } => {
                let slot = self.builder.create_sized_stack_slot(StackSlotData::new(
                    StackSlotKind::ExplicitSlot,
                    *bytes,
                    align.trailing_zeros() as u8,
                ));
                self.builder.ins().stack_addr(types::I64, slot, 0)
            }
            Operation::Phi(_) | Operation::Call { .. } | Operation::ForeignCall { .. } => {
                return Err("invalid instruction position".into());
            }
        };
        Ok(Some(value))
    }
    /// Keep bit reinterpretation separate from integer extension and signed negation.
    fn unary(&mut self, op: UnaryOp, operand: &Operand, ty: Scalar) -> Result<Value, String> {
        if matches!(op, UnaryOp::Cast) {
            let actual = self.actual_type(operand)?;
            let value = self.operand(operand, actual)?;
            return Ok(self
                .builder
                .ins()
                .bitcast(native_type(ty), MemFlagsData::new(), value));
        }
        if matches!(op, UnaryOp::ExtUw | UnaryOp::ExtSw) {
            let value = self.operand(operand, Scalar::I32)?;
            return self.coerce(value, native_type(ty), op == UnaryOp::ExtSw);
        }
        let value = self.operand(operand, ty)?;
        Ok(match op {
            UnaryOp::Copy => value,
            UnaryOp::Neg if ty == Scalar::F64 => self.builder.ins().fneg(value),
            UnaryOp::Neg => self.builder.ins().ineg(value),
            _ => return Err("unsupported unary operation".into()),
        })
    }
    /// Map already guarded arithmetic and comparisons without introducing language-level traps.
    fn binary(
        &mut self,
        op: BinaryOp,
        left: &Operand,
        right: &Operand,
        ty: Scalar,
    ) -> Result<Value, String> {
        let input = if let BinaryOp::Compare(_, input) = op {
            input
        } else {
            ty
        };
        let a = self.operand(left, input)?;
        let b = self.operand(right, input)?;
        if let BinaryOp::Compare(cmp, _) = op {
            let result = if input == Scalar::F64 {
                self.builder.ins().fcmp(float_cc(cmp)?, a, b)
            } else {
                self.builder.ins().icmp(int_cc(cmp), a, b)
            };
            return self.coerce(result, native_type(ty), false);
        }
        if input == Scalar::F64 {
            return Ok(match op {
                BinaryOp::Add => self.builder.ins().fadd(a, b),
                BinaryOp::Sub => self.builder.ins().fsub(a, b),
                BinaryOp::Mul => self.builder.ins().fmul(a, b),
                BinaryOp::Div => self.builder.ins().fdiv(a, b),
                _ => return Err("integer-only operation on Float".into()),
            });
        }
        Ok(match op {
            BinaryOp::Add => self.builder.ins().iadd(a, b),
            BinaryOp::Sub => self.builder.ins().isub(a, b),
            BinaryOp::Mul => self.builder.ins().imul(a, b),
            BinaryOp::Div => self.builder.ins().sdiv(a, b),
            BinaryOp::UDiv => self.builder.ins().udiv(a, b),
            BinaryOp::Rem => self.builder.ins().srem(a, b),
            BinaryOp::And => self.builder.ins().band(a, b),
            BinaryOp::Or => self.builder.ins().bor(a, b),
            BinaryOp::Xor => self.builder.ins().bxor(a, b),
            BinaryOp::Shl => self.builder.ins().ishl(a, b),
            BinaryOp::Shr => self.builder.ins().ushr(a, b),
            BinaryOp::Sar => self.builder.ins().sshr(a, b),
            BinaryOp::Compare(..) => return Err("comparison dispatch failed".into()),
        })
    }
    /// Narrow to the declared C representation, then restore signedness/full logical width.
    fn foreign_call(
        &mut self,
        declaration: &crate::ffi::Declaration,
        args: &[(Scalar, Operand)],
        requested: Option<Scalar>,
    ) -> Result<Option<Value>, String> {
        use crate::ffi::AbiType;
        let (id, _) = self
            .backend
            .foreign
            .get(&declaration.symbol)
            .ok_or("undeclared foreign function")?;
        let id = *id;
        let mut values = Vec::new();
        for ((logical, arg), physical) in args.iter().zip(&declaration.params) {
            let value = self.operand(arg, *logical)?;
            let value = if *physical == AbiType::F32 {
                self.builder.ins().fdemote(types::F32, value)
            } else if *physical == AbiType::Bool {
                self.builder.ins().icmp_imm_u(IntCC::NotEqual, value, 0)
            } else {
                self.coerce(
                    value,
                    super::foreign::abi_param(physical)?.value_type,
                    false,
                )?
            };
            values.push(value);
        }
        let reference = self
            .backend
            .module
            .declare_func_in_func(id, self.builder.func);
        let call = self.builder.ins().call(reference, &values);
        let Some(logical) = requested else {
            return Ok(None);
        };
        let value = self
            .builder
            .inst_results(call)
            .first()
            .copied()
            .ok_or("void foreign call cannot produce a value")?;
        let value = if declaration.result == AbiType::F32 {
            self.builder.ins().fpromote(types::F64, value)
        } else {
            self.coerce(
                value,
                native_type(logical),
                matches!(
                    declaration.result,
                    AbiType::I8 | AbiType::I16 | AbiType::I32
                ),
            )?
        };
        Ok(Some(value))
    }

    /// Honor the provided argument width before canonical ABI conversion and optional result discard.
    fn call(
        &mut self,
        callee: &Operand,
        args: &[(Scalar, Operand)],
        variadic: Option<usize>,
        requested: Option<Scalar>,
    ) -> Result<Option<Value>, String> {
        if variadic.is_some() {
            return Err("variadic calls require fixed-signature wrappers".into());
        }
        let direct = if let Operand::Symbol(name) = callee {
            Some(machine::bare(name))
        } else {
            None
        };
        let (id, signature) = if let Some(name) = direct {
            let (id, signature) = self
                .backend
                .functions
                .get(name)
                .cloned()
                .ok_or_else(|| format!("unknown native function {name}"))?;
            (Some(id), signature)
        } else {
            // All indirect calls are Morrow closures. Unit callbacks still return a word.
            (
                None,
                Signature {
                    params: args.iter().map(|(ty, _)| *ty).collect(),
                    result: Some(requested.unwrap_or(Scalar::I32)),
                },
            )
        };
        if args.len() != signature.params.len() {
            return Err(format!("native call arity mismatch: {callee:?}"));
        }
        let mut values = Vec::new();
        for ((provided, arg), ty) in args.iter().zip(&signature.params) {
            let value = self.operand(arg, *provided)?;
            values.push(self.coerce(value, native_type(*ty), false)?);
        }
        let instruction = if let Some(id) = id {
            let reference = self
                .backend
                .module
                .declare_func_in_func(id, self.builder.func);
            self.builder.ins().call(reference, &values)
        } else {
            let native = self.backend.signature(&signature);
            let reference = self.builder.import_signature(native);
            let address = self.operand(callee, Scalar::I64)?;
            self.builder
                .ins()
                .call_indirect(reference, address, &values)
        };
        Ok(self.builder.inst_results(instruction).first().copied())
    }
}
/// Select storage width independently of the SSA width receiving a loaded value.
fn load_type(kind: LoadKind) -> ir::Type {
    match kind {
        LoadKind::I64 => types::I64,
        LoadKind::I32 => types::I32,
        LoadKind::I8Unsigned | LoadKind::I8Signed => types::I8,
        LoadKind::F64 => types::F64,
    }
}
/// Preserve the explicit signedness of each machine integer comparison.
fn int_cc(c: Comparison) -> IntCC {
    match c {
        Comparison::Eq => IntCC::Equal,
        Comparison::Ne => IntCC::NotEqual,
        Comparison::SLt => IntCC::SignedLessThan,
        Comparison::SLe => IntCC::SignedLessThanOrEqual,
        Comparison::SGt => IntCC::SignedGreaterThan,
        Comparison::SGe => IntCC::SignedGreaterThanOrEqual,
        Comparison::ULt => IntCC::UnsignedLessThan,
        Comparison::ULe => IntCC::UnsignedLessThanOrEqual,
        Comparison::UGt => IntCC::UnsignedGreaterThan,
        Comparison::UGe => IntCC::UnsignedGreaterThanOrEqual,
    }
}
/// Retain IEEE comparison behavior, including unordered not-equal results.
fn float_cc(c: Comparison) -> Result<FloatCC, String> {
    Ok(match c {
        Comparison::Eq => FloatCC::Equal,
        Comparison::Ne => FloatCC::NotEqual,
        Comparison::SLt => FloatCC::LessThan,
        Comparison::SLe => FloatCC::LessThanOrEqual,
        Comparison::SGt => FloatCC::GreaterThan,
        Comparison::SGe => FloatCC::GreaterThanOrEqual,
        _ => return Err("unsigned comparison on Float".into()),
    })
}
