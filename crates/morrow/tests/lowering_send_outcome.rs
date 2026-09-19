use morrow_compiler::{check, lowering, parse};

fn emit(source: &str) -> String {
    lowering::emit(&check::check(&parse::parse(source).unwrap()).unwrap()).unwrap()
}

#[test]
fn immediate_send_match_uses_one_scalar_outcome_without_result_accessors() {
    let native = emit(
        "fn worker(): ()\nfn main():\n    let pid: Pid(Int) = spawn(worker)\n    match send(pid, 9007199254740993):\n        Ok(_) -> ()\n        Err(code) -> println(code)\n",
    );
    assert_eq!(
        native.matches("call $morrow_managed_send_outcome(").count(),
        1
    );
    assert!(!native.contains("call $morrow_managed_send("));
    assert!(!native.contains("call $morrow_result_is_ok("));
    assert!(!native.contains("call $morrow_result_unwrap("));
}

#[test]
fn escaped_send_and_local_result_keep_the_boxed_abi() {
    let native = emit(
        "fn worker(): ()\nfn enqueue(pid: Pid(Int)) -> Result((), Int): send(pid, 7)\nfn main():\n    let pid: Pid(Int) = spawn(worker)\n    let result = enqueue(pid)\n    match result:\n        Ok(_) -> ()\n        Err(code) -> println(code)\n",
    );
    assert!(native.contains("call $morrow_managed_send("));
    assert!(!native.contains("call $morrow_managed_send_outcome("));
    assert!(native.contains("call $morrow_result_is_ok("));
}

#[test]
fn whole_result_pattern_binding_keeps_boxed_value_for_nested_consumers() {
    let native = emit(
        "fn worker(): ()\nfn main():\n    let pid: Pid(Int) = spawn(worker)\n    match send(pid, 7):\n        result ->\n            match result:\n                Ok(_) -> ()\n                Err(code) -> println(code)\n",
    );
    assert!(native.contains("call $morrow_managed_send("));
    assert!(!native.contains("call $morrow_managed_send_outcome("));
}

#[test]
fn immediate_enqueue_match_does_not_discharge_the_borrowed_message_result_duty() {
    let source = "fn worker(): ()\nfn main():\n    let pid: Pid(Result(Int, Int)) = spawn(worker)\n    let message: Result(Int, Int) = Err(7)\n    match send(pid, message):\n        Ok(_) -> ()\n        Err(_) -> ()\n";
    assert!(check::check(&parse::parse(source).unwrap()).is_err());
}

#[test]
fn public_ir_still_rejects_forged_send_and_match_patterns() {
    use morrow_compiler::{Type, ir::*};
    let source = "fn worker(): ()\nfn main():\n    let pid: Pid(Int) = spawn(worker)\n    match send(pid, 7):\n        Ok(_) -> ()\n        Err(code) -> println(code)\n";
    let checked = check::check(&parse::parse(source).unwrap()).unwrap();
    for case in 0..3 {
        let mut program = checked.clone();
        let main = program
            .functions
            .iter_mut()
            .find(|f| f.name == "main")
            .unwrap();
        let ExprKind::Block(statements) = &mut main.body.kind else {
            panic!("block")
        };
        let Stmt::Expr(Expr {
            kind: ExprKind::Match { value, arms },
            ..
        }) = statements.last_mut().unwrap()
        else {
            panic!("match")
        };
        match case {
            0 => value.ty = Type::Result(Box::new(Type::Unit), Box::new(Type::Float)),
            1 => {
                arms[0].pattern = Pattern::Variant {
                    tag: 99,
                    fields: vec![],
                }
            }
            _ => {
                let ExprKind::Actor(ActorExpr::Send { message, .. }) = &mut value.kind else {
                    panic!("send")
                };
                message.ty = Type::String;
            }
        }
        assert!(lowering::emit(&program).is_err(), "forged case {case}");
    }
}
