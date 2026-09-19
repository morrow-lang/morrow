//! Closed process ADTs use ordinary nominal construction, patterns and concrete layouts.
use super::*;
impl Registry {
    pub(super) fn register_processes(&mut self) {
        use crate::processes as p;
        for (name, parameters, variants) in [
            (
                "Process.Event",
                vec!["m".into()],
                vec![
                    ("Message", vec![Type::Generic("m".into())]),
                    ("Down", vec![p::monitor_ref(), p::identity(), p::reason()]),
                    ("Exit", vec![p::identity(), p::reason()]),
                ],
            ),
            (
                "Process.ExitReason",
                vec![],
                vec![
                    ("Normal", vec![]),
                    ("Shutdown", vec![]),
                    ("ShutdownDetail", vec![Type::String]),
                    ("Fault", vec![Type::Int]),
                    ("Failure", vec![Type::String]),
                    ("Kill", vec![]),
                    ("Killed", vec![]),
                    ("NoProcess", vec![]),
                ],
            ),
            (
                "Process.Error",
                vec![],
                vec![
                    ("ResourceLimit", vec![]),
                    ("ForeignInvocation", vec![]),
                    ("WrongMonitorOwner", vec![]),
                    ("UnsupportedTarget", vec![]),
                    ("InvalidOptions", vec![]),
                ],
            ),
            (
                "Process.DemonitorOptions",
                vec![],
                vec![("DemonitorOptions", vec![Type::Bool, Type::Bool])],
            ),
        ] {
            let record = name == "Process.DemonitorOptions";
            let variants = variants
                .into_iter()
                .enumerate()
                .map(|(tag, (variant, fields))| {
                    let qualified = format!("Process.{variant}");
                    self.constructors
                        .insert(qualified.clone(), (name.into(), tag));
                    self.constructors
                        .insert(format!("{name}.{variant}"), (name.into(), tag));
                    ast::Variant {
                        name: qualified,
                        span: Span::default(),
                        fields: fields
                            .into_iter()
                            .enumerate()
                            .map(|(index, ty)| ast::Field {
                                name: record
                                    .then(|| if index == 0 { "flush" } else { "info" }.into()),
                                ty,
                                span: Span::default(),
                            })
                            .collect(),
                    }
                })
                .collect();
            self.declarations.insert(
                name.into(),
                ast::TypeDecl {
                    public: true,
                    derives: vec![],
                    name: name.into(),
                    parameters,
                    record,
                    variants,
                    span: Span::default(),
                },
            );
        }
    }
}
