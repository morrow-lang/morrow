//! Checked documentation loads current module snapshots and protects every source identity.
use morrow_compiler::{
    check::editor::{self, FunctionInfo},
    documentation::{self, InferredDocument, Output},
    modules,
};
use std::{
    collections::HashMap,
    io::Write,
    path::{Path, PathBuf},
};

/// One documented module with its checked source-local signatures.
pub(super) struct Input {
    pub(super) path: PathBuf,
    pub(super) name: String,
    pub(super) source: String,
    pub(super) schemes: Vec<FunctionInfo>,
}
struct GraphBudget {
    instances: usize,
    bytes: usize,
}

/// Read and check every documented module once; snapshots retain each loaded source identity.
pub(super) fn checked_inputs(
    path: &Path,
) -> Result<(Vec<Input>, HashMap<PathBuf, String>), String> {
    let mut inputs = inputs(path)?;
    let mut snapshots: HashMap<_, _> = inputs
        .iter()
        .map(|input| (input.path.clone(), input.source.clone()))
        .collect();
    let mut budget = GraphBudget {
        instances: 0,
        bytes: snapshots.values().map(String::len).sum(),
    };
    for input in &mut inputs {
        check(input, &mut snapshots, &mut budget)?;
    }
    Ok((inputs, snapshots))
}

/// Render all checked modules before printing or atomically replacing any destination.
pub(super) fn run(path: &Path, output: Option<&Path>, format: Output) -> Result<u8, String> {
    let (inputs, snapshots) = checked_inputs(path)?;
    let title = path.canonicalize().map_err(|e| e.to_string())?;
    let title = title.file_name().unwrap_or_default().to_string_lossy();
    let text = if path.is_dir() {
        let documents: Vec<_> = inputs
            .iter()
            .map(|i| InferredDocument {
                path: &i.name,
                source: &i.source,
                schemes: &i.schemes,
            })
            .collect();
        documentation::render_inferred_project(&documents, &title, format)
    } else {
        documentation::render_with_schemes(
            &inputs[0].source,
            &title,
            format,
            Some(&inputs[0].schemes),
        )
    }
    .map_err(|error| error.message)?;
    if let Some(output) = output {
        for source in snapshots.keys() {
            super::super::output_destination(source, output)?;
        }
        super::super::emit_file(&inputs[0].path, output, &text)?;
    } else {
        std::io::stdout()
            .lock()
            .write_all(text.as_bytes())
            .map_err(|error| error.to_string())?;
    }
    Ok(0)
}

/// Capture all documented files before checking imports, retaining deterministic display paths.
fn inputs(root: &Path) -> Result<Vec<Input>, String> {
    let files = super::sources(root)?;
    let mut inputs = Vec::new();
    let mut bytes = 0;
    for path in files {
        let source = super::read_source(&path, &mut bytes)?;
        let name = if root.is_dir() {
            path.strip_prefix(root)
                .unwrap_or(&path)
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        } else {
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        };
        let path = modules::source_identity(&path).map_err(|e| e.message)?;
        inputs.push(Input {
            path,
            name,
            source,
            schemes: Vec::new(),
        });
    }
    Ok(inputs)
}

/// Check each graph once, cache imported snapshots and relocate only the documented source's facts.
fn check(
    input: &mut Input,
    snapshots: &mut HashMap<PathBuf, String>,
    budget: &mut GraphBudget,
) -> Result<(), String> {
    let loaded = modules::load_documentation_sources(&input.path, snapshots)
        .map_err(|e| format!("{}: {}", input.path.display(), e.message))?;
    let source = loaded
        .sources()
        .find(|s| s.path == input.path)
        .ok_or("missing documented source identity")?;
    let start = source.start;
    let end = start + source.text.len();
    for source in loaded.sources() {
        budget.instances += 1;
        budget.bytes += source.text.len();
        if budget.instances > 1024 || budget.bytes > 16 * 1024 * 1024 {
            return Err("inferred documentation graph budget exceeded".into());
        }
        if !snapshots.contains_key(source.path) {
            budget.bytes += source.text.len();
            if snapshots.len() == 1024 || budget.bytes > 16 * 1024 * 1024 {
                return Err("inferred documentation snapshot budget exceeded".into());
            }
            snapshots.insert(source.path.to_path_buf(), source.text.into());
        }
    }
    let schemes = editor::function_schemes(&loaded.program).map_err(|error| {
        let source = loaded
            .sources()
            .find(|s| s.start <= error.span.start && error.span.end <= s.start + s.text.len());
        let (path, line) = source
            .map(|s| {
                (
                    s.path,
                    1 + s.text[..error.span.start - s.start]
                        .bytes()
                        .filter(|b| *b == b'\n')
                        .count(),
                )
            })
            .unwrap_or((&input.path, 1));
        format!("{}:{line}: error: {}", path.display(), error.message)
    })?;
    input.schemes = schemes
        .into_iter()
        .filter(|info| start <= info.origin.start && info.origin.end <= end)
        .map(|mut info| {
            info.origin.start -= start;
            info.origin.end -= start;
            info
        })
        .collect();
    Ok(())
}
