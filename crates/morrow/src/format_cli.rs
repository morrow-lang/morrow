//! Validate and stage a bounded source set before publishing formatted files.
use super::{native::Workspace, source_directory};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};
const SOURCE_LIMIT: usize = 1024 * 1024;
const PROJECT_LIMIT: usize = 8 * SOURCE_LIMIT;

/// A source's display name, canonical destination, and fully validated replacement.
struct Formatted {
    name: PathBuf,
    path: PathBuf,
    text: String,
    changed: bool,
}

/// Format an explicit file or directory; check mode has no filesystem writes.
pub(super) fn run(source: &Path, check_only: bool) -> Result<u8, String> {
    let directory = source.is_dir();
    let paths = if directory {
        source_directory::discover(source, "formatting")?
    } else {
        vec![source.to_path_buf()]
    };
    let files = prepare(&paths)?;
    if check_only {
        let dirty: Vec<_> = files
            .iter()
            .filter(|file| file.changed)
            .map(|file| format!("{}: formatting changes required", file.name.display()))
            .collect();
        return if dirty.is_empty() {
            Ok(0)
        } else {
            Err(dirty.join("\n"))
        };
    }
    let staged = stage(&files)?;
    for (file, workspace) in files.iter().filter(|file| file.changed).zip(&staged) {
        fs::rename(workspace.file("formatted.fn"), &file.path)
            .map_err(|error| format!("{}: {error}", file.name.display()))?;
    }
    for file in files.iter().filter(|file| file.changed || !directory) {
        println!("Formatted {}", file.name.display());
    }
    Ok(0)
}

/// Bound all input before formatting; retain at most 8 MiB each of input and output.
fn prepare(paths: &[PathBuf]) -> Result<Vec<Formatted>, String> {
    let mut sources = Vec::new();
    let mut bytes = 0;
    for name in paths {
        let path = name
            .canonicalize()
            .map_err(|error| format!("{}: {error}", name.display()))?;
        let mut text = String::new();
        fs::File::open(&path)
            .and_then(|file| file.take(SOURCE_LIMIT as u64 + 1).read_to_string(&mut text))
            .map_err(|error| format!("{}: {error}", name.display()))?;
        if text.len() > SOURCE_LIMIT {
            return Err(format!(
                "{}: formatting source exceeds 1 MiB",
                name.display()
            ));
        }
        bytes += text.len();
        if bytes > PROJECT_LIMIT {
            return Err("project formatting source exceeds 8 MiB".into());
        }
        sources.push((name, path, text));
    }
    let mut files = Vec::new();
    let mut output_bytes = 0;
    for (name, path, text) in sources {
        let formatted = morrow_compiler::format::format(&text).map_err(|error| {
            let line = text
                .bytes()
                .take(error.span.start)
                .filter(|byte| *byte == b'\n')
                .count()
                + 1;
            format!("{}:{line}: error: {}", name.display(), error.message)
        })?;
        output_bytes += formatted.len();
        if output_bytes > PROJECT_LIMIT {
            return Err("project formatting output exceeds 8 MiB".into());
        }
        files.push(Formatted {
            name: name.clone(),
            path,
            changed: text != formatted,
            text: formatted,
        });
    }
    Ok(files)
}

/// Stage every changed file beside its destination, with original permissions, before publication.
/// Dropping workspaces cleans up all partial staging if preparation of any file fails.
fn stage(files: &[Formatted]) -> Result<Vec<Workspace>, String> {
    let mut staged = Vec::new();
    for file in files.iter().filter(|file| file.changed) {
        let parent = file.path.parent().ok_or("source has no parent directory")?;
        let workspace =
            Workspace::new(parent).map_err(|error| format!("{}: {error}", file.name.display()))?;
        let path = workspace.file("formatted.fn");
        fs::write(&path, &file.text)
            .and_then(|()| fs::set_permissions(&path, fs::metadata(&file.path)?.permissions()))
            .map_err(|error| format!("{}: {error}", file.name.display()))?;
        staged.push(workspace);
    }
    Ok(staged)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_later_preparation_removes_all_staging_without_replacing_sources() {
        let root = Workspace::new(&std::env::temp_dir()).unwrap();
        let original = root.file("a.fn");
        fs::write(&original, "unchanged").unwrap();
        let files = vec![
            Formatted {
                name: original.clone(),
                path: original.clone(),
                text: "replacement".into(),
                changed: true,
            },
            Formatted {
                name: root.file("missing.fn"),
                path: root.file("missing.fn"),
                text: "replacement".into(),
                changed: true,
            },
        ];
        assert!(stage(&files).is_err());
        assert_eq!(fs::read_to_string(&original).unwrap(), "unchanged");
        assert_eq!(fs::read_dir(original.parent().unwrap()).unwrap().count(), 1);
    }
}
