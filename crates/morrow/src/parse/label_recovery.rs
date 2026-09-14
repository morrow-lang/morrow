//! Token-proven argument positions for source-name completion, never executable checking.
use super::*;

#[derive(Clone, Debug)]
pub(crate) struct LabelSite {
    selector: Span,
    prefix: String,
    colon: bool,
    arguments: Option<Span>,
    supplied: Vec<(Option<String>, Span)>,
}
impl LabelSite {
    /// Return the exact original source replacement span.
    pub(crate) fn selector(&self) -> Span {
        self.selector
    }
    /// Return the source prefix without claiming a typed or mandatory parameter.
    pub(crate) fn prefix(&self) -> &str {
        &self.prefix
    }
    /// Preserve an existing label's colon and value when replacing its name.
    pub(crate) fn colon(&self) -> bool {
        self.colon
    }
    /// Return only the parser-proven innermost call delimiter identity.
    pub(crate) fn arguments(&self) -> Option<Span> {
        self.arguments
    }
    /// Retain source slots before pipe placeholders are removed from executable syntax.
    pub(crate) fn supplied(&self) -> &[(Option<String>, Span)] {
        &self.supplied
    }
    /// Move call identity into the loaded graph; replacement coordinates remain source-local.
    pub(crate) fn shift(&mut self, offset: usize) {
        if let Some(span) = &mut self.arguments {
            span.start += offset;
            span.end += offset;
        }
    }
}

/// Recover exactly one closed or EOF-open argument position; all other tokens parse normally.
pub(crate) fn recover_labels(source: &str, cursor: usize) -> ParseResult<(Program, LabelSite)> {
    if source.len() > MAX_SOURCE || !source.is_char_boundary(cursor) {
        return Err(Diagnostic::new(
            Span::default(),
            "invalid label source or cursor",
        ));
    }
    let close = source[cursor..].chars().all(char::is_whitespace);
    let mut parser = parser_tokens(lex_layout(source, close)?, false);
    let (index, replace, site) = selection(source, cursor, &parser.tokens)?;
    if !site.colon {
        let token = Token {
            kind: Kind::LabelHole,
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
    }
    parser.label_hole = Some(site);
    let program = parser.program()?;
    let site = parser
        .label_hole
        .filter(|s| s.arguments.is_some())
        .ok_or_else(|| Diagnostic::new(Span::default(), "not a call argument site"))?;
    Ok((program, site))
}

/// Require a real identifier/empty slot after '(' or ',', excluding all literal/comment content.
fn selection(
    source: &str,
    cursor: usize,
    tokens: &[Token],
) -> ParseResult<(usize, bool, LabelSite)> {
    if !source.is_char_boundary(cursor) {
        return Err(Diagnostic::new(Span::default(), "invalid argument cursor"));
    }
    for index in 1..tokens.len() {
        let token = &tokens[index];
        let prior = &tokens[index - 1];
        if !matches!(prior.kind, Kind::Left | Kind::Comma) {
            continue;
        }
        let replace = matches!(token.kind, Kind::Name(_))
            && token.span.start <= cursor
            && cursor <= token.span.end;
        let empty = prior.span.end <= cursor
            && cursor <= token.span.start
            && source[prior.span.end..cursor]
                .chars()
                .all(char::is_whitespace);
        if !replace && !empty {
            continue;
        }
        let selector = if replace {
            token.span
        } else {
            Span {
                start: cursor,
                end: cursor,
            }
        };
        let colon = replace && tokens.get(index + 1).is_some_and(|t| t.kind == Kind::Colon);
        if !colon
            && replace
            && !tokens
                .get(index + 1)
                .is_some_and(|t| matches!(t.kind, Kind::Comma | Kind::Right))
        {
            continue;
        }
        if !replace && !matches!(token.kind, Kind::Comma | Kind::Right) {
            continue;
        }
        return Ok((
            index,
            replace,
            LabelSite {
                selector,
                prefix: source[selector.start..cursor].into(),
                colon,
                arguments: None,
                supplied: Vec::new(),
            },
        ));
    }
    Err(Diagnostic::new(Span::default(), "not an argument selector"))
}

impl Parser {
    /// Attach the innermost argument delimiter pair containing the proven selector.
    pub(super) fn finish_label_arguments(
        &mut self,
        opening: usize,
        end: usize,
        args: &[Argument],
    ) -> ParseResult<()> {
        if let Some(site) = &mut self.label_hole
            && site.arguments.is_none()
            && opening < site.selector.start
            && site.selector.end <= end
        {
            site.arguments = Some(Span {
                start: opening,
                end,
            });
            site.supplied = args
                .iter()
                .map(|arg| {
                    (
                        arg.label.as_ref().map(|l| l.name.clone()),
                        arg.label.as_ref().map_or(arg.span, |l| l.span),
                    )
                })
                .collect();
        }
        Ok(())
    }
}
