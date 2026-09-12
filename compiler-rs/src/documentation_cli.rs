//! Literal-path documentation command; generation is independent of runtime/backend availability.
use fern_prototype::documentation::{self, Output};
use std::{ffi::OsString, fs, io::Read, path::PathBuf};
mod directory;
mod inferred;
mod opener;
struct Options {
    source: PathBuf,
    output: Option<PathBuf>,
    format: Output,
    inferred: bool,
    open: bool,
}

/// Parse the doc action, validate source, then print or atomically install the complete document.
pub(super) fn run(arguments: Vec<OsString>) -> Result<u8, String> {
    if arguments.len() == 2 && (arguments[1] == "--help" || arguments[1] == "-h") {
        use std::io::Write;
        std::io::stdout().lock().write_all(b"Usage: fern doc [source.fn|directory] [--html] [--inferred] [--open] [-o output]\nGenerate source documentation without executing code. Markdown is written to stdout by default. Directory HTML includes module navigation and local search. --inferred checks the current module graph and adds resolved signatures. --open implies HTML, retains -o output (default: fern-docs.html in the current directory), then best-effort launches the platform opener.\n")
            .map_err(|error| error.to_string())?;
        return Ok(0);
    }
    let options = options(arguments)?;
    let code = generate(&options)?;
    if options.open {
        if let Some(output) = &options.output {
            opener::open(output);
        }
    }
    Ok(code)
}

/// Complete generation and atomic publication precede any optional external launcher.
fn generate(options: &Options) -> Result<u8, String> {
    if options.inferred {
        return inferred::run(&options.source, options.output.as_deref(), options.format);
    }
    if options.source.is_dir() {
        return directory::run(&options.source, options.output.as_deref(), options.format);
    }
    let source = read_source(&options.source, &mut 0)?;
    let title = options
        .source
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let rendered = documentation::render(&source, &title, options.format).map_err(|error| {
        let prefix = source.get(..error.span.start).unwrap_or("");
        let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
        let column = prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1;
        format!(
            "{}:{line}:{column}: error: {}",
            options.source.display(),
            error.message
        )
    })?;
    if let Some(output) = &options.output {
        super::emit_file(&options.source, output, &rendered)?;
    } else {
        use std::io::Write;
        std::io::stdout()
            .lock()
            .write_all(rendered.as_bytes())
            .map_err(|error| error.to_string())?;
    }
    Ok(0)
}

/// Accept one source, one optional output and an explicit HTML switch in any argument order.
fn options(arguments: Vec<OsString>) -> Result<Options, String> {
    let mut source = None;
    let mut output = None;
    let mut html = false;
    let mut inferred = false;
    let mut open = false;
    let mut literal = false;
    let mut arguments = arguments.into_iter().skip(1);
    while let Some(argument) = arguments.next() {
        if !literal && argument == "--" {
            literal = true;
            continue;
        }
        if literal {
            if source.replace(PathBuf::from(argument)).is_some() {
                return Err("doc accepts one source file or directory".into());
            }
            continue;
        }
        if argument == "--inferred" {
            if inferred {
                return Err("--inferred specified more than once".into());
            }
            inferred = true;
        } else if argument == "--open" {
            if open {
                return Err("--open specified more than once".into());
            }
            open = true;
        } else if argument == "--html" {
            if html {
                return Err("--html specified more than once".into());
            }
            html = true;
        } else if argument == "-o" || argument == "--output" {
            if output.is_some() {
                return Err("output specified more than once".into());
            }
            output = Some(PathBuf::from(arguments.next().ok_or("-o requires a path")?));
        } else if argument.to_string_lossy().starts_with('-') {
            return Err(format!(
                "unknown doc option: {}",
                argument.to_string_lossy()
            ));
        } else if source.replace(PathBuf::from(argument)).is_some() {
            return Err("doc accepts one source file or directory".into());
        }
    }
    if open && output.is_none() {
        output = Some(PathBuf::from("fern-docs.html"));
    }
    Ok(Options {
        source: source.unwrap_or_else(|| PathBuf::from(".")),
        output,
        format: if html || open {
            Output::Html
        } else {
            Output::Markdown
        },
        inferred,
        open,
    })
}

/// Share bounded directory discovery with the explicit executable documentation-test command.
pub(super) fn sources(path: &std::path::Path) -> Result<Vec<PathBuf>, String> {
    if path.is_dir() {
        directory::discover(path)
    } else {
        Ok(vec![path.to_path_buf()])
    }
}

/// Check bounded raw byte lengths before decoding, including a partial UTF-8 sentinel byte.
fn read_source(path: &std::path::Path, bytes: &mut usize) -> Result<String, String> {
    let mut source = Vec::new();
    fs::File::open(path)
        .and_then(|file| file.take(1024 * 1024 + 1).read_to_end(&mut source))
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if source.len() > 1024 * 1024 {
        return Err(format!(
            "{}: documentation source exceeds 1 MiB",
            path.display()
        ));
    }
    *bytes = bytes
        .checked_add(source.len())
        .ok_or("documentation source size overflow")?;
    if *bytes > 8 * 1024 * 1024 {
        return Err("project documentation source exceeds 8 MiB".into());
    }
    String::from_utf8(source).map_err(|error| format!("{}: invalid UTF-8: {error}", path.display()))
}
