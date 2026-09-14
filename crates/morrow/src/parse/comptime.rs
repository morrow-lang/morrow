//! Constants reuse the ordinary function body grammar and checked value representation.
use super::*;

impl Parser {
    pub(super) fn constant(&mut self, public: bool) -> ParseResult<Function> {
        let start = self.take().span.start;
        let (name, _) = self.name()?;
        if name == "main" {
            return Err(self.error("the program entry must be a function, not a constant"));
        }
        let return_type = if self.eat(&Kind::Colon) {
            Some(self.ty()?)
        } else {
            None
        };
        self.expect(Kind::Assign, "expected '=' after constant name or type")?;
        if !self.word("comptime") {
            return Err(self.error("constant initializer requires 'comptime:'"));
        }
        self.take();
        self.expect(Kind::Colon, "expected ':' after comptime")?;
        let body = self.suite()?;
        Ok(Function {
            constraints: Vec::new(),
            public,
            name,
            params: Vec::new(),
            return_type,
            guard: None,
            syntax: FunctionSyntax::Constant,
            group_start: start,
            span: Span {
                start,
                end: body.node.span.end,
            },
            body: body.node,
        })
    }
}
