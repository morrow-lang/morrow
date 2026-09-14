use fern_message_path::{Case, Codec, run};
use serde::Serialize;
use std::{
    io::Write,
    process::{Command, ExitCode},
};

#[derive(Serialize)]
struct Batch {
    batch: usize,
    order: usize,
    report: fern_message_path::Report,
}
#[derive(Serialize)]
struct Results {
    experiment: &'static str,
    source_revision: String,
    source_status: String,
    rustc: String,
    platform: String,
    executable_bytes: u64,
    profile: &'static str,
    repetitions: usize,
    ephemeral_operations: usize,
    durable_operations: usize,
    batches: Vec<Batch>,
}
fn command(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_else(|| "unavailable".into())
}
fn bounded(text: Option<&String>, default: usize, maximum: usize) -> Result<usize, String> {
    let value = text
        .map(|text| {
            if text.len() > 10 {
                return Err("argument too long".to_string());
            }
            text.parse::<usize>()
                .map_err(|_| "expected positive integer".to_string())
        })
        .transpose()?
        .unwrap_or(default);
    if value == 0 || value > maximum {
        return Err(format!("argument must be 1..={maximum}"));
    }
    Ok(value)
}
fn measure() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).take(4).collect();
    if args.len() > 3 {
        return Err("usage: fern-message-path [ephemeral-ops durable-ops batches]".into());
    }
    let ephemeral = bounded(args.first(), 150, 2000)?;
    let durable = bounded(args.get(1), 20, 2000)?;
    let repetitions = bounded(args.get(2), 5, 21)?;
    let mut results = Results {
        experiment: "native-fern-actors-over-loopback-websocket-v1",
        source_revision: command("git", &["rev-parse", "HEAD"]),
        source_status: command("git", &["status", "--short"]),
        rustc: command("rustc", &["--version"]),
        platform: command("uname", &["-srm"]),
        executable_bytes: std::env::current_exe()
            .and_then(std::fs::metadata)
            .map_err(|e| e.to_string())?
            .len(),
        profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release; thin LTO; one codegen unit"
        },
        repetitions,
        ephemeral_operations: ephemeral,
        durable_operations: durable,
        batches: Vec::with_capacity(repetitions * 18),
    };
    // Untimed runtime/codec/socket warmup with an independently checked transition.
    for codec in [Codec::Json, Codec::Cbor, Codec::Protobuf] {
        run(Case {
            codec,
            durable: false,
            tasks: 3,
            operations: 3,
        })?;
    }
    for batch in 0..repetitions {
        for (mode, persistent) in [false, true].into_iter().enumerate() {
            for (size, tasks) in [1, 25, 100].into_iter().enumerate() {
                for order in 0..3 {
                    let codec = [Codec::Json, Codec::Cbor, Codec::Protobuf]
                        [(batch + mode + size + order) % 3];
                    eprintln!(
                        "batch {}/{repetitions}: {codec:?}, durable={persistent}, tasks={tasks}",
                        batch + 1
                    );
                    let report = run(Case {
                        codec,
                        durable: persistent,
                        tasks,
                        operations: if persistent { durable } else { ephemeral },
                    })?;
                    results.batches.push(Batch {
                        batch,
                        order,
                        report,
                    });
                }
            }
        }
    }
    let mut stdout = std::io::BufWriter::new(std::io::stdout().lock());
    serde_json::to_writer_pretty(&mut stdout, &results).map_err(|e| e.to_string())?;
    stdout.write_all(b"\n").map_err(|e| e.to_string())
}
fn main() -> ExitCode {
    match measure() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("message-path: {error}");
            ExitCode::FAILURE
        }
    }
}
