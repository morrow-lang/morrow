use morrow_compiler::repl::Session;

fn session() -> Session {
    let mut s = Session::default();
    s.evaluate("type AuthFailure:\n    AuthError(message: String)")
        .unwrap();
    s.evaluate("type LoadFailure:\n    LoadError(code: Int)")
        .unwrap();
    s.evaluate("fn auth(ok: Bool) -> Result(Int, AuthFailure):\n    println(\"auth\")\n    if ok: Ok(7) else: Err(AuthError(\"denied\"))").unwrap();
    s.evaluate("fn load(id: Int, ok: Bool) -> Result(String, LoadFailure):\n    println(id)\n    if ok: Ok(\"done\") else: Err(LoadError(5))").unwrap();
    s
}

#[test]
fn with_uses_typed_handlers_for_distinct_error_types() {
    let mut s = session();
    s.evaluate("fn execute(auth_ok: Bool, load_ok: Bool) -> String:\n    with\n        id <- auth(ok: auth_ok),\n        text <- load(id, ok: load_ok)\n    do\n        text\n    else\n        Err(AuthError(message)) -> message\n        Err(LoadError(code)) -> \"failed {code}\"").unwrap();
    assert_eq!(
        s.evaluate("execute(auth_ok: true, load_ok: true)").unwrap(),
        "auth\n7\n\"done\" : String\n"
    );
    assert_eq!(
        s.evaluate("execute(auth_ok: false, load_ok: true)")
            .unwrap(),
        "auth\n\"denied\" : String\n"
    );
    assert_eq!(
        s.evaluate("execute(auth_ok: true, load_ok: false)")
            .unwrap(),
        "auth\n7\n\"failed 5\" : String\n"
    );
}

#[test]
fn with_handler_returns_use_the_enclosing_function_cleanup() {
    let mut s = session();
    s.evaluate("fn execute(ok: Bool) -> Int:\n    defer println(\"cleanup\")\n    with\n        id <- auth(ok: ok)\n    do\n        id\n    else\n        Err(_) -> return 9").unwrap();
    assert_eq!(
        s.evaluate("execute(ok: false)").unwrap(),
        "auth\ncleanup\n9 : Int\n"
    );
    assert_eq!(
        s.evaluate("execute(ok: true)").unwrap(),
        "auth\ncleanup\n7 : Int\n"
    );
}

#[test]
fn with_without_else_propagates_errors_and_skips_success_effects() {
    let mut s = Session::default();
    s.evaluate("fn failure() -> Result(Int, String): Err(\"bad\")")
        .unwrap();
    s.evaluate("fn execute() -> Result(Int, String):\n    defer println(\"cleanup\")\n    with\n        value <- failure()\n    do\n        println(\"unreachable\")\n        Ok(value)").unwrap();
    assert_eq!(
        s.evaluate("match execute():\n    Ok(_) -> \"ok\"\n    Err(error) -> error")
            .unwrap(),
        "cleanup\n\"bad\" : String\n"
    );
}
