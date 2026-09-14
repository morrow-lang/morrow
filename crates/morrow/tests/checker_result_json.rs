use morrow_compiler::{check, parse, repl::Session};
fn checked(source: &str) -> Result<(), String> {
    check::check_library(&parse::parse(source).unwrap())
        .map(|_| ())
        .map_err(|e| e.message)
}
fn rejected(source: &str) {
    let error = checked(source).expect_err(source);
    assert!(error.contains("Result obligation"), "{error}");
}
#[test]
fn concrete_and_generic_codec_outputs_create_independent_duties() {
    checked("fn encode(value):json.encode(value)\nfn main():println(Result.is_ok(encode(1)))\n")
        .unwrap();
    rejected(
        "fn encode(value):json.encode(value)\nfn main():\n    let r=encode(1)\n    println(List.len([r]))\n",
    );
    rejected(
        "fn unused(value):\n    let r=json.encode(value)\n    println(List.len([r]))\nfn main():()\n",
    );
    rejected(
        "fn main():\n    let first=json.decode(\"1\",Int)\n    let second=json.decode(\"2\",Int)\n    println(Result.is_ok(first))\n    println(List.len([second]))\n",
    );
}
#[test]
fn codec_inputs_keep_real_borrowing_effects_and_propagation_exits() {
    let helper = "fn text(r:Result(Int,String))->String:\n    println(List.len([r]))\n    \"1\"\n";
    rejected(&format!(
        "{helper}fn main():\n    let r:Result(Int,String)=Err(\"lost\")\n    println(Result.is_ok(json.decode(text(r),Int)))\n"
    ));
    checked(&format!("{helper}fn main():\n    let r:Result(Int,String)=Err(\"handled\")\n    println(Result.is_ok(json.decode(text(r),Int)))\n    println(Result.is_err(r))\n")).unwrap();
    rejected(
        "fn main()->Result(Unit,json.Error):\n    let earlier:Result(Int,String)=Err(\"lost\")\n    println(json.decode(\"1\",Int)?)\n    println(Result.is_err(earlier))\n    Ok(())\n",
    );
}
#[test]
fn recursive_codec_payloads_and_phantom_arguments_do_not_create_stored_debt() {
    checked("type Node derive(Json):\n    children:List(Node)\nfn main():println(Result.is_ok(json.decode(\"\\{\\\"children\\\":[]\\}\",Node)))\n").unwrap();
    checked("type Phantom(a) derive(Json):\n    value:Int\nfn encode(value):json.encode(value)\nfn main():\n    let value:Phantom(Result(Int,String))=Phantom(1)\n    println(Result.is_ok(encode(value)))\n    let callback:Phantom(()->Int)=Phantom(2)\n    println(Result.is_ok(encode(callback)))\n").unwrap();
}
#[test]
fn higher_order_codec_calls_do_not_drop_generated_results() {
    checked("fn encode(value):json.encode(value)\nfn main():\n    let results=List.map([1,2],encode)\n    for result in results:println(Result.is_ok(result))\n").unwrap();
    rejected(
        "fn encode(value):json.encode(value)\nfn main():\n    let results=List.map([1,2],encode)\n    println(List.len(results))\n",
    );
}
#[test]
fn failed_codec_obligations_do_not_publish_repl_bindings() {
    let mut session = Session::default();
    assert!(
        session
            .evaluate("let lost=json.decode(\"1\",Int)\nprintln(List.len([lost]))")
            .is_err()
    );
    assert!(session.evaluate("Result.is_ok(lost)").is_err());
    session
        .evaluate("Result.is_ok(json.decode(\"1\",Int))")
        .unwrap();
}
#[test]
fn recursive_codec_templates_preserve_input_responsibilities_without_self_credit() {
    checked("fn left(value:a,n:Int)->Result(String,json.Error):if n>0:right(value,n-1) else:json.encode(value)\nfn right(value:a,n:Int)->Result(String,json.Error):left(value,n)\nfn main():println(Result.is_ok(right(1,2)))\n").unwrap();
    rejected(
        "fn inspect(value:a,n:Int)->Int:\n    println(List.len([value]))\n    if n>0:inspect(value,n-1) else:0\nfn main():\n    let r:Result(Int,String)=Err(\"lost\")\n    println(inspect(r,2))\n",
    );
}
#[test]
fn propagating_one_codec_error_cannot_abandon_other_produced_results() {
    rejected(
        "fn encode(value):json.encode(value)\nfn main()->Result(Unit,json.Error):\n    let outputs=List.map([1,2],encode)\n    for output in outputs:println(output?)\n    Ok(())\n",
    );
    checked("fn encode(value):json.encode(value)\nfn main()->Result(Unit,json.Error):\n    let outputs=List.map([1,2],encode)\n    for output in outputs:\n        match output:\n            Ok(text)->println(text)\n            Err(error)->println(json.error_message(error))\n    Ok(())\n").unwrap();
}
