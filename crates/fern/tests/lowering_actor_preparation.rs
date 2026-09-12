use fern_compiler::{check, lowering, parse};

#[test]
fn non_actor_output_omits_only_actor_fault_support() {
    let program = check::check(&parse::parse("fn main(): 42").unwrap()).unwrap();
    let output = lowering::emit(&program).unwrap();
    assert!(!output.contains("data $fern_rs_fault_8"));
    assert!(!output.contains("data $fern_rs_fault_12"));
    for code in 1..=7 {
        assert!(output.contains(&format!("data $fern_rs_fault_{code} =")));
    }
    assert!(
        output.contains("@check_7\n    call $write(w 2, l $fern_rs_fault_7, l 61)\n    ret\n}")
    );
}

#[test]
fn actor_output_retains_all_actor_fault_mappings() {
    let source = "fn worker()->():\n    receive:\n        n -> println(n)\nfn main()->Result((),Int):\n    let pid:Pid(Int)=spawn(worker)\n    send(pid,42)?\n    Ok(())\n";
    let program = check::check(&parse::parse(source).unwrap()).unwrap();
    let output = lowering::emit(&program).unwrap();
    for code in 1..=12 {
        assert!(output.contains(&format!("data $fern_rs_fault_{code} =")));
        assert!(output.contains(&format!("call $write(w 2, l $fern_rs_fault_{code},")));
    }
}
