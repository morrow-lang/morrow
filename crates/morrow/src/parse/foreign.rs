//! Trusted C declarations synthesize ordinary source wrappers with explicit ABI metadata.
use super::*;
use crate::ffi::Declaration;
impl Parser {
    pub(super) fn foreign_function(&mut self, public: bool) -> ParseResult<Function> {
        let start = self.take().span.start;
        let abi = self.foreign_text("expected foreign ABI string \"C\"")?;
        if abi != "C" {
            return Err(self.error("only the explicit C foreign ABI is supported"));
        }
        if !self.word("fn") {
            return Err(self.error("expected fn after foreign ABI"));
        }
        self.take();
        let (name, _) = self.name()?;
        self.expect(Kind::Left, "expected '(' after foreign function name")?;
        let params = self.function_parameters()?;
        let mut arguments = Vec::new();
        let mut physical = Vec::new();
        for param in &params {
            let PatternKind::Bind(name) = &param.pattern.kind else {
                return Err(Diagnostic::new(
                    param.span,
                    "foreign parameters require simple named bindings",
                ));
            };
            let ty = param.annotation.as_ref().ok_or_else(|| {
                Diagnostic::new(param.span, "foreign parameters require explicit ABI types")
            })?;
            physical.push(crate::ffi::abi_type(ty, param.span)?);
            arguments.push(Expr {
                kind: ExprKind::Name(name.clone()),
                span: param.span,
            });
        }
        self.expect(
            Kind::Arrow,
            "foreign function requires explicit return type",
        )?;
        let result = self.ty()?;
        let result_abi = crate::ffi::abi_type(&result, self.current().span)?;
        let symbol = if self.word("as") {
            self.take();
            self.foreign_text("expected C symbol string after as")?
        } else {
            name.clone()
        };
        let library = if self.word("from") {
            self.take();
            Some(self.foreign_text("expected library string after from")?)
        } else {
            None
        };
        let span = Span {
            start,
            end: self.current().span.start,
        };
        self.line_end()?;
        let declaration = Declaration {
            symbol,
            library,
            params: physical,
            result: result_abi,
        };
        declaration.validate(span)?;
        Ok(Function {
            constraints: vec![],
            guard: None,
            group_start: start,
            syntax: FunctionSyntax::Foreign,
            public,
            name,
            params,
            return_type: Some(result),
            body: Expr {
                kind: ExprKind::ForeignCall {
                    declaration,
                    args: arguments,
                },
                span,
            },
            span,
        })
    }
    fn foreign_text(&mut self, message: &str) -> ParseResult<String> {
        match self.take().kind {
            Kind::Text(value) => Ok(value),
            _ => Err(self.error(message)),
        }
    }
}
