//! Distinct nominal identities use a single payload without introducing tagged storage.
use super::*;

/// Normalize only declaration metadata; expressions retain explicit unboxed operations.
pub(super) fn declaration(decl: &ast::NewtypeDecl) -> Checked<ast::TypeDecl> {
    let capitalized = |name: &str| {
        name.rsplit('.')
            .next()
            .unwrap_or(name)
            .chars()
            .next()
            .is_some_and(char::is_uppercase)
    };
    let parameters: HashSet<_> = decl.parameters.iter().collect();
    let builtin = |name: &str| {
        matches!(
            name.rsplit('.').next().unwrap_or(name),
            "Int" | "Float" | "Bool" | "Unit" | "Never"
        )
    };
    if builtin(&decl.name)
        || builtin(&decl.constructor)
        || !capitalized(&decl.name)
        || !capitalized(&decl.constructor)
        || parameters.len() != decl.parameters.len()
        || decl
            .parameters
            .iter()
            .any(|name| !name.bytes().next().is_some_and(|b| b.is_ascii_lowercase()))
    {
        return Err(Diagnostic::new(
            decl.span,
            "invalid newtype name, constructor or generic parameter",
        ));
    }
    Ok(ast::TypeDecl {
        derives: decl.derives.clone(),
        public: decl.public,
        name: decl.name.clone(),
        parameters: decl.parameters.clone(),
        record: false,
        span: decl.span,
        variants: vec![ast::Variant {
            name: decl.constructor.clone(),
            span: decl.constructor_span,
            fields: vec![ast::Field {
                name: None,
                ty: decl.inner.clone(),
                span: decl.inner_span,
            }],
        }],
    })
}

impl Registry {
    /// Share bounded declaration metadata with intrinsic capability checking.
    pub(in crate::check) fn newtype_definitions(
        &self,
    ) -> std::rc::Rc<HashMap<String, (Vec<String>, Type)>> {
        self.newtype_definitions.clone()
    }
    /// Share one aggregate representation budget across inference, finalization and tooling.
    pub(in crate::check) fn newtype_budget(&self) -> std::rc::Rc<std::cell::Cell<usize>> {
        self.newtype_work.clone()
    }
    /// Charge payload expansion across every declaration and instantiated representation.
    fn charge_newtype(&self, ty: &Type, span: Span) -> Checked<()> {
        charge_newtype_type(&self.newtype_work, ty, span)
    }
    /// Test nominal storage identity without exposing or expanding its type arguments.
    pub(in crate::check) fn is_newtype(&self, ty: &Type) -> bool {
        matches!(ty, Type::Named(name, _) if self.newtypes.contains(name))
    }
    /// Instantiate exactly one layer; existing containers and tagged types remain opaque.
    pub(in crate::check) fn newtype_inner(&self, ty: &Type, span: Span) -> Checked<Type> {
        if !self.is_newtype(ty) {
            return Err(Diagnostic::new(
                span,
                "operation requires a distinct newtype",
            ));
        }
        self.charge_newtype(ty, span)?;
        let mut layout = self.layout(ty, span)?;
        self.charge_newtype(&layout.variants[0][0], span)?;
        Ok(layout.variants.remove(0).remove(0))
    }
    /// Follow unboxed layers with finite type keys and a bound before every substitution.
    pub(in crate::check) fn representation(&self, ty: &Type, span: Span) -> Checked<Type> {
        self.charge_newtype(ty, span)?;
        let mut current = ty.clone();
        let mut seen = HashSet::new();
        for _ in 0..MAX_TYPE_DEPTH {
            if !self.is_newtype(&current) {
                return Ok(current);
            }
            if !seen.insert(current.clone()) {
                return Err(Diagnostic::new(
                    span,
                    "recursive newtype has no finite unboxed representation",
                ));
            }
            current = self.newtype_inner(&current, span)?;
        }
        Err(Diagnostic::new(
            span,
            "newtype representation expansion limit exceeded",
        ))
    }
    /// Reject impossible generic declaration representations even when never instantiated.
    pub(super) fn validate_newtypes(&self, program: &ast::Program) -> Checked<()> {
        for decl in &program.newtypes {
            let ty = Type::Named(
                decl.name.clone(),
                decl.parameters.iter().cloned().map(Type::Generic).collect(),
            );
            self.representation(&ty, decl.span)?;
        }
        Ok(())
    }
}

impl super::super::Checker<'_> {
    /// Erase only explicitly supported equality operands after preserving nominal unification.
    pub(in crate::check) fn unwrap_equality(&self, value: &mut ir::Expr) -> Checked<()> {
        for _ in 0..MAX_TYPE_DEPTH {
            if !self.registry.is_newtype(&value.ty) {
                return Ok(());
            }
            let ty = self.registry.newtype_inner(&value.ty, value.span)?;
            let old = value.clone();
            *value = ir::Expr {
                kind: ir::ExprKind::Unwrap(Box::new(old)),
                ty,
                span: value.span,
            };
        }
        Err(Diagnostic::new(
            value.span,
            "newtype equality expansion limit exceeded",
        ))
    }
}

impl super::super::Checker<'_> {
    /// Lift an unboxed constructor through the existing closure ABI without boxing its result.
    pub(in crate::check) fn newtype_constructor_value(
        &mut self,
        name: &str,
        span: Span,
    ) -> Checked<super::super::TypedKind> {
        let (result, _, mut fields) = self.pattern_signature(name, span)?;
        let payload = fields.remove(0);
        let id = ir::LocalId(self.local_count);
        self.local_count += 1;
        let value = ir::Expr {
            kind: ir::ExprKind::Local(id),
            ty: payload.clone(),
            span,
        };
        let body = ir::Expr {
            kind: ir::ExprKind::Wrap(Box::new(value)),
            ty: result.clone(),
            span,
        };
        Ok((
            ir::ExprKind::Lambda {
                params: vec![ir::Param {
                    id,
                    ty: payload.clone(),
                }],
                captures: Vec::new(),
                body: Box::new(body),
                local_count: self.local_count,
            },
            Type::Function(vec![payload], Box::new(result)),
        ))
    }
}

/// Charge storage and intrinsic expansion even outside whole-signature inference probes.
pub(in crate::check) fn charge_newtype_type(
    work: &std::cell::Cell<usize>,
    ty: &Type,
    span: Span,
) -> Checked<()> {
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        let used = work.get().saturating_add(1);
        if used > 400_000 {
            return Err(Diagnostic::new(
                span,
                "newtype representation work limit exceeded",
            ));
        }
        work.set(used);
        match ty {
            Type::Union(xs) | Type::Named(_, xs) | Type::Tuple(xs) => pending.extend(xs),
            Type::Function(xs, result) => {
                pending.extend(xs);
                pending.push(result);
            }
            Type::Pid(t) | Type::List(t) | Type::Option(t) => pending.push(t),
            Type::ActorFunction(a, b) | Type::Map(a, b) | Type::Result(a, b) => {
                pending.extend([a.as_ref(), b.as_ref()])
            }
            _ => {}
        }
    }
    Ok(())
}
