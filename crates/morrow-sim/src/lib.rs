//! Deterministic protocol, actor and durable-recovery simulation.
#![forbid(unsafe_code)]
mod engine;
mod oracle;
mod support;
mod types;
pub use types::*;

/// Runs the real protocol and native Morrow actors under a bounded virtual event loop.
/// Checkpoint I/O uses the local filesystem; the driver injects transport and restart faults.
pub fn run(config: Config) -> Result<Report, Failure> {
    let mut engine = engine::Engine::new(config.clone()).map_err(|message| Failure {
        simulator_version: SIMULATOR_VERSION,
        config,
        event: 0,
        time_ms: 0,
        message,
        trace: vec![],
    })?;
    engine.execute().map_err(|message| engine.failure(message))
}

fn replay_error(config: &Config, message: &str) -> Failure {
    Failure {
        simulator_version: SIMULATOR_VERSION,
        config: config.clone(),
        event: 0,
        time_ms: 0,
        message: message.into(),
        trace: vec![],
    }
}

/// Re-executes the recorded configuration and compares every report field.
/// Digests detect divergence; they do not authenticate an untrusted report.
pub fn replay(report: &Report) -> Result<Report, Failure> {
    if report.simulator_version != SIMULATOR_VERSION
        || report.trace.len() > MAX_TRACE as usize
        || report.config_digest != support::digest(&report.config)
    {
        return Err(replay_error(
            &report.config,
            "unsupported or inconsistent replay format",
        ));
    }
    let actual = run(report.config.clone())?;
    if &actual == report {
        return Ok(actual);
    }
    Err(replay_error(
        &report.config,
        "replay report does not match the deterministic execution",
    ))
}

/// A matching failure is a successful reproduction, not a passing simulation.
/// CLI callers preserve the failing exit status and original failure report.
pub fn replay_failure(expected: &Failure) -> Result<Failure, Failure> {
    if expected.simulator_version != SIMULATOR_VERSION || expected.trace.len() > MAX_TRACE as usize
    {
        return Err(replay_error(
            &expected.config,
            "unsupported failure replay format",
        ));
    }
    match run(expected.config.clone()) {
        Err(actual) if actual == *expected => Ok(actual),
        _ => Err(replay_error(
            &expected.config,
            "recorded failure did not reproduce exactly",
        )),
    }
}

/// Source-level actor replay using the safe interpreter and shared typed continuations.
pub mod language;
