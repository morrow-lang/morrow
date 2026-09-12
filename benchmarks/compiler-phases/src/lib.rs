//! Prepared fixtures keep parsing, checking, emission and proof timing boundaries explicit.
use fern_compiler::{ast, check, ir, json_codec::Plan, lowering, parse};

/// One independently checked input and its reusable outputs for subsequent phases.
pub struct Fixture {
    pub name: &'static str,
    pub source: String,
    pub ast: ast::Program,
    pub program: ir::Program,
    pub lowering: String,
}

/// Prepare every fixture outside measured loops; invalid or changed inputs fail immediately.
pub fn fixtures() -> Vec<Fixture> {
    let mut scalar = String::new();
    for index in 0..64 {
        scalar.push_str(&format!("fn add{index}(value:Int)->Int:value + {index}\n"));
    }
    scalar.push_str("fn main():println(add0(42))\n");
    let generic = "fn identity(x):x\nfn first(x):second(identity(x))\nfn second(x):\n    if true:\n        x\n    else:\n        first(x)\nfn main():\n    println(first(42))\n    println(first(\"Fern\"))\n";
    let json = "type User derive(Json):\n    name:String\n    age:Int\n    tags:List(String)\nfn encode(user:User)->Result(String,json.Error):json.encode(user)\nfn main():()\n";
    [
        ("scalar", scalar),
        ("generic_scc", generic.into()),
        ("json_record", json.into()),
    ]
    .into_iter()
    .map(|(name, source)| {
        let ast = parse::parse(&source).expect("benchmark source must parse");
        let program = check::check(&ast).expect("benchmark source must check");
        let lowering = lowering::emit(&program).expect("benchmark IR must emit");
        Fixture {
            name,
            source,
            ast,
            program,
            lowering,
        }
    })
    .collect()
}

/// Borrow the actual checked codec graph; no synthetic plan represents the proof workload.
pub fn codec_plan(program: &ir::Program) -> &Plan {
    let function = program
        .functions
        .iter()
        .find(|f| f.name == "encode")
        .expect("JSON fixture has an encode function");
    let ir::ExprKind::JsonCodec { plan, .. } = &function.body.kind else {
        panic!("JSON fixture must retain its real codec operation")
    };
    plan
}
