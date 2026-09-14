use morrow_compiler::{ir, json_codec::Kind, Type};
use morrow_phase_benchmarks::{codec_plan, fixtures};
#[test]
fn fixtures_are_prechecked_and_preserve_their_distinct_workloads() {
    let cases = fixtures();
    assert_eq!(
        cases.iter().map(|f| f.name).collect::<Vec<_>>(),
        ["scalar", "generic_scc", "json_record"]
    );
    for fixture in &cases {
        assert!(!fixture.source.is_empty());
        let main = fixture
            .program
            .functions
            .iter()
            .find(|f| f.name == "main")
            .unwrap();
        assert_eq!(main.return_type, Type::Unit);
        assert!(fixture.lowering.contains("export function w $morrow_main()"));
        assert!(!fixture.lowering.contains("json_codec_") || fixture.name == "json_record");
    }
    assert!(
        cases[1].program.functions.len() >= 5,
        "generic calls need distinct concrete instances"
    );
}
#[test]
fn codec_proof_benchmark_uses_actual_checked_graph_and_storage_metadata() {
    let cases = fixtures();
    let fixture = &cases[2];
    let plan = codec_plan(&fixture.program);
    let Kind::Record(fields) = &plan.entries[plan.root].kind else {
        panic!("actual record plan")
    };
    assert_eq!(
        fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
        ["name", "age", "tags"]
    );
    assert!(fixture
        .program
        .types
        .iter()
        .any(|layout| layout.storage == ir::LayoutStorage::Tagged));
    plan.validate(&fixture.program.types, morrow_compiler::Span::default())
        .unwrap();
}

#[test]
fn fixture_source_behavior_is_verified_before_benchmarking_compiler_work() {
    for (fixture, expected) in fixtures().into_iter().zip(["42\n", "42\nMorrow\n", ""]) {
        let mut session = morrow_compiler::repl::Session::default();
        session
            .evaluate(&fixture.source.replace("fn main(", "fn benchmark_entry("))
            .unwrap();
        assert_eq!(
            session.evaluate("benchmark_entry()").unwrap(),
            expected,
            "{}",
            fixture.name
        );
    }
}
