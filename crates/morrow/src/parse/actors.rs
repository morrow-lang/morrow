//! Indented selective receive syntax retains timeout expressions separately from messages.
use super::*;
impl Parser {
    /// Parse a bounded receive block; timeout is one final wildcard arm, never a message pattern.
    pub(super) fn receive_expression(&mut self) -> ParseResult<Parsed> {
        let start = self.take().span.start;
        self.expect(Kind::Colon, "expected ':' after receive")?;
        self.expect(Kind::Newline, "receive requires an indented arm block")?;
        self.expect(Kind::Indent, "receive requires an indented arm block")?;
        let mut arms = Vec::new();
        let mut timeout = None;
        let mut depth = 1;
        let mut end = start;
        for _ in 0..self.tokens.len() {
            if self.eat(&Kind::Dedent) {
                break;
            }
            if timeout.is_some() {
                return Err(self.error("receive timeout must be the final arm"));
            }
            if arms.len() >= 128 {
                return Err(self.error("receive arm limit exceeded (128)"));
            }
            let pattern = self.typed_pattern()?;
            if self.word("after") {
                if !matches!(pattern.kind, PatternKind::Wildcard) {
                    return Err(Diagnostic::new(
                        pattern.span,
                        "receive timeout requires '_'",
                    ));
                }
                self.take();
                let duration = self.guard_expression()?;
                self.expect(Kind::Arrow, "expected '->' after receive timeout")?;
                let body = self.suite()?;
                end = body.node.span.end;
                depth = depth.max(duration.depth + 1).max(body.depth + 1);
                timeout = Some((Box::new(duration.node), Box::new(body.node)));
            } else {
                let (arm, arm_depth) = self.receive_arm(pattern)?;
                end = arm.span.end;
                depth = depth.max(arm_depth);
                arms.push(arm);
            }
            if !self.eat(&Kind::Newline)
                && self.current().kind != Kind::Dedent
                && !self.previous_dedent()
            {
                return Err(self.error("expected end of line after receive arm"));
            }
        }
        if arms.is_empty() {
            return Err(self.error("receive requires at least one message arm"));
        }
        expression(
            ExprKind::Receive { arms, timeout },
            Span { start, end },
            depth,
        )
    }
    /// Parse one message arm while retaining the maximum child nesting and its independent scope.
    fn receive_arm(&mut self, pattern: Pattern) -> ParseResult<(MatchArm, usize)> {
        let guard = if self.word("if") {
            self.take();
            Some(self.guard_expression()?)
        } else {
            None
        };
        self.expect(Kind::Arrow, "expected '->' after receive pattern")?;
        let body = self.suite()?;
        let end = body.node.span.end;
        let mut depth = body.depth + 1;
        if let Some(guard) = &guard {
            depth = depth.max(guard.depth + 1);
        }
        let span = Span {
            start: pattern.span.start,
            end,
        };
        Ok((
            MatchArm {
                pattern,
                guard: guard.map(|v| v.node),
                body: body.node,
                span,
            },
            depth,
        ))
    }
}
