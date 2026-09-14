//! Deterministic, capability-free evaluation into ordinary typed constant data.
use super::*;
use crate::{Diagnostic, Span};

/// Evaluate all concrete constant declarations under a shared work budget.
/// The interactive evaluator supplies arithmetic and aggregate semantics, while
/// capability checks prevent host effects before they occur. No executable is run.
pub(crate) fn fold(source: &ast::Program, program: &mut ir::Program) -> Result<(), Diagnostic> {
    let names: std::collections::HashSet<_> = source
        .functions
        .iter()
        .filter(|f| f.syntax == ast::FunctionSyntax::Constant)
        .map(|f| f.name.as_str())
        .collect();
    if names.is_empty() {
        return Ok(());
    }
    let original = Rc::new(program.clone());
    let mut machine = Machine::new(original.clone(), HashMap::new());
    machine.comptime = true;
    let mut budget = Budget {
        nodes: 100_000,
        bytes: 1024 * 1024,
    };
    for function in &mut program.functions {
        if !names.contains(function.name.as_str()) {
            continue;
        }
        let span = function.body.span;
        if !function.params.is_empty()
            || !function.captures.is_empty()
            || function.mailbox.is_some()
        {
            return Err(error(span, "constant requires a closed, non-actor value"));
        }
        let value = machine
            .function(original.clone(), function.id, &[], Vec::new())
            .map_err(|failure| {
                error(
                    span,
                    match failure {
                        Failure::Message(message) => message,
                        Failure::JsonLimit => "JSON resource limit exceeded".into(),
                        _ => "control flow escaped the constant initializer".into(),
                    },
                )
            })?;
        function.body = budget.literal(&value, &function.return_type, &original, span, 0)?;
        function.local_count = 0;
    }
    Ok(())
}

fn error(span: Span, message: impl std::fmt::Display) -> Diagnostic {
    Diagnostic::new(span, format!("comptime evaluation failed: {message}"))
}

struct Budget {
    nodes: usize,
    bytes: usize,
}
impl Budget {
    fn literal(
        &mut self,
        value: &Value,
        ty: &Type,
        program: &ir::Program,
        span: Span,
        depth: usize,
    ) -> Result<ir::Expr, Diagnostic> {
        if depth >= 128 || self.nodes == 0 {
            return Err(error(span, "constant data limit exceeded"));
        }
        self.nodes -= 1;
        if let Some(layout) = program.types.iter().find(|layout| &layout.ty == ty)
            && layout.storage == ir::LayoutStorage::Unboxed
        {
            let inner = layout
                .variants
                .first()
                .and_then(|fields| fields.first())
                .ok_or_else(|| error(span, "missing constant newtype layout"))?;
            let value = self.literal(value, inner, program, span, depth + 1)?;
            return Ok(ir::Expr {
                kind: ir::ExprKind::Wrap(Box::new(value)),
                ty: ty.clone(),
                span,
            });
        }
        let kind = match (value, ty) {
            (Value::Int(n), Type::Int) => ir::ExprKind::Int(*n),
            (Value::Float(n), Type::Float) => ir::ExprKind::Float(*n),
            (Value::Bool(b), Type::Bool) => ir::ExprKind::Bool(*b),
            (Value::Unit, Type::Unit) => ir::ExprKind::Unit,
            (Value::String(text), Type::String) => {
                self.bytes = self
                    .bytes
                    .checked_sub(text.len())
                    .ok_or_else(|| error(span, "constant string budget exceeded"))?;
                ir::ExprKind::String(text.to_string())
            }
            (Value::List(values), Type::List(element)) => ir::ExprKind::List(
                values
                    .iter()
                    .map(|value| self.literal(value, element, program, span, depth + 1))
                    .collect::<Result<_, _>>()?,
            ),
            (Value::Map(entries), Type::Map(key, item)) => ir::ExprKind::Map(
                entries
                    .iter()
                    .map(|(k, v)| {
                        Ok((
                            self.literal(k, key, program, span, depth + 1)?,
                            self.literal(v, item, program, span, depth + 1)?,
                        ))
                    })
                    .collect::<Result<_, Diagnostic>>()?,
            ),
            (Value::Sum(tag, fields), Type::Tuple(types)) if *tag == 0 => {
                ir::ExprKind::Tuple(self.fields(fields, types, program, span, depth + 1)?)
            }
            (Value::Sum(tag, fields), Type::Option(inner)) => {
                let (constructor, value) = match (*tag, fields.as_slice()) {
                    (0, [value]) => (
                        crate::Constructor::Some,
                        Some(Box::new(self.literal(
                            value,
                            inner,
                            program,
                            span,
                            depth + 1,
                        )?)),
                    ),
                    (1, []) => (crate::Constructor::None, None),
                    _ => return Err(error(span, "invalid constant Option layout")),
                };
                ir::ExprKind::Construct { constructor, value }
            }
            (Value::Sum(tag, fields), Type::Result(ok, err)) => {
                let [value] = fields.as_slice() else {
                    return Err(error(span, "invalid constant Result layout"));
                };
                let (constructor, inner) = match tag {
                    0 => (crate::Constructor::Ok, ok),
                    1 => (crate::Constructor::Err, err),
                    _ => return Err(error(span, "invalid constant Result tag")),
                };
                ir::ExprKind::Construct {
                    constructor,
                    value: Some(Box::new(self.literal(
                        value,
                        inner,
                        program,
                        span,
                        depth + 1,
                    )?)),
                }
            }
            (Value::Sum(tag, fields), Type::Named(_, _)) => {
                let types = program
                    .types
                    .iter()
                    .find(|layout| &layout.ty == ty)
                    .and_then(|layout| layout.variants.get(*tag))
                    .ok_or_else(|| error(span, "missing constant type layout"))?;
                ir::ExprKind::CustomConstruct {
                    tag: *tag,
                    fields: self.fields(fields, types, program, span, depth + 1)?,
                }
            }
            (Value::Union(union), Type::Union(_)) => ir::ExprKind::UnionInject {
                value: Box::new(self.literal(
                    &union.value,
                    &union.member,
                    program,
                    span,
                    depth + 1,
                )?),
            },
            (Value::Closure(closure), Type::Function(_, _)) => {
                let target = program
                    .functions
                    .iter()
                    .find(|function| function.id == closure.function)
                    .ok_or_else(|| error(span, "missing constant callable"))?;
                let types: Vec<_> = target
                    .captures
                    .iter()
                    .map(|param| param.ty.clone())
                    .collect();
                ir::ExprKind::Closure {
                    function: closure.function,
                    captures: self.fields(&closure.captures, &types, program, span, depth + 1)?,
                }
            }
            _ => {
                return Err(error(
                    span,
                    "this value cannot be embedded as constant data",
                ));
            }
        };
        Ok(ir::Expr {
            kind,
            ty: ty.clone(),
            span,
        })
    }

    fn fields(
        &mut self,
        values: &[Value],
        types: &[Type],
        program: &ir::Program,
        span: Span,
        depth: usize,
    ) -> Result<Vec<ir::Expr>, Diagnostic> {
        if values.len() != types.len() {
            return Err(error(span, "constant field count mismatch"));
        }
        values
            .iter()
            .zip(types)
            .map(|(value, ty)| self.literal(value, ty, program, span, depth))
            .collect()
    }
}
