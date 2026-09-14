//! Native handles are sealed nominal values; narrow scalars preserve checked conversion invariants.
use super::*;
impl Registry {
    pub(super) fn register_foreign(&mut self) {
        for abi in [
            crate::ffi::AbiType::I8,
            crate::ffi::AbiType::I16,
            crate::ffi::AbiType::I32,
            crate::ffi::AbiType::U8,
            crate::ffi::AbiType::U16,
            crate::ffi::AbiType::U32,
            crate::ffi::AbiType::U64,
            crate::ffi::AbiType::F32,
        ] {
            let Type::Named(name, _) = abi.source_type() else {
                unreachable!()
            };
            let inner = if abi == crate::ffi::AbiType::F32 {
                Type::Float
            } else {
                Type::Int
            };
            self.newtypes.insert(name.clone());
            std::rc::Rc::make_mut(&mut self.newtype_definitions)
                .insert(name.clone(), (vec![], inner.clone()));
            self.declarations
                .insert(name.clone(), declaration(&name, vec![], vec![inner]));
        }
        self.declarations.insert(
            "Ptr".into(),
            declaration(
                "Ptr",
                vec!["a".into()],
                vec![Type::Int, Type::List(Box::new(Type::String))],
            ),
        );
    }
}
fn declaration(name: &str, parameters: Vec<String>, fields: Vec<Type>) -> ast::TypeDecl {
    ast::TypeDecl {
        derives: vec![],
        public: true,
        name: name.into(),
        parameters,
        record: name == "Ptr",
        span: Span::default(),
        variants: vec![ast::Variant {
            name: "$foreign_storage".into(),
            span: Span::default(),
            fields: fields
                .into_iter()
                .enumerate()
                .map(|(index, ty)| ast::Field {
                    name: (name == "Ptr").then(|| {
                        if index == 0 {
                            "address".into()
                        } else {
                            "owners".into()
                        }
                    }),
                    ty,
                    span: Span::default(),
                })
                .collect(),
        }],
    }
}
