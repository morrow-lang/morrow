use fern_compiler::repl::Session;
#[test]
fn unsupported_actors_are_rejected_before_prior_file_effects() {
    let directory = std::env::temp_dir().join(format!("fern-actors-repl-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("must-not-exist");
    let source = format!(
        "match File.write(\"{}\", \"bad\"):\n    Ok(_) -> ()\n    Err(_) -> ()\nlet pid: Pid(()) = spawn(() -> ())",
        path.display()
    );
    let error = Session::default().evaluate(&source).unwrap_err();
    let side_effect = path.exists();
    if side_effect {
        std::fs::remove_file(&path).unwrap();
    }
    std::fs::remove_dir(&directory).unwrap();
    assert!(!side_effect, "actor refusal must precede external effects");
    assert!(error.contains("unsupported in the REPL"), "{error}");
}

#[test]
fn unsupported_definition_is_not_retained() {
    let mut session = Session::default();
    let error = session
        .evaluate("fn worker():\n    receive:\n        1 -> ()")
        .unwrap_err();
    assert!(error.contains("unsupported in the REPL"), "{error}");
    assert!(session.evaluate("worker()").is_err());
}
