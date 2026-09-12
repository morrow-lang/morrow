use fern_compiler::{check, parse};
#[test]
fn concrete_codecs_check_and_retain_single_executable_inputs() {
    for source in [
        "fn decode() -> Result(Int,json.Error): json.decode(\"42\",Int)\nfn main(): match decode():\n    Ok(n) -> println(n)\n    Err(e) -> println(json.error_code(e))\n",
        "type User derive(Json):\n    age:Int\nfn main(): match json.encode(User(42)):\n    Ok(text) -> println(text)\n    Err(e) -> println(json.error_code(e))\n",
        "type Count = Int\nfn main(): match json.decode(\"42\",Count):\n    Ok(n) -> println(n)\n    Err(e) -> println(json.error_code(e))\n",
    ] {
        let ast = parse::parse(source).unwrap();
        check::check(&ast).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    }
}
#[test]
fn payload_obligations_and_ambiguous_optional_targets_fail() {
    for source in [
        "fn main(): json.encode(Ok(1))\n",
        "fn main(): json.decode(\"null\",Option(Unit))\n",
        "fn main(): json.decode(\"42\",Int)\n",
    ] {
        assert!(check::check(&parse::parse(source).unwrap()).is_err());
    }
}
