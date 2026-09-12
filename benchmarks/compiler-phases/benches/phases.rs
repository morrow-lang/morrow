use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use fern_compiler::{check, lowering, parse, Span};
use fern_phase_benchmarks::{codec_plan, fixtures};
use std::{hint::black_box, time::Duration};

fn phases(c: &mut Criterion) {
    let fixtures = fixtures();
    for fixture in &fixtures {
        let mut group = c.benchmark_group(fixture.name);
        group.throughput(Throughput::Bytes(fixture.source.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("parse", "source_bytes"),
            &fixture.source,
            |b, source| {
                b.iter(|| black_box(parse::parse(black_box(source)).expect("preverified parse")))
            },
        );
        group.bench_with_input(
            BenchmarkId::new("check", "parsed_ast"),
            &fixture.ast,
            |b, ast| b.iter(|| black_box(check::check(black_box(ast)).expect("preverified check"))),
        );
        group.bench_with_input(
            BenchmarkId::new("lowering", "checked_ir"),
            &fixture.program,
            |b, program| {
                b.iter(|| {
                    black_box(lowering::emit(black_box(program)).expect("preverified emission"))
                })
            },
        );
        group.finish();
    }
    let json = fixtures
        .iter()
        .find(|f| f.name == "json_record")
        .expect("JSON fixture");
    let plan = codec_plan(&json.program);
    c.bench_function("json_record/codec_proof/checked_plan", |b| {
        b.iter(|| {
            black_box(plan.validate(black_box(&json.program.types), Span::default()))
                .expect("preverified codec proof");
        })
    });
}
criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(50)
        .warm_up_time(Duration::from_millis(500)).measurement_time(Duration::from_secs(2));
    targets = phases
}
criterion_main!(benches);
