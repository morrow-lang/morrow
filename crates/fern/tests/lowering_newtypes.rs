use fern_compiler::{Type, check, lowering, parse};
fn checked(source: &str) -> fern_compiler::ir::Program {
    check::check(&parse::parse(source).unwrap()).unwrap()
}
#[test]
fn scalar_wrapping_and_projection_add_no_allocation_or_truncation() {
    let program = checked(
        "newtype Id = Id(Int)\nfn identity(value: Id) -> Id: value\nfn main() -> Int: if identity(Id(4294967296)).0 == 4294967296: 0 else: 1\n",
    );
    let il = lowering::emit(&program).unwrap();
    assert!(!il.contains("call $fern_alloc("), "{il}");
    assert!(il.contains("4294967296"), "{il}");
}
#[test]
fn floating_newtypes_keep_the_double_calling_convention() {
    let program = checked(
        "newtype Amount = Amount(Float)\nfn identity(value: Amount) -> Amount: value\nfn main() -> Int: if identity(Amount(1.25)).0 == 1.25: 0 else: 1\n",
    );
    let il = lowering::emit(&program).unwrap();
    assert!(il.contains("function d"), "{il}");
    assert!(!il.contains("call $fern_alloc("), "{il}");
}
#[test]
fn public_ir_rejects_missing_or_inconsistent_newtype_layouts() {
    let mut program = checked("newtype Id = Id(Int)\nfn main(): println(Id(42).0)\n");
    let mut missing = program.clone();
    missing.types.clear();
    assert!(lowering::emit(&missing).is_err());
    program.types[0].variants[0][0] = Type::Float;
    assert!(lowering::emit(&program).is_err());
}

#[test]
fn wrapping_a_container_adds_no_heap_allocation_to_its_existing_representation() {
    let plain = lowering::emit(&checked("fn main(): println(List.len([1,2]))\n")).unwrap();
    let wrapped = lowering::emit(&checked(
        "newtype Items = Items(List(Int))\nfn main(): println(List.len(Items([1,2]).0))\n",
    ))
    .unwrap();
    assert_eq!(
        wrapped.matches("call $fern_alloc(").count(),
        plain.matches("call $fern_alloc(").count()
    );
    assert_eq!(
        wrapped.matches("call $fern_list_new(").count(),
        plain.matches("call $fern_list_new(").count()
    );
}

#[test]
fn scalar_collection_equality_uses_underlying_semantics_without_erasing_identity() {
    for source in [
        "newtype Key = Key(String)\nfn main():\n    let map = %{Key(\"a\"): 7}\n    println(Map.contains(map, Key(\"a\")))\n    println(List.contains([Key(\"a\")], Key(\"a\")))\n",
        "newtype Amount = Amount(Float)\nfn main(): println(List.contains([Amount(1.5)], Amount(1.5)))\n",
    ] {
        let il = lowering::emit(&checked(source)).unwrap();
        assert!(
            il.contains("contains_str") || il.contains("contains_float"),
            "{il}"
        );
    }
}

#[test]
fn hostile_unboxed_cycles_tagged_constructors_and_extra_fields_are_rejected() {
    use fern_compiler::ir::{Expr, ExprKind, FunctionId, LayoutStorage};
    let original = checked("newtype Id = Id(Int)\nfn main(): println(Id(42).0)\n");
    let mut cycle = original.clone();
    cycle.types[0].variants[0][0] = cycle.types[0].ty.clone();
    assert!(lowering::emit(&cycle).is_err());
    for storage in [LayoutStorage::Tagged, LayoutStorage::Unboxed] {
        let mut invalid = original.clone();
        invalid.types[0].storage = storage;
        invalid.types[0].variants[0].push(Type::Int);
        assert!(lowering::emit(&invalid).is_err());
    }
    let mut forged = original;
    let mut helper = forged.functions[0].clone();
    helper.id = FunctionId(1);
    helper.name = "forged".into();
    helper.return_type = forged.types[0].ty.clone();
    helper.local_count = 0;
    helper.body = Expr {
        ty: helper.return_type.clone(),
        span: Default::default(),
        kind: ExprKind::CustomConstruct {
            tag: 0,
            fields: vec![Expr {
                ty: Type::Int,
                span: Default::default(),
                kind: ExprKind::Int(42),
            }],
        },
    };
    forged.functions.push(helper);
    assert!(lowering::emit(&forged).is_err());
}

#[test]
fn collection_arguments_with_missing_nominal_layouts_return_diagnostics() {
    use fern_compiler::ir::{Builtin, CallTarget, Expr, ExprKind};
    let mut program = checked("fn main(): 0\n");
    let unknown = Type::Named("Missing".into(), vec![]);
    let argument = Expr {
        ty: unknown.clone(),
        span: Default::default(),
        kind: ExprKind::Int(0),
    };
    program.functions[0].body = Expr {
        ty: Type::Bool,
        span: Default::default(),
        kind: ExprKind::Call {
            target: CallTarget::Builtin(Builtin::ListContains),
            args: vec![
                Expr {
                    ty: Type::List(Box::new(unknown)),
                    span: Default::default(),
                    kind: ExprKind::List(vec![]),
                },
                argument,
            ],
        },
    };
    assert!(lowering::emit(&program).is_err());
}

#[test]
fn pattern_validation_never_confuses_unboxed_and_tagged_layouts() {
    use fern_compiler::ir::{Expr, ExprKind, LayoutStorage, MatchArm, Pattern};
    let original = checked("newtype Id = Id(Int)\nfn main(): 0\n");
    for (storage, pattern) in [
        (
            LayoutStorage::Unboxed,
            Pattern::Variant {
                tag: 0,
                fields: vec![Pattern::Wildcard],
            },
        ),
        (
            LayoutStorage::Tagged,
            Pattern::Newtype(Box::new(Pattern::Wildcard)),
        ),
        (
            LayoutStorage::Unboxed,
            Pattern::Newtype(Box::new(Pattern::Bool(true))),
        ),
    ] {
        let mut program = original.clone();
        // Unused declarations have no emitted layout; retain a concrete checked instance.
        program.types = checked("newtype Id = Id(Int)\nfn main(): println(Id(1).0)\n").types;
        program.types[0].storage = storage;
        let integer = Expr {
            kind: ExprKind::Int(1),
            ty: Type::Int,
            span: Default::default(),
        };
        let subject = Expr {
            ty: program.types[0].ty.clone(),
            span: Default::default(),
            kind: if storage == LayoutStorage::Unboxed {
                ExprKind::Wrap(Box::new(integer.clone()))
            } else {
                ExprKind::CustomConstruct {
                    tag: 0,
                    fields: vec![integer.clone()],
                }
            },
        };
        program.functions[0].body = Expr {
            ty: Type::Int,
            span: Default::default(),
            kind: ExprKind::Match {
                value: Box::new(subject),
                arms: vec![MatchArm {
                    span: Default::default(),
                    pattern,
                    guard: None,
                    body: integer,
                }],
            },
        };
        assert!(lowering::emit(&program).is_err());
    }
}
