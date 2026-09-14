//! Replay source-level actor programs independently of native memory and real time.
pub use morrow_compiler::repl::{ActorReplay as Report, ActorReport as SchedulerReport};

/// Run bounded interactive source entries with captured output and prohibited host I/O.
/// Definitions and dormant actors survive between entries; failed entries do not bind values.
pub fn run(entries: &[&str]) -> Result<Report, String> {
    morrow_compiler::repl::simulate_actors(entries)
}

/// Require the entire transcript and final scheduler state to reproduce exactly.
pub fn replay(entries: &[&str], expected: &Report) -> Result<Report, String> {
    let actual = run(entries)?;
    if actual != *expected {
        return Err(
            "source actor replay differs from its expected transcript or scheduler state".into(),
        );
    }
    Ok(actual)
}
