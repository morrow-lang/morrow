//! Scheduler thread configuration.
//!
//! The scheduler count is deployment configuration, never language surface: it
//! appears in no Morrow source and in no compiler-emitted symbol. An embedder
//! sets it explicitly, a deployment sets it through the environment, and
//! otherwise the runtime asks the machine.
use std::sync::atomic::{AtomicUsize, Ordering};

/// Zero means "not configured", so an embedder can still call `configure`
/// before the first invocation and have it take effect.
static CONFIGURED: AtomicUsize = AtomicUsize::new(0);

/// How many scheduler threads an invocation runs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeOptions {
    pub schedulers: usize,
}

impl RuntimeOptions {
    /// Resolve a count from an explicit environment value.
    ///
    /// A nonsense value falls back rather than producing zero schedulers: a
    /// runtime with no scheduler would hang rather than report a bad setting,
    /// which is a far worse failure than ignoring it.
    pub fn resolve(environment: Option<&str>) -> Self {
        let configured = CONFIGURED.load(Ordering::Acquire);
        if configured != 0 {
            return Self {
                schedulers: configured,
            };
        }
        let schedulers = environment
            .and_then(|value| value.trim().parse::<usize>().ok())
            .filter(|&count| (1..=MAX).contains(&count))
            .unwrap_or_else(available);
        Self { schedulers }
    }

    /// The count for this process, from `MORROW_SCHEDULERS` or the machine.
    pub fn from_environment() -> Self {
        Self::resolve(std::env::var("MORROW_SCHEDULERS").ok().as_deref())
    }
}

/// An upper bound on an external input, like every other limit in the runtime.
const MAX: usize = 1024;

fn available() -> usize {
    std::thread::available_parallelism()
        .map(|count| count.get().min(MAX))
        .unwrap_or(1)
}

/// Fix the scheduler count for this process, overriding the environment.
///
/// Intended for embedders that own their threading policy. Call before the
/// first invocation; later calls do not disturb one already running.
pub fn configure(options: RuntimeOptions) {
    CONFIGURED.store(options.schedulers.clamp(1, MAX), Ordering::Release);
}

#[cfg(test)]
mod tests {
    use super::*;

    // CONFIGURED is process-wide and the test harness runs tests in parallel,
    // so resolution and override are one test rather than two that would race
    // on it and pass most of the time.
    #[test]
    fn configuration_resolves_an_explicit_setting_then_the_environment_then_the_machine() {
        assert_eq!(RuntimeOptions::resolve(Some("4")).schedulers, 4);
        assert_eq!(
            RuntimeOptions::resolve(Some("0")).schedulers,
            available(),
            "a nonsense count falls back rather than producing zero schedulers"
        );
        assert_eq!(
            RuntimeOptions::resolve(Some("not a number")).schedulers,
            available()
        );
        assert!(RuntimeOptions::resolve(None).schedulers >= 1);
        assert!(RuntimeOptions::resolve(Some("999999")).schedulers <= MAX);

        configure(RuntimeOptions { schedulers: 3 });
        assert_eq!(
            RuntimeOptions::resolve(Some("8")).schedulers,
            3,
            "an embedder's explicit policy outranks the environment"
        );
        configure(RuntimeOptions { schedulers: 0 });
        assert_eq!(
            RuntimeOptions::resolve(Some("8")).schedulers,
            1,
            "a clamped configuration is still a configuration"
        );
        CONFIGURED.store(0, Ordering::Release);
        assert_eq!(RuntimeOptions::resolve(Some("8")).schedulers, 8);
    }
}
