//! Immutable descriptor tables bind runtime callbacks to exact semantic layouts and closure identities.
use super::*;

impl Emitter<'_> {
    /// Describe every known closure identity before runtime registration validates capture layouts.
    pub(in crate::lowering) fn actor_descriptors(&mut self) -> Lowering<()> {
        if !self.actors.active {
            return Ok(());
        }
        let functions: Vec<_> = self.functions.values().copied().collect();
        let mut table = Vec::new();
        for function in functions {
            let id = function.id.0;
            let mut captures = Vec::new();
            for capture in &function.captures {
                captures.push(self.actor_type(&capture.ty)?);
            }
            let captures = self.actor_array(&format!("actor_captures{id}"), &captures);
            let mailbox = function
                .mailbox
                .as_ref()
                .or_else(|| self.actors.steps.get(&id))
                .or_else(|| self.actors.selectors.get(&id))
                .cloned();
            let mailbox = mailbox
                .as_ref()
                .map(|ty| self.actor_type(ty))
                .transpose()?
                .unwrap_or_else(|| "0".into());
            let identity = if self.actors.entries.contains_key(&id) {
                self.data.data(
                    &format!("$actor_identity{}", id),
                    vec![DataValue::Word(native_operand(&(id).to_string()))],
                );
                format!("$actor_identity{id}")
            } else {
                format!("$f{id}")
            };
            let selector = self.actors.selectors.contains_key(&id);
            let (step, select) = if selector {
                ("0".into(), format!("$actor_callback{id}"))
            } else {
                (format!("$actor_callback{id}"), "0".into())
            };
            self.data.data(
                &format!("$actor_descriptor{}", id),
                vec![
                    DataValue::Word(native_operand(&(identity))),
                    DataValue::Word(native_operand(&(step))),
                    DataValue::Word(native_operand(&(select))),
                    DataValue::Word(native_operand(&(function.captures.len()).to_string())),
                    DataValue::Word(native_operand(&(captures))),
                    DataValue::Word(native_operand(&(mailbox))),
                ],
            );
            table.push(format!("$actor_descriptor{id}"));
            self.actor_callback(function, selector);
        }
        self.actor_array("actor_functions", &table);
        Ok(())
    }

    /// Bridge only validated step/selector signatures; unrelated callable entries cannot be spawned.
    fn actor_callback(&mut self, function: &Function, selector: bool) {
        let id = function.id.0;
        let target = self.actors.entries.get(&id).copied().unwrap_or(id);
        let generated = self.actors.steps.contains_key(&target);
        let mut params = vec![(Scalar::I64, "%exec".into()), (Scalar::I64, "%env".into())];
        if selector {
            params.push((Scalar::I64, "%payload".into()));
        }
        self.output.begin(
            &format!("$actor_callback{id}"),
            Some(Scalar::I64),
            params,
            false,
        );
        self.output.statement(Statement::Label("@start".to_owned()));
        if !selector
            && (!function.params.is_empty() || (!generated && function.return_type != Type::Unit))
        {
            self.output
                .statement(Statement::Return(Some(native_operand("3"))));
            self.output.end();
            return;
        }
        self.output.statement(Statement::Assign {
            destination: "%fault".to_owned(),
            ty: Scalar::I64,
            operation: NativeOperation::Call {
                callee: native_operand("$fern_managed_fault"),
                args: vec![(Scalar::I64, native_operand("%exec"))],
                variadic: None,
            },
        });
        let mut args = vec![
            (Scalar::I64, native_operand("%env")),
            (Scalar::I64, native_operand("%fault")),
        ];
        if self.actors.managed.contains(&target) {
            args.push((Scalar::I64, native_operand("%exec")));
        }
        if selector {
            let ty = &function.params[0].ty;
            let width = self.width(ty.clone());
            let value = if width == 'l' {
                "%payload"
            } else {
                self.output.statement(Statement::Assign {
                    destination: "%decoded".into(),
                    ty: machine_width(width),
                    operation: NativeOperation::Unary(
                        if width == 'd' {
                            MachineUnary::Cast
                        } else {
                            MachineUnary::Copy
                        },
                        native_operand("%payload"),
                    ),
                });
                "%decoded"
            };
            args.push((machine_width(width), native_operand(value)));
        }
        if generated || selector {
            self.output.statement(Statement::Assign {
                destination: "%status".to_owned(),
                ty: Scalar::I64,
                operation: NativeOperation::Call {
                    callee: native_operand(&format!("$f{}", target)),
                    args: args.clone(),
                    variadic: None,
                },
            });
            self.output
                .statement(Statement::Return(Some(native_operand("%status"))));
            self.output.end();
        } else {
            self.output
                .statement(Statement::Effect(NativeOperation::Call {
                    callee: native_operand(&format!("$f{}", target)),
                    args: args.clone(),
                    variadic: None,
                }));
            self.output
                .statement(Statement::Return(Some(native_operand("2"))));
            self.output.end();
        }
    }

    /// Deduplicate by semantic type, reserving identity before descending recursive nominal fields.
    pub(super) fn actor_type(&mut self, ty: &Type) -> Lowering<String> {
        if let Some(id) = self.actors.types.get(ty) {
            return Ok(format!("$actor_type{id}"));
        }
        if self.actors.types.len() >= 4096 {
            return Err(invalid(
                Span::default(),
                "actor type descriptor limit exceeded",
            ));
        }
        let id = self.actors.types.len();
        self.actors.types.insert(ty.clone(), id);
        let (kind, groups) = self.actor_shape(ty)?;
        let mut children = Vec::new();
        for group in &groups {
            for child in group {
                children.push(self.actor_type(child)?);
            }
        }
        let child_array = self.actor_array(&format!("actor_children{id}"), &children);
        let count = if kind == 4 {
            groups.len()
        } else {
            children.len()
        };
        let arities = if kind == 4 {
            let lengths: Vec<_> = groups.iter().map(|group| group.len().to_string()).collect();
            self.actor_array(&format!("actor_arities{id}"), &lengths)
        } else {
            "0".into()
        };
        self.data.data(
            &format!("$actor_type{}", id),
            vec![
                DataValue::Word(native_operand(&(kind).to_string())),
                DataValue::Word(native_operand(&(count).to_string())),
                DataValue::Word(native_operand(&(child_array))),
                DataValue::Word(native_operand(&(arities))),
            ],
        );
        Ok(format!("$actor_type{id}"))
    }

    /// Translate approved immutable language layouts, retaining tags and full-width payload identity.
    fn actor_shape(&self, ty: &Type) -> Lowering<(usize, Vec<Vec<Type>>)> {
        Ok(match ty {
            Type::Int | Type::Bool | Type::Unit | Type::Float => (0, vec![]),
            Type::String => (1, vec![]),
            Type::List(item) => (2, vec![vec![*item.clone()]]),
            Type::Tuple(fields) => (3, vec![fields.clone()]),
            Type::Option(item) => (4, vec![vec![*item.clone()], vec![]]),
            Type::Result(ok, err) => (4, vec![vec![*ok.clone()], vec![*err.clone()]]),
            Type::Union(members) => (4, members.iter().map(|ty| vec![ty.clone()]).collect()),
            Type::Named(_, _) => {
                let layout = self
                    .layouts
                    .get(ty)
                    .ok_or_else(|| invalid(Span::default(), "unknown actor nominal layout"))?;
                (
                    if layout.storage == ir::LayoutStorage::Unboxed {
                        5
                    } else {
                        4
                    },
                    layout.variants.clone(),
                )
            }
            Type::Pid(mailbox) => (6, vec![vec![*mailbox.clone()]]),
            Type::Function(_, _) | Type::ActorFunction(_, _) => (7, vec![]),
            Type::Map(key, value) => (9, vec![vec![*key.clone(), *value.clone()]]),
            Type::Range => (10, vec![]),
            Type::Native(crate::runtime::NativeType::JsonValue) => (12, vec![]),
            Type::Native(_) => (11, vec![]),
            _ => {
                return Err(invalid(
                    Span::default(),
                    "native handle is not supported in actor frame accounting",
                ));
            }
        })
    }

    /// Empty descriptor vectors use null; every nonempty vector has an immutable fixed-length object.
    fn actor_array(&mut self, name: &str, values: &[String]) -> String {
        if values.is_empty() {
            return "0".into();
        }
        let entries = values
            .iter()
            .map(|value| DataValue::Word(native_operand(value)))
            .collect();
        self.data.data(&format!("${name}"), entries);
        format!("${name}")
    }
}
