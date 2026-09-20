//! Pure, deterministic supervisor engine following OTP 29.0.6 `supervisor.erl`.
//!
//! The engine is owner-local safe Rust: it owns no processes, reads no clock
//! and performs no scheduling. A driver validates and starts a tree with
//! [`Engine::start`], performs the returned [`Action`]s, and reports child
//! acknowledgements, exits, deadlines, retries, clock readings, parent exits
//! and management requests through [`Engine::handle`].
//!
//! Semantics: children start in declaration order, each waiting for its
//! acknowledgement; a startup failure terminates started children in reverse
//! order before reporting `StartFailed`. Permanent children restart on every
//! exit, Transient children only on abnormal reasons, Temporary children never
//! and their specs are removed. One-for-one restarts the exited child,
//! one-for-all terminates every other running child in reverse order and
//! starts all remaining specs forward, rest-for-one does the same for children
//! declared after the exited one. Each automatic restart attempt is charged to
//! a rolling whole-second window before the strategy runs; a failed start
//! schedules a retry through the driver that is charged again and re-runs the
//! strategy for the failed child. Exceeding the intensity, or a natural exit
//! of a significant child under the configured auto-shutdown policy,
//! terminates all children in reverse order and retires with `Shutdown`.
//! Supervisor-induced exits are never restarted and never trigger auto
//! shutdown. Graceful termination sends a shutdown signal and arms a deadline
//! whose expiry kills the child; Infinity waits without escalation; Immediate
//! kills at once.
mod engine;
mod spec;
mod window;

pub use engine::{Action, Engine, Input, MAX_QUEUED_REQUESTS, Reply, Request, RequestId, Response};
pub use spec::{
    AutoShutdown, ChildInfo, ChildKind, ChildPolicy, ChildSpec, ChildState, Error, ExitReason,
    Flags, MAX_CHILDREN, MAX_DEPTH, MAX_GRACEFUL_MILLISECONDS, MAX_INTENSITY, MAX_NAME_BYTES,
    MAX_PERIOD_SECONDS, ProcessId, Restart, Shutdown, Strategy, Template, validate, validate_child,
    validate_flags, validate_spec,
};
pub use window::Window;

#[cfg(test)]
mod tests;
