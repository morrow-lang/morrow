//! Deterministic multi-module documentation with a static accessible module index.
use super::*;

/// One module's display path and original UTF-8 source, borrowed for rendering.
#[derive(Clone, Copy)]
pub struct SourceDocument<'a> {
    pub path: &'a str,
    pub source: &'a str,
}

/// One original module paired with finalized source-local function schemes.
pub struct InferredDocument<'a> {
    pub path: &'a str,
    pub source: &'a str,
    pub schemes: &'a [crate::check::editor::FunctionInfo],
}

/// Render checked module documentation using the same navigation, escaping and output bounds.
pub fn render_inferred_project(
    documents: &[InferredDocument<'_>],
    title: &str,
    output: Output,
) -> Result<String, Diagnostic> {
    if documents.len() > 256 {
        return Err(limit("project documentation file limit exceeded"));
    }
    let sources: Vec<_> = documents
        .iter()
        .map(|d| SourceDocument {
            path: d.path,
            source: d.source,
        })
        .collect();
    let schemes: std::collections::HashMap<_, _> =
        documents.iter().map(|d| (d.path, d.schemes)).collect();
    render(&sources, title, output, Some(&schemes))
}

/// Render 1–256 unique modules without loading imports or executing examples.
/// Aggregate source is limited to 8 MiB and generated output to 16 MiB.
pub fn render_project(
    documents: &[SourceDocument<'_>],
    title: &str,
    output: Output,
) -> Result<String, Diagnostic> {
    render(documents, title, output, None)
}

/// Share source rendering while optionally joining each module's checked source-local anchors.
fn render(
    documents: &[SourceDocument<'_>],
    title: &str,
    output: Output,
    schemes: Option<&std::collections::HashMap<&str, &[crate::check::editor::FunctionInfo]>>,
) -> Result<String, Diagnostic> {
    validate(documents, title)?;
    let mut documents = documents.to_vec();
    documents.sort_by_key(|document| document.path);
    let mut writer = Writer {
        text: String::new(),
        maximum: 16 * 1024 * 1024,
        declaration_level: 3,
    };
    writer.start(title, output)?;
    if output == Output::Html {
        navigation(&mut writer, &documents)?;
    }
    let mut count = 0;
    for (index, document) in documents.iter().enumerate() {
        let parsed =
            parse::parse(document.source).map_err(|error| source_error(document, error))?;
        let mut declarations = declarations(document.source, &parsed)
            .map_err(|error| source_error(document, error))?;
        if let Some(schemes) = schemes {
            let metadata = schemes
                .get(document.path)
                .ok_or_else(|| limit("missing checked module metadata"))?;
            inferred::attach(&parsed, &mut declarations, metadata)
                .map_err(|error| source_error(document, error))?;
        }
        if declarations.len() > 4096_usize.saturating_sub(count) {
            return Err(limit("project documentation declaration limit exceeded"));
        }
        module_start(&mut writer, index, document.path, output)?;
        if let Some(doc) = &parsed.module_doc {
            writer.module_doc(&doc.text, output)?;
        }
        for declaration in &declarations {
            writer.declaration(count, declaration, declaration.doc, output)?;
            count += 1;
        }
        if output == Output::Html {
            writer.push("</article>\n")?;
        }
    }
    if output == Output::Html {
        writer.push(include_str!("project.html"))?;
        writer.push("</main></body></html>\n")?;
    }
    Ok(writer.text)
}

/// Check all retained inputs before cloning paths or parsing any module.
fn validate(documents: &[SourceDocument<'_>], title: &str) -> Result<(), Diagnostic> {
    if documents.is_empty() || documents.len() > 256 || title.len() > 4096 {
        return Err(limit(
            "project documentation requires 1–256 files and a title within 4096 bytes",
        ));
    }
    let mut names = std::collections::HashSet::new();
    let mut bytes = 0;
    for document in documents {
        if document.path.is_empty() || document.path.len() > 4096 || !names.insert(document.path) {
            return Err(limit(
                "project documentation paths must be unique and within 4096 bytes",
            ));
        }
        bytes += document.source.len();
        if bytes > 8 * 1024 * 1024 {
            return Err(limit("project documentation source exceeds 8 MiB"));
        }
    }
    Ok(())
}

/// Retain the original module's line and column in a multi-source diagnostic.
fn source_error(document: &SourceDocument<'_>, error: Diagnostic) -> Diagnostic {
    let prefix = document.source.get(..error.span.start).unwrap_or("");
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1;
    Diagnostic::new(
        error.span,
        format!("{}:{line}:{column}: {}", document.path, error.message),
    )
}

/// Render accessible module links using generated numeric IDs, never source paths as markup.
fn navigation(writer: &mut Writer, documents: &[SourceDocument<'_>]) -> Result<(), Diagnostic> {
    writer.push("<nav aria-label=\"Modules\"><h2>Modules</h2><ul>\n")?;
    for (index, document) in documents.iter().enumerate() {
        writer.push(&format!("<li><a href=\"#module-{index}\">"))?;
        writer.html(document.path)?;
        writer.push("</a></li>\n")?;
    }
    writer.push("</ul></nav><p>Use your browser’s Find command (<kbd>Ctrl+F</kbd> or <kbd>Command+F</kbd> on macOS) to find a module, declaration, signature or documentation. All modules and declarations appear below.</p>\n")
}

/// Mark each source module with its own anchor; declaration IDs remain globally unique.
fn module_start(
    writer: &mut Writer,
    index: usize,
    path: &str,
    output: Output,
) -> Result<(), Diagnostic> {
    if output == Output::Html {
        writer.push(&format!(
            "<article class=\"module\" id=\"module-{index}\"><h2 class=\"module-path\">"
        ))?;
        writer.html(path)?;
        writer.push("</h2>\n")
    } else {
        writer.push("## Module: ")?;
        writer.heading(path)?;
        writer.push("\n\n")
    }
}
