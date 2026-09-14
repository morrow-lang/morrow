use super::*;

#[test]
fn native_language_tour_composes_features_with_an_independent_output_oracle() {
    let source = include_str!("../../../../examples/language_tour.fn");
    let checked =
        morrow_compiler::check::check(&morrow_compiler::parse::parse(source).unwrap()).unwrap();
    let program = morrow_compiler::lowering::lower(&checked).unwrap();
    let harness = r#"unsafe extern "C" { fn morrow_main() -> i32; }
fn main() { assert_eq!(unsafe { morrow_main() }, 0); }"#;
    assert_eq!(NativeFixture::new().execute_linked(&program, harness, &[core_runtime_archive().into_os_string()]),
        b"compile-time batch size: 10\nunique targets: 2\n#9007199254740993: One language, both sides\nroundtrip equal: true\ntarget: native\ntarget: WASM\ncleanup complete\n");
}
