//! Canonical supervisor nominals share the ordinary record/enum checking path.
use super::*;
impl Registry {
    pub(super) fn register_supervisors(&mut self) {
        for schema in crate::supervisors::schemas() {
            let name = format!("Supervisor.{}", schema.name);
            let record = !schema.fields.is_empty();
            let variants = schema.variants.into_iter().enumerate().map(|(tag, (variant, fields))| {
                let qualified = format!("Supervisor.{variant}");
                // Error and ChildState share Stopped/Restarting; full owner paths are unambiguous.
                self.constructors.entry(qualified.clone()).or_insert((name.clone(), tag));
                self.constructors.insert(format!("{name}.{variant}"), (name.clone(), tag));
                ast::Variant { name: qualified, span: Span::default(), fields: fields.into_iter().enumerate().map(|(index, ty)| ast::Field {
                    name: schema.fields.get(index).map(|field| (*field).into()), ty, span: Span::default(),
                }).collect() }
            }).collect();
            self.declarations.insert(name.clone(), ast::TypeDecl { public: true, derives: vec![], name, parameters: vec![], record, variants, span: Span::default() });
        }
    }
}
