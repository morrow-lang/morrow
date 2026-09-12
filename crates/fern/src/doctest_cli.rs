//! Explicit native documentation-test command with bounded discovery and execution.
use super::{cli_controls::Controls, native};
use fern_compiler::{check, doctest, ir, lowering, modules, unit_test};
use std::{
    collections::HashMap,
    ffi::OsString,
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};
struct Options {
    path: PathBuf,
    timeout: Duration,
    doc_only: bool,
}

/// Run source-owned examples in independent native processes and report every failed example.
pub(super) fn run(arguments: Vec<OsString>, controls: Controls) -> Result<u8, String> {
    if arguments.len() == 2 && (arguments[1] == "--help" || arguments[1] == "-h") {
        println!(
            "Usage: fern test [--doc] [source.fn|directory] [--timeout seconds]\nExecute zero-argument test_ functions and fenced Fern documentation examples. --doc runs only documentation examples and # => pattern expectations.\nDefaults to the current directory and a 10-second timeout per example (1–60).\nExamples execute user code. Unit tests must return Unit or Result(Unit, E). Assertion libraries, coverage, benchmarks and watch mode are not yet supported."
        );
        return Ok(0);
    }
    let options = options(arguments)?;
    let files = super::documentation_cli::sources(&options.path)?;
    let mut totals = Totals {
        passed: 0,
        total: 0,
    };
    let mut bytes = 0;
    for file in files {
        let source = read_source(&file, &mut bytes)?;
        run_file(&file, &source, &options, &mut totals, controls)?;
    }
    let label = if options.doc_only {
        "doc tests"
    } else {
        "tests"
    };
    controls.information(&format!(
        "{label}: {}/{} passed",
        totals.passed, totals.total
    ));
    Ok(u8::from(totals.passed != totals.total))
}

struct Totals {
    passed: usize,
    total: usize,
}

/// Read one bounded source while charging aggregate discovery bytes before test preparation.
fn read_source(path: &Path, bytes: &mut usize) -> Result<String, String> {
    let mut source = String::new();
    fs::File::open(path)
        .map_err(|error| format!("{}: {error}", path.display()))?
        .take(1024 * 1024 + 1)
        .read_to_string(&mut source)
        .map_err(|error| error.to_string())?;
    *bytes += source.len();
    if source.len() > 1024 * 1024 || *bytes > 8 * 1024 * 1024 {
        return Err("test source exceeds 1 MiB per file or 8 MiB aggregate".into());
    }
    Ok(source)
}

/// Select source-owned unit groups and doc examples, retaining failures without skipping later tests.
fn run_file(
    path: &Path,
    source: &str,
    options: &Options,
    totals: &mut Totals,
    controls: Controls,
) -> Result<(), String> {
    let examples = doctest::extract(source)
        .map_err(|error| format!("{}: {}", path.display(), error.message))?;
    let units = if options.doc_only {
        Vec::new()
    } else {
        unit_test::discover(source)
            .map_err(|error| format!("{}: {}", path.display(), error.message))?
    };
    totals.total += units.len() + examples.len();
    if totals.total > 256 {
        return Err("combined unit/doc test limit exceeds256".into());
    }
    for case in units {
        let result = execute_unit(path, source, &case, options.timeout);
        record(
            path,
            source,
            case.span.start,
            &case.name,
            result,
            totals,
            controls,
        );
    }
    for example in examples {
        let result = execute(path, source, &example, options.timeout);
        record(
            path,
            source,
            example.doc_span.start,
            &format!("doc example {}", example.ordinal),
            result,
            totals,
            controls,
        );
    }
    Ok(())
}

/// Attribute each failure to its original source line and count only successful native executions.
fn record(
    path: &Path,
    source: &str,
    start: usize,
    label: &str,
    result: Result<(), String>,
    totals: &mut Totals,
    controls: Controls,
) {
    match result {
        Ok(()) => totals.passed += 1,
        Err(error) => {
            let line = source[..start].bytes().filter(|b| *b == b'\n').count() + 1;
            controls.error(&format!(
                "{}:{line}: {label} failed: {error}",
                path.display()
            ));
        }
    }
}

/// Resolve a declared test by its exact original source anchor; imports cannot become local tests.
fn execute_unit(
    path: &Path,
    source: &str,
    case: &unit_test::Case,
    timeout: Duration,
) -> Result<(), String> {
    let path = modules::source_identity(path).map_err(|error| error.message)?;
    let overlays = HashMap::from([(path.clone(), source.to_owned())]);
    let loaded = modules::load_with_sources(&path, &overlays).map_err(|error| error.message)?;
    let offset = loaded
        .sources()
        .find(|source| source.path == path)
        .ok_or("missing unit source identity")?
        .start;
    let selected = loaded
        .program
        .functions
        .iter()
        .find(|function| function.span.start == offset + case.span.start)
        .ok_or("missing resolved unit function")?;
    let program =
        unit_test::prepare(&loaded.program, &selected.name).map_err(|error| error.message)?;
    execute_program(&program, timeout)
}

/// Preserve ordinary module/private visibility through an overlay, then select only the test entry.
fn execute(
    path: &Path,
    source: &str,
    example: &doctest::Example,
    timeout: Duration,
) -> Result<(), String> {
    let prepared = doctest::prepare(source, example).map_err(|error| error.message)?;
    let path = modules::source_identity(path).map_err(|error| error.message)?;
    let overlays = HashMap::from([(path.clone(), prepared.source)]);
    let loaded = modules::load_with_sources(&path, &overlays).map_err(|error| error.message)?;
    let offset = loaded
        .sources()
        .find(|source| source.path == path)
        .ok_or("missing doc source identity")?
        .start;
    let selected = loaded
        .program
        .functions
        .iter()
        .find(|function| function.span.start == offset + prepared.function_span.start)
        .ok_or("missing resolved doc function")?
        .name
        .clone();
    let mut program = check::check_library(&loaded.program).map_err(|error| error.message)?;
    doctest::select_entry(&mut program, &selected).map_err(|error| error.message)?;
    execute_program(&program, timeout)
}

/// Compile and execute one checked selected entry with independent bounded output and lifetime.
fn execute_program(program: &ir::Program, timeout: Duration) -> Result<(), String> {
    let machine = lowering::lower_test(program).map_err(|error| error.message)?;
    let object = fern_compiler::cranelift::emit_object(&machine)?;
    let workspace =
        native::Workspace::new(&std::env::temp_dir()).map_err(|error| error.to_string())?;
    let executable = native::compile_object(&object, &workspace)?;
    let result = native::capture::run(&executable, timeout)?;
    if result.status.success() {
        return Ok(());
    }
    Err(format!(
        "expected pattern mismatch or runtime failure ({})\n{}{}",
        result.status,
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    ))
}

/// Parse one optional source path and a bounded whole-second timeout; reject unsupported modes.
fn options(arguments: Vec<OsString>) -> Result<Options, String> {
    let mut path = None;
    let mut timeout = None;
    let mut doc = false;
    let mut literal = false;
    let mut arguments = arguments.into_iter().skip(1);
    while let Some(argument) = arguments.next() {
        if !literal && argument == "--" {
            literal = true;
            continue;
        }
        if literal {
            if path.replace(PathBuf::from(argument)).is_some() {
                return Err("test accepts one source file or directory".into());
            }
            continue;
        }
        if argument == "--doc" {
            if doc {
                return Err("--doc specified more than once".into());
            }
            doc = true;
        } else if argument == "--timeout" {
            if timeout.is_some() {
                return Err("--timeout specified more than once".into());
            }
            let seconds = arguments
                .next()
                .and_then(|s| s.to_str().and_then(|s| s.parse::<u64>().ok()))
                .ok_or("--timeout requires seconds from 1 to 60")?;
            if !(1..=60).contains(&seconds) {
                return Err("--timeout requires seconds from 1 to 60".into());
            }
            timeout = Some(Duration::from_secs(seconds));
        } else if argument.to_string_lossy().starts_with('-') {
            return Err(format!(
                "unknown test option: {}",
                argument.to_string_lossy()
            ));
        } else if path.replace(PathBuf::from(argument)).is_some() {
            return Err("test accepts one source file or directory".into());
        }
    }
    Ok(Options {
        path: path.unwrap_or_else(|| PathBuf::from(".")),
        timeout: timeout.unwrap_or(Duration::from_secs(10)),
        doc_only: doc,
    })
}
