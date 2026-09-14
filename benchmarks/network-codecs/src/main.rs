use fern_network_codecs::{
    Codec, Message, corpus, decode, decode_prepared, encode, encode_prepared, schema,
};
use serde_json::json;
use std::{hint::black_box, time::Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    let iterations = arguments
        .first()
        .map(|s| s.parse::<usize>())
        .transpose()?
        .unwrap_or(100);
    let repeats = arguments
        .get(1)
        .map(|s| s.parse::<usize>())
        .transpose()?
        .unwrap_or(9);
    if !(1..=10_000).contains(&iterations) || !(3..=101).contains(&repeats) {
        return Err("iterations1..10000,repeats3..101".into());
    }
    let mut rows = Vec::new();
    for (index, (name, message)) in corpus::fixtures().into_iter().enumerate() {
        let prepared = schema::Wire::from(&message);
        let mut codecs = [Codec::Json, Codec::Cbor, Codec::Protobuf];
        codecs.rotate_left(index % 3);
        for codec in codecs {
            let bytes = encode(codec, &message)?;
            assert_eq!(decode(codec, &bytes, message.is_client())?, message);
            let mut phases = serde_json::Map::new();
            phases.insert(
                "encode_actual_message_ns".into(),
                measure(iterations, repeats, || {
                    black_box(encode(codec, black_box(&message)).unwrap())
                }),
            );
            phases.insert(
                "decode_validate_actual_message_ns".into(),
                measure(iterations, repeats, || {
                    black_box(decode(codec, black_box(&bytes), message.is_client()).unwrap())
                }),
            );
            if !matches!(codec, Codec::Json) {
                phases.insert(
                    "encode_prepared_wire_ns".into(),
                    measure(iterations, repeats, || {
                        black_box(encode_prepared(codec, black_box(&prepared)).unwrap())
                    }),
                );
                phases.insert(
                    "decode_validate_prepared_wire_ns".into(),
                    measure(iterations, repeats, || {
                        black_box(decode_prepared(codec, black_box(&bytes)).unwrap())
                    }),
                );
                phases.insert(
                    "adapt_actual_to_wire_ns".into(),
                    measure(iterations, repeats, || {
                        black_box(schema::Wire::from(black_box(&message)))
                    }),
                );
                phases.insert(
                    "clone_wire_adapt_validate_actual_ns".into(),
                    measure(iterations, repeats, || {
                        black_box(Message::try_from(black_box(prepared.clone())).unwrap())
                    }),
                );
            }
            rows.push(json!({"fixture":name,"codec":format!("{codec:?}"),"bytes":bytes.len(),"measurements":phases}));
        }
    }
    let rustc = std::process::Command::new("rustc")
        .arg("--version")
        .output()?;
    let output = json!({"format_version":1,"corpus_version":"2026-09-14","unix_seconds":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs(),"kind":"native_codec_microbenchmark","iterations_per_sample":iterations,"samples":repeats,"warmup_iterations":20,"seed":0,"codec_order":"deterministic rotation by corpus index","compiler":String::from_utf8_lossy(&rustc.stdout).trim(),"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"timing":"nanoseconds per operation derived from repeated batch means; output destruction included; no per-message p99 claim","debug_assertions":cfg!(debug_assertions),"documented_release_profile":"release,thin LTO,codegen-units1","fixture_count":rows.len()/3,"rows":rows});
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}
fn measure<T>(
    iterations: usize,
    repeats: usize,
    mut action: impl FnMut() -> T,
) -> serde_json::Value {
    for _ in 0..20 {
        black_box(action());
    }
    let mut raw = Vec::with_capacity(repeats);
    for _ in 0..repeats {
        let started = Instant::now();
        for _ in 0..iterations {
            black_box(action());
        }
        raw.push(started.elapsed().as_nanos() as f64 / iterations as f64);
    }
    let mut ordered = raw.clone();
    ordered.sort_by(f64::total_cmp);
    json!({"median":ordered[repeats/2],"min":ordered[0],"max":ordered[repeats-1],"batch_mean_samples":raw})
}
