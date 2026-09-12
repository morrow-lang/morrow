//! Bounded source discovery and atomic publication for directory documentation.
use fern_compiler::documentation::{self, Output, SourceDocument};
use std::path::{Path, PathBuf};

/// Parse every discovered source before protecting all input identities and writing output.
pub(super) fn run(root: &Path, output: Option<&Path>, format: Output) -> Result<u8, String> {
    let files = discover(root)?;
    let mut sources = Vec::new();
    let mut bytes = 0;
    for path in &files {
        let source = super::read_source(path, &mut bytes)?;
        sources.push(source);
    }
    let names: Vec<_> = files
        .iter()
        .map(|path| {
            path.strip_prefix(root)
                .unwrap_or(path)
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        })
        .collect();
    let documents: Vec<_> = names
        .iter()
        .zip(&sources)
        .map(|(path, source)| SourceDocument { path, source })
        .collect();
    let title = root.canonicalize().map_err(|error| error.to_string())?;
    let title = title.file_name().unwrap_or_default().to_string_lossy();
    let text =
        documentation::render_project(&documents, &title, format).map_err(|error| error.message)?;
    if let Some(output) = output {
        for file in &files {
            super::super::output_destination(file, output)?;
        }
        super::super::emit_file(&files[0], output, &text)?;
    } else {
        use std::io::Write;
        std::io::stdout()
            .lock()
            .write_all(text.as_bytes())
            .map_err(|error| error.to_string())?;
    }
    Ok(0)
}

/// Discover documentation sources using the shared bounded traversal.
pub(super) fn discover(root: &Path) -> Result<Vec<PathBuf>, String> {
    super::super::source_directory::discover(root, "documentation")
}
