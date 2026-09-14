//! Embed only the declared browser build output into the deployable executable.
use std::{
    env, fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

fn read_asset(path: &Path) -> io::Result<Vec<u8>> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(io::Error::other("symlink browser asset"));
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("browser asset is not a regular file"));
    }
    let mut bytes = Vec::new();
    file.take(32 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 32 * 1024 * 1024 {
        return Err(io::Error::other("browser asset exceeds 32 MiB"));
    }
    Ok(bytes)
}

fn main() -> io::Result<()> {
    println!("cargo:rerun-if-env-changed=MORROW_WEB_ASSETS_DIR");
    let output = PathBuf::from(
        env::var_os("OUT_DIR").ok_or_else(|| io::Error::other("missing Cargo OUT_DIR"))?,
    );
    let mut source = "pub const EMBEDDED_ASSETS: crate::Assets = &[\n".to_owned();
    if let Some(directory) = env::var_os("MORROW_WEB_ASSETS_DIR") {
        let directory = fs::canonicalize(directory)?;
        let mut total = 0u64;
        for name in [
            "index.html",
            "bootstrap.js",
            "morrow_browser.js",
            "morrow_browser_bg.wasm",
            "morrow_app.wasm",
            "style.css",
            "worker.js",
        ] {
            let input = directory.join(name);
            println!("cargo:rerun-if-changed={}", input.display());
            let bytes = read_asset(&input)?;
            total += bytes.len() as u64;
            if total > 64 * 1024 * 1024 {
                return Err(io::Error::other("browser assets exceed 64 MiB"));
            }
            if name.ends_with(".wasm") && !bytes.starts_with(b"\0asm\x01\0\0\0") {
                return Err(io::Error::other(format!("invalid WASM asset: {name}")));
            }
            let target = output.join(name);
            fs::write(&target, bytes)?;
            let target = target
                .to_str()
                .ok_or_else(|| io::Error::other("asset output path must be UTF-8"))?;
            source.push_str(&format!("({name:?}, include_bytes!({target:?})),\n"));
        }
    }
    source.push_str("];\n");
    fs::write(output.join("assets.rs"), source)
}
