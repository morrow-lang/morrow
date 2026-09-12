use fern_compiler::{
    Type, check, ir,
    json_codec::{Entry, Kind, Plan},
    lowering, parse,
};
use std::rc::Rc;
fn program() -> ir::Program {
    check::check(&parse::parse("fn first()->Result(String,json.Error):json.encode(1)\nfn second()->Result(String,json.Error):json.encode(2)\nfn main():()\n").unwrap()).unwrap()
}
#[test]
fn shared_concrete_codec_table_is_emitted_once() {
    let text = lowering::emit(&program()).unwrap();
    assert_eq!(text.matches("data $json_codec_").count(), 1, "{text}");
    assert_eq!(text.matches("call $fern_json_codec_encode").count(), 2);
}
#[test]
fn separate_public_plans_share_the_program_validation_allowance() {
    let mut program = program();
    let template = program.functions[0].clone();
    program.functions.retain(|f| f.name == "main");
    let entry = Entry {
        ty: Type::Tuple(vec![Type::Int; 300]),
        kind: Kind::Tuple(vec![0; 300]),
    };
    let mut plan = Plan {
        root: 0,
        entries: vec![Entry {
            ty: Type::Int,
            kind: Kind::Int,
        }],
    };
    plan.entries.extend(vec![entry; 100]);
    for i in 0..5 {
        let mut f = template.clone();
        f.id = ir::FunctionId(i + 100);
        f.name = format!("copy{i}");
        let ir::ExprKind::JsonCodec { plan: target, .. } = &mut f.body.kind else {
            panic!("codec body")
        };
        *target = Rc::new(plan.clone());
        program.functions.push(f);
    }
    let error = lowering::emit(&program).unwrap_err();
    assert!(error.message.contains("work limit"), "{error:?}");
}
#[test]
fn public_record_wire_names_cannot_contain_nul() {
    let mut program=check::check(&parse::parse("type Box derive(Json):\n    value:Int\nfn encode()->Result(String,json.Error):json.encode(Box(1))\nfn main():()\n").unwrap()).unwrap();
    let ir::ExprKind::JsonCodec { plan, .. } = &mut program.functions[0].body.kind else {
        panic!("codec")
    };
    let plan = Rc::make_mut(plan);
    let Kind::Record(fields) = &mut plan.entries[plan.root].kind else {
        panic!("record")
    };
    fields[0].name = "value\0other".into();
    program.types[0].fields[0] = fields[0].name.clone();
    assert!(lowering::emit(&program).is_err());
}
#[test]
fn inactive_union_overlap_is_rejected_before_any_qbe_table_is_emitted() {
    let mut program = program();
    let function = program
        .functions
        .iter_mut()
        .find(|f| f.name == "first")
        .unwrap();
    let ir::ExprKind::JsonCodec { plan, .. } = &mut function.body.kind else {
        panic!("codec")
    };
    let plan = Rc::make_mut(plan);
    assert_eq!(plan.entries[0].ty, Type::Int);
    plan.entries.push(Entry {
        ty: Type::Float,
        kind: Kind::Float,
    });
    let mut members = vec![Type::Int, Type::Float];
    members.sort();
    plan.entries.push(Entry {
        ty: Type::Union(members),
        kind: Kind::Union(vec![0, 1]),
    });
    assert!(
        lowering::emit(&program)
            .unwrap_err()
            .message
            .contains("not provably disjoint")
    );
}
