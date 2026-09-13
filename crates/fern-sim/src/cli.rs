use fern_runtime::managed::simulation as actors;
use fern_sim::{Config, Report};
use serde::{Deserialize, Serialize};
use std::os::unix::fs::OpenOptionsExt;
use std::{
    collections::BTreeSet,
    fs::OpenOptions,
    io::{Read, Write},
    path::PathBuf,
};

const REPLAY_BYTES: u64 = 2 * 1024 * 1024;
const HELP: &str = "Fern deterministic simulation\n\n\
fern-sim [--seed N] [--steps N] [--days N] [--json]\n\
fern-sim --actors [--seed N] [--steps N] [--json]\n\
fern-sim --replay report.json [--json]\n\n\
Application knobs: --clients N --rooms N --delay-ms N --trace-limit N\n\
  --delay-per-mille N --drop-per-mille N --duplicate-per-mille N\n\
  --disconnect-per-mille N --restart-per-mille N --ephemeral\n\n\
Seeds accept decimal or 0x hexadecimal. Probabilities are 0..=1000.\n\
Application mode runs the production Hub, Client and compiled Fern actors.\n\
Virtual time advances without sleeps. Checkpoint filesystem I/O is real.\n\
Reported simulated days are scenario time, not equivalent production coverage.\n\
Save --json output to replay it; replay checks the complete versioned report.\n\
Application failures are JSON on stderr and can also be replayed; failure replay exits 1.\n";

struct Options {
    config: Config,
    actors: bool,
    json: bool,
    replay: Option<PathBuf>,
    help: bool,
}
fn number(text: &str) -> Result<u64, String> {
    let (digits, radix) = text
        .strip_prefix("0x")
        .map_or((text, 10), |text| (text, 16));
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii() && c.is_digit(radix)) {
        return Err(format!("invalid unsigned integer: {text}"));
    }
    u64::from_str_radix(digits, radix).map_err(|_| format!("integer exceeds u64: {text}"))
}
fn parse(arguments: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut options = Options {
        config: Config::default(),
        actors: false,
        json: false,
        replay: None,
        help: false,
    };
    let mut seen = BTreeSet::new();
    let mut args = arguments;
    while let Some(mut flag) = args.next() {
        if flag == "actor-run" && seen.is_empty() {
            flag = "--actors".into();
        }
        if !seen.insert(flag.clone()) {
            return Err(format!("duplicate argument: {flag}"));
        }
        match flag.as_str() {
            "--help" | "-h" => options.help = true,
            "--actors" => options.actors = true,
            "--json" => options.json = true,
            "--ephemeral" => options.config.durable = false,
            "--replay" => {
                options.replay = Some(PathBuf::from(
                    args.next().ok_or("--replay requires a path")?,
                ))
            }
            "--seed"
            | "--steps"
            | "--days"
            | "--clients"
            | "--rooms"
            | "--delay-ms"
            | "--trace-limit"
            | "--delay-per-mille"
            | "--drop-per-mille"
            | "--duplicate-per-mille"
            | "--disconnect-per-mille"
            | "--restart-per-mille" => {
                let value = number(
                    &args
                        .next()
                        .ok_or_else(|| format!("{flag} requires a value"))?,
                )?;
                let too_large = || format!("value too large for {flag}");
                match flag.as_str() {
                    "--seed" => options.config.seed = value,
                    "--steps" => {
                        options.config.steps = value.try_into().map_err(|_| too_large())?
                    }
                    "--days" => {
                        options.config.duration_ms =
                            value.checked_mul(86_400_000).ok_or_else(too_large)?
                    }
                    "--clients" => {
                        options.config.clients = value.try_into().map_err(|_| too_large())?
                    }
                    "--rooms" => {
                        options.config.rooms = value.try_into().map_err(|_| too_large())?
                    }
                    "--delay-ms" => {
                        options.config.max_delay_ms = value.try_into().map_err(|_| too_large())?
                    }
                    "--trace-limit" => {
                        options.config.trace_limit = value.try_into().map_err(|_| too_large())?
                    }
                    _ => {
                        let value: u16 = value.try_into().map_err(|_| too_large())?;
                        match flag.as_str() {
                            "--delay-per-mille" => options.config.faults.delay_per_mille = value,
                            "--drop-per-mille" => options.config.faults.drop_per_mille = value,
                            "--duplicate-per-mille" => {
                                options.config.faults.duplicate_per_mille = value
                            }
                            "--disconnect-per-mille" => {
                                options.config.faults.disconnect_per_mille = value
                            }
                            "--restart-per-mille" => {
                                options.config.faults.restart_per_mille = value
                            }
                            _ => unreachable!(),
                        }
                    }
                }
            }
            _ => return Err(format!("unknown argument: {flag}")),
        }
    }
    if options.help {
        if seen.len() != 1 {
            return Err("--help cannot accompany other arguments".into());
        }
        return Ok(options);
    }
    if options.replay.is_some()
        && seen
            .iter()
            .any(|flag| !matches!(flag.as_str(), "--json" | "--replay"))
    {
        return Err("--replay cannot override a recorded simulation configuration".into());
    }
    if options.actors {
        if seen
            .iter()
            .any(|flag| !matches!(flag.as_str(), "--actors" | "--seed" | "--steps" | "--json"))
        {
            return Err("actor mode accepts only --seed, --steps and --json".into());
        }
        if !(1..=actors::MAX_STEPS).contains(&options.config.steps) {
            return Err("actor steps outside supported range".into());
        }
    } else {
        options.config.validate()?;
    }
    Ok(options)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActorConfig {
    seed: u64,
    steps: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActorReport {
    simulator_kind: String,
    simulator_version: u32,
    config: ActorConfig,
    virtual_ms: u64,
    callbacks: u64,
    delivered: u64,
    timeouts: u64,
    restarts: u64,
    churn: u64,
    trace_hash: u64,
    final_live: usize,
    final_messages: usize,
    final_heap_bytes: usize,
    final_heap_objects: usize,
}
fn actor_run(config: ActorConfig) -> Result<ActorReport, String> {
    let report = actors::run(actors::Config {
        seed: config.seed,
        steps: config.steps,
    })
    .map_err(|failure| {
        format!(
            "{failure}; replay with --actors --seed {} --steps {}",
            config.seed, config.steps
        )
    })?;
    Ok(ActorReport {
        simulator_kind: "actors".into(),
        simulator_version: report.version,
        config,
        virtual_ms: report.virtual_ms,
        callbacks: report.callbacks,
        delivered: report.delivered,
        timeouts: report.timeouts,
        restarts: report.restarts,
        churn: report.churn,
        trace_hash: report.trace_hash,
        final_live: report.final_live,
        final_messages: report.final_messages,
        final_heap_bytes: report.final_heap_bytes,
        final_heap_objects: report.final_heap_objects,
    })
}
fn actor_text(report: &ActorReport) -> String {
    format!(
        "Actor simulation passed: seed {}, {} steps, {} virtual ms\n{} callbacks; {} delivered; {} timeouts; {} restarts; {} churn\nCleanup: {} live actors, {} messages, {} heap bytes, {} heap objects\nTrace hash: {:016x}\nVirtual duration describes this scenario, not equivalent production coverage.\n",
        report.config.seed,
        report.config.steps,
        report.virtual_ms,
        report.callbacks,
        report.delivered,
        report.timeouts,
        report.restarts,
        report.churn,
        report.final_live,
        report.final_messages,
        report.final_heap_bytes,
        report.final_heap_objects,
        report.trace_hash
    )
}
fn app_text(report: &Report) -> String {
    format!(
        "Application simulation passed: seed {}, {} decisions, {} virtual ms\n{} events; {} applied; {} checked snapshots; {} durable room recoveries\nFaults: {} drops, {} duplicates, {} delays, {} disconnects, {} server restarts\nHealthy phase: {} room commits and all clients converged\nTrace SHA-256: {}\nFinal state SHA-256: {}\nVirtual duration describes this scenario, not equivalent production coverage.\n",
        report.config.seed,
        report.config.steps,
        report.virtual_duration_ms,
        report.counts.processed_events,
        report.counts.applied,
        report.counts.snapshots_checked,
        report.counts.recovered_rooms,
        report.counts.drops,
        report.counts.duplicates,
        report.counts.delays,
        report.counts.disconnects,
        report.counts.restarts,
        report.counts.healing_commits,
        report.trace_digest,
        report.final_state_digest
    )
}
fn json(value: &impl Serialize) -> Result<String, String> {
    serde_json::to_string(value)
        .map(|text| format!("{text}\n"))
        .map_err(|e| e.to_string())
}

pub fn run(arguments: impl Iterator<Item = String>) -> Result<(), String> {
    let options = parse(arguments)?;
    let output = if options.help {
        HELP.into()
    } else if let Some(path) = options.replay {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&path)
            .map_err(|e| format!("cannot open replay: {e}"))?;
        if !file.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err("replay must be a regular file".into());
        }
        let mut bytes = Vec::new();
        file.take(REPLAY_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > REPLAY_BYTES {
            return Err("replay exceeds 2 MiB".into());
        }
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if value.get("simulator_kind").is_some() {
            let expected: ActorReport =
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if expected.simulator_kind != "actors" || expected.simulator_version != actors::VERSION
            {
                return Err("unsupported actor replay format".into());
            }
            let actual = actor_run(expected.config.clone())?;
            if actual != expected {
                return Err("actor replay report differs from deterministic execution".into());
            }
            if options.json {
                json(&actual)?
            } else {
                actor_text(&actual)
            }
        } else if value.get("message").is_some() {
            let expected: fern_sim::Failure =
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            let actual = fern_sim::replay_failure(&expected)
                .map_err(|e| json(&e).unwrap_or_else(|_| e.to_string()))?;
            return Err(if options.json {
                json(&actual)?
            } else {
                format!("Reproduced failure: {}", json(&actual)?)
            });
        } else {
            let expected: Report = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            let actual = fern_sim::replay(&expected)
                .map_err(|e| json(&e).unwrap_or_else(|_| e.to_string()))?;
            if options.json {
                json(&actual)?
            } else {
                app_text(&actual)
            }
        }
    } else if options.actors {
        let report = actor_run(ActorConfig {
            seed: options.config.seed,
            steps: options.config.steps,
        })?;
        if options.json {
            json(&report)?
        } else {
            actor_text(&report)
        }
    } else {
        let report = fern_sim::run(options.config)
            .map_err(|e| json(&e).unwrap_or_else(|_| e.to_string()))?;
        if options.json {
            json(&report)?
        } else {
            app_text(&report)
        }
    };
    std::io::stdout()
        .lock()
        .write_all(output.as_bytes())
        .map_err(|e| e.to_string())
}
