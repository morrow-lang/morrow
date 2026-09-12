//! Bounded documentation from parsed source declarations, without executing user code.
use crate::{Diagnostic, Span, ast, parse};
mod inferred;
mod project;
pub use inferred::{render_inferred, render_with_schemes};
pub use project::{InferredDocument, SourceDocument, render_inferred_project, render_project};
const MAX_OUTPUT: usize = 8 * 1024 * 1024;
const MAX_DECLARATIONS: usize = 4096;

/// Documentation output is plain Markdown or standalone HTML with escaped literal text.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Output {
    Markdown,
    Html,
}
struct Declaration<'a> {
    name: &'a str,
    span: Span,
    headers: Vec<String>,
    doc: &'a str,
    checked: Option<String>,
}
struct Writer {
    text: String,
    maximum: usize,
    declaration_level: u8,
}

/// Parse one bounded source and render declarations in source order.
/// No main or typechecked executable is required; malformed syntax and limits are errors.
pub fn render(source: &str, title: &str, output: Output) -> Result<String, Diagnostic> {
    if title.len() > 4096 {
        return Err(limit("documentation title exceeds 4096 bytes"));
    }
    render_with_schemes(source, title, output, None)
}

/// Render parsed declarations after optional checked signatures have been attached.
fn render_declarations(
    title: &str,
    output: Output,
    declarations: &[Declaration<'_>],
) -> Result<String, Diagnostic> {
    let mut writer = Writer {
        text: String::new(),
        maximum: MAX_OUTPUT,
        declaration_level: 2,
    };
    writer.start(title, output)?;
    for (index, declaration) in declarations.iter().enumerate() {
        writer.declaration(index, declaration, declaration.doc, output)?;
    }
    if output == Output::Html {
        writer.push("</main></body></html>\n")?;
    }
    Ok(writer.text)
}

/// Gather source headers by parser group identity, including complete type declarations.
fn declarations<'a>(
    source: &str,
    program: &'a ast::Program,
) -> Result<Vec<Declaration<'a>>, Diagnostic> {
    if program.functions.len()
        + program.types.len()
        + program.aliases.len()
        + program.newtypes.len()
        > MAX_DECLARATIONS
    {
        return Err(limit("documentation declaration limit exceeded"));
    }
    let mut declarations: Vec<Declaration<'a>> = Vec::new();
    for function in &program.functions {
        let header = source
            .get(function.span.start..function.body.span.start)
            .ok_or_else(|| limit("invalid source header span"))?
            .trim_end();
        let header = format!("{}{header}", if function.public { "pub " } else { "" });
        if let Some(previous) = declarations
            .last_mut()
            .filter(|d| d.span.start == function.group_start)
        {
            previous.headers.push(header);
        } else {
            declarations.push(Declaration {
                name: &function.name,
                span: function.span,
                headers: vec![header],
                doc: "",
                checked: None,
            });
        }
    }
    for (name, span, public) in type_declarations(program) {
        let header = source
            .get(span.start..span.end)
            .ok_or_else(|| limit("invalid type declaration span"))?
            .trim_end();
        declarations.push(Declaration {
            name,
            span,
            headers: vec![format!("{}{header}", if public { "pub " } else { "" })],
            doc: "",
            checked: None,
        });
    }
    declarations.sort_by_key(|declaration| declaration.span.start);
    for doc in &program.docs {
        let index = declarations.partition_point(|item| item.span.start < doc.span.end);
        if let Some(item) = declarations
            .get_mut(index)
            .filter(|item| item.name == doc.target)
        {
            item.doc = &doc.text;
        }
    }
    Ok(declarations)
}

/// Preserve source identities for all nominal, transparent and distinct type declarations.
fn type_declarations(program: &ast::Program) -> impl Iterator<Item = (&String, Span, bool)> {
    program
        .types
        .iter()
        .map(|decl| (&decl.name, decl.span, decl.public))
        .chain(
            program
                .aliases
                .iter()
                .map(|decl| (&decl.name, decl.span, decl.public)),
        )
        .chain(
            program
                .newtypes
                .iter()
                .map(|decl| (&decl.name, decl.span, decl.public)),
        )
}

impl Writer {
    /// Append only after checking aggregate UTF-8 output size; never return partial output.
    fn push(&mut self, text: &str) -> Result<(), Diagnostic> {
        if text.len() > self.maximum.saturating_sub(self.text.len()) {
            return Err(limit("documentation output exceeds its byte limit"));
        }
        self.text.push_str(text);
        Ok(())
    }
    /// Encode each HTML metacharacter as literal text, including untrusted doc contents.
    fn html(&mut self, text: &str) -> Result<(), Diagnostic> {
        for character in text.chars() {
            match character {
                '&' => self.push("&amp;")?,
                '<' => self.push("&lt;")?,
                '>' => self.push("&gt;")?,
                '"' => self.push("&quot;")?,
                '\'' => self.push("&#39;")?,
                _ => self.push(character.encode_utf8(&mut [0; 4]))?,
            }
        }
        Ok(())
    }
    /// Keep titles and declaration names on one Markdown heading line and escape markup.
    fn heading(&mut self, text: &str) -> Result<(), Diagnostic> {
        for character in text.chars() {
            if character.is_control() {
                self.push(" ")?;
                continue;
            }
            if "\\`*_{}[]<>()#+-.!|".contains(character) {
                self.push("\\")?;
            }
            self.push(character.encode_utf8(&mut [0; 4]))?;
        }
        Ok(())
    }
    /// Start a local standalone document with no scripts, network assets or executable docs.
    fn start(&mut self, title: &str, output: Output) -> Result<(), Diagnostic> {
        if output == Output::Markdown {
            self.push("# ")?;
            self.heading(title)?;
            return self.push("\n\n");
        }
        self.push("<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>")?;
        self.html(title)?;
        self.push("</title><style>body{margin:0;background:#f7f8f5;color:#17291f;font:17px/1.6 system-ui,sans-serif}main{max-width:960px;margin:auto;padding:3rem 1.5rem}section{margin:2.5rem 0;border-top:1px solid #ced8d0}h1,h2{line-height:1.2}pre{white-space:pre-wrap;overflow-wrap:anywhere;background:#edf1ec;padding:1rem;border-radius:.5rem}code{font:15px/1.6 ui-monospace,monospace}.doc{background:none;padding:0;font:inherit}</style></head><body><main><h1>")?;
        self.html(title)?;
        self.push("</h1>\n")
    }
    /// Emit each clause group once, with separate original headers and its owned doc text.
    fn declaration(
        &mut self,
        index: usize,
        item: &Declaration<'_>,
        doc: &str,
        output: Output,
    ) -> Result<(), Diagnostic> {
        if output == Output::Html {
            let level = self.declaration_level;
            self.push(&format!("<section id=\"declaration-{index}\"><h{level}>"))?;
            self.html(item.name)?;
            self.push(&format!("</h{level}>\n"))?;
            for header in &item.headers {
                self.push("<pre><code>")?;
                self.html(header)?;
                self.push("</code></pre>\n")?;
            }
            if let Some(checked) = &item.checked {
                self.push("<p class=\"checked\">Checked signature</p><pre><code>")?;
                self.html(checked)?;
                self.push("</code></pre>\n")?;
            }
            if !doc.is_empty() {
                self.push("<pre class=\"doc\">")?;
                self.html(doc)?;
                self.push("</pre>\n")?;
            }
            return self.push("</section>\n");
        }
        self.push(&format!("{} ", "#".repeat(self.declaration_level.into())))?;
        self.heading(item.name)?;
        self.push("\n\n")?;
        for header in &item.headers {
            self.code(header)?;
        }
        if let Some(checked) = &item.checked {
            self.push("Checked signature:\n\n")?;
            self.code(checked)?;
        }
        if !doc.is_empty() {
            self.push(doc)?;
            self.push("\n\n")?;
        }
        Ok(())
    }
    /// Choose a fence longer than every backtick run in source pattern/header literals.
    fn code(&mut self, text: &str) -> Result<(), Diagnostic> {
        let mut longest = 0;
        let mut run = 0;
        for byte in text.bytes() {
            run = if byte == b'`' { run + 1 } else { 0 };
            longest = longest.max(run);
        }
        let fence = "`".repeat((longest + 1).max(3));
        self.push(&fence)?;
        self.push("fern\n")?;
        self.push(text)?;
        self.push("\n")?;
        self.push(&fence)?;
        self.push("\n\n")
    }
}

/// Report documentation resource/structure errors with a valid source diagnostic shape.
fn limit(message: &str) -> Diagnostic {
    Diagnostic::new(Span::default(), message)
}
