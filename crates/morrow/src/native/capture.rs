//! Source tests share the repository's bounded retained-child process adapter.
pub use morrow_test_supervisor::Captured;
use std::{path::Path, process::Command, time::Duration};

/// Execute native tests with bounded output and retained-child cleanup.
pub fn run(executable: &Path, timeout: Duration) -> Result<Captured, String> {
    let helper = super::component("MORROW_TEST_SUPERVISOR", "morrow-test-supervisor")?;
    morrow_test_supervisor::capture(Command::new(executable), Command::new(helper), timeout)
}
