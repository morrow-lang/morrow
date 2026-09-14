//! Compiler-owned immutable sets retain a distinct nominal identity over map storage.
use super::*;

impl Registry {
    /// Register a closed builtin layout without exposing a constructor or mutable storage.
    pub(super) fn register_set(&mut self) {
        let item = Type::Generic("a".into());
        let inner = Type::Map(Box::new(item), Box::new(Type::Unit));
        self.newtypes.insert("Set".into());
        std::rc::Rc::make_mut(&mut self.newtype_definitions)
            .insert("Set".into(), (vec!["a".into()], inner.clone()));
        self.declarations.insert(
            "Set".into(),
            ast::TypeDecl {
                derives: vec![],
                public: true,
                name: "Set".into(),
                parameters: vec!["a".into()],
                record: false,
                span: Span::default(),
                variants: vec![ast::Variant {
                    name: "$set_storage".into(),
                    span: Span::default(),
                    fields: vec![ast::Field {
                        name: None,
                        ty: inner,
                        span: Span::default(),
                    }],
                }],
            },
        );
    }
}
