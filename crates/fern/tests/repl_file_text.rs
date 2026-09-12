use fern_compiler::repl::Session;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct File(PathBuf);
impl File {
    fn new(bytes: &[u8]) -> Self {
        let path = std::env::temp_dir().join(format!(
            "fern-file-text-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, bytes).unwrap();
        Self(path)
    }
    fn quoted(&self) -> String {
        format!("{:?}", self.0.to_string_lossy())
    }
}
impl Drop for File {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
fn read_result(file: &File) -> Result<String, String> {
    Session::default().evaluate(&format!(
        "match File.read({}):\n    Ok(text) -> String.len(text)\n    Err(code) -> -code",
        file.quoted()
    ))
}
#[test]
fn invalid_text_is_an_ordinary_error_without_truncated_publication() {
    for bytes in [
        &b"a\0b"[..],
        &b"\xc0\xaf"[..],
        &b"\xed\xa0\x80"[..],
        &b"\xf0\x9f"[..],
    ] {
        assert_eq!(read_result(&File::new(bytes)).unwrap(), "-3 : Int\n");
    }
}
#[test]
fn native_content_ceiling_precedes_stricter_interactive_storage_limit() {
    let file = File::new(b"");
    std::fs::OpenOptions::new()
        .write(true)
        .open(&file.0)
        .unwrap()
        .set_len(16777217)
        .unwrap();
    assert_eq!(read_result(&file).unwrap(), "-3 : Int\n");
    std::fs::OpenOptions::new()
        .write(true)
        .open(&file.0)
        .unwrap()
        .set_len(1048577)
        .unwrap();
    assert!(
        read_result(&file)
            .unwrap_err()
            .contains("interactive string limit exceeded")
    );
}
#[test]
fn valid_text_roundtrip_retains_exact_byte_counts_and_empty_success() {
    let file = File::new(b"");
    let mut session = Session::default();
    assert_eq!(
        session
            .evaluate(&format!(
                "println(Result.unwrap_or(File.write({}, \"🌿é\"), -1))",
                file.quoted()
            ))
            .unwrap(),
        "6\n"
    );
    assert_eq!(
        session
            .evaluate(&format!(
                "println(Result.unwrap_or(File.append({}, \"!\"), -1))",
                file.quoted()
            ))
            .unwrap(),
        "1\n"
    );
    assert_eq!(read_result(&file).unwrap(), "7 : Int\n");
    assert_eq!(std::fs::read(&file.0).unwrap(), "🌿é!".as_bytes());
    std::fs::write(&file.0, b"").unwrap();
    assert_eq!(read_result(&file).unwrap(), "0 : Int\n");
}
