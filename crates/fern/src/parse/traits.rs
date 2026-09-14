//! Trait syntax shares the ordinary type, parameter and expression parser.
use super::*;
use crate::ast::{Implementation, TraitBound, TraitDecl, TraitMethod};

impl Parser {
    pub(super) fn trait_bounds(&mut self) -> ParseResult<Vec<TraitBound>> {
        let mut bounds = Vec::new();
        loop {
            if bounds.len() >= 32 {
                return Err(self.error("trait constraint limit exceeded"));
            }
            bounds.push(self.trait_bound()?);
            if !self.eat(&Kind::Comma) {
                return Ok(bounds);
            }
        }
    }

    fn trait_bound(&mut self) -> ParseResult<TraitBound> {
        let (name, mut span) = self.qualified_name()?;
        if let Some(spans) = &mut self.type_spans {
            spans.push(span);
        }
        self.expect(Kind::Left, "expected '(' after trait name")?;
        let ty = self.ty()?;
        span.end = self
            .expect(Kind::Right, "traits require exactly one type argument")?
            .span
            .end;
        Ok(TraitBound { name, ty, span })
    }

    pub(super) fn trait_declaration(
        &mut self,
        program: &mut Program,
        public: bool,
    ) -> ParseResult<()> {
        let start = self.take().span.start;
        let bound = self.trait_bound()?;
        let Type::Generic(parameter) = bound.ty else {
            return Err(self.error("trait parameter must be a lowercase type variable"));
        };
        let parents = if self.word("with") {
            self.take();
            self.trait_bounds()?
        } else {
            Vec::new()
        };
        self.expect(Kind::Colon, "expected ':' after trait declaration")?;
        self.expect(Kind::Newline, "expected indented trait methods")?;
        self.expect(Kind::Indent, "expected indented trait methods")?;
        let mut methods = Vec::new();
        while !self.eat(&Kind::Dedent) {
            if self.eat(&Kind::Newline) {
                continue;
            }
            if methods.len() >= 255 {
                return Err(self.error("trait method limit exceeded"));
            }
            if !self.word("fn") {
                return Err(self.error("expected trait method signature"));
            }
            let method_start = self.take().span.start;
            let (name, _) = self.name()?;
            self.expect(Kind::Left, "expected '(' after method name")?;
            let mut params = self.function_parameters()?;
            for (index, param) in params.iter_mut().enumerate() {
                if param.annotation.is_none() {
                    let PatternKind::Bind(ty) = &param.pattern.kind else {
                        return Err(Diagnostic::new(
                            param.span,
                            "trait method parameters require type annotations",
                        ));
                    };
                    param.annotation = Some(if ty == &parameter {
                        Type::Generic(parameter.clone())
                    } else {
                        return Err(Diagnostic::new(
                            param.span,
                            "trait method parameters require type annotations",
                        ));
                    });
                    param.pattern.kind = PatternKind::Bind(format!("$traitarg{index}"));
                }
            }
            self.expect(Kind::Arrow, "trait methods require an explicit result type")?;
            let return_type = Some(self.ty()?);
            let mut constraints = vec![TraitBound {
                name: bound.name.clone(),
                ty: Type::Generic(parameter.clone()),
                span: bound.span,
            }];
            if self.word("where") {
                self.take();
                constraints.extend(self.trait_bounds()?);
            }
            let mut function = Function {
                constraints,
                public,
                name: name.clone(),
                params,
                return_type,
                guard: None,
                syntax: FunctionSyntax::Trait,
                group_start: method_start,
                body: Expr {
                    kind: ExprKind::Unit,
                    span: self.current().span,
                },
                span: Span {
                    start: method_start,
                    end: self.current().span.start,
                },
            };
            let default = if self.eat(&Kind::Colon) {
                let body = self.suite()?.node;
                function.span.end = body.span.end;
                let mut implementation = function.clone();
                implementation.name = format!("$default{}_{name}", program.traits.len());
                implementation.public = false;
                implementation.syntax = FunctionSyntax::Colon;
                implementation.body = body;
                let name = implementation.name.clone();
                program.functions.push(implementation);
                Some(name)
            } else {
                self.line_end()?;
                None
            };
            methods.push(TraitMethod {
                name: name.clone(),
                function: name,
                default,
            });
            program.functions.push(function);
        }
        if methods.is_empty() {
            return Err(self.error("trait requires at least one method"));
        }
        program.traits.push(TraitDecl {
            public,
            name: bound.name,
            parameter,
            parents,
            methods,
            span: Span {
                start,
                end: self.tokens[self.position - 1].span.end,
            },
        });
        Ok(())
    }

    pub(super) fn implementation(&mut self, program: &mut Program) -> ParseResult<()> {
        let start = self.take().span.start;
        let bound = self.trait_bound()?;
        let constraints = if self.word("where") {
            self.take();
            self.trait_bounds()?
        } else {
            Vec::new()
        };
        self.expect(Kind::Colon, "expected ':' after implementation")?;
        self.expect(Kind::Newline, "expected indented implementation methods")?;
        self.expect(Kind::Indent, "expected indented implementation methods")?;
        let mut methods = Vec::new();
        while !self.eat(&Kind::Dedent) {
            if self.eat(&Kind::Newline) {
                continue;
            }
            if methods.len() >= 255 {
                return Err(self.error("implementation method limit exceeded"));
            }
            if !self.word("fn") {
                return Err(self.error("expected implementation method"));
            }
            let mut function = self.function(false)?;
            let name = function.name.clone();
            function.name = format!("$impl{}_{name}", program.implementations.len());
            function.constraints.extend(constraints.clone());
            if let Some((_, previous)) = methods.iter().find(|(method, _)| method == &name) {
                let previous_function = program
                    .functions
                    .last()
                    .filter(|f| &f.name == previous)
                    .ok_or_else(|| {
                        Diagnostic::new(
                            function.span,
                            "implementation method clauses must be contiguous",
                        )
                    })?;
                function.group_start = previous_function.group_start;
            } else {
                methods.push((name, function.name.clone()));
            }
            program.functions.push(function);
        }
        program.implementations.push(Implementation {
            bound,
            constraints,
            methods,
            span: Span {
                start,
                end: self.tokens[self.position - 1].span.end,
            },
        });
        Ok(())
    }
}
