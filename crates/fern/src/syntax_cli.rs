//! Source-only debug commands do not need modules, the runtime or backend tools.
use fern_compiler::{Diagnostic, Span, parse};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
};

/// Bound source input, produce a complete dump, then write it as uncolored artifact data.
pub(super) fn run(action: &str, path: &Path) -> Result<u8, String> {
    if path.as_os_str().len() > 4096 {
        return Err("syntax source path exceeds 4096 bytes".into());
    }
    let source = read_source(path)?;
    let output = if action == "lex" {
        parse::debug_tokens(&source)
    } else {
        parse::debug_ast(&source)
    }
    .map_err(|error| located(path, &source, error))?;
    std::io::stdout()
        .lock()
        .write_all(output.as_bytes())
        .map_err(|error| error.to_string())?;
    Ok(0)
}

/// Check the byte cap before UTF-8 decoding, including a codepoint cut at the read limit.
fn read_source(path: &Path) -> Result<String, String> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .and_then(|file| file.take(1024 * 1024 + 1).read_to_end(&mut bytes))
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if bytes.len() > 1024 * 1024 {
        return Err(located(
            path,
            "",
            Diagnostic::new(Span::default(), "source size exceeds 1 MiB prototype limit"),
        ));
    }
    String::from_utf8(bytes).map_err(|_| format!("{}: source is not valid UTF-8", path.display()))
}

/// Derive human line/column positions from a validated parser span without rescanning imports.
fn located(path: &Path, source: &str, error: Diagnostic) -> String {
    let prefix = source.get(..error.span.start).unwrap_or("");
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1;
    format!(
        "{}:{line}:{column}: error: {}",
        path.display(),
        error.message
    )
}
