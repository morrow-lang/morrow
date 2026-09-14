//! Executable examples use parsed documentation and ordinary checked pattern matching.
use crate::{Diagnostic, Span, Type, ast, ir, parse};

/// An independently executed fenced block, attributed to its owning documentation.
#[derive(Clone, Debug)]
pub struct Example {
    pub code: String,
    pub doc_span: Span,
    pub ordinal: usize,
}
/// Complete source overlay and the distinct test function selected after normal checking.
pub struct Prepared {
    pub source: String,
    pub name: String,
    pub expectations: usize,
    pub function_span: Span,
}

/// Extract at most 256 closed Fern fences from parser-owned literal documentation.
pub fn extract(source: &str) -> Result<Vec<Example>, Diagnostic> {
    let program = parse::parse(source)?;
    let mut result = Vec::new();
    for doc in program.module_doc.iter().chain(&program.docs) {
        let mut active: Option<(usize, bool, String)> = None;
        for line in doc.text.lines() {
            let trimmed = line.trim();
            let width = trimmed.bytes().take_while(|byte| *byte == b'`').count();
            if let Some((opening, fern, code)) = &mut active {
                if width >= *opening && trimmed[width..].trim().is_empty() {
                    if *fern && !code.trim().is_empty() {
                        if result.len() == 256 {
                            return Err(error(doc.span, "doc example limit exceeds 256"));
                        }
                        result.push(Example {
                            code: dedent(code),
                            doc_span: doc.span,
                            ordinal: result.len() + 1,
                        });
                    }
                    active = None;
                } else if *fern {
                    if code.len() + line.len() + 1 > 65_536 {
                        return Err(error(doc.span, "doc example exceeds 64 KiB"));
                    }
                    code.push_str(line);
                    code.push('\n');
                }
            } else if width >= 3 {
                active = Some((width, trimmed[width..].trim() == "fern", String::new()));
            }
        }
        if active.as_ref().is_some_and(|(_, fern, _)| *fern) {
            return Err(error(doc.span, "unterminated Fern documentation fence"));
        }
    }
    Ok(result)
}

/// Remove common Markdown indentation while preserving nested Fern suites.
fn dedent(code: &str) -> String {
    let common = code
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.bytes().take_while(|b| *b == b' ').count())
        .min()
        .unwrap_or(0);
    code.lines()
        .map(|line| {
            if line.trim().is_empty() {
                "\n".into()
            } else {
                format!("{}\n", &line[common..])
            }
        })
        .collect()
}

/// Build one source overlay without changing files or resolving names by string substitution.
/// Expectations attach to complete top-level expression statements, never string/comment contents.
pub fn prepare(source: &str, example: &Example) -> Result<Prepared, Diagnostic> {
    if example.code.len() > 65_536 {
        return Err(error(example.doc_span, "doc example exceeds 64 KiB"));
    }
    parse::parse(source)?;
    let name =
        fresh_name(source, &example.code).map_err(|e| generated_error(e, example.doc_span))?;
    let mut wrapper = format!("fn {name}() -> Int:\n");
    for line in example.code.lines() {
        wrapper.push_str("    ");
        wrapper.push_str(line);
        wrapper.push('\n');
    }
    wrapper.push_str("    0\n");
    let parsed = parse::parse(&wrapper).map_err(|e| generated_error(e, example.doc_span))?;
    let ast::ExprKind::Block(statements) = &parsed.functions[0].body.kind else {
        return Err(error(
            example.doc_span,
            "doc example must contain statements",
        ));
    };
    let edits =
        expectations(&wrapper, statements).map_err(|e| generated_error(e, example.doc_span))?;
    let count = edits.len();
    for (span, replacement) in edits.into_iter().rev() {
        wrapper.replace_range(span.start..span.end, &replacement);
    }
    let mut combined = source.to_owned();
    combined.push('\n');
    combined.push_str(&wrapper);
    if combined.len() > 1024 * 1024 {
        return Err(error(
            example.doc_span,
            "doc test source overlay exceeds 1 MiB",
        ));
    }
    let parsed = parse::parse(&combined).map_err(|e| generated_error(e, example.doc_span))?;
    let function_span = parsed
        .functions
        .iter()
        .find(|function| function.name == name)
        .ok_or_else(|| error(example.doc_span, "missing generated doc function"))?
        .span;
    Ok(Prepared {
        source: combined,
        name,
        expectations: count,
        function_span,
    })
}

/// Generate a name absent from every original source identifier, under a finite attempt bound.
fn fresh_name(source: &str, code: &str) -> Result<String, Diagnostic> {
    let mut names = std::collections::HashSet::new();
    for text in [source, code] {
        let index = parse::identifier_index(text)?;
        names.extend(index.identifiers.iter().map(|s| &text[s.start..s.end]));
    }

    for number in 0..4097 {
        let name = format!("fern_doc_example_{number}");
        if !names.contains(name.as_str()) {
            return Ok(name);
        }
    }
    Err(error(Span::default(), "cannot allocate doc test identity"))
}

/// Replace each annotated expression with a guarded ordinary match; evaluate its value once.
fn expectations(source: &str, statements: &[ast::Stmt]) -> Result<Vec<(Span, String)>, Diagnostic> {
    let mut edits = Vec::new();
    let mut expressions = statements
        .iter()
        .filter_map(|statement| match statement {
            ast::Stmt::Expr(expr) => Some(expr),
            _ => None,
        })
        .peekable();
    let mut previous = None;
    for comment in parse::identifier_index(source)?.comments {
        let text = &source[comment.start..comment.end];
        let Some(pattern) = text
            .strip_prefix('#')
            .and_then(|s| s.trim_start().strip_prefix("=>"))
        else {
            continue;
        };
        let pattern = clean_pattern(pattern.trim(), comment)?;
        while expressions
            .peek()
            .is_some_and(|expr| expr.span.end <= comment.start)
        {
            previous = expressions.next();
        }
        let expression = previous
            .filter(|expr| {
                source[expr.span.end..comment.start]
                    .bytes()
                    .all(|b| b == b' ' || b == b'\t')
            })
            .ok_or_else(|| {
                error(
                    comment,
                    "doc expectation requires a complete expression statement",
                )
            })?;
        let code = &source[expression.span.start..expression.span.end];
        let replacement =
            format!("match ({code}):\n        {pattern} if true -> ()\n        _ -> return 1");
        edits.push((expression.span, replacement));
    }
    Ok(edits)
}

/// Strip an explanatory line comment and validate the remaining expected Fern pattern syntactically.
fn clean_pattern(text: &str, span: Span) -> Result<&str, Diagnostic> {
    let comments = parse::identifier_index(text)?.comments;
    let text = text[..comments.first().map_or(text.len(), |s| s.start)].trim();
    if text.is_empty() {
        return Err(error(span, "doc expectation requires a pattern"));
    }
    let code =
        format!("fn expect(value: Int): match value:\n    {text} if true -> ()\n    _ -> ()\n");
    parse::parse(&code).map_err(|diagnostic| {
        error(
            span,
            &format!("invalid doc expectation: {}", diagnostic.message),
        )
    })?;
    Ok(text)
}

/// Select the checked test entry by name while preserving all ordinary calls by FunctionId.
/// Unit main's implicit final-value discard becomes an explicit Unit block before renaming.
pub fn select_entry(program: &mut ir::Program, name: &str) -> Result<(), Diagnostic> {
    let mut matches = program
        .functions
        .iter()
        .filter(|function| function.name == name);
    let valid = matches.next().is_some_and(|function| {
        function.params.is_empty()
            && function.captures.is_empty()
            && function.return_type == Type::Int
    });
    if !valid || matches.next().is_some() {
        return Err(error(
            Span::default(),
            "missing unique checked doc test entry",
        ));
    }
    rename_entry(program, name);
    Ok(())
}

/// Rename one verified entry while preserving original main calls by resolved function identity.
/// Callers must first verify that exactly one eligible selected function exists.
pub(crate) fn rename_entry(program: &mut ir::Program, name: &str) {
    if name == "main" {
        return;
    }
    for function in &mut program.functions {
        if function.name == "main" {
            function.name = "$doc.original.main".into();
            if function.return_type == Type::Unit
                && !matches!(function.body.ty, Type::Unit | Type::Never)
            {
                let unit = ir::Expr {
                    kind: ir::ExprKind::Unit,
                    ty: Type::Unit,
                    span: function.body.span,
                };
                let body = std::mem::replace(&mut function.body, unit.clone());
                function.body.kind =
                    ir::ExprKind::Block(vec![ir::Stmt::Expr(body), ir::Stmt::Expr(unit)]);
            }
        } else if function.name == name {
            function.name = "main".into();
        }
    }
}

/// Attach extraction and transformation failures to a meaningful bounded source span.
fn error(span: Span, message: &str) -> Diagnostic {
    Diagnostic::new(span, message)
}

/// Generated wrapper offsets are not offsets in the caller's original source file.
fn generated_error(diagnostic: Diagnostic, span: Span) -> Diagnostic {
    Diagnostic::new(span, diagnostic.message)
}
