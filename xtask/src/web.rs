//! Reproducible browser assets and a server executable containing their complete bundle.
pub mod acceptance;
use base64::Engine as _;
pub mod publication;
use sha2::{Digest, Sha256};
use std::{
    env,
    fmt::Write as _,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
};

/// Public assets form one cache revision. The worker itself embeds its WASM bytes.
pub const ASSETS: &[&str] = &[
    "index.html",
    "bootstrap.js",
    "morrow_browser.js",
    "morrow_browser_bg.wasm",
    "morrow_app.wasm",
    "style.css",
];
const MAX_ASSET: u64 = 8 * 1024 * 1024;
const WASM_HEADER: &[u8] = b"\0asm\x01\0\0\0";

/// Bind each worker installation request to the bytes used for its cache revision.
pub fn integrity_manifest(directory: &Path) -> Result<String, String> {
    let mut manifest = String::new();
    for name in ASSETS {
        let bytes = read_asset(&directory.join(name), MAX_ASSET)?;
        let digest = base64::engine::general_purpose::STANDARD.encode(Sha256::digest(&bytes));
        writeln!(manifest, "/{name} sha256-{digest}").map_err(|error| error.to_string())?;
    }
    Ok(manifest)
}

fn read_asset(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(format!("invalid or oversized asset: {}", path.display()));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("asset grew past its byte limit".into());
    }
    Ok(bytes)
}

/// Cache identities include ordered names, lengths and contents, without ambiguous concatenation.
pub fn asset_digest(directory: &Path) -> Result<String, String> {
    let mut hash = Sha256::new();
    let mut total = 0;
    for name in ASSETS {
        let bytes = read_asset(&directory.join(name), MAX_ASSET)?;
        if name.ends_with(".wasm") && !bytes.starts_with(WASM_HEADER) {
            return Err(format!("invalid WebAssembly asset: {name}"));
        }
        total += bytes.len();
        if total > 16 * 1024 * 1024 {
            return Err("browser asset bundle exceeds 16 MiB".into());
        }
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(&bytes);
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// Embed generated bindings and binary bytes so a stopped worker can boot fully offline.
pub fn worker_script(glue: &str, wasm: &[u8]) -> Result<String, String> {
    if glue.len() > 2 * 1024 * 1024
        || wasm.len() > 2 * 1024 * 1024
        || !wasm.starts_with(WASM_HEADER)
    {
        return Err("invalid or oversized worker artifacts".into());
    }
    let mut output = String::with_capacity(glue.len() + wasm.len() * 4 + 100);
    output.push_str(glue);
    output.push_str("\nwasm_bindgen.initSync({ module: new Uint8Array([");
    for (index, byte) in wasm.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        write!(output, "{byte}").map_err(|error| error.to_string())?;
    }
    output.push_str("]) });\n");
    Ok(output)
}

fn cargo(root: &Path) -> Command {
    let mut command = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    command.current_dir(root);
    command
}

fn target_directory(root: &Path) -> Result<PathBuf, String> {
    let output = cargo(root)
        .args(["metadata", "--no-deps", "--format-version=1", "--locked"])
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())?;
    metadata["target_directory"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| "Cargo metadata has no target directory".into())
}

/// Check target identity and reject an ELF interpreter or DT_NEEDED shared libraries.
/// This is a dependency gate; executing the artifact remains a separate platform test.
pub fn validate_static_linux(bytes: &[u8], target: &str) -> Result<(), String> {
    let machine =
        match target {
            "aarch64-unknown-linux-musl" => 183,
            "x86_64-unknown-linux-musl" => 62,
            _ => return Err(
                "supported static targets: aarch64-unknown-linux-musl, x86_64-unknown-linux-musl"
                    .into(),
            ),
        };
    if bytes.len() < 64
        || &bytes[..7] != b"\x7fELF\x02\x01\x01"
        || u16::from_le_bytes([bytes[18], bytes[19]]) != machine
    {
        return Err("artifact is not the requested little-endian ELF64 target".into());
    }
    if !matches!(u16::from_le_bytes([bytes[16], bytes[17]]), 2 | 3) {
        return Err("ELF is not executable".into());
    }
    let number = |offset: usize| -> Result<usize, String> {
        let end = offset.checked_add(8).ok_or("ELF offset overflow")?;
        let raw: [u8; 8] = bytes
            .get(offset..end)
            .ok_or("truncated ELF")?
            .try_into()
            .map_err(|_| "truncated ELF number")?;
        usize::try_from(u64::from_le_bytes(raw)).map_err(|_| "ELF offset exceeds host size".into())
    };
    let start = number(32)?;
    let stride = u16::from_le_bytes([bytes[54], bytes[55]]) as usize;
    let count = u16::from_le_bytes([bytes[56], bytes[57]]) as usize;
    if stride != 56 || !(1..=128).contains(&count) {
        return Err("unsupported ELF program headers".into());
    }
    for index in 0..count {
        let offset = start
            .checked_add(index * stride)
            .ok_or("ELF offset overflow")?;
        let end = offset.checked_add(stride).ok_or("ELF offset overflow")?;
        let header = bytes
            .get(offset..end)
            .ok_or("truncated ELF program header")?;
        let kind = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
        if kind == 3 {
            return Err("ELF requires an external dynamic loader".into());
        }
        if kind == 2 {
            let dynamic = number(offset + 8)?;
            let size = number(offset + 32)?;
            let end = dynamic
                .checked_add(size)
                .ok_or("ELF dynamic range overflow")?;
            let entries = bytes
                .get(dynamic..end)
                .ok_or("truncated ELF dynamic data")?;
            if size == 0 || size % 16 != 0 {
                return Err("invalid ELF dynamic table".into());
            }
            let mut terminated = false;
            for entry in entries.as_chunks::<16>().0 {
                let tag = i64::from_le_bytes(entry[..8].try_into().map_err(|_| "invalid ELF tag")?);
                if tag == 0 {
                    terminated = true;
                    break;
                }
                if tag == 1 {
                    return Err("ELF requires an external shared library".into());
                }
            }
            if !terminated {
                return Err("unterminated ELF dynamic table".into());
            }
        }
    }
    Ok(())
}

/// Build browser and worker artifacts, then embed them into one native preview server.
pub fn build(root: &Path, output: Option<&Path>) -> Result<PathBuf, String> {
    let server_target = env::var("MORROW_WEB_TARGET").ok();
    if server_target.as_deref().is_some_and(|target| {
        !matches!(
            target,
            "aarch64-unknown-linux-musl" | "x86_64-unknown-linux-musl"
        )
    }) {
        return Err("MORROW_WEB_TARGET supports aarch64-unknown-linux-musl or x86_64-unknown-linux-musl; omit it for a host build".into());
    }
    let output = match output {
        Some(path) => path.to_path_buf(),
        None => {
            fs::create_dir_all(root.join("dist")).map_err(|error| error.to_string())?;
            root.join(match server_target.as_deref() {
                Some("aarch64-unknown-linux-musl") => "dist/morrow-web-linux-arm64",
                Some("x86_64-unknown-linux-musl") => "dist/morrow-web-linux-x86_64",
                _ => "dist/morrow-web",
            })
        }
    };
    publication::validate(&output)?;
    let bindgen = env::var_os("WASM_BINDGEN").unwrap_or_else(|| "wasm-bindgen".into());
    let version = Command::new(&bindgen)
        .arg("--version")
        .output()
        .map_err(|_| {
            "install wasm-bindgen-cli 0.2.128 and add wasm32-unknown-unknown with rustup".to_owned()
        })?;
    if !version.status.success()
        || String::from_utf8_lossy(&version.stdout).trim() != "wasm-bindgen 0.2.128"
    {
        return Err("web-build requires wasm-bindgen-cli exactly 0.2.128".into());
    }
    let target = target_directory(root)?;
    let work = crate::Temporary::new(&env::temp_dir())?;
    let assets = work.0.join("assets");
    fs::create_dir(&assets).map_err(|error| error.to_string())?;
    crate::execute(cargo(root).args(["build", "--release", "--locked", "-p", "morrow"]))?;
    crate::execute(
        Command::new(target.join("release/morrow"))
            .current_dir(root)
            .args(["build", "--target=wasm32"])
            .arg(root.join("examples/web/checklist.mr"))
            .arg("-o")
            .arg(assets.join("morrow_app.wasm")),
    )?;
    crate::execute(cargo(root).args([
        "build",
        "--release",
        "--locked",
        "--target",
        "wasm32-unknown-unknown",
        "-p",
        "morrow-browser",
    ]))?;
    crate::execute(
        Command::new(&bindgen)
            .arg(target.join("wasm32-unknown-unknown/release/morrow_browser.wasm"))
            .args([
                "--target",
                "web",
                "--out-name",
                "morrow_browser",
                "--no-typescript",
                "--out-dir",
            ])
            .arg(&assets),
    )?;
    for name in ["index.html", "style.css"] {
        let bytes = read_asset(
            &root.join("crates/morrow-browser/assets").join(name),
            MAX_ASSET,
        )?;
        fs::write(assets.join(name), bytes).map_err(|error| error.to_string())?;
    }
    // Only module loading is generated JavaScript; browser behavior is Rust/Morrow.
    fs::write(
        assets.join("bootstrap.js"),
        "import init, { mount } from '/morrow_browser.js';\nawait init();\nawait mount();\n",
    )
    .map_err(|error| error.to_string())?;
    let revision = asset_digest(&assets)?;
    let integrities = integrity_manifest(&assets)?;
    crate::execute(
        cargo(root)
            .env("MORROW_WEB_CACHE_VERSION", &revision)
            .env("MORROW_WEB_ASSET_INTEGRITIES", integrities)
            .args([
                "build",
                "--release",
                "--locked",
                "--target",
                "wasm32-unknown-unknown",
                "-p",
                "morrow-browser-worker",
            ]),
    )?;
    let worker = work.0.join("worker");
    crate::execute(
        Command::new(&bindgen)
            .arg(target.join("wasm32-unknown-unknown/release/morrow_browser_worker.wasm"))
            .args([
                "--target",
                "no-modules",
                "--out-name",
                "morrow_worker",
                "--no-typescript",
                "--out-dir",
            ])
            .arg(&worker),
    )?;
    let glue = String::from_utf8(read_asset(
        &worker.join("morrow_worker.js"),
        2 * 1024 * 1024,
    )?)
    .map_err(|error| error.to_string())?;
    let wasm = read_asset(&worker.join("morrow_worker_bg.wasm"), 2 * 1024 * 1024)?;
    fs::write(assets.join("worker.js"), worker_script(&glue, &wasm)?)
        .map_err(|error| error.to_string())?;
    let mut server = cargo(root);
    server.env("MORROW_WEB_ASSETS_DIR", &assets).args([
        "build",
        "--release",
        "--locked",
        "-p",
        "morrow-web",
    ]);
    let artifact = if let Some(triple) = &server_target {
        server.args(["--target", triple]);
        server.env(
            format!(
                "CARGO_TARGET_{}_LINKER",
                triple.to_uppercase().replace('-', "_")
            ),
            "rust-lld",
        );
        target.join(triple).join("release/morrow-web")
    } else {
        target.join("release/morrow-web")
    };
    crate::execute(&mut server)?;
    if let Some(triple) = &server_target {
        validate_static_linux(&read_asset(&artifact, 64 * 1024 * 1024)?, triple)?;
    }
    publication::validate(&output)?;
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let staging = crate::Temporary::new(parent)?;
    let executable = staging.0.join("morrow-web");
    fs::copy(artifact, &executable).map_err(|error| error.to_string())?;
    fs::rename(&executable, &output).map_err(|error| error.to_string())?;
    println!("Browser asset revision: {revision}");
    println!(
        "Created server with embedded browser assets: {}",
        output.display()
    );
    Ok(output)
}
