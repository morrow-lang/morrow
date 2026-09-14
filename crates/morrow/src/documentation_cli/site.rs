//! Multi-page site generation with private staging and one atomic directory swap.
//!
//! The destination is only ever replaced when it does not exist or is a previously generated
//! site; inputs, their ancestors, symbolic links and unrelated directories are refused before
//! any page is written.
use morrow_compiler::{
    check::editor::FunctionInfo,
    documentation::{
        SourceDocument,
        site::{self, Extra, Link, Page},
    },
};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

/// Site inputs parsed from the command line; paths remain literal.
pub(super) struct Request {
    pub(super) source: Option<PathBuf>,
    pub(super) output: PathBuf,
    pub(super) title: Option<String>,
    pub(super) version: Option<String>,
    pub(super) extras: Vec<PathBuf>,
    pub(super) links: Vec<(String, String)>,
    pub(super) inferred: bool,
}

struct Guide {
    path: String,
    source: String,
}

/// Generate every page in memory, then publish the complete site through one directory swap.
pub(super) fn run(request: &Request) -> Result<PathBuf, String> {
    let destination = destination(&request.output)?;
    let (files, sources, schemes) = modules(request)?;
    let guides = guides(&request.extras)?;
    let mut inputs: Vec<PathBuf> = files.clone();
    inputs.extend(guides.iter().map(|(path, _)| path.clone()));
    protect(&destination, &inputs)?;
    let names: Vec<String> = files
        .iter()
        .map(|path| display_name(request.source.as_deref(), path))
        .collect();
    let documents: Vec<SourceDocument<'_>> = names
        .iter()
        .zip(&sources)
        .map(|(path, source)| SourceDocument { path, source })
        .collect();
    let scheme_map: Option<HashMap<&str, &[FunctionInfo]>> = schemes.as_ref().map(|schemes| {
        names
            .iter()
            .zip(schemes)
            .map(|(name, info)| (name.as_str(), info.as_slice()))
            .collect()
    });
    let extras: Vec<Extra<'_>> = guides
        .iter()
        .map(|(_, guide)| Extra {
            path: &guide.path,
            source: &guide.source,
        })
        .collect();
    let links: Vec<Link<'_>> = request
        .links
        .iter()
        .map(|(label, url)| Link { label, url })
        .collect();
    let title = match &request.title {
        Some(title) => title.clone(),
        None => default_title(request.source.as_deref(), &destination)?,
    };
    let pages = site::render_site(&site::Site {
        title: &title,
        version: request.version.as_deref(),
        modules: &documents,
        schemes: scheme_map.as_ref(),
        extras: &extras,
        links: &links,
    })
    .map_err(|error| error.message)?;
    publish(&destination, &pages)?;
    Ok(destination)
}

/// Read documented modules, optionally checking each module graph for inferred signatures.
type Modules = (Vec<PathBuf>, Vec<String>, Option<Vec<Vec<FunctionInfo>>>);
fn modules(request: &Request) -> Result<Modules, String> {
    let Some(source) = &request.source else {
        return Ok((Vec::new(), Vec::new(), None));
    };
    if request.inferred {
        let (inputs, _) = super::inferred::checked_inputs(source)?;
        let mut files = Vec::new();
        let mut sources = Vec::new();
        let mut schemes = Vec::new();
        for input in inputs {
            files.push(input.path);
            sources.push(input.source);
            schemes.push(input.schemes);
        }
        return Ok((files, sources, Some(schemes)));
    }
    let files = super::sources(source)?;
    let mut sources = Vec::new();
    let mut bytes = 0;
    for path in &files {
        sources.push(super::read_source(path, &mut bytes)?);
    }
    Ok((files, sources, None))
}

/// Module display paths are relative to the documented root, matching directory documentation.
fn display_name(root: Option<&Path>, path: &Path) -> String {
    let relative = root
        .filter(|root| root.is_dir())
        .and_then(|root| {
            let canonical_root = root.canonicalize().ok()?;
            path.strip_prefix(root)
                .or_else(|_| path.strip_prefix(&canonical_root))
                .ok()
                .map(Path::to_path_buf)
        })
        .unwrap_or_else(|| PathBuf::from(path.file_name().unwrap_or_default()));
    relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Collect Markdown guides from literal files or the direct entries of directories.
fn guides(extras: &[PathBuf]) -> Result<Vec<(PathBuf, Guide)>, String> {
    let mut guides = Vec::new();
    let mut bytes = 0usize;
    for extra in extras {
        let mut files = Vec::new();
        if extra.is_dir() {
            for entry in
                fs::read_dir(extra).map_err(|error| format!("{}: {error}", extra.display()))?
            {
                let entry = entry.map_err(|error| error.to_string())?;
                let kind = entry.file_type().map_err(|error| error.to_string())?;
                let path = entry.path();
                if kind.is_file()
                    && path.extension().is_some_and(|extension| extension == "md")
                    && !entry.file_name().to_string_lossy().starts_with('.')
                {
                    files.push(path);
                }
            }
            files.sort();
        } else {
            files.push(extra.clone());
        }
        for path in files {
            if guides.len() == 256 {
                return Err("site documentation accepts at most 256 guides".into());
            }
            let metadata = fs::symlink_metadata(&path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            if !metadata.is_file() {
                return Err(format!(
                    "{}: guides must be regular Markdown files",
                    path.display()
                ));
            }
            let source = super::read_source(&path, &mut bytes)?;
            let display = path
                .components()
                .filter(|part| !matches!(part, std::path::Component::CurDir))
                .map(|part| part.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            guides.push((
                path,
                Guide {
                    path: display,
                    source,
                },
            ));
        }
    }
    Ok(guides)
}

fn default_title(source: Option<&Path>, destination: &Path) -> Result<String, String> {
    let base = match source {
        Some(source) => source.canonicalize().map_err(|error| error.to_string())?,
        None => destination
            .parent()
            .ok_or("site destination has no parent")?
            .to_path_buf(),
    };
    let name = if base.is_dir() {
        base.file_name()
    } else {
        base.file_stem()
    };
    Ok(name.unwrap_or_default().to_string_lossy().into_owned())
}

/// Resolve the destination beside its existing canonical parent without following the leaf.
fn destination(output: &Path) -> Result<PathBuf, String> {
    if output.as_os_str().is_empty() || output.as_os_str().len() > 4096 {
        return Err("site destination must be a path within 4096 bytes".into());
    }
    let name = output
        .file_name()
        .filter(|name| *name != "." && *name != "..")
        .ok_or_else(|| {
            format!(
                "refusing to use {} as a site destination; name a dedicated directory",
                output.display()
            )
        })?;
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()
        .map_err(|error| format!("site destination directory: {error}"))?;
    Ok(parent.join(name))
}

/// Refuse destinations that are links, files, input ancestors or foreign directories.
fn protect(destination: &Path, inputs: &[PathBuf]) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(destination) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("site destination: {error}")),
    };
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "refusing to replace symbolic link {}",
            destination.display()
        ));
    }
    if !metadata.is_dir() {
        return Err(format!(
            "refusing to replace non-directory {}",
            destination.display()
        ));
    }
    let canonical = destination
        .canonicalize()
        .map_err(|error| format!("site destination: {error}"))?;
    for input in inputs {
        let input = input
            .canonicalize()
            .map_err(|error| format!("{}: {error}", input.display()))?;
        if input.starts_with(&canonical) {
            return Err(format!(
                "refusing to replace {}: it contains documentation input {}",
                destination.display(),
                input.display()
            ));
        }
    }
    let generated = ["index.html", "morrow-docs.css", "morrow-search.js"]
        .iter()
        .all(|name| destination.join(name).is_file());
    let mut entries = fs::read_dir(destination).map_err(|error| error.to_string())?;
    if entries.next().is_some() && !generated {
        return Err(format!(
            "refusing to replace {}: it is not an empty directory or a generated documentation site",
            destination.display()
        ));
    }
    Ok(())
}

/// Write every page into private staging, then swap directories; the old site is restored
/// if the final rename fails.
fn publish(destination: &Path, pages: &[Page]) -> Result<(), String> {
    let parent = destination.parent().ok_or("invalid site destination")?;
    let workspace =
        super::super::native::Workspace::new(parent).map_err(|error| error.to_string())?;
    let staged = workspace.file("site");
    fs::create_dir(&staged).map_err(|error| format!("cannot stage site: {error}"))?;
    for page in pages {
        let relative = Path::new(&page.path);
        if relative
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err(format!("invalid generated page path: {}", page.path));
        }
        let target = staged.join(relative);
        if let Some(directory) = target.parent() {
            fs::create_dir_all(directory).map_err(|error| format!("cannot stage site: {error}"))?;
        }
        fs::write(&target, &page.contents)
            .map_err(|error| format!("cannot write {}: {error}", page.path))?;
    }
    let previous = workspace.file("previous");
    let existed = match fs::symlink_metadata(destination) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            fs::rename(destination, &previous)
                .map_err(|error| format!("cannot replace existing site: {error}"))?;
            true
        }
        Ok(_) => {
            return Err(format!(
                "refusing to replace {}: destination changed during generation",
                destination.display()
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(format!("site destination: {error}")),
    };
    if let Err(error) = fs::rename(&staged, destination) {
        if existed {
            let _ = fs::rename(&previous, destination);
        }
        return Err(format!("cannot install site: {error}"));
    }
    Ok(())
}
