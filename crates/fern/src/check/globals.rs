//! Check explicitly resolved declarations without reinterpreting their canonical roots as locals.
use super::*;
impl Checker<'_> {
    /// Dispatch proven global values and calls while preserving lexical argument evaluation.
    pub(super) fn resolved_global(
        &mut self,
        kind: &ast::ExprKind,
        expected: Option<&Type>,
        span: Span,
        depth: usize,
    ) -> Checked<TypedKind> {
        match kind {
            ast::ExprKind::GlobalName { resolved, .. } => self.global_name(resolved, span),
            ast::ExprKind::GlobalCall { resolved, args, .. } => {
                self.global_call(resolved, args, expected, span, depth)
            }
            ast::ExprKind::GlobalPipe {
                value,
                resolved,
                args,
                position,
                label,
                ..
            } => self.pipe(
                value,
                (resolved, true),
                args,
                (*position, label),
                span,
                depth,
            ),
            _ => unreachable!("only global source references reach this dispatcher"),
        }
    }
    /// Resolve a proven global without consulting lexical bindings of its canonical prefix.
    pub(super) fn global_name(&mut self, name: &str, span: Span) -> Checked<TypedKind> {
        if self
            .signatures
            .get(name)
            .is_some_and(|signature| signature.constant)
        {
            return self.constant_reference(name, span);
        }
        for (offset, _) in name.match_indices('.').rev() {
            let root = &name[..offset];
            if self
                .signatures
                .get(root)
                .is_some_and(|signature| signature.constant)
            {
                let (kind, ty) = self.constant_reference(root, span)?;
                let mut value = ir::Expr { kind, ty, span };
                for field in name[offset + 1..].split('.') {
                    let (kind, ty) = self.field(value, field, span)?;
                    value = ir::Expr { kind, ty, span };
                }
                return Ok((value.kind, value.ty));
            }
        }
        if crate::ffi::is_api(name) {
            return self.foreign_api_value(name, span);
        }
        if sets::is_api(name) {
            return self.set_function(name, span);
        }
        if let Some(value) = self.actor_name(name, span)? {
            return Ok(value);
        }
        if self.registry.is_alias(name)
            && !self.signatures.contains_key(name)
            && self.registry.constructor(name).is_none()
        {
            return Err(Diagnostic::new(
                span,
                "a type alias does not introduce a value or constructor",
            ));
        }
        if let Some((owner, _, _, _)) = self.registry.constructor(name) {
            if self.registry.is_newtype(&Type::Named(owner, Vec::new())) {
                return self.newtype_constructor_value(name, span);
            }
            return self.custom_construct(name, &[], None, span, 0);
        }
        if name == "None" {
            return Ok((
                ir::ExprKind::Construct {
                    constructor: Constructor::None,
                    value: None,
                },
                Type::Option(Box::new(self.inference.fresh())),
            ));
        }
        if builtin(name).is_some()
            || self.signatures.contains_key(name)
            || runtime::lookup(name).is_some()
        {
            let (target, params, result) = self.resolve_callable(name, span)?;
            return Ok((
                ir::ExprKind::FunctionValue { target },
                Type::Function(params, Box::new(result)),
            ));
        }
        Err(Diagnostic::new(
            span,
            format!("unknown name '{name}'{}", self.value_hint(name)),
        ))
    }

    fn constant_reference(&mut self, name: &str, span: Span) -> Checked<TypedKind> {
        let (target, params, result) = self.resolve_callable(name, span)?;
        if !params.is_empty() {
            return Err(Diagnostic::new(span, "constant cannot have parameters"));
        }
        Ok((
            ir::ExprKind::Call {
                target,
                args: Vec::new(),
            },
            result,
        ))
    }

    pub(super) fn constant_path(&self, name: &str) -> bool {
        self.signatures
            .get(name)
            .is_some_and(|signature| signature.constant)
            || name.match_indices('.').any(|(offset, _)| {
                self.signatures
                    .get(&name[..offset])
                    .is_some_and(|signature| signature.constant)
            })
    }
}
