//! Cargo-native development, acceptance and distribution commands.
use std::{
    env,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};
use xtask::{acceptance, build, distribution, execute};

fn cargo(root: &Path, arguments: &[&str]) -> Result<(), String> {
    execute(
        Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
            .current_dir(root)
            .args(arguments),
    )
}

fn run() -> Result<(), String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("missing workspace root")?
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let mut arguments: Vec<_> = env::args_os().skip(1).collect();
    let release = if let Some(index) = arguments
        .iter()
        .position(|argument| argument == "--release")
    {
        arguments.remove(index);
        true
    } else {
        false
    };
    let command = arguments
        .first()
        .and_then(|argument| argument.to_str())
        .unwrap_or("help");
    let rest = arguments.get(1..).unwrap_or_default();
    if ["build", "check", "test", "fmt", "lint", "examples", "help"].contains(&command)
        && !rest.is_empty()
    {
        return Err(format!("unexpected arguments for {command}"));
    }
    match command {
        "help" | "--help" | "-h" => println!(
            "cargo xtask <command> [--release]\n\
build                 Build and stage compiler, runtime and supervisor in bin/\n\
web-build [output]    Build the browser preview and embed assets in one server executable\n\
web-check [server]    Verify two real browser clients and offline reload (MORROW_BROWSER required)\n\
simulate [options]    Run seeded virtual-time actor/application scenarios (--help for options)\n\
docs [output] [--no-rust] Build the documentation site (guides, examples, rustdoc) in dist/docs\n\
check                 Format, Clippy, workspace tests and native acceptance\n\
test                  Workspace tests and native acceptance\n\
native [filter]       Execute native expected-output fixtures from bin/\n\
fuzz [count] [seed]    Run deterministic grammar and mutation acceptance\n\
examples              Typecheck all public examples from bin/\n\
fmt                   Format the Rust workspace\n\
lint                  Check Rust formatting and Clippy\n\
lint-policy           Verify rejected lint fixtures with the pinned toolchain\n\
notices [--check]     Refresh or check locked workspace dependency license notices\n\
compatibility         Check native API and atomic rejection fixtures\n\
perf <report.json>    Measure explicitly staged compiler/runtime components\n\
package [directory]   Build a release and publish a verified host archive\n\
install <prefix>      Build a release and install its complete layout\n\
uninstall <prefix>    Remove only Morrow installation components\n\
verify <tar> <sha256> Validate a release archive without extracting it"
        ),
        "build" => {
            let bin = build::build(&root, release)?;
            println!("Built {}", bin.join("morrow").display());
        }
        "docs" if rest.len() <= 2 => {
            let rust = !rest.iter().any(|argument| argument == "--no-rust");
            let output = rest
                .iter()
                .filter(|argument| *argument != "--no-rust")
                .map(PathBuf::from)
                .next()
                .unwrap_or_else(|| root.join("dist/docs"));
            if rest.len() == 2 && rust {
                return Err("docs accepts one output directory and --no-rust".into());
            }
            let bin = build::build(&root, release)?;
            xtask::docs::run(&root, &bin, &output, rust)?;
        }
        "web-build" if rest.len() <= 1 => {
            xtask::web::build(&root, rest.first().map(Path::new))?;
        }
        "web-check" if rest.len() <= 1 => {
            let server = rest
                .first()
                .map(PathBuf::from)
                .unwrap_or_else(|| root.join("dist/morrow-web"));
            xtask::web::acceptance::run(&server)?;
        }
        "simulate" => {
            let mut simulation =
                Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
            simulation
                .current_dir(&root)
                .args(["run", "--locked", "-p", "morrow-sim"]);
            if release {
                simulation.arg("--release");
            }
            execute(simulation.arg("--").args(rest))?;
        }
        "fmt" => cargo(&root, &["fmt", "--all"])?,
        "lint" => {
            cargo(&root, &["fmt", "--all", "--", "--check"])?;
            cargo(
                &root,
                &[
                    "clippy",
                    "--workspace",
                    "--all-targets",
                    "--locked",
                    "--",
                    "-D",
                    "warnings",
                ],
            )?;
        }
        "check" | "test" => {
            if command == "check" {
                cargo(&root, &["fmt", "--all", "--", "--check"])?;
                xtask::notices::run(&root, true)?;
                cargo(
                    &root,
                    &[
                        "clippy",
                        "--workspace",
                        "--all-targets",
                        "--locked",
                        "--",
                        "-D",
                        "warnings",
                    ],
                )?;
            }
            let bin = build::build(&root, release)?;
            let mut arguments = vec!["test", "--workspace", "--locked"];
            if release {
                arguments.push("--release");
            }
            cargo(&root, &arguments)?;
            acceptance::native(&root, &bin, None)?;
            acceptance::examples(&root, &bin)?;
            xtask::compatibility::run(&root, &bin)?;
            xtask::fuzz::run(&root, &bin, 64, 0xC0FFEE)?;
        }
        "native" if rest.len() <= 1 => acceptance::native(
            &root,
            &root.join("bin"),
            rest.first()
                .map(|arg| arg.to_str().ok_or("filter must be UTF-8"))
                .transpose()?,
        )?,
        "examples" => acceptance::examples(&root, &root.join("bin"))?,
        "compatibility" if rest.is_empty() => xtask::compatibility::run(&root, &root.join("bin"))?,
        "lint-policy" if rest.is_empty() => xtask::lint_policy::run(&root)?,
        "notices" if rest.is_empty() || rest == ["--check"] => {
            xtask::notices::run(&root, !rest.is_empty())?;
        }
        "perf" if rest.len() == 1 => {
            xtask::performance::run(&root, &root.join("bin"), Path::new(&rest[0]))?
        }
        "fuzz" if rest.len() <= 2 => {
            let iterations = rest
                .first()
                .map(|value| {
                    value
                        .to_str()
                        .ok_or("count must be UTF-8")?
                        .parse::<u32>()
                        .map_err(|error| error.to_string())
                })
                .transpose()?
                .unwrap_or(64);
            if !(1..=1_000_000).contains(&iterations) {
                return Err("fuzz count must be between 1 and 1000000".into());
            }
            let seed = rest
                .get(1)
                .map(|value| {
                    let value = value.to_str().ok_or("seed must be UTF-8")?;
                    if let Some(hex) = value.strip_prefix("0x") {
                        u64::from_str_radix(hex, 16)
                    } else {
                        value.parse::<u64>()
                    }
                    .map_err(|error| error.to_string())
                })
                .transpose()?
                .unwrap_or(0xC0FFEE);
            xtask::fuzz::run(&root, &root.join("bin"), iterations, seed)?;
        }
        "package" if rest.len() <= 1 => {
            if env::var("GITHUB_REF_TYPE").as_deref() == Ok("tag")
                && env::var("GITHUB_REF_NAME").ok().as_deref()
                    != Some(&format!("v{}", env!("CARGO_PKG_VERSION")))
            {
                return Err("release tag does not match the workspace package version".into());
            }
            let bin = build::build(&root, true)?;
            let stage = packaging_stage(&root, &bin)?;
            let output = rest
                .first()
                .map(PathBuf::from)
                .unwrap_or_else(|| root.join("dist"));
            let archive =
                distribution::package(&root, &stage.0, &output, env!("CARGO_PKG_VERSION"))?;
            println!("{}", archive.display());
        }
        "install" if rest.len() == 1 => {
            let bin = build::build(&root, true)?;
            let stage = packaging_stage(&root, &bin)?;
            distribution::install(&stage.0, Path::new(&rest[0]))?;
        }
        "uninstall" if rest.len() == 1 => distribution::uninstall(Path::new(&rest[0]))?,
        "verify" if rest.len() == 2 => {
            distribution::verify(Path::new(&rest[0]), Path::new(&rest[1]))?
        }
        _ => {
            return Err(format!(
                "unknown command or invalid arguments: {command}; run cargo xtask help"
            ));
        }
    }
    Ok(())
}

fn packaging_stage(root: &Path, bin: &Path) -> Result<xtask::Temporary, String> {
    let stage = xtask::Temporary::new(root)?;
    for name in distribution::REQUIRED {
        let source = if ["LICENSE", "THIRD_PARTY_NOTICES.md"].contains(name) {
            root.join(name)
        } else {
            bin.join(name)
        };
        std::fs::copy(source, stage.0.join(name)).map_err(|error| error.to_string())?;
    }
    std::fs::copy(root.join("README.md"), stage.0.join("README.md"))
        .map_err(|error| error.to_string())?;
    Ok(stage)
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error}");
            ExitCode::FAILURE
        }
    }
}
