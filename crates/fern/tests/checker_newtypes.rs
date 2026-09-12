use fern_compiler::{check, parse};
fn valid(source: &str) {
    check::check(&parse::parse(source).unwrap()).unwrap();
}
fn invalid(source: &str) {
    assert!(
        check::check(&parse::parse(source).unwrap()).is_err(),
        "{source}"
    );
}
#[test]
fn identities_require_explicit_construction_and_allow_unwrap_patterns() {
    valid(
        "newtype UserId = UserId(Int)\nnewtype ProductId = ProductId(Int)\nfn raw(UserId(value): UserId) -> Int: value\nfn main(): println(raw(UserId(4294967296)) + UserId(1).0)\n",
    );
    for body in [
        "take(ProductId(1))",
        "take(1)",
        "let value: Int = UserId(1)\n    value",
        "UserId(1).1",
        "UserId(1) + UserId(2)",
        "println(UserId(1))",
    ] {
        invalid(&format!(
            "newtype UserId = UserId(Int)\nnewtype ProductId = ProductId(Int)\nfn take(value: UserId) -> Int: value.0\nfn main():\n    {body}\n"
        ));
    }
}
#[test]
fn generic_nested_container_newtypes_keep_concrete_inference() {
    valid(
        "newtype Wrap(a) = Wrap(a)\nnewtype Items(a) = Items(List(Wrap(a)))\nfn unwrap(Wrap(value)): value\nfn main():\n    let values = Items([Wrap(1.25)])\n    let Items([first,.._]) = values else: return ()\n    println(unwrap(first))\n",
    );
}
#[test]
fn scalar_identity_equality_and_keys_are_nominal() {
    valid(
        "newtype UserId = UserId(Int)\nnewtype Key = Key(String)\nfn main():\n    println(UserId(42) == UserId(42))\n    let entries: Map(Key, Int) = %{Key(\"answer\"):42}\n    println(Option.unwrap_or(Map.get(entries, Key(\"answer\")), 0))\n",
    );
    invalid(
        "newtype Left = Left(Int)\nnewtype Right = Right(Int)\nfn main(): println(Left(1) == Right(1))\n",
    );
    invalid("newtype Key = Key(Float)\nfn main():\n    let entries: Map(Key, Int) = %{}\n    ()\n");
}
#[test]
fn unguarded_representation_cycles_fail_but_existing_heap_indirection_is_finite() {
    for source in [
        "newtype Loop = Loop(Loop)",
        "newtype Left = Left(Right)\nnewtype Right = Right(Left)",
        "newtype Grow(a) = Grow(Grow(List(a)))",
    ] {
        invalid(&format!("{source}\nfn main(): ()\n"));
    }
    valid("newtype Chain = Chain(List(Chain))\nfn main(): println(List.len(Chain([]).0))\n");
}
#[test]
fn wrapped_results_keep_handling_and_capture_obligations() {
    invalid(
        "newtype Outcome = Outcome(Result(Int,String))\nfn main():\n    let ignored = Outcome(Ok(42))\n    ()\n",
    );
    valid(
        "newtype Outcome = Outcome(Result(Int,String))\nfn main():\n    let Outcome(result) = Outcome(Ok(42))\n    println(Result.unwrap_or(result, 0))\n",
    );
}

#[test]
fn generic_intrinsic_requirements_follow_payloads_without_inheriting_arithmetic_or_show() {
    valid(
        "newtype Box(a) = Box(a)\nfn equal(a, b): a == b\nfn keyed(value): %{value: true}\nfn main():\n    println(equal(a: Box(1.25), b: Box(1.25)))\n    println(List.contains([Box(\"x\")], Box(\"x\")))\n    println(Map.len(keyed(Box(\"x\"))))\n    let nested: Box(Box(Int)) = Box(Box(7))\n    println(nested.0.0)\n",
    );
    for body in [
        "println(Box(1))",
        "Box(1) - Box(2)",
        "Box(1) < Box(2)",
        "Box(()) == Box(())",
        "Box([1]) == Box([1])",
        "List.contains([Box([1])], Box([1]))",
        "%{Box(1.25): true}",
        "\"value={Box(1)}\"",
    ] {
        invalid(&format!("newtype Box(a) = Box(a)\nfn main(): {body}\n"));
    }
}

#[test]
fn wrapped_json_is_opaque_and_cannot_gain_scalar_capabilities() {
    valid(
        "newtype Document = Document(json.Value)\nfn wrap(value: json.Value) -> Document: Document(value)\nfn raw(Document(value): Document) -> json.Value: value\nfn main():\n    let wrapped = wrap(json.null())\n    println(json.is_null(raw(wrapped)))\n",
    );
    for body in [
        "Document(json.null()) == Document(json.null())",
        "println(Document(json.null()))",
        "%{Document(json.null()): 1}",
    ] {
        invalid(&format!(
            "newtype Document = Document(json.Value)\nfn main(): {body}\n"
        ));
    }
}

#[test]
fn wrapped_results_cannot_hide_wildcard_discards_or_ordinary_closure_captures() {
    let prefix = "newtype Fallible = Fallible(Result(Int, String))\nfn main():\n    let wrapped = Fallible(Ok(1))\n";
    for body in [
        "let Fallible(_) = wrapped\n    ()",
        "match wrapped:\n        Fallible(_) -> ()",
        "let callback = () -> wrapped\n    callback()",
    ] {
        invalid(&format!("{prefix}    {body}\n"));
    }
    valid(
        "newtype Fallible = Fallible(Result(Int, String))\nfn main():\n    let wrapped = Fallible(Ok(1))\n    defer println(Result.unwrap_or(wrapped.0, 0))\n",
    );
}

#[test]
fn unused_invalid_declarations_and_wrong_arities_are_rejected() {
    for decl in [
        "newtype Bad(a, a) = Bad(a)",
        "newtype Bad(a) = Bad(b)",
        "newtype Int = Int(Int)",
        "newtype Bad = Some(Int)",
        "newtype Box(a) = Box(a)\nnewtype Bad = Bad(Box)",
        "newtype Box = Box(Int)\ntype Box = Int",
    ] {
        invalid(&format!("{decl}\nfn main(): ()\n"));
    }
    invalid("newtype Box = Box(Int)\nfn main(): Box()\n");
    invalid("newtype Box = Box(Int)\nfn main(): Box(1, 2)\n");
}

#[test]
fn unusable_key_types_fail_even_in_unused_payload_declarations() {
    invalid("newtype Key = Key(Float)\nnewtype Entries = Entries(Map(Key, Int))\nfn main(): ()\n");
    invalid(
        "type Record:\n    id: Int\nnewtype Entries = Entries(Map(Record, Int))\nfn main(): ()\n",
    );
    valid(
        "newtype Key(a) = Key(a)\nnewtype Entries(a) = Entries(Map(Key(a), Int))\nfn main(): println(Map.len(Entries(%{Key(\"x\"):1}).0))\n",
    );
}

#[test]
fn delayed_projection_evidence_and_explicit_generic_capabilities_preserve_identity() {
    valid(
        "newtype Wrap(a) = Wrap(a)\nfn equal(x: Wrap(a), y: Wrap(a)) -> Bool: x == y\nfn project(value):\n    let raw = value.0\n    let anchor: Wrap(Float) = value\n    raw\nfn main():\n    println(equal(x: Wrap(1.25), y: Wrap(1.25)))\n    println(equal(x: Wrap(\"x\"), y: Wrap(\"x\")))\n    println(project(Wrap(2.5)))\n",
    );
    invalid(
        "newtype Wrap(a) = Wrap(a)\nfn equal(x: Wrap(a), y: Wrap(a)) -> Bool: x == y\nfn main(): println(equal(x: Wrap([1]), y: Wrap([1])))\n",
    );
}

#[test]
fn source_and_representation_expansions_have_deterministic_limits() {
    let mut source = String::from("newtype Base = Base(Int)\n");
    for index in 0..100 {
        let previous = if index == 0 {
            "Base".into()
        } else {
            format!("Layer{}", index - 1)
        };
        source.push_str(&format!(
            "newtype Layer{index} = Layer{index}({previous})\n"
        ));
    }
    for index in 0..2500 {
        source.push_str(&format!("newtype Top{index} = Top{index}(Layer99)\n"));
    }
    source.push_str("fn main(): ()\n");
    let syntax = parse::parse(&source).unwrap();
    let error = match check::check(&syntax) {
        Err(error) => error,
        Ok(_) => panic!("resource stress input unexpectedly accepted"),
    };
    assert!(error.message.contains("work limit"), "{}", error.message);
    invalid("newtype Blow(a) = Blow(Blow((a, a)))\nfn main(): ()\n");
}

#[test]
fn constructors_are_first_class_with_independent_generic_instances() {
    valid(
        "newtype Wrap(a) = Packed(a)\nfn main():\n    let make: (Int) -> Wrap(Int) = Packed\n    println(make(4294967296).0)\n    let floats = List.map([1.25, 2.5], Packed)\n    println(List.head(floats).0)\n    let strings = List.map([\"x\"], Packed)\n    println(List.head(strings).0)\n",
    );
}

#[test]
fn scalar_capability_expansion_shares_the_declaration_work_budget() {
    let fields = vec!["Int"; 200].join(", ");
    let mut source = format!(
        "newtype Phantom(a) = Phantom(String)\nfn main():\n    let value: Phantom(({fields})) = Phantom(\"x\")\n    let values = [value]\n"
    );
    for _ in 0..3000 {
        source.push_str("    List.contains(values, value)\n");
    }
    let syntax = parse::parse(&source).unwrap();
    let error = match check::check(&syntax) {
        Err(error) => error,
        Ok(_) => panic!("resource stress input unexpectedly accepted"),
    };
    assert!(
        error.message.contains("newtype representation work limit"),
        "{}",
        error.message
    );
}

#[test]
fn unicode_capitalized_newtype_and_constructor_names_follow_source_identifiers() {
    let source = "newtype Δείκτης = Τύλιγμα(Int)\nfn unwrap(value:Δείκτης)->Int:value.0\nfn main():println(unwrap(Τύλιγμα(42)))\n";
    let syntax = fern_compiler::parse::parse(source).unwrap();
    assert!(fern_compiler::check::check(&syntax).is_ok());
}
