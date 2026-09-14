//! Bounded source-only inspection; private tokens remain an implementation detail.
use super::{ParseResult, parse, source_parser};
use crate::{Diagnostic, Span};
use std::fmt::{self, Write};

const MAX_DUMP: usize = 16 * 1024 * 1024;

/// Render the real layout-aware lexer with source byte spans and escaped token payloads.
/// Uses ordinary source/token/depth limits; no recovered token or partial text is published.
pub fn debug_tokens(source: &str) -> ParseResult<String> {
    let parser = source_parser(source, false)?;
    let mut output = Dump::default();
    for token in parser.tokens {
        writeln!(
            output,
            "{}..{} {:?}",
            token.span.start, token.span.end, token.kind
        )
        .map_err(|_| limit())?;
    }
    Ok(output.text)
}

/// Render the parsed source AST without loading imports, checking types or executing code.
/// The format is for compiler inspection, not stable AST serialization.
pub fn debug_ast(source: &str) -> ParseResult<String> {
    let program = parse(source)?;
    let mut output = Dump::default();
    writeln!(output, "{program:#?}").map_err(|_| limit())?;
    Ok(output.text)
}

/// Own a bounded complete dump before the CLI is allowed to publish it.
#[derive(Default)]
struct Dump {
    text: String,
}

impl fmt::Write for Dump {
    /// Reject each append before allocation when its complete contents exceed the budget.
    fn write_str(&mut self, text: &str) -> fmt::Result {
        if !self
            .text
            .len()
            .checked_add(text.len())
            .is_some_and(|size| size <= MAX_DUMP)
        {
            return Err(fmt::Error);
        }
        self.text.push_str(text);
        Ok(())
    }
}

/// Report a stable bounded-output failure without exposing an incomplete dump.
fn limit() -> Diagnostic {
    Diagnostic::new(Span::default(), "syntax dump exceeds 16 MiB output limit")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_limit_rejects_before_appending_partial_text() {
        let mut output = Dump {
            text: "x".repeat(MAX_DUMP - 2),
        };
        output.write_str("é").unwrap();
        assert_eq!(output.text.len(), MAX_DUMP);
        assert!(output.write_str("x").is_err());
        assert_eq!(output.text.len(), MAX_DUMP);
        assert!(output.text.ends_with('é'));
    }
}
