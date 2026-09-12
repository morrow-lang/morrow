use fern_compiler::{Span, Type, ir, lowering, runtime};

fn emit(name: &str, arguments: Vec<&str>) -> String {
    let id = runtime::resolve(name).expect("registered tuple API");
    let args = arguments
        .into_iter()
        .map(|value| ir::Expr {
            kind: ir::ExprKind::String(value.into()),
            ty: Type::String,
            span: Span::default(),
        })
        .collect();
    let body = ir::Expr {
        kind: ir::ExprKind::Call {
            target: ir::CallTarget::Runtime(id),
            args,
        },
        ty: runtime::signature(id).unwrap().return_type,
        span: Span::default(),
    };
    lowering::emit(&ir::Program {
        types: vec![],
        functions: vec![ir::Function {
            mailbox: None,
            captures: vec![],
            id: ir::FunctionId(0),
            name: "main".into(),
            params: vec![],
            return_type: Type::Unit,
            body,
            local_count: 0,
        }],
    })
    .unwrap()
}

#[test]
fn process_and_terminal_records_are_copied_into_tagged_rust_tuples() {
    for (name, args, bytes) in [
        ("System.exec", vec!["/usr/bin/printf safe"], 32),
        ("Tui.Term.size", vec![], 24),
    ] {
        let il = emit(name, args);
        assert!(il.contains(&format!("call $fern_alloc(l {bytes})")), "{il}");
        assert!(il.contains("storel 0, %"), "Rust tuple tag: {il}");
        assert!(il.contains("hlt"), "null pointer guard: {il}");
        assert!(il.contains("=l loadl %"), "full width native fields: {il}");
    }
}

#[test]
fn absent_regex_match_branches_before_nullable_text_is_loaded() {
    let il = emit("Regex.find", vec!["abc", "missing"]);
    assert!(il.contains("=w csgel %"), "signed absence sentinel: {il}");
    assert!(il.contains("call $fern_result_ok(l %"), "{il}");
    assert!(il.contains("call $fern_result_err(l 0)"), "{il}");
    assert!(il.contains("call $fern_alloc(l 32)"), "match tuple: {il}");
    assert!(!il.contains("fern_option_"));
}

#[test]
fn regex_capture_records_are_copied_with_a_bounded_native_stride() {
    let il = emit("Regex.captures", vec!["abc123", "([a-z]+)([0-9]+)"]);
    assert!(il.contains("call $fern_regex_captures("), "{il}");
    assert!(
        il.contains("=l mul %") && il.contains(", 24"),
        "native record stride: {il}"
    );
    assert!(il.contains("=w csltl %"), "count bounded loop: {il}");
    assert!(il.contains("call $fern_list_push_mut(l %"), "{il}");
    assert!(il.contains("=l phi"), "loop SSA: {il}");
}

#[test]
fn directory_results_copy_success_lists_and_preserve_error_pointers() {
    let il = emit("fs.list_dir", vec!["directory"]);
    assert!(il.contains("call $fern_read_dir_result("), "{il}");
    assert!(il.contains("call $fern_result_is_ok(l %"), "{il}");
    assert!(il.contains("call $fern_list_push_mut(l %"), "{il}");
    assert!(il.contains("call $fern_result_ok(l %"), "{il}");
    assert!(
        !il.contains("call $fern_result_err("),
        "original errors must survive: {il}"
    );
    assert!(
        il.contains("=l phi"),
        "success and original error merge: {il}"
    );
    assert_eq!(
        runtime::resolve("fs.list_dir"),
        runtime::resolve("File.list_dir")
    );
}
