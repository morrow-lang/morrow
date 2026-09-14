//! Foreign wrappers are fully checked source functions with explicit trusted native effects.
use super::*;
impl Checker<'_> {
    pub(super) fn foreign_call(
        &mut self,
        declaration: &crate::ffi::Declaration,
        args: &[ast::Expr],
        span: Span,
        depth: usize,
    ) -> Checked<TypedKind> {
        declaration.validate(span)?;
        if args.len() != declaration.params.len() {
            return Err(Diagnostic::new(
                span,
                "foreign call arity differs from its declaration",
            ));
        }
        let mut checked = Vec::new();
        for (arg, abi) in args.iter().zip(&declaration.params) {
            let ty = abi.source_type();
            self.registry.validate(&ty, &HashSet::new(), arg.span)?;
            checked.push(self.expression_expected(arg, Some(&ty), depth)?);
        }
        let result = declaration.result.source_type();
        self.registry.validate(&result, &HashSet::new(), span)?;
        Ok((
            ir::ExprKind::ForeignCall {
                declaration: declaration.clone(),
                args: checked,
            },
            result,
        ))
    }
}
