//! Union-bearing argument syntax is a type candidate, never a runtime type witness.
use super::*;

const LOOKAHEAD_LIMIT: usize = 400_000;

impl Parser {
    /// Preserve ordinary expression syntax; only a new complete type-only union takes this path.
    pub(super) fn argument_value(&mut self) -> ParseResult<Parsed> {
        if !self.union_argument()? {
            return self.expr(0);
        }
        let start = self.current().span.start;
        let ty = self.ty()?;
        if !matches!(self.current().kind, Kind::Comma | Kind::Right) {
            return Err(self.error("expected ',' or ')' after static type argument"));
        }
        let end = self.tokens[self.position - 1].span.end;
        expression(ExprKind::TypeTarget(ty), Span { start, end }, 1)
    }

    /// Build linear token metadata once, after editor recovery finishes changing its token stream.
    fn union_argument(&mut self) -> ParseResult<bool> {
        let span = self.current().span;
        charge(&mut self.type_argument_work, 1, span)?;
        if self.type_argument_shapes.is_none() {
            self.type_argument_shapes = Some(Shapes::build(
                &self.tokens,
                &mut self.type_argument_work,
                span,
            )?);
        }
        Ok(self
            .type_argument_shapes
            .as_ref()
            .is_some_and(|shape| shape.candidate(&self.tokens, self.position)))
    }
}

#[derive(Clone, Copy, Default)]
struct Counts {
    unions: usize,
    invalid: usize,
}

/// Prefix counts and argument boundaries make every subsequent recognition query constant time.
pub(super) struct Shapes {
    ends: Vec<usize>,
    counts: Vec<Counts>,
}
impl Shapes {
    /// Precharge metadata arrays, delimiter stack and both linear passes before any allocation or token read.
    fn build(tokens: &[Token], work: &mut usize, span: Span) -> ParseResult<Self> {
        charge(work, tokens.len().saturating_mul(5).saturating_add(1), span)?;
        let mut counts = Vec::with_capacity(tokens.len() + 1);
        let mut count = Counts::default();
        counts.push(count);
        for (index, token) in tokens.iter().enumerate() {
            count.unions += usize::from(token.kind == Kind::Bar);
            count.invalid += usize::from(!type_token(tokens, index));
            counts.push(count);
        }
        let mut ends = vec![tokens.len(); tokens.len()];
        let mut outer = Vec::new();
        let mut end = tokens.len();
        for (index, token) in tokens.iter().enumerate().rev() {
            match token.kind {
                Kind::Right => {
                    outer.push(end);
                    end = index;
                }
                Kind::Left => end = outer.pop().unwrap_or(tokens.len()),
                Kind::Comma => end = index,
                _ => {}
            }
            ends[index] = end;
        }
        Ok(Self { ends, counts })
    }

    /// Syntax is merely a candidate: canonical decoder-slot checking alone establishes type context.
    fn candidate(&self, tokens: &[Token], start: usize) -> bool {
        let Some(&end) = self.ends.get(start) else {
            return false;
        };
        let Some(token) = tokens.get(end) else {
            return false;
        };
        matches!(token.kind, Kind::Right | Kind::Comma)
            && self.counts[end].unions > self.counts[start].unions
            && self.counts[end].invalid == self.counts[start].invalid
    }
}

/// Exclude executable calls and arrows without treating uppercase spelling as semantic authority.
fn type_token(tokens: &[Token], index: usize) -> bool {
    match &tokens[index].kind {
        Kind::Name(name) => {
            !tokens.get(index + 1).is_some_and(|t| t.kind == Kind::Left)
                || name.starts_with(char::is_uppercase)
        }
        Kind::Left | Kind::Right | Kind::Comma | Kind::Dot | Kind::Bar => true,
        _ => false,
    }
}

/// Retain one shared preinspection budget; cached queries cannot restart it.
fn charge(work: &mut usize, amount: usize, span: Span) -> ParseResult<()> {
    *work = work.saturating_add(amount);
    if *work > LOOKAHEAD_LIMIT {
        Err(Diagnostic::new(
            span,
            "static type argument lookahead limit exceeded",
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scanning_is_read_only_and_shares_one_preinspection_budget() {
        let mut parser = source_parser("f(Int | String)", true).unwrap();
        parser.position = 2;
        assert!(parser.union_argument().unwrap());
        assert_eq!(parser.position, 2);
        assert!(parser.type_spans.as_ref().unwrap().is_empty());
        parser.type_argument_work = LOOKAHEAD_LIMIT;
        let error = parser.union_argument().unwrap_err();
        assert!(error.message.contains("lookahead limit"));
        assert_eq!(parser.position, 2);
        assert!(parser.type_spans.unwrap().is_empty());
    }
    /// Every token can be queried without repeating any subtree walk or exhausting the shared cap.
    #[test]
    fn shape_metadata_matches_a_separate_balanced_scan_with_linear_total_work() {
        let mut parser=source_parser("f(List(Int | String), (Int | Bool,String), json.decode(x,Int | String), (x)->json.decode(x,Int | String), %{ x: y })", false).unwrap();
        for position in 0..parser.tokens.len() {
            parser.position = position;
            let expected = linear_candidate(&parser.tokens[position..]);
            assert_eq!(
                parser.union_argument().unwrap(),
                expected,
                "token {position}"
            );
        }
        assert_eq!(parser.type_argument_work, 6 * parser.tokens.len() + 1);
    }
    /// The test oracle directly follows nesting instead of reusing cached ranges or prefix counts.
    fn linear_candidate(tokens: &[Token]) -> bool {
        let mut nesting = 0;
        let mut found = false;
        for (index, token) in tokens.iter().enumerate() {
            match &token.kind {
                Kind::Right | Kind::Comma if nesting == 0 => return found,
                Kind::Left => nesting += 1,
                Kind::Right => nesting -= 1,
                Kind::Bar => found = true,
                Kind::Name(name) => {
                    if tokens.get(index + 1).is_some_and(|t| t.kind == Kind::Left)
                        && !name.chars().next().is_some_and(char::is_uppercase)
                    {
                        return false;
                    }
                }
                Kind::Comma | Kind::Dot => {}
                _ => return false,
            }
        }
        false
    }
}
