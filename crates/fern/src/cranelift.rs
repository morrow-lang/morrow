//! Native object generation from the same typed machine program as QBE.
//! No source semantics or QBE parsing belong in this backend.
mod function;
use crate::{
    machine::{self, *},
    runtime_abi,
};
use cranelift_codegen::{
    ir::{self, AbiParam, types},
    settings::{self, Configurable},
};
use cranelift_module::{DataDescription, DataId, FuncId, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use std::collections::BTreeMap;

#[derive(Clone)]
struct Signature {
    params: Vec<Scalar>,
    result: Option<Scalar>,
}
struct Backend {
    module: ObjectModule,
    functions: BTreeMap<String, (FuncId, Signature)>,
    data: BTreeMap<String, DataId>,
}

/// Map a validated machine scalar to its native register representation.
fn native_type(ty: Scalar) -> ir::Type {
    match ty {
        Scalar::I32 => types::I32,
        Scalar::I64 => types::I64,
        Scalar::F64 => types::F64,
    }
}

/// Validate first, emit PIC objects directly, and return bytes without touching caller files.
/// The initial acceptance mode favors predictable lowering over speculative optimization.
pub fn emit_object(program: &Program) -> Result<Vec<u8>, String> {
    program.validate()?;
    if cfg!(target_endian = "big") {
        return Err("Cranelift currently requires a little-endian host target".into());
    }
    let mut flags = settings::builder();
    for (name, value) in [
        ("opt_level", "none"),
        ("is_pic", "true"),
        ("preserve_frame_pointers", "true"),
        ("enable_verifier", "true"),
    ] {
        flags.set(name, value).map_err(|e| e.to_string())?;
    }
    let isa = cranelift_native::builder()
        .map_err(str::to_owned)?
        .finish(settings::Flags::new(flags))
        .map_err(|e| e.to_string())?;
    if isa.pointer_type() != types::I64 {
        return Err("Cranelift currently requires a 64-bit target".into());
    }
    let builder = ObjectBuilder::new(isa, "fern", cranelift_module::default_libcall_names())
        .map_err(|e| e.to_string())?;
    let mut backend = Backend {
        module: ObjectModule::new(builder),
        functions: BTreeMap::new(),
        data: BTreeMap::new(),
    };
    backend.declare(program)?;
    backend.define_data(program)?;
    for function in &program.functions {
        backend.define_function(function)?;
    }
    backend
        .module
        .finish()
        .emit()
        .map_err(|e| format!("Cranelift object: {e}"))
}

impl Backend {
    /// Build a canonical signature using the target ABI, independently of result consumption.
    fn signature(&self, signature: &Signature) -> ir::Signature {
        let mut result = self.module.make_signature();
        result.params.extend(
            signature
                .params
                .iter()
                .map(|ty| AbiParam::new(native_type(*ty))),
        );
        result
            .returns
            .extend(signature.result.map(|ty| AbiParam::new(native_type(ty))));
        result
    }

    /// Register a function and its true ABI before resolving forward calls or relocations.
    fn declare_function(
        &mut self,
        name: &str,
        signature: Signature,
        linkage: Linkage,
    ) -> Result<(), String> {
        let native = self.signature(&signature);
        let id = self
            .module
            .declare_function(name, linkage, &native)
            .map_err(|e| e.to_string())?;
        self.functions.insert(name.into(), (id, signature));
        Ok(())
    }

    /// Resolve only audited imports; unknown or variadic boundaries fail explicitly.
    fn external(&mut self, name: &str) -> Result<(), String> {
        let name = machine::bare(name);
        if self.functions.contains_key(name) {
            return Ok(());
        }
        let signature =
            runtime_abi::signature(name).ok_or_else(|| format!("unknown native ABI: {name}"))?;
        if signature.variadic.is_some() {
            return Err(format!(
                "variadic native ABI is unsupported by Cranelift: {name}; use a fixed-signature runtime wrapper"
            ));
        }
        self.declare_function(
            name,
            Signature {
                params: signature.params,
                result: signature.result,
            },
            Linkage::Import,
        )
    }

    /// Reserve all module identities before emitting code or cyclic descriptor references.
    fn declare(&mut self, program: &Program) -> Result<(), String> {
        for function in &program.functions {
            let sig = Signature {
                params: function.params.iter().map(|(ty, _)| *ty).collect(),
                result: function.result,
            };
            self.declare_function(
                machine::bare(&function.name),
                sig,
                if function.export {
                    Linkage::Export
                } else {
                    Linkage::Local
                },
            )?;
        }
        for data in &program.data {
            let id = self
                .module
                .declare_data(machine::bare(&data.name), Linkage::Local, false, false)
                .map_err(|e| e.to_string())?;
            self.data.insert(machine::bare(&data.name).into(), id);
        }
        for data in &program.data {
            for value in &data.values {
                if let DataValue::Word(Operand::Symbol(name)) = value
                    && !self.data.contains_key(machine::bare(name))
                {
                    self.external(name)?;
                }
            }
        }
        for function in &program.functions {
            for statement in &function.body {
                let operation = match statement {
                    Statement::Assign { operation, .. } | Statement::Effect(operation) => {
                        Some(operation)
                    }
                    _ => None,
                };
                if let Some(Operation::Call {
                    callee: Operand::Symbol(name),
                    variadic,
                    ..
                }) = operation
                {
                    if variadic.is_some() {
                        return Err(format!("variadic call unsupported by Cranelift: {name}"));
                    }
                    self.external(name)?;
                }
            }
        }
        Ok(())
    }

    /// Materialize immutable bytes and native-address relocations after symbol declaration.
    fn define_data(&mut self, program: &Program) -> Result<(), String> {
        for data in &program.data {
            let mut description = DataDescription::new();
            let mut bytes = Vec::new();
            let mut relocations = Vec::new();
            for value in &data.values {
                match value {
                    DataValue::Bytes(value) => bytes.extend_from_slice(value),
                    DataValue::Zero(count) => bytes.resize(bytes.len() + *count as usize, 0),
                    DataValue::Word(Operand::Int(value)) => {
                        bytes.extend_from_slice(&value.to_le_bytes())
                    }
                    DataValue::Word(Operand::Float(bits)) => {
                        bytes.extend_from_slice(&bits.to_le_bytes())
                    }
                    DataValue::Word(Operand::Symbol(name)) => {
                        let offset = u32::try_from(bytes.len())
                            .map_err(|_| "data relocation exceeds native offset")?;
                        relocations.push((offset, machine::bare(name).to_string()));
                        bytes.extend_from_slice(&[0; 8]);
                    }
                    DataValue::Word(Operand::Temp(_)) => {
                        return Err("temporary in immutable data".into());
                    }
                }
            }
            description.define(bytes.into_boxed_slice());
            description.set_align(8);
            for (offset, name) in relocations {
                if let Some((id, _)) = self.functions.get(&name) {
                    let reference = self.module.declare_func_in_data(*id, &mut description);
                    description.write_function_addr(offset, reference);
                } else if let Some(id) = self.data.get(&name) {
                    let reference = self.module.declare_data_in_data(*id, &mut description);
                    description.write_data_addr(offset, reference, 0);
                } else {
                    return Err(format!("unknown data relocation: {name}"));
                }
            }
            let id = *self
                .data
                .get(machine::bare(&data.name))
                .ok_or("missing data identity")?;
            self.module
                .define_data(id, &description)
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}
