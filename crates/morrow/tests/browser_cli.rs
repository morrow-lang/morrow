//! Browser compilation publishes a real module without native runtime dependencies.
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "morrow-browser-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::write(path.join("source.mr"), "fn main() -> Int: 42\n").unwrap();
        Self(path)
    }
    fn run(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_morrow"))
            .current_dir(&self.0)
            .args(args)
            .env("PATH", &self.0)
            .env("CC", self.0.join("absent-linker"))
            .env("MORROW_RUNTIME_LIB", self.0.join("absent-runtime"))
            .output()
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn browser_build_needs_no_native_tools_and_defaults_to_wasm_extension() {
    let fixture = Fixture::new();
    for args in [
        vec!["build", "--target=wasm32", "source.mr"],
        vec!["build", "source.mr", "--target", "wasm32"],
    ] {
        let result = fixture.run(&args);
        assert!(result.status.success(), "{result:?}");
        let bytes = fs::read(fixture.0.join("source.wasm")).unwrap();
        assert_eq!(&bytes[..8], b"\0asm\x01\0\0\0");
        assert!(!fixture.0.join("source").exists());
    }
}

#[test]
fn browser_library_exports_do_not_require_a_native_main() {
    let fixture = Fixture::new();
    fs::write(
        fixture.0.join("source.mr"),
        "pub fn update(value: Int) -> Int: value + 1\n",
    )
    .unwrap();
    let result = fixture.run(&["build", "--target=wasm32", "source.mr"]);
    assert!(result.status.success(), "{result:?}");
    assert!(
        fs::read(fixture.0.join("source.wasm"))
            .unwrap()
            .starts_with(b"\0asm")
    );
    let bytes = fs::read(fixture.0.join("source.wasm")).unwrap();
    let engine = wasmi::Engine::default();
    let module = wasmi::Module::new(&engine, &bytes).unwrap();
    let mut store = wasmi::Store::new(&engine, ());
    let instance = wasmi::Linker::new(&engine)
        .instantiate_and_start(&mut store, &module)
        .unwrap();
    let update = instance
        .get_typed_func::<i64, i64>(&store, "source.update")
        .unwrap();
    assert_eq!(
        update.call(&mut store, 9_007_199_254_740_993).unwrap(),
        9_007_199_254_740_994
    );
    assert!(!fixture.run(&["build", "source.mr"]).status.success());
}

#[test]
fn browser_failure_preserves_output_and_rejects_native_option_combinations() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("out.wasm"), b"previous").unwrap();
    for args in [
        vec!["build", "--target=unknown", "source.mr", "-o", "out.wasm"],
        vec![
            "build",
            "--target=wasm32",
            "--backend=cranelift",
            "source.mr",
            "-o",
            "out.wasm",
        ],
        vec![
            "build",
            "--target=wasm32",
            "--target=wasm32",
            "source.mr",
            "-o",
            "out.wasm",
        ],
        vec!["run", "--target=wasm32", "source.mr"],
        vec!["emit", "--target=wasm32", "source.mr"],
    ] {
        let result = fixture.run(&args);
        assert!(!result.status.success(), "{result:?}");
        assert_eq!(fs::read(fixture.0.join("out.wasm")).unwrap(), b"previous");
    }
    fs::write(
        fixture.0.join("source.mr"),
        "fn main(): println(\"native IO\")\n",
    )
    .unwrap();
    let result = fixture.run(&["build", "--target=wasm32", "source.mr", "-o", "out.wasm"]);
    assert!(!result.status.success(), "{result:?}");
    assert_eq!(fs::read(fixture.0.join("out.wasm")).unwrap(), b"previous");
}

#[test]
fn browser_output_cannot_overwrite_any_loaded_source() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("shared.mr"), "pub fn answer() -> Int: 42\n").unwrap();
    fs::write(
        fixture.0.join("source.mr"),
        "import shared\nfn main() -> Int: shared.answer()\n",
    )
    .unwrap();
    for name in ["source.mr", "shared.mr"] {
        let before = fs::read(fixture.0.join(name)).unwrap();
        let result = fixture.run(&["build", "--target=wasm32", "source.mr", "-o", name]);
        assert!(!result.status.success(), "{result:?}");
        assert!(
            String::from_utf8_lossy(&result.stderr).contains("refusing to overwrite source"),
            "{result:?}"
        );
        assert_eq!(fs::read(fixture.0.join(name)).unwrap(), before);
    }
}
