//! Check Clippy policy using isolated, offline crates and bounded subprocesses.
use std::{fs, path::Path, process::Command, time::Duration};

const PANIC: &str = "#![cfg_attr(not(test), deny(clippy::panic, clippy::panic_in_result_fn))]";
const REQUIRED: &[&str] = &[
    "dbg_macro",
    "todo",
    "unimplemented",
    "exit",
    "unchecked_time_subtraction",
    "unused_peekable",
    "redundant_clone",
    "or_fun_call",
];
const CASES: &[(&str, &str, bool)] = &[
    (
        "panic",
        "pub fn value()->u32 { panic!(\"unexpected\") }",
        false,
    ),
    (
        "panic_in_result_fn",
        "pub fn value()->Result<u32,String> { panic!(\"unexpected\") }",
        false,
    ),
    ("dbg_macro", "pub fn value(x:u32)->u32 { dbg!(x) }", false),
    ("todo", "pub fn value()->u32 { todo!() }", false),
    (
        "unimplemented",
        "pub fn value()->u32 { unimplemented!() }",
        false,
    ),
    ("exit", "pub fn value() { std::process::exit(2); }", false),
    (
        "unchecked_time_subtraction",
        "pub fn value(a:std::time::Instant,b:std::time::Duration)->std::time::Instant { a-b }",
        false,
    ),
    (
        "unused_peekable",
        "pub fn value(x:&[u32])->u32 { let mut values=x.iter().peekable(); values.next().copied().unwrap_or(0) }",
        false,
    ),
    (
        "redundant_clone",
        "pub fn value(x:String)->String { x.clone() }",
        false,
    ),
    (
        "or_fun_call",
        "pub fn value(x:Option<String>)->String { x.unwrap_or(String::from(\"fallback\")) }",
        false,
    ),
    (
        "unwrap_used",
        "pub fn value(x:Option<u32>)->u32 { x.unwrap() }",
        true,
    ),
    (
        "expect_used",
        "pub fn value(x:Option<u32>)->u32 { x.expect(\"required\") }",
        true,
    ),
    (
        "indexing_slicing",
        "pub fn value(x:&[u32],n:usize)->u32 { x[n] }",
        true,
    ),
    (
        "as_conversions",
        "pub fn value(x:u64)->u8 { x as u8 }",
        true,
    ),
    (
        "unreachable",
        "pub fn value()->u32 { unreachable!() }",
        true,
    ),
    (
        "string_slice",
        "pub fn value(x:&str)->&str { &x[1..] }",
        true,
    ),
    (
        "arithmetic_side_effects",
        "pub fn value(a:u32,b:u32)->u32 { a+b }",
        true,
    ),
    (
        "unchecked_time_subtraction",
        "pub fn value(a:std::time::Duration,b:std::time::Duration)->std::time::Duration { a-b }",
        false,
    ),
];

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
}
fn compact(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}
fn policy(
    manifest: &str,
    lib: &str,
    main: &str,
    boundary: &str,
) -> Result<(String, String), String> {
    let section = manifest
        .split("[lints.clippy]")
        .nth(1)
        .ok_or("missing Clippy policy")?
        .split('[')
        .next()
        .unwrap_or_default();
    let entries: Vec<_> = section
        .lines()
        .map(|line| compact(line.split('#').next().unwrap_or_default()))
        .collect();
    for lint in REQUIRED {
        if !entries.contains(&format!("{lint}=\"deny\"")) {
            return Err(format!("Clippy policy must deny {lint}"));
        }
    }
    if section.contains("unchecked_duration_subtraction") {
        return Err("obsolete Clippy lint unchecked_duration_subtraction".into());
    }
    if !compact(lib).contains(&compact(PANIC)) || !compact(main).contains(&compact(PANIC)) {
        return Err("compiler library and binary must deny production panic lints".into());
    }
    let prelude = boundary
        .split("use std")
        .next()
        .ok_or("missing boundary lint prelude")?;
    for (lint, _, scoped) in CASES {
        if *scoped && !prelude.contains(&format!("clippy::{lint}")) {
            return Err(format!("missing scoped lint {lint}"));
        }
    }
    Ok((section.into(), prelude.into()))
}
fn expected_failure(success: bool, diagnostics: &str, lint: &str) -> bool {
    !success
        && !diagnostics.contains("unknown lint")
        && !diagnostics.contains("unknown_lints")
        && (diagnostics.contains(&format!("clippy::{lint}"))
            || diagnostics.contains(&lint.replace('_', "-")))
}

/// Exercise all eighteen forbidden snippets and both supported positive cases.
pub fn run(root: &Path) -> Result<(), String> {
    let compiler = root.join("crates/fern");
    let (lints, boundary) = policy(
        &read(&compiler.join("Cargo.toml"))?,
        &read(&compiler.join("src/lib.rs"))?,
        &read(&compiler.join("src/main.rs"))?,
        &read(&compiler.join("src/source_directory.rs"))?,
    )?;
    let temp = crate::Temporary::new(&std::env::temp_dir())?;
    fs::create_dir(temp.0.join("src")).map_err(|e| e.to_string())?;
    fs::write(temp.0.join("Cargo.toml"), format!("[workspace]\n[package]\nname=\"fern-lint-policy-probe\"\nversion=\"0.0.0\"\nedition=\"2024\"\n[lints.clippy]\n{lints}")).map_err(|e| e.to_string())?;
    let helper = std::env::var_os("FERN_TEST_SUPERVISOR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("bin/fern-test-supervisor"));
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| env!("CARGO").into());
    let check = |body: &str, scoped: bool, tests: bool| -> Result<(bool, String), String> {
        fs::write(
            temp.0.join("src/lib.rs"),
            format!(
                "{PANIC}\n{}\n{body}\n",
                if scoped { boundary.as_str() } else { "" }
            ),
        )
        .map_err(|e| e.to_string())?;
        let mut command = Command::new(&cargo);
        command
            .args(["clippy", "--offline", "--manifest-path"])
            .arg(temp.0.join("Cargo.toml"));
        if tests {
            command.arg("--tests");
        }
        command.args(["--", "-D", "warnings"]);
        let mut supervisor = Command::new(&helper);
        supervisor
            .current_dir(root)
            .env("CARGO_TARGET_DIR", temp.0.join("target"))
            .env_remove("RUSTFLAGS")
            .env_remove("CARGO_ENCODED_RUSTFLAGS");
        let output = fern_test_supervisor::capture(command, supervisor, Duration::from_secs(60))?;
        Ok((
            output.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ),
        ))
    };
    for (index, (lint, body, scoped)) in CASES.iter().enumerate() {
        let (success, diagnostics) = check(body, *scoped, false)?;
        if !expected_failure(success, &diagnostics, lint) {
            return Err(format!(
                "lint fixture {index} must fail specifically for {lint}:\n{diagnostics}"
            ));
        }
    }
    for (body, tests) in [
        (
            "pub fn value(a:std::time::Instant,b:std::time::Duration)->Option<std::time::Instant> { a.checked_sub(b) }",
            false,
        ),
        (
            "#[cfg(test)] mod tests { #[test] fn assertion() { panic!(\"test oracle\"); } }",
            true,
        ),
    ] {
        let (success, diagnostics) = check(body, false, tests)?;
        if !success {
            return Err(format!("positive lint fixture rejected:\n{diagnostics}"));
        }
    }
    println!(
        "Clippy policy: 18 forbidden cases rejected; checked subtraction and test-only panic accepted"
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pinned_toolchain_enforces_all_forbidden_cases_and_both_positive_cases() {
        run(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
    }
    #[test]
    fn generic_failure_unknown_lint_and_success_never_count_as_enforcement() {
        assert!(!expected_failure(
            false,
            "error: compiler unavailable",
            "panic"
        ));
        assert!(!expected_failure(
            false,
            "unknown lint clippy::panic",
            "panic"
        ));
        assert!(!expected_failure(true, "clippy::panic", "panic"));
        assert!(expected_failure(false, "error: clippy::panic", "panic"));
    }
    #[test]
    fn weakening_the_manifest_or_production_boundary_is_rejected() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("crates/fern");
        let manifest = read(&root.join("Cargo.toml")).unwrap();
        let lib = read(&root.join("src/lib.rs")).unwrap();
        let main = read(&root.join("src/main.rs")).unwrap();
        let boundary = read(&root.join("src/source_directory.rs")).unwrap();
        assert!(
            policy(
                &manifest.replace("dbg_macro = \"deny\"", "dbg_macro = \"allow\""),
                &lib,
                &main,
                &boundary
            )
            .is_err()
        );
        assert!(policy(&manifest, "", &main, &boundary).is_err());
        assert!(policy(&manifest, &lib, "", &boundary).is_err());
        assert!(policy(&manifest, &lib, &main, "").is_err());
    }
}
