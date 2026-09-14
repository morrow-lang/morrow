//! Independent, bounded lexer and recursive-descent parser for the prototype.
mod actors;
mod inspection;
mod traits;
mod type_arguments;
pub use inspection::{debug_ast, debug_tokens};
mod label_recovery;
mod recovery;
use crate::ast::{
    Argument, ArgumentLabel, BinaryOp, Expr, ExprKind, Field, Function, FunctionSyntax, Import,
    MatchArm, Param, Pattern, PatternKind, Program, Stmt, TypeDecl, UnaryOp, Variant,
};
use crate::{Constructor, Diagnostic, Span, Type};
pub(crate) use label_recovery::{LabelSite, recover_labels};
pub(crate) use recovery::{HoleSite, recover_member};

const MAX_SOURCE: usize = 1024 * 1024;
const MAX_TOKENS: usize = 65_536;
const MAX_DEPTH: usize = 128;
const MAX_PATTERN_PREFIX: usize = 128;
type ParseResult<T> = Result<T, Diagnostic>;

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    MemberHole,
    LabelHole,
    Name(String),
    Number(String),
    Text(String),
    Comment,
    Doc(String),
    MultilineOpen,
    MultilineClose,
    StringOpen,
    StringClose,
    HoleOpen,
    HoleClose,
    Left,
    Right,
    LeftBracket,
    RightBracket,
    LeftBrace,
    RightBrace,
    Colon,
    Comma,
    Dot,
    Arrow,
    Bind,
    Range,
    RangeInclusive,
    Pipe,
    Bar,
    Assign,
    Plus,
    Minus,
    Star,
    Power,
    BitAnd,
    BitOr,
    BitXor,
    BitNot,
    ShiftLeft,
    ShiftRight,
    Slash,
    Percent,
    Question,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Newline,
    Indent,
    Dedent,
    End,
}

#[derive(Clone, Debug)]
struct Token {
    kind: Kind,
    span: Span,
}

/// Parse UTF-8 Fern source into syntax, without invoking the C compiler.
/// Returns a source-located diagnostic for unsupported/malformed syntax or limits.
/// Input has no preconditions; source is limited to 1 MiB and 65,536 tokens.
pub fn parse(source: &str) -> ParseResult<Program> {
    source_parser(source, false)?.program()
}

/// Return exact parsed type-syntax ranges for a valid bounded current source snapshot.
/// Failed speculative arrow-header parses are excluded; no lexical guesses are published.
pub fn annotation_spans(source: &str) -> ParseResult<Vec<Span>> {
    Ok(source_roles(source)?.types)
}

/// Committed syntax ranges distinguish type annotations from dual-namespace import selectors.
pub(crate) struct SourceRoles {
    pub types: Vec<Span>,
    pub selectors: Vec<Span>,
}

/// Parse once, recording only successful type syntax and actual import-list delimiter tokens.
pub(crate) fn source_roles(source: &str) -> ParseResult<SourceRoles> {
    let mut parser = source_parser(source, true)?;
    let program = parser.program()?;
    let mut selectors = Vec::new();
    for import in program
        .imports
        .iter()
        .filter(|import| import.items.is_some())
    {
        let start = parser
            .tokens
            .partition_point(|token| token.span.start < import.span.start);
        for token in parser.tokens[start..]
            .iter()
            .take_while(|token| token.span.end <= import.span.end)
        {
            if token.kind == Kind::LeftBrace {
                selectors.push(Span {
                    start: token.span.end,
                    end: import.span.end,
                });
                break;
            }
        }
    }
    Ok(SourceRoles {
        types: parser.type_spans.unwrap_or_default(),
        selectors,
    })
}

/// Create the same bounded parser, optionally recording type ranges for source tooling.
fn source_parser(source: &str, record: bool) -> ParseResult<Parser> {
    if source.len() > MAX_SOURCE {
        return Err(Diagnostic::new(
            Span::default(),
            "source size exceeds 1 MiB prototype limit",
        ));
    }
    Ok(parser_tokens(lex(source)?, record))
}

/// Initialize private parser state identically for ordinary and editor-only token streams.
fn parser_tokens(tokens: Vec<Token>, record: bool) -> Parser {
    Parser {
        tokens,
        position: 0,
        depth: 0,
        guard_arrow: None,
        type_arm_boundary: None,
        type_spans: record.then(Vec::new),
        member_hole: None,
        label_hole: None,
        type_argument_work: 0,
        type_argument_shapes: None,
    }
}

/// Append a located token, rejecting excessive input before allocation grows.
fn push(tokens: &mut Vec<Token>, kind: Kind, start: usize, end: usize) -> ParseResult<()> {
    if tokens.len() >= MAX_TOKENS {
        return Err(Diagnostic::new(Span { start, end }, "token limit exceeded"));
    }
    tokens.push(Token {
        kind,
        span: Span { start, end },
    });
    Ok(())
}

struct SuiteLayout {
    delimiters: usize,
    indent: usize,
    levels: usize,
}

struct LayoutLexer {
    tokens: Vec<Token>,
    levels: Vec<usize>,
    delimiters: Vec<Token>,
    suites: Vec<SuiteLayout>,
    indentation_style: Option<u8>,
}

/// Tokenize logical rows while preserving bounded expression suites inside delimiters.
fn lex(source: &str) -> ParseResult<Vec<Token>> {
    lex_layout(source, false)
}

/// Only label recovery may close a bounded all-parenthesis suffix at the source end.
fn lex_layout(source: &str, close_parentheses: bool) -> ParseResult<Vec<Token>> {
    let mut lexer = LayoutLexer {
        tokens: Vec::new(),
        levels: vec![0],
        delimiters: Vec::new(),
        suites: Vec::new(),
        indentation_style: None,
    };
    let mut offset = 0;
    while offset < source.len() {
        offset += lexer.line(&source[offset..], offset)?;
    }
    if let Some(token) = lexer.delimiters.last() {
        if !close_parentheses || lexer.delimiters.iter().any(|t| t.kind != Kind::Left) {
            return Err(Diagnostic::new(token.span, "unclosed delimiter"));
        }
        for _ in &lexer.delimiters {
            push(&mut lexer.tokens, Kind::Right, source.len(), source.len())?;
        }
    }
    for _ in 1..lexer.levels.len() {
        push(&mut lexer.tokens, Kind::Dedent, source.len(), source.len())?;
    }
    push(&mut lexer.tokens, Kind::End, source.len(), source.len())?;
    Ok(lexer.tokens)
}

impl LayoutLexer {
    /// Select significant layout for one physical line; nested ordinary delimiters suspend it.
    fn line(&mut self, content: &str, offset: usize) -> ParseResult<usize> {
        let bytes = content
            .bytes()
            .take_while(|b| matches!(b, b' ' | b'\t'))
            .count();
        let leading = &content[..bytes];
        let indent = leading.bytes().fold(0, |column, byte| {
            if byte == b'\t' {
                (column + 8) & !7
            } else {
                column + 1
            }
        });
        let rest = &content[bytes..];
        let span = Span {
            start: offset,
            end: offset + bytes,
        };
        let mut raw = Vec::new();
        let consumed = lex_line(content, bytes, offset, &mut raw)?;
        raw.retain(|token| token.kind != Kind::Comment);
        if raw.is_empty() {
            return Ok(consumed);
        }
        self.prepare_layout(rest, indent, leading, span)?;
        let opens = suite_header(&raw, self.delimiters.len())?;
        self.append_line(raw)?;
        if opens && !self.delimiters.is_empty() && !self.layout_active() {
            if self.suites.len() >= MAX_DEPTH {
                return Err(Diagnostic::new(
                    span,
                    "embedded suite layout depth limit exceeded",
                ));
            }
            self.suites.push(SuiteLayout {
                delimiters: self.delimiters.len(),
                indent,
                levels: self.levels.len(),
            });
        }
        if self.layout_active() {
            push(
                &mut self.tokens,
                Kind::Newline,
                offset + consumed,
                offset + consumed,
            )?;
        }
        Ok(consumed)
    }

    /// End embedded frames at parent layout while retaining enclosing indentation.
    fn prepare_layout(
        &mut self,
        rest: &str,
        indent: usize,
        leading: &str,
        span: Span,
    ) -> ParseResult<()> {
        while let Some(frame) = self.suites.last() {
            if self.delimiters.len() != frame.delimiters || indent > frame.indent {
                break;
            }
            self.close_suite(span)?;
        }
        if self.layout_active() {
            self.check_indentation_style(leading, span)?;
            if rest.starts_with("|>") && indent >= *self.levels.last().unwrap() {
                if self.tokens.last().is_some_and(|t| t.kind == Kind::Newline) {
                    self.tokens.pop();
                }
            } else {
                layout(&mut self.tokens, &mut self.levels, indent, span)?;
            }
        }
        Ok(())
    }

    /// Only significant code indentation fixes the source-wide space-or-tab style.
    fn check_indentation_style(&mut self, leading: &str, span: Span) -> ParseResult<()> {
        let Some(style) = leading.bytes().next() else {
            return Ok(());
        };
        if leading.bytes().any(|byte| byte != style)
            || self
                .indentation_style
                .is_some_and(|previous| previous != style)
        {
            return Err(Diagnostic::new(
                span,
                "mixed tabs and spaces in indentation",
            ));
        }
        self.indentation_style = Some(style);
        Ok(())
    }

    /// Close complete embedded bodies before their caller's comma or closing delimiter.
    fn append_line(&mut self, raw: Vec<Token>) -> ParseResult<()> {
        for token in raw {
            if matches!(
                token.kind,
                Kind::Comma | Kind::Right | Kind::RightBracket | Kind::RightBrace
            ) {
                while self
                    .suites
                    .last()
                    .is_some_and(|frame| frame.delimiters == self.delimiters.len())
                {
                    self.close_suite(token.span)?;
                }
            }
            track_delimiters(std::slice::from_ref(&token), &mut self.delimiters)?;
            push(
                &mut self.tokens,
                token.kind,
                token.span.start,
                token.span.end,
            )?;
        }
        Ok(())
    }

    /// Emit only the indentation owned by one bounded embedded suite frame.
    fn close_suite(&mut self, span: Span) -> ParseResult<()> {
        let frame = self.suites.last().expect("checked suite frame");
        if self.levels.len() == frame.levels {
            return Err(Diagnostic::new(
                span,
                "expected indented embedded suite body",
            ));
        }
        while self.levels.len() > frame.levels {
            self.levels.pop();
            push(&mut self.tokens, Kind::Dedent, span.start, span.end)?;
        }
        self.suites.pop();
        Ok(())
    }

    /// A suite restores layout only at its own surrounding delimiter depth.
    fn layout_active(&self) -> bool {
        self.delimiters.is_empty()
            || self
                .suites
                .last()
                .is_some_and(|frame| frame.delimiters == self.delimiters.len())
    }
}

/// Recognize a final suite introducer without treating map keys or string holes as layout.
fn suite_header(tokens: &[Token], mut depth: usize) -> ParseResult<bool> {
    let mut heads = Vec::new();
    let mut strings = 0usize;
    let mut opens = false;
    for token in tokens {
        opens = false;
        match token.kind {
            Kind::StringOpen | Kind::MultilineOpen => {
                strings += 1;
                continue;
            }
            Kind::StringClose | Kind::MultilineClose => {
                strings = strings.saturating_sub(1);
                continue;
            }
            _ if strings > 0 => continue,
            _ => {}
        }
        match &token.kind {
            Kind::Left | Kind::LeftBracket | Kind::LeftBrace => depth += 1,
            Kind::Right | Kind::RightBracket | Kind::RightBrace => {
                heads.retain(|head| *head < depth);
                depth = depth.saturating_sub(1);
            }
            Kind::Name(name) if matches!(name.as_str(), "if" | "match" | "for" | "else") => {
                if heads.len() >= MAX_DEPTH {
                    return Err(Diagnostic::new(
                        token.span,
                        "embedded suite header depth limit exceeded",
                    ));
                }
                heads.push(depth);
                opens = name == "else";
            }
            Kind::Name(name) if matches!(name.as_str(), "with" | "do") => opens = true,
            Kind::Colon => {
                if let Some(index) = heads.iter().rposition(|head| *head == depth) {
                    heads.remove(index);
                    opens = true;
                }
            }
            Kind::Arrow => opens = true,
            _ => {}
        }
    }
    Ok(opens)
}

/// Validate delimiter pairing and suspend layout inside parenthesized/list syntax.
fn track_delimiters(tokens: &[Token], stack: &mut Vec<Token>) -> ParseResult<()> {
    for token in tokens {
        match token.kind {
            Kind::Left | Kind::LeftBracket | Kind::LeftBrace => {
                if stack.len() >= MAX_DEPTH {
                    return Err(Diagnostic::new(
                        token.span,
                        "delimiter depth limit exceeded",
                    ));
                }
                stack.push(token.clone());
            }
            Kind::Right | Kind::RightBracket | Kind::RightBrace => {
                let expected = if token.kind == Kind::Right {
                    Kind::Left
                } else if token.kind == Kind::RightBracket {
                    Kind::LeftBracket
                } else {
                    Kind::LeftBrace
                };
                if stack.pop().map(|opening| opening.kind) != Some(expected) {
                    return Err(Diagnostic::new(token.span, "mismatched closing delimiter"));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Emit indentation changes, requiring dedents to match an earlier level.
fn layout(
    tokens: &mut Vec<Token>,
    levels: &mut Vec<usize>,
    indent: usize,
    span: Span,
) -> ParseResult<()> {
    let current = levels.last().copied().unwrap_or(0);
    match indent.cmp(&current) {
        std::cmp::Ordering::Greater => {
            if levels.len() >= MAX_DEPTH {
                return Err(Diagnostic::new(span, "indentation depth limit exceeded"));
            }
            levels.push(indent);
            push(tokens, Kind::Indent, span.start, span.end)?;
        }
        std::cmp::Ordering::Less => {
            for _ in 0..levels.len() {
                if levels.last().copied().unwrap_or(0) <= indent {
                    break;
                }
                levels.pop();
                push(tokens, Kind::Dedent, span.start, span.end)?;
            }
            if levels.last().copied() != Some(indent) {
                return Err(Diagnostic::new(span, "inconsistent indentation"));
            }
        }
        std::cmp::Ordering::Equal => {}
    }
    Ok(())
}

/// Scan a logical row, allowing strings/comments to span physical lines.
fn lex_line(
    line: &str,
    mut at: usize,
    offset: usize,
    tokens: &mut Vec<Token>,
) -> ParseResult<usize> {
    while at < line.len() {
        match line.as_bytes()[at] {
            b'\n' => return Ok(at + 1),
            b' ' | b'\t' | b'\r' => {
                at += 1;
                continue;
            }
            b'#' => {
                at = scan_line_comment(line, at, offset, tokens)?;
                continue;
            }
            _ => {}
        }
        at = lex_token(line, at, offset, tokens, 0)?;
    }
    Ok(at)
}

/// Retain a comment's original span without consuming its terminating newline.
fn scan_line_comment(
    line: &str,
    at: usize,
    offset: usize,
    tokens: &mut Vec<Token>,
) -> ParseResult<usize> {
    let end = line[at..].find('\n').map_or(line.len(), |n| at + n);
    push(tokens, Kind::Comment, offset + at, offset + end)?;
    Ok(end)
}

/// Skip nested comments with a bounded counter and preserve the complete original text.
fn scan_block_comment(
    line: &str,
    start: usize,
    offset: usize,
    tokens: &mut Vec<Token>,
) -> ParseResult<usize> {
    let mut at = start + 2;
    let mut depth = 1;
    while at < line.len() {
        if line[at..].starts_with("/*") {
            depth += 1;
            if depth > MAX_DEPTH {
                return Err(Diagnostic::new(
                    Span {
                        start: offset + start,
                        end: offset + at,
                    },
                    "comment nesting limit exceeded",
                ));
            }
            at += 2;
        } else if line[at..].starts_with("*/") {
            at += 2;
            depth -= 1;
            if depth == 0 {
                push(tokens, Kind::Comment, offset + start, offset + at)?;
                return Ok(at);
            }
        } else {
            at += line[at..].chars().next().unwrap().len_utf8();
        }
    }
    Err(Diagnostic::new(
        Span {
            start: offset + start,
            end: offset + at,
        },
        "unterminated block comment",
    ))
}

/// Accept Fern's byte-preserving Unicode identifier policy without normalization.
pub(crate) fn identifier_char(c: char, initial: bool) -> bool {
    c.is_ascii_alphabetic()
        || c == '_'
        || (!initial && c.is_ascii_digit())
        || (!c.is_ascii() && !c.is_whitespace())
}

/// Validate external label names with the same Unicode and keyword rules as source bindings.
pub(crate) fn valid_label(name: &str) -> bool {
    !name.is_empty()
        && !reserved(name)
        && name
            .chars()
            .enumerate()
            .all(|(index, c)| identifier_char(c, index == 0))
}

/// Scan one expression token; embedded strings share the caller's token budget.
fn lex_token(
    line: &str,
    mut at: usize,
    offset: usize,
    tokens: &mut Vec<Token>,
    depth: usize,
) -> ParseResult<usize> {
    if line[at..].starts_with("/*") {
        return scan_block_comment(line, at, offset, tokens);
    }
    if line[at..].starts_with('@') {
        return scan_doc(line, at, offset, tokens);
    }
    let bytes = line.as_bytes();
    let start = at;
    let kind = match bytes[at] {
        b'"' => return scan_string(line, at, offset, tokens, depth),
        b'0'..=b'9' => {
            at = if tokens.last().is_some_and(|t| t.kind == Kind::Dot) {
                let mut end = at;
                while end < bytes.len() && bytes[end].is_ascii_digit() {
                    end += 1;
                }
                end
            } else {
                number_end(line, at, offset)?
            };
            Kind::Number(line[start..at].into())
        }
        _ if identifier_char(line[at..].chars().next().unwrap(), true) => {
            for c in line[at..].chars() {
                if !identifier_char(c, false) {
                    break;
                }
                at += c.len_utf8();
            }
            Kind::Name(line[start..at].into())
        }
        _ => {
            let (kind, width) = punctuation(&line[at..], offset + at)?;
            at += width;
            kind
        }
    };
    push(tokens, kind, offset + start, offset + at)?;
    Ok(at)
}

struct StringScan {
    start: usize,
    at: usize,
    segment: usize,
    text: String,
    interpolated: bool,
    multiline: bool,
    literal: bool,
}

/// Decode one quoted string, flattening interpolation tokens with exact byte locations.
fn scan_string(
    line: &str,
    start: usize,
    offset: usize,
    tokens: &mut Vec<Token>,
    depth: usize,
) -> ParseResult<usize> {
    scan_quoted(line, start, offset, tokens, depth, false)
}

/// Decode literal documentation through the same escape grammar without interpolation.
fn scan_doc(
    line: &str,
    start: usize,
    offset: usize,
    tokens: &mut Vec<Token>,
) -> ParseResult<usize> {
    let rest = &line[start..];
    let quote = start + rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
    let at = quote
        + line[quote..]
            .bytes()
            .take_while(|b| matches!(b, b' ' | b'\t'))
            .count();
    if &line[start..quote] != "@doc" || !line[at..].starts_with("\"\"\"") {
        return Err(Diagnostic::new(
            Span {
                start: offset + start,
                end: offset + at,
            },
            "unsupported attribute; expected @doc followed by a triple-quoted string",
        ));
    }
    let mut parts = Vec::new();
    let end = scan_quoted(line, at, offset, &mut parts, 0, true)?;
    let mut text = String::new();
    for token in parts {
        if let Kind::Text(part) = token.kind {
            text.push_str(&part);
        }
    }
    push(tokens, Kind::Doc(text), offset + start, offset + end)?;
    Ok(end)
}

/// Share bounded scanning across ordinary, multiline, and literal documentation strings.
fn scan_quoted(
    line: &str,
    start: usize,
    offset: usize,
    tokens: &mut Vec<Token>,
    depth: usize,
    literal: bool,
) -> ParseResult<usize> {
    let span = Span {
        start: offset + start,
        end: offset + line.len(),
    };
    if depth >= MAX_DEPTH {
        return Err(Diagnostic::new(span, "string nesting limit exceeded"));
    }
    let multiline = line[start..].starts_with("\"\"\"");
    let width = if multiline { 3 } else { 1 };
    let mut state = StringScan {
        start,
        at: start + width,
        segment: start + width,
        text: String::new(),
        interpolated: multiline,
        multiline,
        literal,
    };
    if multiline {
        push(
            tokens,
            Kind::MultilineOpen,
            offset + start,
            offset + start + 3,
        )?;
    }
    while state.at < line.len() {
        if line[state.at..].starts_with(if multiline { "\"\"\"" } else { "\"" }) {
            return close_string(&mut state, offset, tokens);
        }
        string_character(&mut state, line, offset, tokens, depth, span)?;
    }
    Err(Diagnostic::new(
        span,
        if multiline {
            "unterminated multiline string literal"
        } else {
            "unterminated string literal"
        },
    ))
}

/// Consume one scalar, escape or expression hole while enforcing ordinary line boundaries.
fn string_character(
    state: &mut StringScan,
    line: &str,
    offset: usize,
    tokens: &mut Vec<Token>,
    depth: usize,
    span: Span,
) -> ParseResult<()> {
    let c = line[state.at..].chars().next().unwrap();
    match c {
        '{' if !state.literal => string_hole(state, line, offset, tokens, depth)?,
        '}' if !state.literal => {
            return Err(Diagnostic::new(
                Span {
                    start: offset + state.at,
                    end: offset + state.at + 1,
                },
                "unmatched '}' in string; escape literal braces with a backslash",
            ));
        }
        '\\' => scan_escape(state, line, offset, span)?,
        '\n' | '\r' if !state.multiline => {
            return Err(Diagnostic::new(
                span,
                "unterminated string literal; use triple quotes for multiline text",
            ));
        }
        '\0' => {
            return Err(Diagnostic::new(
                span,
                "NUL is unsupported in prototype strings",
            ));
        }
        _ => {
            state.text.push(c);
            state.at += c.len_utf8();
        }
    }
    Ok(())
}

/// Consume one escape at the current string cursor, retaining the original byte span.
fn scan_escape(state: &mut StringScan, line: &str, offset: usize, span: Span) -> ParseResult<()> {
    state.at += 1;
    let c = line
        .get(state.at..)
        .and_then(|rest| rest.chars().next())
        .ok_or_else(|| Diagnostic::new(span, "unterminated string escape"))?;
    state.text.push(string_escape(
        c,
        Span {
            start: offset + state.at - 1,
            end: offset + state.at + c.len_utf8(),
        },
    )?);
    state.at += c.len_utf8();
    Ok(())
}

/// Finish either a plain literal token or the final interpolated text segment.
fn close_string(
    state: &mut StringScan,
    offset: usize,
    tokens: &mut Vec<Token>,
) -> ParseResult<usize> {
    let width = if state.multiline { 3 } else { 1 };
    let text = Kind::Text(std::mem::take(&mut state.text));
    if state.interpolated {
        push(tokens, text, offset + state.segment, offset + state.at)?;
        push(
            tokens,
            if state.multiline {
                Kind::MultilineClose
            } else {
                Kind::StringClose
            },
            offset + state.at,
            offset + state.at + width,
        )?;
    } else {
        push(
            tokens,
            text,
            offset + state.start,
            offset + state.at + width,
        )?;
    }
    Ok(state.at + width)
}

/// Flush literal text before parsing a hole and resume immediately after its closing brace.
fn string_hole(
    state: &mut StringScan,
    line: &str,
    offset: usize,
    tokens: &mut Vec<Token>,
    depth: usize,
) -> ParseResult<()> {
    if !state.interpolated {
        push(
            tokens,
            Kind::StringOpen,
            offset + state.start,
            offset + state.start + 1,
        )?;
        state.interpolated = true;
    }
    push(
        tokens,
        Kind::Text(std::mem::take(&mut state.text)),
        offset + state.segment,
        offset + state.at,
    )?;
    push(
        tokens,
        Kind::HoleOpen,
        offset + state.at,
        offset + state.at + 1,
    )?;
    let hole_start = state.at;
    state.at = scan_hole(line, state.at + 1, offset, tokens, depth + 1)?;
    if !state.multiline && line[hole_start..state.at].contains(['\n', '\r']) {
        return Err(Diagnostic::new(
            Span {
                start: offset + hole_start,
                end: offset + state.at,
            },
            "ordinary string interpolation cannot cross a newline",
        ));
    }
    state.segment = state.at;
    Ok(())
}

/// Scan one interpolation expression, balancing braces without entering nested strings twice.
fn scan_hole(
    line: &str,
    mut at: usize,
    offset: usize,
    tokens: &mut Vec<Token>,
    depth: usize,
) -> ParseResult<usize> {
    let start = at;
    let mut braces = 0;
    while at < line.len() {
        match line.as_bytes()[at] {
            b' ' | b'\n' | b'\r' => {
                at += 1;
                continue;
            }
            b'}' if braces == 0 => {
                push(tokens, Kind::HoleClose, offset + at, offset + at + 1)?;
                return Ok(at + 1);
            }
            b'}' => braces -= 1,
            b'{' => {
                braces += 1;
                if braces >= MAX_DEPTH {
                    return Err(Diagnostic::new(
                        Span {
                            start: offset + start,
                            end: offset + at,
                        },
                        "interpolation nesting limit exceeded",
                    ));
                }
            }
            b'#' => {
                at = scan_line_comment(line, at, offset, tokens)?;
                continue;
            }
            _ => {}
        }
        at = lex_token(line, at, offset, tokens, depth)?;
    }
    Err(Diagnostic::new(
        Span {
            start: offset + start,
            end: offset + at,
        },
        "unterminated interpolation; expected '}'",
    ))
}

/// Decode Fern's supported escapes, including literal interpolation delimiters.
fn string_escape(c: char, span: Span) -> ParseResult<char> {
    Ok(match c {
        'n' => '\n',
        'r' => '\r',
        't' => '\t',
        '"' => '"',
        '\\' => '\\',
        '{' => '{',
        '}' => '}',
        _ => {
            return Err(Diagnostic::new(
                span,
                "invalid or unsupported string escape",
            ));
        }
    })
}

/// Locate comments across multiline literals using the actual bounded source scanner.
pub(crate) fn comment_spans(source: &str) -> Vec<Span> {
    if source.len() > MAX_SOURCE {
        return Vec::new();
    }
    let mut tokens = Vec::new();
    let mut offset = 0;
    while offset < source.len() {
        match lex_line(&source[offset..], 0, offset, &mut tokens) {
            Ok(consumed) => offset += consumed,
            Err(_) => break,
        }
    }
    tokens
        .into_iter()
        .filter(|token| token.kind == Kind::Comment)
        .map(|token| token.span)
        .collect()
}

/// Indent code lines for an enclosing suite without changing multiline literal bytes.
/// Source must lex successfully; resulting source obeys the same 1 MiB size limit.
pub(crate) fn indent_code(source: &str, prefix: &str) -> ParseResult<String> {
    if source.len() > MAX_SOURCE {
        return Err(Diagnostic::new(
            Span::default(),
            "source size exceeds 1 MiB limit",
        ));
    }
    let spans = multiline_spans(source)?;
    let mut output = String::new();
    let mut offset = 0;
    let mut span_index = 0;
    for line in source.split_inclusive('\n') {
        while span_index < spans.len() && spans[span_index].end <= offset {
            span_index += 1;
        }
        let inside = spans
            .get(span_index)
            .is_some_and(|span| span.start < offset && offset < span.end);
        let added = line
            .len()
            .saturating_add(if inside { 0 } else { prefix.len() });
        if output.len().saturating_add(added) > MAX_SOURCE {
            return Err(Diagnostic::new(
                Span::default(),
                "indented source exceeds 1 MiB limit",
            ));
        }
        if !inside {
            output.push_str(prefix);
        }
        output.push_str(line);
        offset += line.len();
    }
    Ok(output)
}

/// Collect only outer multiline/doc spans so interpolation strings remain untouched too.
fn multiline_spans(source: &str) -> ParseResult<Vec<Span>> {
    let mut tokens = Vec::new();
    let mut offset = 0;
    while offset < source.len() {
        offset += lex_line(&source[offset..], 0, offset, &mut tokens)?;
    }
    let mut spans = Vec::new();
    let mut starts = Vec::new();
    for token in tokens {
        match token.kind {
            Kind::MultilineOpen => starts.push(token.span.start),
            Kind::MultilineClose => {
                if let Some(start) = starts.pop()
                    && starts.is_empty()
                {
                    spans.push(Span {
                        start,
                        end: token.span.end,
                    });
                }
            }
            Kind::Doc(_) if starts.is_empty() => spans.push(token.span),
            _ => {}
        }
    }
    Ok(spans)
}

/// Detect an unfinished interactive delimiter, string, documentation or suite.
/// Malformed tokens and mismatched closes return false for immediate diagnostics.
pub(crate) fn line_continues(source: &str) -> bool {
    if source.len() > MAX_SOURCE {
        return false;
    }
    let mut tokens = Vec::new();
    let mut delimiters = Vec::new();
    let mut offset = 0;
    while offset < source.len() {
        let from = tokens.len();
        match lex_line(&source[offset..], 0, offset, &mut tokens) {
            Ok(consumed) => offset += consumed,
            Err(error) => {
                return error.message.starts_with("unterminated multiline")
                    || error.message == "unterminated block comment";
            }
        }
        if track_delimiters(&tokens[from..], &mut delimiters).is_err() {
            return false;
        }
    }
    tokens.retain(|token| token.kind != Kind::Comment);
    !delimiters.is_empty() || tokens.last().is_some_and(|token| {
        matches!(token.kind, Kind::Colon | Kind::Arrow | Kind::Doc(_))
            || matches!(&token.kind,Kind::Name(name) if matches!(name.as_str(),"with"|"do"|"else"))
    })
}

/// Source token ranges used by editor navigation; literal text and comments are excluded.
#[derive(Clone, Debug, Default)]
pub struct IdentifierIndex {
    pub identifiers: Vec<Span>,
    /// Numeric tokens permit precise tuple-slot and scalar hover without rescanning text.
    pub numbers: Vec<Span>,
    pub excluded: Vec<Span>,
    /// Actual line comments, excluding marker-like text inside strings and block comments.
    pub comments: Vec<Span>,
}

/// Index exact UTF-8 identifier boundaries without requiring complete delimiter layout.
pub fn identifier_index(source: &str) -> Result<IdentifierIndex, Diagnostic> {
    if source.len() > MAX_SOURCE {
        return Err(Diagnostic::new(
            Span::default(),
            "source exceeds 1 MiB limit",
        ));
    }
    let mut tokens = Vec::new();
    let mut offset = 0;
    while offset < source.len() {
        offset += lex_line(&source[offset..], 0, offset, &mut tokens)?;
    }
    let mut index = IdentifierIndex::default();
    for token in tokens {
        match token.kind {
            Kind::Name(_) => index.identifiers.push(token.span),
            Kind::Number(_) => index.numbers.push(token.span),
            Kind::Comment => {
                if source[token.span.start..token.span.end].starts_with('#') {
                    index.comments.push(token.span);
                }
                index.excluded.push(token.span);
            }
            Kind::Text(_)
            | Kind::Doc(_)
            | Kind::StringOpen
            | Kind::StringClose
            | Kind::MultilineOpen
            | Kind::MultilineClose => {
                index.excluded.push(token.span);
            }
            _ => {}
        }
    }
    Ok(index)
}

/// Recognize punctuation or report unsupported characters at a UTF-8 boundary.
fn punctuation(rest: &str, start: usize) -> ParseResult<(Kind, usize)> {
    let pairs = [
        ("&&&", Kind::BitAnd),
        ("|||", Kind::BitOr),
        ("^^^", Kind::BitXor),
        ("~~~", Kind::BitNot),
        ("<<<", Kind::ShiftLeft),
        (">>>", Kind::ShiftRight),
        ("**", Kind::Power),
        ("..=", Kind::RangeInclusive),
        ("..", Kind::Range),
        ("<-", Kind::Bind),
        ("->", Kind::Arrow),
        ("|>", Kind::Pipe),
        ("==", Kind::Eq),
        ("!=", Kind::Ne),
        ("<=", Kind::Le),
        (">=", Kind::Ge),
    ];
    for (text, kind) in pairs {
        if rest.starts_with(text) {
            return Ok((kind, text.len()));
        }
    }
    let c = rest.chars().next().unwrap_or('\0');
    let kind = match c {
        '(' => Kind::Left,
        ')' => Kind::Right,
        '[' => Kind::LeftBracket,
        ']' => Kind::RightBracket,
        '{' => Kind::LeftBrace,
        '}' => Kind::RightBrace,
        ':' => Kind::Colon,
        ',' => Kind::Comma,
        '.' => Kind::Dot,
        '=' => Kind::Assign,
        '+' => Kind::Plus,
        '-' => Kind::Minus,
        '*' => Kind::Star,
        '/' => Kind::Slash,
        '%' => Kind::Percent,
        '|' => Kind::Bar,
        '?' => Kind::Question,
        '<' => Kind::Lt,
        '>' => Kind::Gt,
        _ => {
            return Err(Diagnostic::new(
                Span {
                    start,
                    end: start + c.len_utf8(),
                },
                format!("unsupported character {c:?} in Rust prototype"),
            ));
        }
    };
    Ok((kind, 1))
}

struct Parsed {
    node: Expr,
    depth: usize,
}

/// Construct syntax only when its eventual recursive traversal/drop is bounded.
fn expression(kind: ExprKind, span: Span, depth: usize) -> ParseResult<Parsed> {
    if depth > MAX_DEPTH {
        return Err(Diagnostic::new(span, "expression depth limit exceeded"));
    }
    Ok(Parsed {
        node: Expr { kind, span },
        depth,
    })
}

#[derive(Default)]
struct ClauseGroups {
    last: Option<(String, usize)>,
    seen: std::collections::HashSet<String>,
}

struct Parser {
    tokens: Vec<Token>,
    position: usize,
    depth: usize,
    guard_arrow: Option<usize>,
    type_arm_boundary: Option<usize>,
    type_spans: Option<Vec<Span>>,
    member_hole: Option<HoleSite>,
    label_hole: Option<LabelSite>,
    type_argument_work: usize,
    type_argument_shapes: Option<type_arguments::Shapes>,
}

impl Parser {
    /// Borrow the current token; construction always appends an End sentinel.
    fn current(&self) -> &Token {
        &self.tokens[self.position]
    }

    /// Advance without moving past the sentinel; return the consumed token.
    fn take(&mut self) -> Token {
        let token = self.current().clone();
        if self.position + 1 < self.tokens.len() {
            self.position += 1;
        }
        token
    }

    /// Consume a punctuation token when it matches.
    fn eat(&mut self, kind: &Kind) -> bool {
        if &self.current().kind == kind {
            self.take();
            true
        } else {
            false
        }
    }

    /// Check a keyword without treating arbitrary identifiers as syntax.
    fn word(&self, word: &str) -> bool {
        matches!(&self.current().kind, Kind::Name(name) if name == word)
    }

    /// Require punctuation and report an actionable diagnostic on mismatch.
    fn expect(&mut self, kind: Kind, message: &str) -> ParseResult<Token> {
        if self.current().kind == kind {
            Ok(self.take())
        } else {
            Err(self.error(message))
        }
    }

    /// Locate a parser error at the token that could not be consumed.
    fn error(&self, message: &str) -> Diagnostic {
        Diagnostic::new(self.current().span, message)
    }

    /// Parse module declarations, imports, public exports, functions and custom types.
    fn program(&mut self) -> ParseResult<Program> {
        let mut program = Program::default();
        let mut pending = None;
        let mut groups = ClauseGroups::default();
        for _ in 0..self.tokens.len() {
            if self.eat(&Kind::Newline) {
                continue;
            }
            if self.current().kind == Kind::End {
                if pending.is_some() {
                    return Err(self.error("@doc must precede a function or type declaration"));
                }
                return Ok(program);
            }
            if let Kind::Doc(text) = self.current().kind.clone() {
                if pending.is_some() {
                    return Err(self.error("duplicate @doc before declaration"));
                }
                pending = Some((text, self.take().span));
                self.line_end()?;
                continue;
            }
            let public = if self.word("pub") {
                self.take();
                true
            } else {
                false
            };
            let documented = self.documentation(&mut program, pending.take())?;
            let is_function = self.word("fn");
            if self.word("fn") {
                let function = self.function(public)?;
                self.add_clause(&mut program, &mut groups, function, documented)?;
            } else if self.word("foreign") {
                let function = self.foreign_function(public)?;
                self.add_clause(&mut program, &mut groups, function, documented)?;
            } else if self.word("const") {
                let constant = self.constant(public)?;
                self.add_clause(&mut program, &mut groups, constant, documented)?;
            } else if self.word("type") {
                self.type_declaration(&mut program, public)?;
            } else if self.word("trait") {
                self.trait_declaration(&mut program, public)?;
            } else if self.word("impl") && !public {
                self.implementation(&mut program)?;
            } else if self.word("newtype") {
                self.newtype_declaration(&mut program, public)?;
            } else if self.word("import") {
                program.imports.push(self.import(public)?);
            } else if self.word("module") && !public {
                if program.module.is_some() {
                    return Err(self.error("duplicate module declaration"));
                }
                self.take();
                program.module = Some(self.qualified_name()?.0);
                self.line_end()?;
            } else {
                return Err(self.error("expected fn, type, import, or module declaration"));
            }
            if !is_function {
                groups.last = None;
            }
        }
        Err(self.error("parser token limit exceeded"))
    }

    /// Require a physical declaration boundary without consuming enclosing layout.
    fn line_end(&mut self) -> ParseResult<()> {
        if self.eat(&Kind::Newline) || self.current().kind == Kind::End {
            Ok(())
        } else {
            Err(self.error("expected end of line after declaration"))
        }
    }

    /// Read a dotted name without consuming import selectors after the final dot.
    fn qualified_name(&mut self) -> ParseResult<(String, Span)> {
        let (mut name, mut span) = self.name()?;
        for _ in 0..self.tokens.len() {
            if self.current().kind != Kind::Dot
                || !self
                    .tokens
                    .get(self.position + 1)
                    .is_some_and(|t| matches!(t.kind, Kind::Name(_)))
            {
                break;
            }
            self.take();
            let (part, next) = self.name()?;
            name.push('.');
            name.push_str(&part);
            span.end = next.end;
        }
        Ok((name, span))
    }

    /// Capture full/selective/wildcard imports and aliases without loading modules.
    fn import(&mut self, public: bool) -> ParseResult<Import> {
        let start = self.take().span.start;
        let (module, _) = self.qualified_name()?;
        let items = if self.eat(&Kind::Dot) {
            if self.eat(&Kind::Star) {
                Some(vec!["*".into()])
            } else {
                self.expect(
                    Kind::LeftBrace,
                    "expected '{items}' or '*' after import module",
                )?;
                let mut names = Vec::new();
                for _ in 0..self.tokens.len() {
                    if self.eat(&Kind::RightBrace) {
                        break;
                    }
                    names.push(self.name()?.0);
                    if !self.eat(&Kind::Comma) {
                        self.expect(Kind::RightBrace, "expected ',' or '}' after import item")?;
                        break;
                    }
                }
                if names.is_empty() {
                    return Err(self.error("selective import requires at least one item"));
                }
                Some(names)
            }
        } else {
            None
        };
        let alias = if self.word("as") {
            self.take();
            Some(self.name()?.0)
        } else {
            None
        };
        let end = self.tokens[self.position.saturating_sub(1)].span.end;
        self.line_end()?;
        Ok(Import {
            module,
            alias,
            items,
            public,
            span: Span { start, end },
        })
    }

    /// Parse generic parameter names and either record fields or sum constructors.
    fn type_declaration(&mut self, program: &mut Program, public: bool) -> ParseResult<()> {
        let start = self.take().span.start;
        let (name, _) = self.name()?;
        let parameters = self.type_parameters()?;
        if public {
            program.exports.push(name.clone());
        }
        if self.eat(&Kind::Assign) {
            let target = self.ty()?;
            let end = self.tokens[self.position.saturating_sub(1)].span.end;
            self.line_end()?;
            program.aliases.push(crate::ast::TypeAlias {
                public,
                name,
                parameters,
                target,
                span: Span { start, end },
            });
        } else {
            program
                .types
                .push(self.nominal_body(start, name, parameters, public)?);
        }
        Ok(())
    }

    /// Read bounded generic declaration parameters with ordinary source identifier rules.
    fn type_parameters(&mut self) -> ParseResult<Vec<String>> {
        let mut parameters = Vec::new();
        if self.eat(&Kind::Left) {
            for _ in 0..self.tokens.len() {
                if self.eat(&Kind::Right) {
                    break;
                }
                parameters.push(self.name()?.0);
                if !self.eat(&Kind::Comma) {
                    self.expect(Kind::Right, "expected ',' or ')' after type parameter")?;
                    break;
                }
            }
        }
        Ok(parameters)
    }

    /// Preserve newtype identity, constructor spelling and payload source annotation separately.
    fn newtype_declaration(&mut self, program: &mut Program, public: bool) -> ParseResult<()> {
        let start = self.take().span.start;
        let (name, _) = self.name()?;
        let parameters = self.type_parameters()?;
        let derives = self.derivations()?;
        self.expect(Kind::Assign, "expected '=' before newtype constructor")?;
        let (constructor, constructor_span) = self.name()?;
        self.expect(
            Kind::Left,
            "newtype constructor requires exactly one payload type",
        )?;
        let inner_start = self.current().span.start;
        let inner = self.ty()?;
        let inner_span = Span {
            start: inner_start,
            end: self.tokens[self.position - 1].span.end,
        };
        let end = self.current().span.end;
        self.expect(
            Kind::Right,
            "newtype constructor requires exactly one payload type",
        )?;
        self.line_end()?;
        if public {
            program.exports.push(name.clone());
        }
        program.newtypes.push(crate::ast::NewtypeDecl {
            derives,
            public,
            name,
            parameters,
            constructor,
            inner,
            span: Span { start, end },
            constructor_span,
            inner_span,
        });
        Ok(())
    }

    /// Retain a nonempty bounded derive list; duplicate requests never silently merge.
    fn derivations(&mut self) -> ParseResult<Vec<crate::ast::Derivation>> {
        if !self.word("derive") {
            return Ok(Vec::new());
        }
        self.take();
        self.expect(Kind::Left, "expected '(' after derive")?;
        let mut result = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..32 {
            if self.current().kind == Kind::Right {
                return Err(self.error("derive requires a nonempty trait list"));
            }
            let (name, span) = self.name()?;
            if !seen.insert(name.clone()) {
                return Err(Diagnostic::new(span, "duplicate derive trait"));
            }
            result.push(crate::ast::Derivation { name, span });
            if self.eat(&Kind::Right) {
                return Ok(result);
            }
            self.expect(Kind::Comma, "expected ',' or ')' in derive list")?;
        }
        Err(self.error("derive trait count limit exceeded (32)"))
    }

    /// Parse the indented payload shared by existing record and sum declarations.
    fn nominal_body(
        &mut self,
        start: usize,
        name: String,
        parameters: Vec<String>,
        public: bool,
    ) -> ParseResult<TypeDecl> {
        let derives = self.derivations()?;
        self.expect(Kind::Colon, "expected ':' before type body")?;
        self.expect(Kind::Newline, "type declarations require an indented body")?;
        self.expect(Kind::Indent, "type declarations require an indented body")?;
        let record = self
            .tokens
            .get(self.position + 1)
            .is_some_and(|t| t.kind == Kind::Colon);
        let mut variants = Vec::new();
        let mut fields = Vec::new();
        for _ in 0..self.tokens.len() {
            if self.eat(&Kind::Dedent) {
                break;
            }
            if record {
                fields.push(self.declaration_field(true)?);
            } else {
                variants.push(self.variant()?);
            }
            self.line_end()?;
        }
        let end = self.tokens[self.position.saturating_sub(1)].span.end;
        let span = Span { start, end };
        if record {
            variants.push(Variant {
                name: name.clone(),
                fields,
                span,
            });
        }
        if variants.is_empty() {
            return Err(Diagnostic::new(span, "type requires fields or variants"));
        }
        Ok(TypeDecl {
            derives,
            public,
            name,
            parameters,
            variants,
            record,
            span,
        })
    }

    /// Parse one optional-name field and retain its complete type source range.
    fn declaration_field(&mut self, require_name: bool) -> ParseResult<Field> {
        let start = self.current().span.start;
        let named = self
            .tokens
            .get(self.position + 1)
            .is_some_and(|t| t.kind == Kind::Colon);
        let name = if named || require_name {
            let name = self.name()?.0;
            self.expect(Kind::Colon, "expected ':' after record field name")?;
            Some(name)
        } else {
            None
        };
        let ty = self.ty()?;
        let end = self.tokens[self.position.saturating_sub(1)].span.end;
        Ok(Field {
            name,
            ty,
            span: Span { start, end },
        })
    }

    /// Parse a named sum constructor with positional or named payload fields.
    fn variant(&mut self) -> ParseResult<Variant> {
        let (name, mut span) = self.name()?;
        let mut fields = Vec::new();
        if self.eat(&Kind::Left) {
            for _ in 0..self.tokens.len() {
                if self.current().kind == Kind::Right {
                    span.end = self.take().span.end;
                    break;
                }
                fields.push(self.declaration_field(false)?);
                if !self.eat(&Kind::Comma) {
                    span.end = self
                        .expect(Kind::Right, "expected ',' or ')' after variant field")?
                        .span
                        .end;
                    break;
                }
            }
        }
        Ok(Variant { name, fields, span })
    }

    /// Attach literal documentation without losing its following declaration identity.
    fn documentation(
        &self,
        program: &mut Program,
        pending: Option<(String, Span)>,
    ) -> ParseResult<bool> {
        let Some((text, span)) = pending else {
            return Ok(false);
        };
        if !self.word("fn")
            && !self.word("type")
            && !self.word("newtype")
            && !self.word("const")
            && !self.word("trait")
        {
            return Err(self.error("@doc must precede a function or type declaration"));
        }
        let target = match self.tokens.get(self.position + 1).map(|t| &t.kind) {
            Some(Kind::Name(name)) => name.clone(),
            _ => return Err(self.error("expected documented declaration name")),
        };
        program
            .docs
            .push(crate::ast::DocComment { target, text, span });
        Ok(true)
    }

    /// Preserve group provenance before declaration kinds move into separate syntax vectors.
    fn add_clause(
        &self,
        program: &mut Program,
        groups: &mut ClauseGroups,
        mut function: Function,
        documented: bool,
    ) -> ParseResult<()> {
        if let Some((_, start)) = groups
            .last
            .as_ref()
            .filter(|(name, _)| name == &function.name)
        {
            if documented {
                return Err(Diagnostic::new(
                    function.span,
                    "@doc belongs before the first clause of a function",
                ));
            }
            function.group_start = *start;
        } else if !groups.seen.insert(function.name.clone()) {
            return Err(Diagnostic::new(
                function.span,
                "function clauses must be adjacent",
            ));
        }
        groups.last = Some((function.name.clone(), function.group_start));
        if function.public && !program.exports.contains(&function.name) {
            program.exports.push(function.name.clone());
        }
        program.functions.push(function);
        Ok(())
    }

    /// Parse source clauses while retaining typed-colon and expression-arrow body spelling.
    fn function(&mut self, public: bool) -> ParseResult<Function> {
        let start = self.take().span.start;
        let (name, _) = self.name()?;
        self.expect(Kind::Left, "expected '(' after function name")?;
        let params = self.function_parameters()?;
        let guard = if self.word("if") {
            self.take();
            Some(self.guard_expression()?.node)
        } else {
            None
        };
        let (return_type, syntax, constraints) = self.function_body_separator()?;
        let body = self.suite()?;
        if self.current().kind != Kind::End
            && self.current().kind != Kind::Dedent
            && self.current().kind != Kind::Newline
            && !self.previous_dedent()
        {
            return Err(self.error("expected end of line after function body"));
        }
        Ok(Function {
            constraints,
            public,
            name,
            params,
            return_type,
            guard,
            syntax,
            group_start: start,
            span: Span {
                start,
                end: body.node.span.end,
            },
            body: body.node,
        })
    }

    /// Keep missing annotations explicit for the checker while sharing all bounded patterns.
    fn function_parameters(&mut self) -> ParseResult<Vec<Param>> {
        let mut params = Vec::new();
        for _ in 0..self.tokens.len() {
            if self.eat(&Kind::Right) {
                break;
            }
            let label = self.parameter_label()?;
            let pattern = self.pattern()?;
            let annotation = if self.eat(&Kind::Colon) {
                Some(self.ty()?)
            } else {
                None
            };
            let span = pattern.span;
            params.push(Param {
                label,
                pattern,
                annotation,
                span,
            });
            if !self.eat(&Kind::Comma) {
                self.expect(Kind::Right, "expected ',' or ')' after parameter")?;
                break;
            }
        }
        Ok(params)
    }

    /// An external label is an identifier followed by another pattern start, not ':' or ','.
    fn parameter_label(&mut self) -> ParseResult<Option<ArgumentLabel>> {
        let Kind::Name(name) = self.current().kind.clone() else {
            return Ok(None);
        };
        let next = self.tokens.get(self.position + 1).map(|token| &token.kind);
        let starts = matches!(
            next,
            Some(
                Kind::Name(_)
                    | Kind::Number(_)
                    | Kind::Text(_)
                    | Kind::Minus
                    | Kind::Left
                    | Kind::LeftBracket
            )
        );
        if !starts
            || matches!(
                name.as_str(),
                "Some" | "None" | "Ok" | "Err" | "true" | "false"
            )
        {
            return Ok(None);
        }
        // A constructor immediately followed by '(' belongs to its pattern.
        if matches!(next, Some(Kind::Left)) && name.chars().next().is_some_and(char::is_uppercase) {
            return Ok(None);
        }
        if !valid_label(&name) {
            return Err(self.error("invalid argument label"));
        }
        let span = self.take().span;
        Ok(Some(ArgumentLabel { name, span }))
    }

    /// Try one bounded type annotation; an arrow without a following type-colon begins a body.
    fn function_body_separator(
        &mut self,
    ) -> ParseResult<(Option<Type>, FunctionSyntax, Vec<crate::ast::TraitBound>)> {
        if !self.eat(&Kind::Arrow) {
            self.expect(Kind::Colon, "expected ':' or '->' before function body")?;
            return Ok((None, FunctionSyntax::Colon, Vec::new()));
        }
        let position = self.position;
        let depth = self.depth;
        let spans = self.type_spans.as_ref().map_or(0, Vec::len);
        if let Ok(ty) = self.ty() {
            if self.word("where") {
                self.take();
                let constraints = self.trait_bounds()?;
                self.expect(Kind::Colon, "expected ':' after where constraints")?;
                return Ok((Some(ty), FunctionSyntax::Colon, constraints));
            }
            if self.eat(&Kind::Colon) {
                return Ok((Some(ty), FunctionSyntax::Colon, Vec::new()));
            }
        }
        self.position = position;
        self.depth = depth;
        if let Some(recorded) = &mut self.type_spans {
            recorded.truncate(spans);
        }
        Ok((None, FunctionSyntax::Arrow, Vec::new()))
    }

    /// Read a non-reserved identifier for a binding/function/parameter.
    fn name(&mut self) -> ParseResult<(String, Span)> {
        match self.current().kind.clone() {
            Kind::Name(name) if !reserved(&name) => {
                let span = self.take().span;
                Ok((name, span))
            }
            _ => {
                Err(self
                    .error("expected identifier; this syntax is unsupported in the Rust prototype"))
            }
        }
    }

    /// Bound recursive concrete types independently of expression parsing.
    fn ty(&mut self) -> ParseResult<Type> {
        if self.depth >= MAX_DEPTH {
            return Err(self.error("type depth limit exceeded"));
        }
        self.depth += 1;
        let start = self.current().span.start;
        let result = self.union_type();
        self.depth -= 1;
        if result.is_ok()
            && let Some(spans) = &mut self.type_spans
        {
            spans.push(Span {
                start,
                end: self.tokens[self.position.saturating_sub(1)].span.end,
            });
        }
        result
    }

    /// Preserve written union members; semantic normalization follows alias resolution.
    fn union_type(&mut self) -> ParseResult<Type> {
        let first = self.type_value()?;
        if !self.eat(&Kind::Bar) {
            return Ok(first);
        }
        let mut members = vec![first];
        loop {
            if members.len() >= 128 {
                return Err(self.error("union alternative limit exceeded"));
            }
            members.push(self.type_value()?);
            if !self.eat(&Kind::Bar) {
                break;
            }
        }
        Ok(Type::Union(members))
    }

    /// Parse primitive, nominal, and generic types without resolving declarations.
    fn type_value(&mut self) -> ParseResult<Type> {
        if self.word("fn") {
            self.take();
            self.expect(Kind::Left, "expected '(' in function type")?;
            let (params, _) = self.parenthesized_types()?;
            self.expect(Kind::Arrow, "expected '->' in function type")?;
            return Ok(Type::Function(params, Box::new(self.ty()?)));
        }
        if self.eat(&Kind::Left) {
            return self.tuple_type();
        }
        let (name, span) = self
            .qualified_name()
            .map_err(|_| self.error("expected type annotation"))?;
        let mut arguments = Vec::new();
        if self.eat(&Kind::Left) {
            for _ in 0..self.tokens.len() {
                if self.eat(&Kind::Right) {
                    break;
                }
                arguments.push(self.ty()?);
                if !self.eat(&Kind::Comma) {
                    self.expect(Kind::Right, "expected ',' or ')' after type argument")?;
                    break;
                }
            }
        }
        named_type(name, arguments, span)
    }

    /// Distinguish structural tuple/group types from right-associative function signatures.
    fn tuple_type(&mut self) -> ParseResult<Type> {
        let (mut fields, comma) = self.parenthesized_types()?;
        if self.type_arm_boundary != Some(self.depth) && self.eat(&Kind::Arrow) {
            return Ok(Type::Function(fields, Box::new(self.ty()?)));
        }
        Ok(if fields.is_empty() {
            Type::Unit
        } else if fields.len() == 1 && !comma {
            fields.remove(0)
        } else {
            Type::Tuple(fields)
        })
    }

    /// Parse a bounded parenthesized type sequence, retaining a singleton comma.
    fn parenthesized_types(&mut self) -> ParseResult<(Vec<Type>, bool)> {
        let mut fields = Vec::new();
        let mut comma = false;
        for _ in 0..self.tokens.len() {
            if self.eat(&Kind::Right) {
                break;
            }
            fields.push(self.ty()?);
            if !self.eat(&Kind::Comma) {
                self.expect(Kind::Right, "expected ',' or ')' after type")?;
                break;
            }
            comma = true;
        }
        Ok((fields, comma))
    }

    /// Parse one inline expression or an indented block of bindings/expressions.
    fn suite(&mut self) -> ParseResult<Parsed> {
        if self.depth >= MAX_DEPTH {
            return Err(self.error("parser depth limit exceeded"));
        }
        self.depth += 1;
        let result = self.suite_value();
        self.depth -= 1;
        result
    }

    /// Parse suite contents after reserving recursive layout work against the shared bound.
    fn suite_value(&mut self) -> ParseResult<Parsed> {
        if !self.eat(&Kind::Newline) {
            return self.statement_expression();
        }
        let start = self
            .expect(Kind::Indent, "expected indented block")?
            .span
            .start;
        let mut statements = Vec::new();
        let mut depth = 1;
        let mut end = start;
        for _ in 0..self.tokens.len() {
            if self.eat(&Kind::Dedent) {
                if statements.is_empty() {
                    return Err(self.error("expected expression in indented block"));
                }
                return expression(ExprKind::Block(statements), Span { start, end }, depth);
            }
            let (statement, statement_depth, statement_end) = self.statement()?;
            end = statement_end;
            depth = depth.max(statement_depth + 1);
            statements.push(statement);
            if !self.eat(&Kind::Newline)
                && self.current().kind != Kind::Dedent
                && !self.previous_dedent()
            {
                return Err(self.error("expected end of line after statement"));
            }
        }
        Err(self.error("unterminated block"))
    }

    /// Recognize layout already consumed by a nested expression's block.
    fn previous_dedent(&self) -> bool {
        self.position > 0 && self.tokens[self.position - 1].kind == Kind::Dedent
    }

    /// Parse an immutable binding or value statement and retain its tree depth.
    fn statement(&mut self) -> ParseResult<(Stmt, usize, usize)> {
        if self.word("let") {
            return self.let_statement();
        }
        let value = self.statement_expression()?;
        let end = value.node.span.end;
        Ok((Stmt::Expr(value.node), value.depth, end))
    }

    /// Parse binding patterns before introducing optional failure-only else suites.
    fn let_statement(&mut self) -> ParseResult<(Stmt, usize, usize)> {
        let start = self.take().span.start;
        let binding = self.plain_binding();
        let mut pattern = self.pattern()?;
        let annotation = if self.eat(&Kind::Colon) {
            Some(self.ty()?)
        } else {
            None
        };
        self.expect(Kind::Assign, "expected '=' in let binding")?;
        let value = self.expr(0)?;
        if self.word("else") {
            self.take();
            self.expect(Kind::Colon, "expected ':' after let-else")?;
            let otherwise = self.suite()?;
            let end = otherwise.node.span.end;
            return Ok((
                Stmt::LetElse {
                    pattern,
                    annotation,
                    value: value.node,
                    else_branch: otherwise.node,
                    span: Span { start, end },
                },
                value.depth.max(otherwise.depth),
                end,
            ));
        }
        if let Some(name) = binding {
            pattern.kind = PatternKind::Bind(name);
        }
        let end = value.node.span.end;
        let span = Span { start, end };
        let statement = match pattern.kind {
            PatternKind::Bool(_) | PatternKind::Int(_) | PatternKind::String(_) => {
                return Err(Diagnostic::new(
                    pattern.span,
                    "literal let patterns require an else branch",
                ));
            }
            PatternKind::Bind(name) => Stmt::Let {
                name,
                annotation,
                value: value.node,
                span,
            },
            PatternKind::Wildcard => Stmt::Let {
                name: "_".into(),
                annotation,
                value: value.node,
                span,
            },
            _ => Stmt::LetPattern {
                pattern,
                annotation,
                value: value.node,
                span,
            },
        };
        Ok((statement, value.depth, end))
    }

    /// Ordinary let names may be uppercase; explicit calls and let-else still use patterns.
    fn plain_binding(&self) -> Option<String> {
        let Kind::Name(name) = &self.current().kind else {
            return None;
        };
        if !reserved(name)
            && self
                .tokens
                .get(self.position + 1)
                .is_some_and(|token| matches!(token.kind, Kind::Assign | Kind::Colon))
        {
            Some(name.clone())
        } else {
            None
        }
    }

    /// Apply a postfix condition to the whole statement, including an early return.
    fn statement_expression(&mut self) -> ParseResult<Parsed> {
        let mut value = if self.word("defer") {
            let start = self.take().span.start;
            let deferred = self.expr(0)?;
            let span = Span {
                start,
                end: deferred.node.span.end,
            };
            expression(
                ExprKind::Defer(Box::new(deferred.node)),
                span,
                deferred.depth + 1,
            )?
        } else {
            self.expr(0)?
        };
        if self.word("if") && !self.previous_dedent() {
            self.take();
            let condition = self.expr(0)?;
            let span = Span {
                start: value.node.span.start,
                end: condition.node.span.end,
            };
            let depth = value.depth.max(condition.depth) + 1;
            value = expression(
                ExprKind::PostfixIf {
                    value: Box::new(value.node),
                    condition: Box::new(condition.node),
                },
                span,
                depth,
            )?;
        }
        Ok(value)
    }

    /// Guard recursive parser entry before processing precedence or nested syntax.
    fn expr(&mut self, minimum: u8) -> ParseResult<Parsed> {
        if self.depth >= MAX_DEPTH {
            return Err(self.error("parser depth limit exceeded"));
        }
        self.depth += 1;
        let result = self.binary(minimum);
        self.depth -= 1;
        result
    }

    /// Parse left-associative operators with Fern precedence and bounded AST depth.
    fn binary(&mut self, minimum: u8) -> ParseResult<Parsed> {
        let mut left = self.postfix()?;
        for _ in 0..self.tokens.len() {
            if self.current().kind == Kind::Bind {
                return Err(self.error("'<-' bindings are only allowed inside with"));
            }
            if minimum == 0 && self.eat(&Kind::Pipe) {
                let right = self.expr(1)?;
                left = pipe(left, right)?;
                continue;
            }
            if minimum <= 2 && matches!(self.current().kind, Kind::Range | Kind::RangeInclusive) {
                left = self.range(left)?;
                continue;
            }
            let Some((op, precedence)) = operator(&self.current().kind) else {
                break;
            };
            if precedence < minimum {
                break;
            }
            self.take();
            let right = self.expr(precedence + u8::from(op != BinaryOp::Power))?;
            let span = Span {
                start: left.node.span.start,
                end: right.node.span.end,
            };
            let depth = left.depth.max(right.depth) + 1;
            left = expression(
                ExprKind::Binary {
                    op,
                    left: Box::new(left.node),
                    right: Box::new(right.node),
                },
                span,
                depth,
            )?;
        }
        Ok(left)
    }

    /// Wrap postfix Result propagation before applying surrounding binary operators.
    fn postfix(&mut self) -> ParseResult<Parsed> {
        let mut value = self.prefix()?;
        for _ in 0..self.tokens.len() {
            if self.previous_dedent()
                && matches!(self.current().kind, Kind::Left | Kind::LeftBracket)
            {
                break;
            }
            if self.current().kind == Kind::Question {
                let span = Span {
                    start: value.node.span.start,
                    end: self.take().span.end,
                };
                value = expression(ExprKind::Try(Box::new(value.node)), span, value.depth + 1)?;
            } else if self.eat(&Kind::Left) {
                let (args, end, depth) = self.arguments()?;
                let span = Span {
                    start: value.node.span.start,
                    end,
                };
                value = expression(
                    ExprKind::Apply {
                        callee: Box::new(value.node),
                        args,
                    },
                    span,
                    value.depth.max(depth) + 1,
                )?;
            } else if self.eat(&Kind::Dot) {
                value = self.field_postfix(value)?;
            } else if self.eat(&Kind::LeftBracket) {
                value = self.index_postfix(value)?;
            } else {
                break;
            }
        }
        Ok(value)
    }

    /// Lower source indexing to the same typed, bounds-checked operation as List.get.
    /// Argument order keeps the collection and index evaluated once, left to right.
    fn index_postfix(&mut self, value: Parsed) -> ParseResult<Parsed> {
        let index = self.expr(0)?;
        let end = self
            .expect(Kind::RightBracket, "expected ']' after list index")?
            .span
            .end;
        let span = Span {
            start: value.node.span.start,
            end,
        };
        let depth = value.depth.max(index.depth) + 1;
        expression(
            ExprKind::Call {
                name: "List.get".into(),
                args: vec![
                    Argument::positional(value.node),
                    Argument::positional(index.node),
                ],
            },
            span,
            depth,
        )
    }

    /// Parse one member selector, retaining opaque recovery identity only for the private token.
    fn field_postfix(&mut self, value: Parsed) -> ParseResult<Parsed> {
        let (name, end) = if self.current().kind == Kind::MemberHole {
            let selector = self.take().span;
            let site = self
                .member_hole
                .as_mut()
                .ok_or_else(|| Diagnostic::new(selector, "invalid editor hole"))?;
            site.attach(
                value.node.span,
                Span {
                    start: value.node.span.start,
                    end: selector.end,
                },
            )?;
            (String::new(), selector)
        } else if let Kind::Number(number) = self.current().kind.clone() {
            let token = self.take();
            if !number.bytes().all(|b| b.is_ascii_digit()) {
                return Err(Diagnostic::new(
                    token.span,
                    "tuple index must be a nonnegative integer",
                ));
            }
            (number, token.span)
        } else {
            self.name()?
        };
        let span = Span {
            start: value.node.span.start,
            end: end.end,
        };
        expression(
            ExprKind::Field {
                value: Box::new(value.node),
                name,
            },
            span,
            value.depth + 1,
        )
    }

    /// Parse unary operators, literals, calls, grouping, and conditionals.
    fn prefix(&mut self) -> ParseResult<Parsed> {
        if self.word("receive") {
            return self.receive_expression();
        }
        if self.word("with") {
            return self.with_expression();
        }
        if self.word("for") {
            return self.for_expression();
        }
        if self.word("return") {
            let start = self.take().span.start;
            let value = self.expr(0)?;
            let span = Span {
                start,
                end: value.node.span.end,
            };
            return expression(
                ExprKind::Return(Box::new(value.node)),
                span,
                value.depth + 1,
            );
        }
        if self.word("fn") || (self.current().kind == Kind::Left && self.lambda_ahead()) {
            return self.lambda();
        }
        if self.word("if") {
            return self.conditional();
        }
        if self.word("match") {
            return self.match_expression();
        }
        if self.word("not") || matches!(self.current().kind, Kind::Minus | Kind::BitNot) {
            return self.unary();
        }
        let token = self.take();
        match token.kind {
            Kind::Name(name) if name == "break" => expression(ExprKind::Break, token.span, 1),
            Kind::Name(name) if name == "continue" => expression(ExprKind::Continue, token.span, 1),
            Kind::Number(text) => number(&text, token.span),
            Kind::Text(text) => expression(ExprKind::String(text), token.span, 1),
            Kind::StringOpen => self.interpolation(token.span, false),
            Kind::MultilineOpen => self.interpolation(token.span, true),
            Kind::Name(name) if name == "true" || name == "false" => {
                expression(ExprKind::Bool(name == "true"), token.span, 1)
            }
            Kind::Name(name) if !reserved(&name) || name == "spawn" || name == "send" => {
                self.named(name, token.span)
            }
            Kind::LeftBracket => self.list(token.span),
            Kind::Percent => self.map_or_update(token.span),
            Kind::Left => self.parenthesized(token.span),
            _ => Err(Diagnostic::new(
                token.span,
                "expected expression; this syntax is unsupported in the Rust prototype",
            )),
        }
    }

    /// Ranges bind below arithmetic and reject chained endpoints without materializing values.
    fn range(&mut self, left: Parsed) -> ParseResult<Parsed> {
        if matches!(left.node.kind, ExprKind::Range { .. }) {
            return Err(self.error("range operators cannot be chained"));
        }
        let inclusive = self.take().kind == Kind::RangeInclusive;
        let right = self.expr(3)?;
        let span = Span {
            start: left.node.span.start,
            end: right.node.span.end,
        };
        let depth = left.depth.max(right.depth) + 1;
        expression(
            ExprKind::Range {
                start: Box::new(left.node),
                end: Box::new(right.node),
                inclusive,
            },
            span,
            depth,
        )
    }

    /// Parse an immutable collection loop with one pattern and a scoped suite.
    fn for_expression(&mut self) -> ParseResult<Parsed> {
        let start = self.take().span.start;
        let pattern = self.pattern()?;
        if !self.word("in") {
            return Err(self.error("expected 'in' after for pattern"));
        }
        self.take();
        let iterable = self.expr(0)?;
        self.expect(Kind::Colon, "expected ':' after for iterable")?;
        let body = self.suite()?;
        let span = Span {
            start,
            end: body.node.span.end,
        };
        let depth = iterable.depth.max(body.depth) + 1;
        expression(
            ExprKind::For {
                pattern,
                iterable: Box::new(iterable.node),
                body: Box::new(body.node),
            },
            span,
            depth,
        )
    }

    /// Parse sequential Result bindings before success and optional error suites.
    fn with_expression(&mut self) -> ParseResult<Parsed> {
        let start = self.take().span.start;
        let (bindings, mut depth) = self.with_bindings()?;
        if !self.word("do") {
            return Err(self.error("expected 'do' after with bindings"));
        }
        self.take();
        let body = self.suite()?;
        depth = depth.max(body.depth + 1);
        let mut end = body.node.span.end;
        if self.current().kind == Kind::Newline
            && self
                .tokens
                .get(self.position + 1)
                .is_some_and(|token| matches!(&token.kind,Kind::Name(name) if name=="else"))
        {
            self.take();
        }
        let arms = if self.word("else") {
            self.take();
            let (arms, arm_depth, arm_end) = self.match_arms(false)?;
            depth = depth.max(arm_depth);
            end = arm_end;
            Some(arms)
        } else {
            None
        };
        expression(
            ExprKind::With {
                bindings,
                body: Box::new(body.node),
                arms,
            },
            Span { start, end },
            depth,
        )
    }

    /// Bind lists own one indentation level; nested initializer suites consume only theirs.
    fn with_bindings(&mut self) -> ParseResult<(Vec<crate::ast::WithBinding>, usize)> {
        let block = self.eat(&Kind::Newline);
        if block {
            self.expect(Kind::Indent, "expected indented with bindings")?;
        }
        let mut bindings = Vec::new();
        let mut depth = 1;
        for _ in 0..self.tokens.len() {
            let pattern = self.pattern()?;
            self.expect(Kind::Bind, "expected '<-' in with binding")?;
            let value = self.expr(0)?;
            depth = depth.max(value.depth + 1);
            let span = Span {
                start: pattern.span.start,
                end: value.node.span.end,
            };
            bindings.push(crate::ast::WithBinding {
                pattern,
                value: value.node,
                span,
            });
            if block
                && self.current().kind == Kind::Newline
                && self
                    .tokens
                    .get(self.position + 1)
                    .is_some_and(|token| token.kind == Kind::Comma)
            {
                self.take();
            }
            let comma = self.eat(&Kind::Comma);
            if block {
                if !self.eat(&Kind::Newline) && !self.previous_dedent() {
                    return Err(self.error("expected newline after with binding"));
                }
                if self.eat(&Kind::Dedent) {
                    break;
                }
                if !comma {
                    return Err(self.error("expected ',' between with bindings"));
                }
            } else if !comma {
                break;
            }
        }
        Ok((bindings, depth))
    }

    /// Recognize a lambda only when the matching parameter delimiter is followed by an arrow.
    fn lambda_ahead(&self) -> bool {
        let mut depth = 0;
        for (index, token) in self.tokens.iter().enumerate().skip(self.position) {
            match token.kind {
                Kind::Left => depth += 1,
                Kind::Right => {
                    depth -= 1;
                    if depth == 0 {
                        return self.guard_arrow != Some(index + 1)
                            && self
                                .tokens
                                .get(index + 1)
                                .is_some_and(|t| t.kind == Kind::Arrow);
                    }
                }
                Kind::End => break,
                _ => {}
            }
        }
        false
    }

    /// Parse optionally annotated anonymous parameters and their inline or indented body.
    fn lambda(&mut self) -> ParseResult<Parsed> {
        let start = self.current().span.start;
        if self.word("fn") {
            self.take();
        }
        self.expect(Kind::Left, "expected '(' before lambda parameters")?;
        let mut params = Vec::new();
        for _ in 0..self.tokens.len() {
            if self.eat(&Kind::Right) {
                break;
            }
            let (name, mut span) = self.name()?;
            let annotation = if self.eat(&Kind::Colon) {
                Some(self.ty()?)
            } else {
                None
            };
            span.end = self.tokens[self.position - 1].span.end;
            params.push(crate::ast::LambdaParam {
                name,
                annotation,
                span,
            });
            if !self.eat(&Kind::Comma) {
                self.expect(Kind::Right, "expected ',' or ')' after lambda parameter")?;
                break;
            }
        }
        self.expect(Kind::Arrow, "expected '->' before lambda body")?;
        let body = self.suite()?;
        let span = Span {
            start,
            end: body.node.span.end,
        };
        expression(
            ExprKind::Lambda {
                params,
                body: Box::new(body.node),
            },
            span,
            body.depth + 1,
        )
    }

    /// Parse source-ordered positional and labeled arguments after their opening delimiter.
    fn arguments(&mut self) -> ParseResult<(Vec<Argument>, usize, usize)> {
        let opening = self.tokens[self.position - 1].span.start;
        let mut args = Vec::new();
        let mut depth = 1;
        let mut labeled = false;
        for _ in 0..self.tokens.len() {
            if self.current().kind == Kind::Right {
                let end = self.take().span.end;
                self.finish_label_arguments(opening, end, &args)?;
                return Ok((args, end, depth));
            }
            if self.current().kind == Kind::LabelHole {
                labeled = true;
                self.take();
                self.eat(&Kind::Comma);
                continue;
            }
            let start = self.current().span.start;
            let label = self.argument_label()?;
            if label.is_none() && labeled {
                return Err(self.error("positional arguments must precede labeled arguments"));
            }
            labeled |= label.is_some();
            let arg = self.argument_value()?;
            depth = depth.max(arg.depth);
            let span = Span {
                start,
                end: arg.node.span.end,
            };
            args.push(Argument {
                label,
                value: arg.node,
                span,
            });
            if !self.eat(&Kind::Comma) {
                let end = self
                    .expect(Kind::Right, "expected ',' or ')' after argument")?
                    .span
                    .end;
                self.finish_label_arguments(opening, end, &args)?;
                return Ok((args, end, depth));
            }
        }
        Err(self.error("unterminated call arguments"))
    }

    /// Consume a label only when an identifier is immediately followed by ':'.
    fn argument_label(&mut self) -> ParseResult<Option<ArgumentLabel>> {
        let Kind::Name(name) = self.current().kind.clone() else {
            return Ok(None);
        };
        if !self
            .tokens
            .get(self.position + 1)
            .is_some_and(|token| token.kind == Kind::Colon)
        {
            return Ok(None);
        }
        if !valid_label(&name) {
            return Err(self.error("invalid argument label"));
        }
        let span = self.take().span;
        self.take();
        Ok(Some(ArgumentLabel { name, span }))
    }

    /// Parse embedded expressions using the ordinary grammar and one shared depth bound.
    fn interpolation(&mut self, mut span: Span, multiline: bool) -> ParseResult<Parsed> {
        let mut parts = Vec::new();
        let mut depth = 1;
        for _ in 0..self.tokens.len() {
            let token = self.take();
            match token.kind {
                Kind::Text(text) => parts.push(crate::ast::StringPart::Text(text)),
                Kind::HoleOpen => {
                    let value = self.expr(0)?;
                    depth = depth.max(value.depth + 1);
                    self.expect(
                        Kind::HoleClose,
                        "expected '}' after interpolation expression",
                    )?;
                    parts.push(crate::ast::StringPart::Value(value.node));
                }
                Kind::StringClose | Kind::MultilineClose => {
                    span.end = token.span.end;
                    let kind = if multiline {
                        ExprKind::MultilineString(parts)
                    } else {
                        ExprKind::Interpolate(parts)
                    };
                    return expression(kind, span, depth);
                }
                _ => {
                    return Err(Diagnostic::new(
                        token.span,
                        "expected string segment or interpolation",
                    ));
                }
            }
        }
        Err(Diagnostic::new(span, "unterminated interpolated string"))
    }

    /// Distinguish Unit, grouping, and structural tuples by a comma.
    fn parenthesized(&mut self, mut span: Span) -> ParseResult<Parsed> {
        if self.current().kind == Kind::Right {
            span.end = self.take().span.end;
            return expression(ExprKind::Unit, span, 1);
        }
        let first = self.expr(0)?;
        if !self.eat(&Kind::Comma) {
            self.expect(Kind::Right, "expected ',' or ')' after expression")?;
            return Ok(first);
        }
        let mut depth = first.depth + 1;
        let mut fields = vec![first.node];
        for _ in 0..self.tokens.len() {
            if self.current().kind == Kind::Right {
                span.end = self.take().span.end;
                break;
            }
            let value = self.expr(0)?;
            depth = depth.max(value.depth + 1);
            fields.push(value.node);
            if !self.eat(&Kind::Comma) {
                span.end = self
                    .expect(Kind::Right, "expected ',' or ')' after tuple field")?
                    .span
                    .end;
                break;
            }
        }
        expression(ExprKind::Tuple(fields), span, depth)
    }

    /// Distinguish maps from record updates after their first complete expression.
    fn map_or_update(&mut self, mut span: Span) -> ParseResult<Parsed> {
        self.expect(
            Kind::LeftBrace,
            "expected '{' after '%' in map or record update",
        )?;
        if self.current().kind == Kind::RightBrace {
            span.end = self.take().span.end;
            return expression(ExprKind::Map(Vec::new()), span, 1);
        }
        let first = self.expr(0)?;
        if self.eat(&Kind::Bar) {
            return self.record_update(first, span);
        }
        let mut depth = first.depth + 1;
        let mut pairs = Vec::new();
        let mut key = first;
        for _ in 0..self.tokens.len() {
            self.expect(
                Kind::Colon,
                "expected ':' after map key or '|' after record base",
            )?;
            let value = self.expr(0)?;
            depth = depth.max(key.depth.max(value.depth) + 1);
            pairs.push((key.node, value.node));
            if !self.eat(&Kind::Comma) || self.current().kind == Kind::RightBrace {
                span.end = self
                    .expect(Kind::RightBrace, "expected ',' or '}' after map entry")?
                    .span
                    .end;
                return expression(ExprKind::Map(pairs), span, depth);
            }
            key = self.expr(0)?;
        }
        Err(Diagnostic::new(span, "unterminated map literal"))
    }

    /// Preserve written field order and reject ambiguous repeated replacements.
    fn record_update(&mut self, value: Parsed, mut span: Span) -> ParseResult<Parsed> {
        let mut fields = Vec::new();
        let mut names = std::collections::BTreeSet::new();
        let mut depth = value.depth + 1;
        for _ in 0..self.tokens.len() {
            let (name, field_span) = self
                .name()
                .map_err(|_| self.error("expected field name in record update"))?;
            if !names.insert(name.clone()) {
                return Err(Diagnostic::new(
                    field_span,
                    format!("duplicate record update field '{name}'"),
                ));
            }
            self.expect(Kind::Colon, "expected ':' after record field name")?;
            let field = self.expr(0)?;
            depth = depth.max(field.depth + 1);
            fields.push(crate::ast::RecordField {
                name,
                value: field.node,
                span: field_span,
            });
            if !self.eat(&Kind::Comma) || self.current().kind == Kind::RightBrace {
                span.end = self
                    .expect(
                        Kind::RightBrace,
                        "expected ',' or '}' after record update field",
                    )?
                    .span
                    .end;
                return expression(
                    ExprKind::RecordUpdate {
                        value: Box::new(value.node),
                        fields,
                    },
                    span,
                    depth,
                );
            }
        }
        Err(Diagnostic::new(span, "unterminated record update"))
    }

    /// Parse immutable list literals with optional trailing commas.
    fn list(&mut self, mut span: Span) -> ParseResult<Parsed> {
        let mut items = Vec::new();
        let mut depth = 1;
        for _ in 0..self.tokens.len() {
            if self.current().kind == Kind::RightBracket {
                span.end = self.take().span.end;
                break;
            }
            let item = self.expr(0)?;
            depth = depth.max(item.depth + 1);
            items.push(item.node);
            if !self.eat(&Kind::Comma) {
                span.end = self
                    .expect(Kind::RightBracket, "expected ',' or ']' after list element")?
                    .span
                    .end;
                break;
            }
        }
        expression(ExprKind::List(items), span, depth)
    }

    /// Parse an indented sequence of pattern arms and bound its resulting tree.
    fn match_expression(&mut self) -> ParseResult<Parsed> {
        let start = self.take().span.start;
        if self.eat(&Kind::Colon) {
            return self.condition_match(start);
        }
        let value = self.expr(0)?;
        self.expect(Kind::Colon, "expected ':' after match value")?;
        let (arms, depth, end) = self.match_arms(true)?;
        expression(
            ExprKind::Match {
                value: Box::new(value.node),
                arms,
            },
            Span { start, end },
            depth.max(value.depth + 1),
        )
    }

    /// Share pattern arms between value matches and explicit with error handlers.
    fn match_arms(&mut self, inline_list: bool) -> ParseResult<(Vec<MatchArm>, usize, usize)> {
        let block = self.eat(&Kind::Newline);
        if block {
            self.expect(Kind::Indent, "match requires an indented arm block")?;
        }
        let mut arms = Vec::new();
        let mut depth = 1;
        let mut end = self.current().span.start;
        for _ in 0..self.tokens.len() {
            if block && self.eat(&Kind::Dedent) {
                break;
            }
            let pattern = self.typed_pattern()?;
            let guard = if self.word("if") {
                self.take();
                Some(self.guard_expression()?)
            } else {
                None
            };
            if let Some(guard) = &guard {
                depth = depth.max(guard.depth + 1);
            }
            self.expect(Kind::Arrow, "expected '->' after match pattern")?;
            let body = self.suite()?;
            end = body.node.span.end;
            depth = depth.max(body.depth + 1);
            let span = Span {
                start: pattern.span.start,
                end,
            };
            arms.push(MatchArm {
                guard: guard.map(|value| value.node),
                pattern,
                body: body.node,
                span,
            });
            if !block {
                if inline_list && self.inline_arm_follows() {
                    self.take(); // The comma belongs to the next pattern arm.
                    continue;
                }
                break;
            }
            if !self.eat(&Kind::Newline)
                && self.current().kind != Kind::Dedent
                && !self.previous_dedent()
            {
                return Err(self.error("expected end of line after match arm"));
            }
        }
        if arms.is_empty() {
            return Err(self.error("match requires at least one arm"));
        }
        Ok((arms, depth, end))
    }

    /// Leave caller commas untouched unless a balanced next pattern/guard has an arm arrow.
    fn inline_arm_follows(&self) -> bool {
        if self.current().kind != Kind::Comma {
            return false;
        }
        let mut depth = 0usize;
        for token in self.tokens.iter().skip(self.position + 1) {
            match token.kind {
                Kind::Left | Kind::LeftBracket | Kind::LeftBrace => depth += 1,
                Kind::Right | Kind::RightBracket | Kind::RightBrace => {
                    if depth == 0 {
                        return false;
                    }
                    depth -= 1;
                }
                Kind::Arrow if depth == 0 => return true,
                Kind::Comma | Kind::Newline | Kind::Dedent | Kind::End if depth == 0 => {
                    return false;
                }
                _ => {}
            }
        }
        false
    }

    /// Parse condition branches separately from pattern matches, retaining wildcard syntax.
    fn condition_match(&mut self, start: usize) -> ParseResult<Parsed> {
        self.expect(
            Kind::Newline,
            "condition match requires an indented arm block",
        )?;
        self.expect(
            Kind::Indent,
            "condition match requires an indented arm block",
        )?;
        let mut arms = Vec::new();
        let mut depth = 1;
        let mut end = start;
        let mut wildcard = false;
        for _ in 0..self.tokens.len() {
            if self.eat(&Kind::Dedent) {
                break;
            }
            if wildcard {
                return Err(self.error("wildcard must be the final condition match arm"));
            }
            let arm_start = self.current().span.start;
            let condition = if self.word("_") {
                self.take();
                wildcard = true;
                None
            } else {
                Some(self.guard_expression()?)
            };
            if let Some(value) = &condition {
                depth = depth.max(value.depth + 1);
            }
            self.expect(Kind::Arrow, "expected '->' after match condition")?;
            let body = self.suite()?;
            end = body.node.span.end;
            depth = depth.max(body.depth + 1);
            arms.push(crate::ast::ConditionArm {
                condition: condition.map(|v| v.node),
                body: body.node,
                span: Span {
                    start: arm_start,
                    end,
                },
            });
            if !self.eat(&Kind::Newline)
                && self.current().kind != Kind::Dedent
                && !self.previous_dedent()
            {
                return Err(self.error("expected end of line after condition match arm"));
            }
        }
        if arms.is_empty() {
            return Err(self.error("condition match requires at least one arm"));
        }
        expression(ExprKind::ConditionMatch(arms), Span { start, end }, depth)
    }

    /// Keep the arm separator out of lambda lookahead while allowing callback guards.
    fn guard_expression(&mut self) -> ParseResult<Parsed> {
        let mut nesting = 0;
        let mut separator = None;
        for (index, token) in self.tokens.iter().enumerate().skip(self.position) {
            match token.kind {
                Kind::Left | Kind::LeftBracket | Kind::LeftBrace => nesting += 1,
                Kind::Right | Kind::RightBracket | Kind::RightBrace => nesting -= 1,
                Kind::Arrow if nesting == 0 => {
                    separator = Some(index);
                    break;
                }
                Kind::End => break,
                _ => {}
            }
        }
        let previous = self.guard_arrow;
        self.guard_arrow = separator;
        let result = self.expr(0);
        self.guard_arrow = previous;
        result
    }

    /// Typed match binders narrow values without changing parameter annotation parsing.
    fn typed_pattern(&mut self) -> ParseResult<Pattern> {
        let pattern = self.pattern()?;
        if !self.eat(&Kind::Colon) {
            return Ok(pattern);
        }
        let annotation = self.pattern_annotation()?;
        let span = Span {
            start: pattern.span.start,
            end: self.tokens[self.position - 1].span.end,
        };
        Ok(Pattern {
            kind: PatternKind::Typed {
                pattern: Box::new(pattern),
                annotation,
            },
            span,
        })
    }

    /// Try full function type syntax, then protect a grouped annotation's arm delimiter.
    fn pattern_annotation(&mut self) -> ParseResult<Type> {
        let position = self.position;
        let depth = self.depth;
        let spans = self.type_spans.as_ref().map_or(0, Vec::len);
        if let Ok(ty) = self.ty()
            && (self.current().kind == Kind::Arrow || self.word("if"))
        {
            return Ok(ty);
        }
        self.position = position;
        self.depth = depth;
        if let Some(recorded) = &mut self.type_spans {
            recorded.truncate(spans);
        }
        let previous = self.type_arm_boundary;
        self.type_arm_boundary = Some(depth + 1);
        let result = self.ty();
        self.type_arm_boundary = previous;
        result
    }

    /// Bound recursive patterns before parsing constructor payloads.
    fn pattern(&mut self) -> ParseResult<Pattern> {
        if self.depth >= MAX_DEPTH {
            return Err(self.error("pattern depth limit exceeded"));
        }
        self.depth += 1;
        let result = self.pattern_value();
        self.depth -= 1;
        result
    }

    /// Parse nested constructors, scalar literals, wildcard and lowercase bindings.
    fn pattern_value(&mut self) -> ParseResult<Pattern> {
        if self.current().kind == Kind::Left {
            return self.sequence_pattern(false);
        }
        if self.current().kind == Kind::LeftBracket {
            return self.sequence_pattern(true);
        }
        if matches!(self.current().kind, Kind::Name(_)) && !self.word("true") && !self.word("false")
        {
            let (name, span) = self.qualified_name()?;
            if name == "_" {
                return Ok(Pattern {
                    kind: PatternKind::Wildcard,
                    span,
                });
            }
            if name
                .rsplit('.')
                .next()
                .is_some_and(|part| part.starts_with(|c: char| c.is_uppercase()))
            {
                return self.named_pattern(name, span);
            }
            return Ok(Pattern {
                kind: PatternKind::Bind(name),
                span,
            });
        }
        let token = self.take();
        let mut span = token.span;
        let kind = match token.kind {
            Kind::Number(text) => PatternKind::Int(pattern_integer(&text, span)?),
            Kind::Minus => {
                let number = self.take();
                span.end = number.span.end;
                let Kind::Number(text) = number.kind else {
                    return Err(Diagnostic::new(span, "expected integer pattern after '-'"));
                };
                PatternKind::Int(pattern_integer(&format!("-{text}"), span)?)
            }
            Kind::Text(text) => PatternKind::String(text),
            Kind::Name(name) if name == "true" || name == "false" => {
                PatternKind::Bool(name == "true")
            }
            _ => {
                return Err(Diagnostic::new(
                    span,
                    "unsupported pattern; expected literal, binding, wildcard or constructor",
                ));
            }
        };
        Ok(Pattern { kind, span })
    }

    /// Parse bounded list/tuple prefixes, preserving grouping and singleton tuple identity.
    fn sequence_pattern(&mut self, list: bool) -> ParseResult<Pattern> {
        let mut span = self.take().span;
        let close = if list {
            Kind::RightBracket
        } else {
            Kind::Right
        };
        let mut prefix = Vec::new();
        let mut rest = None;
        let mut comma = false;
        for _ in 0..self.tokens.len() {
            if self.current().kind == close {
                break;
            }
            if self.eat(&Kind::Range) {
                rest = Some(Box::new(self.rest_pattern()?));
                self.eat(&Kind::Comma);
                if self.current().kind != close {
                    return Err(
                        self.error("rest pattern must be last; only one rest binding is allowed")
                    );
                }
                break;
            }
            if prefix.len() >= MAX_PATTERN_PREFIX {
                return Err(self.error("sequence pattern prefix limit exceeded (128)"));
            }
            prefix.push(self.pattern()?);
            if !self.eat(&Kind::Comma) {
                break;
            }
            comma = true;
        }
        span.end = self
            .expect(
                close,
                "expected ',' or closing delimiter after sequence pattern",
            )?
            .span
            .end;
        let kind = if list {
            PatternKind::List { prefix, rest }
        } else if let Some(rest) = rest {
            PatternKind::TupleRest { prefix, rest }
        } else if prefix.len() == 1 && !comma {
            return Ok(prefix.remove(0));
        } else {
            PatternKind::Tuple(prefix)
        };
        Ok(Pattern { kind, span })
    }

    /// Restrict a suffix to one identifier or wildcard; nested patterns are never rest values.
    fn rest_pattern(&mut self) -> ParseResult<Pattern> {
        let (name, span) = self
            .name()
            .map_err(|_| self.error("rest pattern requires a binding name or '_'"))?;
        let kind = if name == "_" {
            PatternKind::Wildcard
        } else {
            PatternKind::Bind(name)
        };
        Ok(Pattern { kind, span })
    }

    /// Parse recursive constructor fields while retaining the compatible flat AST form.
    fn named_pattern(&mut self, name: String, mut span: Span) -> ParseResult<Pattern> {
        let mut fields = Vec::new();
        if self.eat(&Kind::Left) {
            for _ in 0..self.tokens.len() {
                if self.current().kind == Kind::Right {
                    span.end = self.take().span.end;
                    break;
                }
                fields.push(self.pattern()?);
                if !self.eat(&Kind::Comma) {
                    span.end = self
                        .expect(Kind::Right, "expected ',' or ')' after payload pattern")?
                        .span
                        .end;
                    break;
                }
            }
        }
        let kind = if let Some(constructor) = builtin_constructor(&name) {
            if constructor == Constructor::None && !fields.is_empty() {
                return Err(Diagnostic::new(
                    span,
                    "None patterns do not accept a payload",
                ));
            }
            if constructor == Constructor::None {
                PatternKind::Constructor {
                    constructor,
                    binding: None,
                }
            } else if fields.len() == 1
                && matches!(fields[0].kind, PatternKind::Bind(_) | PatternKind::Wildcard)
            {
                let binding = match &fields[0].kind {
                    PatternKind::Bind(name) => Some(name.clone()),
                    _ => None,
                };
                PatternKind::Constructor {
                    constructor,
                    binding,
                }
            } else {
                PatternKind::NamedConstructor { name, fields }
            }
        } else {
            PatternKind::NamedConstructor { name, fields }
        };
        Ok(Pattern { kind, span })
    }

    /// Parse unary operands, accepting the Int minimum without positive overflow.
    fn unary(&mut self) -> ParseResult<Parsed> {
        let token = self.take();
        let op = match token.kind {
            Kind::Minus => UnaryOp::Negate,
            Kind::BitNot => UnaryOp::BitNot,
            _ => UnaryOp::Not,
        };
        if op == UnaryOp::Negate
            && let Kind::Number(text) = &self.current().kind
        {
            let text = format!("-{text}");
            let end = self.take().span.end;
            return number(
                &text,
                Span {
                    start: token.span.start,
                    end,
                },
            );
        }
        let value = self.expr(12)?;
        let span = Span {
            start: token.span.start,
            end: value.node.span.end,
        };
        expression(
            ExprKind::Unary {
                op,
                value: Box::new(value.node),
            },
            span,
            value.depth + 1,
        )
    }

    /// Parse a qualified name and optional positional argument list.
    fn named(&mut self, mut name: String, mut span: Span) -> ParseResult<Parsed> {
        for _ in 0..self.tokens.len() {
            if self.current().kind != Kind::Dot
                || !matches!(
                    self.tokens.get(self.position + 1).map(|t| &t.kind),
                    Some(Kind::Name(_))
                )
            {
                break;
            }
            self.take();
            let (part, part_span) = self.name()?;
            name.push('.');
            name.push_str(&part);
            span.end = part_span.end;
        }
        if !self.eat(&Kind::Left) {
            return expression(ExprKind::Name(name), span, 1);
        }
        let (args, end, depth) = self.arguments()?;
        span.end = end;
        expression(ExprKind::Call { name, args }, span, depth + 1)
    }

    /// Parse inline or indented if/else expressions, including else: if chains.
    fn conditional(&mut self) -> ParseResult<Parsed> {
        let start = self.take().span.start;
        let condition = self.expr(0)?;
        self.expect(Kind::Colon, "expected ':' after if condition")?;
        let then_branch = self.suite()?;
        // A newline before else belongs to this conditional, not its enclosing block.
        if self.current().kind == Kind::Newline
            && self
                .tokens
                .get(self.position + 1)
                .is_some_and(|t| matches!(&t.kind, Kind::Name(n) if n == "else"))
        {
            self.take();
        }
        let mut end = then_branch.node.span.end;
        let mut depth = condition.depth.max(then_branch.depth) + 1;
        let else_branch = if self.word("else") {
            self.take();
            self.expect(Kind::Colon, "expected ':' after else")?;
            let value = self.suite()?;
            end = value.node.span.end;
            depth = depth.max(value.depth + 1);
            Some(Box::new(value.node))
        } else {
            None
        };
        expression(
            ExprKind::If {
                condition: Box::new(condition.node),
                then_branch: Box::new(then_branch.node),
                else_branch,
            },
            Span { start, end },
            depth,
        )
    }
}

/// Recognize only the built-in Option/Result constructors.
fn builtin_constructor(name: &str) -> Option<Constructor> {
    match name {
        "Some" => Some(Constructor::Some),
        "None" => Some(Constructor::None),
        "Ok" => Some(Constructor::Ok),
        "Err" => Some(Constructor::Err),
        _ => None,
    }
}

/// Parse a literal pattern with the same signed 64-bit range as expressions.
fn pattern_integer(text: &str, span: Span) -> ParseResult<i64> {
    integer_value(text, span)
}

/// Convert a decimal token to Fern's signed 64-bit Int with a stable diagnostic.
fn integer(text: &str, span: Span) -> ParseResult<Parsed> {
    let value = integer_value(text, span)?;
    expression(ExprKind::Int(value), span, 1)
}

/// Return the supported operator and its increasing binding strength.
fn operator(kind: &Kind) -> Option<(BinaryOp, u8)> {
    Some(match kind {
        Kind::Name(n) if n == "or" => (BinaryOp::Or, 0),
        Kind::Name(n) if n == "and" => (BinaryOp::And, 1),
        Kind::Name(n) if n == "in" => (BinaryOp::In, 7),
        Kind::BitOr => (BinaryOp::BitOr, 3),
        Kind::BitXor => (BinaryOp::BitXor, 4),
        Kind::BitAnd => (BinaryOp::BitAnd, 5),
        Kind::Eq => (BinaryOp::Eq, 6),
        Kind::Ne => (BinaryOp::Ne, 6),
        Kind::Lt => (BinaryOp::Lt, 7),
        Kind::Le => (BinaryOp::Le, 7),
        Kind::Gt => (BinaryOp::Gt, 7),
        Kind::Ge => (BinaryOp::Ge, 7),
        Kind::ShiftLeft => (BinaryOp::ShiftLeft, 8),
        Kind::ShiftRight => (BinaryOp::ShiftRight, 8),
        Kind::Plus => (BinaryOp::Add, 9),
        Kind::Minus => (BinaryOp::Subtract, 9),
        Kind::Star => (BinaryOp::Multiply, 10),
        Kind::Slash => (BinaryOp::Divide, 10),
        Kind::Percent => (BinaryOp::Remainder, 10),
        Kind::Power => (BinaryOp::Power, 11),
        _ => return None,
    })
}

/// Keep Fern keywords out of bindings even when their syntax is unsupported.
fn reserved(name: &str) -> bool {
    matches!(
        name,
        "fn" | "let"
            | "const"
            | "comptime"
            | "if"
            | "else"
            | "true"
            | "false"
            | "and"
            | "or"
            | "not"
            | "pub"
            | "type"
            | "match"
            | "return"
            | "for"
            | "in"
            | "while"
            | "import"
            | "with"
            | "trait"
            | "impl"
            | "actor"
            | "receive"
            | "spawn"
            | "where"
            | "do"
            | "defer"
            | "as"
            | "module"
            | "break"
            | "continue"
            | "derive"
            | "newtype"
            | "send"
            | "after"
    )
}

/// Scan decimal integer/float tokens, rejecting malformed suffixes at their source.
fn number_end(line: &str, start: usize, offset: usize) -> ParseResult<usize> {
    let bytes = line.as_bytes();
    let mut at = start;
    if bytes.get(start) == Some(&b'0')
        && matches!(
            bytes.get(start + 1),
            Some(b'x' | b'X' | b'b' | b'B' | b'o' | b'O')
        )
    {
        at += 2;
        while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_') {
            at += 1;
        }
        return Ok(at);
    }
    while at < bytes.len() && (bytes[at].is_ascii_digit() || bytes[at] == b'_') {
        at += 1;
    }
    if bytes.get(at) == Some(&b'.') && bytes.get(at + 1) != Some(&b'.') {
        at += 1;
        let fraction = at;
        while at < bytes.len() && bytes[at].is_ascii_digit() {
            at += 1;
        }
        if at == fraction {
            return Err(Diagnostic::new(
                Span {
                    start: offset + start,
                    end: offset + at,
                },
                "expected digits after decimal point",
            ));
        }
    }
    if matches!(bytes.get(at), Some(b'e' | b'E')) {
        at += 1;
        if matches!(bytes.get(at), Some(b'+' | b'-')) {
            at += 1;
        }
        let exponent = at;
        while at < bytes.len() && bytes[at].is_ascii_digit() {
            at += 1;
        }
        if at == exponent {
            return Err(Diagnostic::new(
                Span {
                    start: offset + start,
                    end: offset + at,
                },
                "expected exponent digits",
            ));
        }
    }
    if bytes.get(at).is_some_and(|b| {
        b.is_ascii_alphabetic() || *b == b'_' || (*b == b'.' && bytes.get(at + 1) != Some(&b'.'))
    }) {
        return Err(Diagnostic::new(
            Span {
                start: offset + start,
                end: offset + at + 1,
            },
            "unsupported numeric literal suffix",
        ));
    }
    Ok(at)
}

/// Preserve decimal Float literals as IEEE doubles; overflowing literals are diagnosed.
fn number(text: &str, span: Span) -> ParseResult<Parsed> {
    if integer_radix(text).0 != 10 || !text.contains(['.', 'e', 'E']) {
        return integer(text, span);
    }
    let value = text
        .parse::<f64>()
        .map_err(|_| Diagnostic::new(span, "invalid Float literal"))?;
    if !value.is_finite() {
        return Err(Diagnostic::new(span, "Float literal exceeds finite range"));
    }
    expression(ExprKind::Float(value), span, 1)
}

/// Identify integer radix without accepting prefixes as digits or losing a leading sign.
fn integer_radix(text: &str) -> (u32, &str) {
    let digits = text.strip_prefix('-').unwrap_or(text);
    if let Some(rest) = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        (16, rest)
    } else if let Some(rest) = digits
        .strip_prefix("0b")
        .or_else(|| digits.strip_prefix("0B"))
    {
        (2, rest)
    } else if let Some(rest) = digits
        .strip_prefix("0o")
        .or_else(|| digits.strip_prefix("0O"))
    {
        (8, rest)
    } else {
        (10, digits)
    }
}

/// Parse separators and unsigned magnitude before applying the asymmetric signed Int limit.
fn integer_value(text: &str, span: Span) -> ParseResult<i64> {
    let (radix, digits) = integer_radix(text);
    if digits.is_empty()
        || digits.starts_with('_')
        || digits.ends_with('_')
        || digits.contains("__")
        || !digits.chars().all(|c| c == '_' || c.is_digit(radix))
    {
        return Err(Diagnostic::new(
            span,
            "invalid integer digits or separators",
        ));
    }
    let magnitude = u64::from_str_radix(&digits.replace('_', ""), radix)
        .map_err(|_| Diagnostic::new(span, "integer is outside Int range"))?;
    let negative = text.starts_with('-');
    if magnitude > i64::MAX as u64 + u64::from(negative) {
        return Err(Diagnostic::new(span, "integer is outside Int range"));
    }
    if negative && magnitude == 1u64 << 63 {
        Ok(i64::MIN)
    } else if negative {
        Ok(-(magnitude as i64))
    } else {
        Ok(magnitude as i64)
    }
}

/// Record piped argument placement without reordering evaluation or duplicating its value.
fn pipe(left: Parsed, right: Parsed) -> ParseResult<Parsed> {
    let span = Span {
        start: left.node.span.start,
        end: right.node.span.end,
    };
    let depth = left.depth.max(right.depth) + 1;
    let (name, mut args) = match right.node.kind {
        ExprKind::Call { name, args } => (name, args),
        ExprKind::Name(name) => (name, Vec::new()),
        _ => {
            return Err(Diagnostic::new(
                span,
                "pipe target must be a function name or call",
            ));
        }
    };
    let positions: Vec<_> = args
        .iter()
        .enumerate()
        .filter_map(|(i, arg)| matches!(&arg.kind, ExprKind::Name(n) if n == "_").then_some(i))
        .collect();
    if positions.len() > 1 {
        return Err(Diagnostic::new(
            span,
            "pipe accepts at most one argument placeholder",
        ));
    }
    let position = positions.first().copied().unwrap_or(0);
    let label = if !positions.is_empty() {
        args.remove(position).label
    } else {
        None
    };
    expression(
        ExprKind::Pipe {
            value: Box::new(left.node),
            name,
            args,
            position,
            label,
        },
        span,
        depth,
    )
}

/// Share type-name arity and primitive identities with canonical static codec targets.
pub(crate) fn named_type(name: String, mut arguments: Vec<Type>, span: Span) -> ParseResult<Type> {
    let arity = match name.as_str() {
        "Pid" | "List" | "Option" => Some(1),
        "Result" | "Map" => Some(2),
        "Int" | "Float" | "Bool" | "String" | "Unit" | "Range" => Some(0),
        _ => None,
    };
    if arity.is_some_and(|arity| arguments.len() != arity) {
        return Err(Diagnostic::new(
            span,
            format!("wrong type argument count for {name}; separate arguments with ','"),
        ));
    }
    Ok(match name.as_str() {
        "Int" => Type::Int,
        "Range" => Type::Range,
        "Float" => Type::Float,
        "Unit" => Type::Unit,
        "Bool" => Type::Bool,
        "String" => Type::String,
        "Pid" => Type::Pid(Box::new(arguments.remove(0))),
        "List" => Type::List(Box::new(arguments.remove(0))),
        "Option" => Type::Option(Box::new(arguments.remove(0))),
        "Result" | "Map" => {
            let second = Box::new(arguments.remove(1));
            let first = Box::new(arguments.remove(0));
            if name == "Map" {
                Type::Map(first, second)
            } else {
                Type::Result(first, second)
            }
        }
        _ if arguments.is_empty()
            && !name.contains('.')
            && name.starts_with(|c: char| c.is_lowercase()) =>
        {
            Type::Generic(name)
        }
        _ if crate::runtime::native_type(&name).is_some() && arguments.is_empty() => {
            Type::Native(crate::runtime::native_type(&name).expect("checked native name"))
        }
        _ => Type::Named(name, arguments),
    })
}

#[cfg(test)]
mod continuation_tests {
    #[test]
    fn indentation_preserves_triple_content_and_closing_lines() {
        let source =
            "let text = \"\"\"\n  first\n  {\"\"\"nested\nline\"\"\"}\n\"\"\"\nprintln(text)\n";
        let expected = "    let text = \"\"\"\n  first\n  {\"\"\"nested\nline\"\"\"}\n\"\"\"\n    println(text)\n";
        assert_eq!(super::indent_code(source, "    ").unwrap(), expected);
        assert!(super::indent_code("let text = \"\"\"open", "    ").is_err());
    }

    #[test]
    fn continuation_tracks_multiline_literals_and_documentation() {
        for source in [
            "let text = \"\"\"\ntext",
            "/* open",
            "@doc \"\"\"text\"\"\"",
            "@doc \"\"\"\nopen",
        ] {
            assert!(super::line_continues(source), "{source}");
        }
        for source in [
            "let text = \"\"\"\ntext\"\"\"",
            "@doc \"\"\"text\"\"\"\nfn f(): 42",
            "\"\"\"bad\\q",
        ] {
            assert!(!super::line_continues(source), "{source}");
        }
    }

    #[test]
    fn continuation_uses_tokens_and_rejects_mismatched_delimiters() {
        for source in [
            "with # sequence",
            "let result = with",
            "with value <- load() do",
            "else # with handler",
            "fn main(): # block",
            "fn f(0: Int) -> # clause body",
            "fn f(x: Int) if x > 0 ->",
            "fn f(x: Int) if ((n: Int) -> n > 0)(x) ->",
            "(x) -> # callback",
            "println(",
            "[1,",
            "apply(\n    [1, 2]\n",
        ] {
            assert!(super::line_continues(source), "{source}");
        }
        for source in [
            "println(42)",
            "fn f(0: Int) -> 1 # complete clause",
            "fn f(x: Int) ->\n    x + 1",
            "fn f(x: Int) if ((n: Int) -> n > 0)(x) -> x",
            "\"colon: arrow -> bracket (\"",
            "\"{(42)}\"",
            "[1)",
            "# comment:",
            "\"unterminated",
        ] {
            assert!(!super::line_continues(source), "{source}");
        }
    }
}

mod comptime;

#[path = "parse/foreign.rs"]
mod foreign;
