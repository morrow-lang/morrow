//! One token-level editor member hole; all positions remain in the original source text.
use super::*;

/// Opaque parser-proven member site; callers cannot fabricate field or receiver identities.
#[derive(Clone, Debug)]
pub(crate) struct HoleSite {
    selector: Span,
    prefix: String,
    original: String,
    field: Option<(Span, Span)>,
}
impl HoleSite {
    /// Record the original receiver and field once, excluding unrelated parser nodes.
    pub(super) fn attach(&mut self, receiver: Span, field: Span) -> ParseResult<()> {
        if self.field.replace((receiver, field)).is_some() {
            return Err(Diagnostic::new(field, "multiple editor member holes"));
        }
        Ok(())
    }
    /// Retain original source replacement coordinates for UTF16 completion edits.
    pub(crate) fn selector(&self) -> Span {
        self.selector
    }
    /// Filter members by the source prefix before the caret, without interpreting it as evidence.
    pub(crate) fn prefix(&self) -> &str {
        &self.prefix
    }
    /// A complete known selector must not hide an unrelated current-source type error.
    pub(crate) fn original(&self) -> &str {
        &self.original
    }
    /// Return the structurally recorded field selection after a successful recovered parse.
    pub(crate) fn field(&self) -> Span {
        self.field.expect("successful recovery attaches field").1
    }
    /// Match only the private empty-selector Field and its exact original receiver.
    pub(crate) fn matches(&self, expr: &crate::ast::Expr) -> bool {
        matches!(&expr.kind,ExprKind::Field {value,name} if name.is_empty() && self.field == Some((value.span,expr.span)))
    }
    /// Relocate only semantic identities; source replacement coordinates stay local.
    pub(crate) fn shift(&mut self, offset: usize) {
        if let Some((receiver, field)) = &mut self.field {
            receiver.start += offset;
            receiver.end += offset;
            field.start += offset;
            field.end += offset;
        }
    }
}

/// Parse one member selector as an opaque hole; all other source must parse normally.
pub(crate) fn recover_member(source: &str, cursor: usize) -> ParseResult<(Program, HoleSite)> {
    if !source.is_char_boundary(cursor) {
        return Err(Diagnostic::new(
            Span::default(),
            "invalid member completion cursor",
        ));
    }
    let mut parser = source_parser(source, false)?;
    let (index, replace, site) = selection(source, cursor, &parser.tokens)?;
    let token = Token {
        kind: Kind::MemberHole,
        span: site.selector,
    };
    if replace {
        parser.tokens[index] = token;
    } else {
        if parser.tokens.len() >= MAX_TOKENS {
            return Err(Diagnostic::new(site.selector, "token limit exceeded"));
        }
        parser.tokens.insert(index, token);
    }
    parser.member_hole = Some(site);
    let program = parser.program()?;
    let site = parser
        .member_hole
        .filter(|site| site.field.is_some())
        .ok_or_else(|| Diagnostic::new(Span::default(), "cursor is not an expression member"))?;
    Ok((program, site))
}

/// Locate a real Dot followed by the caret's identifier/tuple slot, never float or literal text.
fn selection(
    source: &str,
    cursor: usize,
    tokens: &[Token],
) -> ParseResult<(usize, bool, HoleSite)> {
    for (index, pair) in tokens.windows(2).enumerate() {
        if pair[0].kind != Kind::Dot {
            continue;
        }
        let next = &pair[1];
        let replace = matches!(next.kind, Kind::Name(_) | Kind::Number(_))
            && next.span.start <= cursor
            && cursor <= next.span.end;
        if !replace && pair[0].span.end != cursor {
            continue;
        }
        let selector = if replace {
            next.span
        } else {
            Span {
                start: cursor,
                end: cursor,
            }
        };
        let prefix = source
            .get(selector.start..cursor)
            .ok_or_else(|| Diagnostic::new(selector, "invalid member prefix"))?;
        let original = source
            .get(selector.start..selector.end)
            .ok_or_else(|| Diagnostic::new(selector, "invalid member selector"))?;
        return Ok((
            index + 1,
            replace,
            HoleSite {
                selector,
                prefix: prefix.into(),
                original: original.into(),
                field: None,
            },
        ));
    }
    Err(Diagnostic::new(
        Span {
            start: cursor,
            end: cursor,
        },
        "cursor is not a member selector",
    ))
}
