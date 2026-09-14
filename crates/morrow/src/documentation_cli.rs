//! Literal-path documentation command; generation is independent of runtime/backend availability.
use morrow_compiler::documentation::{self, Output};
use std::{ffi::OsString, fs, io::Read, path::PathBuf};
mod directory;
mod inferred;
mod opener;
mod site;
struct Options {
    source: PathBuf,
    explicit_source: bool,
    output: Option<PathBuf>,
    format: Output,
    inferred: bool,
    open: bool,
    site: Option<PathBuf>,
    title: Option<String>,
    version: Option<String>,
    extras: Vec<PathBuf>,
    links: Vec<(String, String)>,
}

const HELP: &str = "Usage: morrow doc [source.fn|directory] [--html] [--inferred] [--open] [-o output]\n       morrow doc [directory] --site <directory> [--title text] [--version text] [--extras path]... [--link label=url]... [--inferred] [--open]\nGenerate source documentation without executing code. Markdown is written to stdout by default. Directory HTML includes module navigation and browser Find guidance. --inferred checks the current module graph and adds resolved signatures. --open implies HTML, retains -o output (default: morrow-docs.html in the current directory), then best-effort launches the platform opener.\n--site publishes a multi-page HTML site (one page per module and Markdown guide, sidebar navigation, local search) into a directory that is created or, when it is a previously generated site, replaced atomically. --extras adds Markdown guides from a file or the direct entries of a directory; the first README.md becomes the landing page. --link adds sidebar links. Without a source operand, --site documents guides alone.\n";

/// Parse the doc action, validate source, then print or atomically install the complete document.
pub(super) fn run(arguments: Vec<OsString>) -> Result<u8, String> {
    if arguments.len() == 2 && (arguments[1] == "--help" || arguments[1] == "-h") {
        use std::io::Write;
        std::io::stdout()
            .lock()
            .write_all(HELP.as_bytes())
            .map_err(|error| error.to_string())?;
        return Ok(0);
    }
    let options = options(arguments)?;
    if let Some(output) = &options.site {
        let request = site::Request {
            source: options.explicit_source.then(|| options.source.clone()),
            output: output.clone(),
            title: options.title.clone(),
            version: options.version.clone(),
            extras: options.extras.clone(),
            links: options.links.clone(),
            inferred: options.inferred,
        };
        let published = site::run(&request)?;
        if options.open {
            opener::open(&published.join("index.html"));
        }
        return Ok(0);
    }
    let code = generate(&options)?;
    if options.open
        && let Some(output) = &options.output
    {
        opener::open(output);
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
    let mut site = None;
    let mut title = None;
    let mut version = None;
    let mut extras = Vec::new();
    let mut links = Vec::new();
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
        if argument == "--site" {
            if site.is_some() {
                return Err("--site specified more than once".into());
            }
            site = Some(PathBuf::from(
                arguments.next().ok_or("--site requires a directory")?,
            ));
        } else if argument == "--title" {
            if title.is_some() {
                return Err("--title specified more than once".into());
            }
            title = Some(text_value(arguments.next(), "--title", 4096)?);
        } else if argument == "--version" {
            if version.is_some() {
                return Err("--version specified more than once".into());
            }
            version = Some(text_value(arguments.next(), "--version", 256)?);
        } else if argument == "--extras" {
            if extras.len() == 256 {
                return Err("--extras accepts at most 256 paths".into());
            }
            extras.push(PathBuf::from(
                arguments
                    .next()
                    .ok_or("--extras requires a Markdown file or directory")?,
            ));
        } else if argument == "--link" {
            if links.len() == 64 {
                return Err("--link accepts at most 64 links".into());
            }
            let value = text_value(arguments.next(), "--link", 4096)?;
            let (label, url) = value
                .split_once('=')
                .filter(|(label, url)| !label.is_empty() && !url.is_empty())
                .ok_or("--link requires label=url")?;
            if !morrow_compiler::documentation::markdown::safe_destination(url) {
                return Err(format!(
                    "--link {label}: only safe http, https, mailto or relative destinations are accepted"
                ));
            }
            links.push((label.to_string(), url.to_string()));
        } else if argument == "--inferred" {
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
    if site.is_some() {
        if output.is_some() {
            return Err("-o cannot accompany --site; the site directory is the output".into());
        }
        if html {
            return Err("--html cannot accompany --site; sites are always HTML".into());
        }
    } else if title.is_some() || version.is_some() || !extras.is_empty() || !links.is_empty() {
        return Err("--title, --version, --extras and --link require --site".into());
    }
    if open && output.is_none() && site.is_none() {
        output = Some(PathBuf::from("morrow-docs.html"));
    }
    let explicit_source = source.is_some() || extras.is_empty();
    Ok(Options {
        source: source.unwrap_or_else(|| PathBuf::from(".")),
        explicit_source,
        output,
        format: if html || open {
            Output::Html
        } else {
            Output::Markdown
        },
        inferred,
        open,
        site,
        title,
        version,
        extras,
        links,
    })
}

/// Accept one UTF-8 option value within a byte bound.
fn text_value(value: Option<OsString>, option: &str, maximum: usize) -> Result<String, String> {
    let value = value.ok_or_else(|| format!("{option} requires a value"))?;
    let text = value
        .into_string()
        .map_err(|_| format!("{option} must be UTF-8"))?;
    if text.is_empty() || text.len() > maximum {
        return Err(format!("{option} must be 1–{maximum} bytes"));
    }
    Ok(text)
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
