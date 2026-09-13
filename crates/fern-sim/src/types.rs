use serde::{Deserialize, Serialize};

pub const SIMULATOR_VERSION: u32 = 1;
pub const MAX_TRACE: u32 = 4096;
pub const MAX_STEPS: u32 = 100_000;

/// Probabilities per thousand, sampled independently at documented boundaries.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Faults {
    pub delay_per_mille: u16,
    pub drop_per_mille: u16,
    pub duplicate_per_mille: u16,
    pub disconnect_per_mille: u16,
    pub restart_per_mille: u16,
}
impl Default for Faults {
    fn default() -> Self {
        Self {
            delay_per_mille: 500,
            drop_per_mille: 80,
            duplicate_per_mille: 100,
            disconnect_per_mille: 40,
            restart_per_mille: 10,
        }
    }
}
impl Faults {
    pub fn none() -> Self {
        Self {
            delay_per_mille: 0,
            drop_per_mille: 0,
            duplicate_per_mille: 0,
            disconnect_per_mille: 0,
            restart_per_mille: 0,
        }
    }
}

/// Work and retained state stay bounded independently of the virtual timespan.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub seed: u64,
    pub steps: u32,
    pub duration_ms: u64,
    pub clients: u16,
    pub rooms: u16,
    pub max_delay_ms: u32,
    pub faults: Faults,
    pub durable: bool,
    pub trace_limit: u32,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            seed: 1,
            steps: 1000,
            duration_ms: 600_000,
            clients: 4,
            rooms: 2,
            max_delay_ms: 5000,
            faults: Faults::default(),
            durable: true,
            trace_limit: 128,
        }
    }
}
impl Config {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=MAX_STEPS).contains(&self.steps)
            || self.duration_ms == 0
            || self.duration_ms > u64::MAX / 4
            || !(1..=16).contains(&self.clients)
            || !(1..=8).contains(&self.rooms)
            || self.rooms > self.clients
            || self.max_delay_ms > 86_400_000
            || self.trace_limit > MAX_TRACE
        {
            return Err("invalid simulation bounds".into());
        }
        let f = &self.faults;
        if [
            f.delay_per_mille,
            f.drop_per_mille,
            f.duplicate_per_mille,
            f.disconnect_per_mille,
            f.restart_per_mille,
        ]
        .iter()
        .any(|&p| p > 1000)
        {
            return Err("fault probabilities must be within 0..=1000".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Counts {
    pub processed_events: u64,
    pub commands: u64,
    pub applied: u64,
    pub conflicts: u64,
    pub drops: u64,
    pub duplicates: u64,
    pub delays: u64,
    pub disconnects: u64,
    pub reconnects: u64,
    pub restarts: u64,
    pub expired_namespaces: u64,
    pub stale_frames: u64,
    pub snapshots_checked: u64,
    pub recovered_rooms: u64,
    pub healing_commits: u64,
    pub max_queued_events: usize,
    pub overload_drops: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceEvent {
    pub index: u64,
    pub time_ms: u64,
    pub kind: String,
    pub client: Option<u16>,
    pub detail: String,
}

/// Contains no wall-clock measurements, filesystem names or native addresses.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub simulator_version: u32,
    pub config: Config,
    pub config_digest: String,
    pub virtual_duration_ms: u64,
    pub counts: Counts,
    pub trace_events: u64,
    pub trace_omitted: u64,
    pub trace: Vec<TraceEvent>,
    pub trace_digest: String,
    pub final_state_digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Failure {
    pub simulator_version: u32,
    pub config: Config,
    pub event: u64,
    pub time_ms: u64,
    pub message: String,
    pub trace: Vec<TraceEvent>,
}
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "seed {} at event {} / virtual {} ms: {}",
            self.config.seed, self.event, self.time_ms, self.message
        )
    }
}
impl std::error::Error for Failure {}
