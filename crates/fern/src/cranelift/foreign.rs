//! Explicit physical C signatures; logical Fern registers never determine foreign widths.
use super::*;
use crate::ffi::{AbiType, Declaration};

pub(super) fn abi_param(ty: &AbiType) -> Result<AbiParam, String> {
    use AbiType::*;
    let physical = match ty {
        I8 | U8 | Bool => types::I8,
        I16 | U16 => types::I16,
        I32 | U32 => types::I32,
        I64 | U64 | Pointer(_) => types::I64,
        F32 => types::F32,
        F64 => types::F64,
        Void => return Err("void has no physical value".into()),
    };
    let param = AbiParam::new(physical);
    Ok(match ty {
        I8 | I16 | I32 => param.sext(),
        U8 | U16 | U32 | Bool => param.uext(),
        _ => param,
    })
}

impl Backend {
    pub(super) fn declare_foreign(&mut self, declaration: &Declaration) -> Result<(), String> {
        declaration
            .validate(crate::Span::default())
            .map_err(|error| error.message)?;
        if let Some((_, existing)) = self.foreign.get(&declaration.symbol) {
            return if existing == declaration {
                Ok(())
            } else {
                Err(format!(
                    "conflicting foreign declarations for {}",
                    declaration.symbol
                ))
            };
        }
        if self.functions.contains_key(&declaration.symbol)
            || self.data.contains_key(&declaration.symbol)
            || runtime_abi::signature(&declaration.symbol).is_some()
        {
            return Err(format!(
                "foreign symbol conflicts with compiler/runtime symbol: {}",
                declaration.symbol
            ));
        }
        let mut signature = self.module.make_signature();
        signature.params = declaration
            .params
            .iter()
            .map(abi_param)
            .collect::<Result<_, _>>()?;
        if declaration.result != AbiType::Void {
            signature.returns.push(abi_param(&declaration.result)?);
        }
        let id = self
            .module
            .declare_function(&declaration.symbol, Linkage::Import, &signature)
            .map_err(|error| error.to_string())?;
        self.foreign
            .insert(declaration.symbol.clone(), (id, declaration.clone()));
        Ok(())
    }
}
