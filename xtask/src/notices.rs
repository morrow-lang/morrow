//! Preserve reviewed notices while adding locked dependency license files.
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// Refresh notices from Cargo's locked native and browser resolutions. Check mode
/// compares without modifying files. Existing license text and anchors are retained.
pub fn run(root: &Path, check: bool) -> Result<(), String> {
    let mut selected = BTreeMap::new();
    for target in [
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "aarch64-unknown-linux-gnu",
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-musl",
        "x86_64-unknown-linux-musl",
        "wasm32-unknown-unknown",
    ] {
        let output = Command::new("cargo")
            .current_dir(root)
            .args([
                "metadata",
                "--locked",
                "--format-version",
                "1",
                "--filter-platform",
                target,
            ])
            .output()
            .map_err(|error| format!("cargo metadata: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "cargo metadata: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        if output.stdout.len() > 16 * 1024 * 1024 {
            return Err("Cargo metadata exceeds 16 MiB".into());
        }
        let metadata: Value =
            serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())?;
        for package in target_packages(&metadata, target == "wasm32-unknown-unknown")? {
            selected.insert(field(&package, "id")?.to_owned(), package);
        }
    }
    let metadata = serde_json::json!({"packages": selected.into_values().collect::<Vec<_>>()});
    let path = root.join("THIRD_PARTY_NOTICES.md");
    let existing = fs::read_to_string(&path).map_err(|error| error.to_string())?;
    let updated = render(&existing, &metadata)?;
    if existing == updated {
        return Ok(());
    }
    if check {
        return Err("THIRD_PARTY_NOTICES.md is incomplete; run cargo xtask notices and review new source notices".into());
    }
    let temporary = crate::Temporary::new(root)?;
    let staged = temporary.0.join("THIRD_PARTY_NOTICES.md");
    fs::write(&staged, updated).map_err(|error| error.to_string())?;
    fs::rename(staged, path).map_err(|error| error.to_string())
}

/// Traverse Cargo's already target-filtered edges. Native inventories start at all
/// workspace members; browser inventories start only at the two shipped browser
/// packages, not unsupported native-runtime compilations for wasm32.
pub fn target_packages(metadata: &Value, browser: bool) -> Result<Vec<Value>, String> {
    let packages = metadata["packages"]
        .as_array()
        .ok_or("missing metadata packages")?;
    let nodes = metadata["resolve"]["nodes"]
        .as_array()
        .ok_or("missing resolved dependency graph")?;
    if packages.len() > 4096 || nodes.len() > 4096 {
        return Err("dependency inventory exceeds 4096 packages".into());
    }
    let mut pending: Vec<_> = if browser {
        packages
            .iter()
            .filter(|package| {
                matches!(
                    package["name"].as_str(),
                    Some("morrow-browser" | "morrow-browser-worker")
                )
            })
            .map(|package| field(package, "id").map(str::to_owned))
            .collect::<Result<_, _>>()?
    } else {
        metadata["workspace_members"]
            .as_array()
            .ok_or("missing workspace members")?
            .iter()
            .map(|id| {
                id.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| "invalid workspace member".to_owned())
            })
            .collect::<Result<_, _>>()?
    };
    if pending.is_empty() {
        return Err("dependency inventory has no target roots".into());
    }
    let mut visited = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id.clone()) {
            continue;
        }
        let node = nodes
            .iter()
            .find(|node| node["id"].as_str() == Some(&id))
            .ok_or_else(|| format!("missing dependency node {id}"))?;
        for dependency in node["deps"].as_array().ok_or("missing node dependencies")? {
            pending.push(field(dependency, "pkg")?.to_owned());
        }
    }
    Ok(packages
        .iter()
        .filter(|package| {
            !package["source"].is_null()
                && package["id"]
                    .as_str()
                    .is_some_and(|id| visited.contains(id))
        })
        .cloned()
        .collect())
}

/// Add previously unlisted registry packages using their actual local license files.
/// This deliberately fails when source terms cannot be selected by the reviewed policy.
pub fn render(existing: &str, metadata: &Value) -> Result<String, String> {
    let inventory_at = existing
        .find("## Dependency inventory")
        .ok_or("missing inventory heading")?;
    let licenses_at = existing
        .find("## License texts")
        .ok_or("missing license heading")?;
    let tail_at = existing
        .find("## SQLite")
        .ok_or("missing preserved notice tail")?;
    if inventory_at >= licenses_at || licenses_at >= tail_at {
        return Err("invalid notice section order".into());
    }
    let mut rows = BTreeMap::new();
    for line in existing[inventory_at..licenses_at]
        .lines()
        .filter(|line| line.starts_with("| [`"))
    {
        let key = line
            .strip_prefix("| [")
            .and_then(|line| line.split_once("]("))
            .map(|(key, _)| key)
            .ok_or("invalid inventory row")?;
        rows.insert(key.to_owned(), line.to_owned());
    }
    let mut texts = BTreeMap::new();
    let mut last_id = 0;
    for section in existing[licenses_at..tail_at].split("### License ").skip(1) {
        let (id, _) = section.split_once('\n').ok_or("invalid license heading")?;
        let id: usize = id.parse().map_err(|_| "invalid license number")?;
        let text = section
            .split_once("```text\n")
            .and_then(|(_, text)| text.split_once("\n```"))
            .map(|(text, _)| text)
            .ok_or("missing full license text")?;
        texts.insert(text.trim().to_owned(), id);
        last_id = last_id.max(id);
    }
    let packages = metadata["packages"]
        .as_array()
        .ok_or("missing metadata packages")?;
    if packages.len() > 4096 {
        return Err("dependency inventory exceeds 4096 packages".into());
    }
    let mut packages: Vec<_> = packages
        .iter()
        .filter(|package| !package["source"].is_null())
        .collect();
    packages.sort_by_key(|package| (package["name"].as_str(), package["version"].as_str()));
    let current: BTreeSet<_> = packages
        .iter()
        .map(|package| {
            Ok(format!(
                "`{}` {}",
                field(package, "name")?,
                field(package, "version")?
            ))
        })
        .collect::<Result<_, String>>()?;
    rows.retain(|key, _| current.contains(key));
    let mut appended = String::new();
    for package in packages {
        let name = field(package, "name")?;
        let version = field(package, "version")?;
        let key = format!("`{name}` {version}");
        if rows.contains_key(&key) {
            continue;
        }
        let expression = field(package, "license")?;
        let selected = licenses(package).map_err(|error| format!("{name} {version}: {error}"))?;
        let mut links = Vec::new();
        for (label, file, text) in selected {
            let normalized = text.trim().to_owned();
            let id = if let Some(id) = texts.get(&normalized) {
                *id
            } else {
                last_id += 1;
                let id = last_id;
                appended.push_str(&format!("### License {id}\n\n`{name}` {version}: `{file}`.\n\n```text\n{normalized}\n```\n\n"));
                texts.insert(normalized, id);
                id
            };
            links.push(format!("[{label}: {file}](#license-{id})"));
        }
        rows.insert(key, format!("| [`{name}` {version}](https://crates.io/crates/{name}/{version}) | {expression} | {} |", links.join(", ")));
    }
    let mut output = existing[..inventory_at].to_owned();
    output.push_str("## Dependency inventory\n\n| Package | Declared license | Selected license and notices |\n| --- | --- | --- |\n");
    for row in rows.values() {
        output.push_str(row);
        output.push('\n');
    }
    output.push('\n');
    output.push_str(&existing[licenses_at..tail_at]);
    output.push_str(&appended);
    output.push_str(&existing[tail_at..]);
    Ok(output)
}

fn field<'a>(value: &'a Value, name: &str) -> Result<&'a str, String> {
    value[name]
        .as_str()
        .ok_or_else(|| format!("missing package {name}"))
}

fn licenses(package: &Value) -> Result<Vec<(String, String, String)>, String> {
    let expression = field(package, "license")?;
    if expression.contains(" AND ")
        && !matches!(
            expression,
            "MIT AND BSD-3-Clause" | "(MIT OR Apache-2.0) AND Unicode-3.0"
        )
    {
        return Err(format!(
            "conjunctive license expression requires review: {expression}"
        ));
    }
    let selected = if expression
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
        .any(|token| token == "MIT")
    {
        "MIT"
    } else if expression == "Apache-2.0" {
        "Apache-2.0"
    } else if expression == "Apache-2.0 WITH LLVM-exception" {
        // Reviewed ar_archive_writer 0.5.2 LICENSE.txt contains Apache 2.0 and
        // the complete LLVM exceptions. Preserve the full expression and text.
        "Apache-2.0 WITH LLVM-exception"
    } else if expression == "BSL-1.0" {
        "BSL-1.0"
    } else if expression == "Zlib" {
        "Zlib"
    } else {
        return Err(format!("license expression requires review: {expression}"));
    };
    let directory = Path::new(field(package, "manifest_path")?)
        .parent()
        .ok_or("manifest has no directory")?;
    let mut files = Vec::new();
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        if !entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_file()
        {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let upper = name.to_ascii_uppercase();
        if upper.starts_with("LICENSE")
            || upper.starts_with("LICENCE")
            || upper.starts_with("NOTICE")
            || upper.starts_with("COPYING")
            || upper.starts_with("COPYRIGHT")
        {
            files.push((name, entry.path()));
        }
    }
    if let Some(file) = package["license_file"].as_str() {
        let path = directory.join(file);
        if !files.iter().any(|(_, existing)| existing == &path) {
            files.push((file.into(), path));
        }
    }
    files.sort();
    let preferred = if selected == "MIT" {
        files
            .iter()
            .find(|(name, _)| name.to_ascii_uppercase().contains("MIT"))
    } else if selected == "Apache-2.0" {
        files
            .iter()
            .find(|(name, _)| name.to_ascii_uppercase().contains("APACHE"))
    } else {
        None
    };
    let primary = preferred
        .or_else(|| {
            files.iter().find(|(name, _)| {
                matches!(
                    name.to_ascii_uppercase().as_str(),
                    "LICENSE" | "LICENSE.TXT" | "LICENSE.MD" | "LICENCE" | "COPYING"
                )
            })
        })
        .ok_or_else(|| {
            format!(
                "no source license files for selected {selected}; found {:?}",
                files.iter().map(|(name, _)| name).collect::<Vec<_>>()
            )
        })?;
    let mut result = vec![(selected.to_owned(), primary.0.clone(), read(&primary.1)?)];
    for (name, path) in &files {
        if name == &primary.0 {
            continue;
        }
        let upper = name.to_ascii_uppercase();
        let optional_alternative = selected == "MIT"
            && (upper.contains("APACHE")
                || upper.contains("LLVM")
                || upper.contains("LGPL")
                || upper.contains("BSD"));
        if optional_alternative && !expression.contains(" AND ") {
            continue;
        }
        result.push(("Additional notice".into(), name.clone(), read(path)?));
    }
    if expression.contains(" AND ") && result.len() < 2 {
        return Err(format!(
            "conjunctive license requires additional source notices: {expression}"
        ));
    }
    Ok(result)
}

fn read(path: &PathBuf) -> Result<String, String> {
    if fs::metadata(path).map_err(|error| error.to_string())?.len() > 1024 * 1024 {
        return Err(format!("license file too large: {}", path.display()));
    }
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    if text.trim().is_empty() || text.contains("```") {
        return Err(format!(
            "license text needs manual rendering: {}",
            path.display()
        ));
    }
    Ok(text.replace("\r\n", "\n"))
}
