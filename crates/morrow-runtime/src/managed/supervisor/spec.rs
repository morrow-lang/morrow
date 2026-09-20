//! Supervisor configuration types, canonical tags and structural validation.
//!
//! Tag orders are frozen for the later ABI layer; new variants must append.
use std::collections::BTreeSet;

/// Highest accepted restart intensity (restarts allowed inside one period).
pub const MAX_INTENSITY: u32 = 1024;
/// Highest accepted rolling window in whole seconds.
pub const MAX_PERIOD_SECONDS: u32 = 86_400;
/// Most direct child specifications one supervisor may hold.
pub const MAX_CHILDREN: usize = 1024;
/// Deepest accepted nesting of branch supervisors; the root counts as depth 1.
pub const MAX_DEPTH: usize = 64;
/// Longest accepted child name in UTF-8 bytes.
pub const MAX_NAME_BYTES: usize = 4096;
/// Longest accepted finite graceful shutdown deadline in milliseconds.
pub const MAX_GRACEFUL_MILLISECONDS: u32 = 600_000;

/// Driver-supplied opaque process identity for one child generation.
pub type ProcessId = u64;

/// Why a process or supervisor stopped. Tags 0..=7 are the ABI order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExitReason {
    Normal,
    Shutdown,
    ShutdownDetail(String),
    Fault(i64),
    Failure(String),
    Kill,
    Killed,
    NoProcess,
}

impl ExitReason {
    /// Canonical ABI tag.
    pub const fn tag(&self) -> i64 {
        match self {
            Self::Normal => 0,
            Self::Shutdown => 1,
            Self::ShutdownDetail(_) => 2,
            Self::Fault(_) => 3,
            Self::Failure(_) => 4,
            Self::Kill => 5,
            Self::Killed => 6,
            Self::NoProcess => 7,
        }
    }

    /// Normal, Shutdown and ShutdownDetail are non-abnormal for transient policy.
    pub const fn is_abnormal(&self) -> bool {
        !matches!(
            self,
            Self::Normal | Self::Shutdown | Self::ShutdownDetail(_)
        )
    }
}

/// Which siblings a child's restart affects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strategy {
    OneForOne,
    OneForAll,
    RestForOne,
}

/// When a child is restarted after it exits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Restart {
    Permanent,
    Transient,
    Temporary,
}

/// How the supervisor terminates a running child.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shutdown {
    /// Send a shutdown signal, then kill after this many milliseconds.
    Graceful(u32),
    /// Send a shutdown signal and wait without escalation.
    Infinity,
    /// Kill without a shutdown signal.
    Immediate,
}

/// Whether natural exits of significant children retire the supervisor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoShutdown {
    Never,
    AnySignificant,
    AllSignificant,
}

/// Supervisor-wide options.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Flags {
    pub strategy: Strategy,
    pub intensity: u32,
    pub period_seconds: u32,
    pub auto_shutdown: AutoShutdown,
}

impl Default for Flags {
    /// OTP defaults: one-for-one, one restart in five seconds, no auto shutdown.
    fn default() -> Self {
        Self {
            strategy: Strategy::OneForOne,
            intensity: 1,
            period_seconds: 5,
            auto_shutdown: AutoShutdown::Never,
        }
    }
}

/// Per-child restart, termination and significance policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChildPolicy {
    pub restart: Restart,
    pub shutdown: Shutdown,
    pub significant: bool,
}

impl ChildPolicy {
    /// Worker default: Permanent, Graceful(5000), not significant.
    pub const fn worker() -> Self {
        Self {
            restart: Restart::Permanent,
            shutdown: Shutdown::Graceful(5000),
            significant: false,
        }
    }

    /// Branch default: Permanent, Infinity, not significant.
    pub const fn branch() -> Self {
        Self {
            restart: Restart::Permanent,
            shutdown: Shutdown::Infinity,
            significant: false,
        }
    }
}

/// Worker or nested supervisor. Tags 0..=1 are the ABI order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChildKind {
    Worker,
    Branch,
}

impl ChildKind {
    /// Canonical ABI tag.
    pub const fn tag(self) -> i64 {
        match self {
            Self::Worker => 0,
            Self::Branch => 1,
        }
    }
}

/// Observable child state. Tags 0..=2 are the ABI order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChildState {
    Running(ProcessId),
    Stopped,
    Restarting,
}

impl ChildState {
    /// Canonical ABI tag.
    pub const fn tag(self) -> i64 {
        match self {
            Self::Running(_) => 0,
            Self::Stopped => 1,
            Self::Restarting => 2,
        }
    }
}

/// One `which_children` row in declaration order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChildInfo {
    pub name: String,
    pub kind: ChildKind,
    pub state: ChildState,
}

/// Supervisor errors. Tags 0..=12 are the ABI order; some tags belong to the
/// compiler and ABI layers and are never produced by the engine itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidOptions,
    ResourceLimit,
    ForeignInvocation,
    UnsupportedContext,
    WrongChildKey,
    DuplicateName,
    AlreadyRunning,
    AlreadyPresent,
    Restarting,
    Stopped,
    Removed,
    StartFailed(String, ExitReason),
    SupervisorStopped,
}

impl Error {
    /// Canonical ABI tag.
    pub const fn tag(&self) -> i64 {
        match self {
            Self::InvalidOptions => 0,
            Self::ResourceLimit => 1,
            Self::ForeignInvocation => 2,
            Self::UnsupportedContext => 3,
            Self::WrongChildKey => 4,
            Self::DuplicateName => 5,
            Self::AlreadyRunning => 6,
            Self::AlreadyPresent => 7,
            Self::Restarting => 8,
            Self::Stopped => 9,
            Self::Removed => 10,
            Self::StartFailed(..) => 11,
            Self::SupervisorStopped => 12,
        }
    }
}

/// Opaque start template the driver uses to create each child generation.
///
/// Branch templates may expose their nested definition so validation can
/// check nesting depth, child counts and names before any child starts.
pub trait Template: Clone {
    /// Nested supervisor definition carried by a branch template, if any.
    fn nested(&self) -> Option<(&Flags, &[ChildSpec<Self>])> {
        None
    }
}

impl Template for u64 {}
impl Template for () {}

/// Static description of one child: unique name, kind, policy and template.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChildSpec<T> {
    pub name: String,
    pub kind: ChildKind,
    pub policy: ChildPolicy,
    pub template: T,
}

impl<T> ChildSpec<T> {
    /// A worker with the default worker policy.
    pub fn worker(name: impl Into<String>, template: T) -> Self {
        Self {
            name: name.into(),
            kind: ChildKind::Worker,
            policy: ChildPolicy::worker(),
            template,
        }
    }

    /// A branch supervisor with the default branch policy.
    pub fn branch(name: impl Into<String>, template: T) -> Self {
        Self {
            name: name.into(),
            kind: ChildKind::Branch,
            policy: ChildPolicy::branch(),
            template,
        }
    }

    pub fn with_policy(mut self, policy: ChildPolicy) -> Self {
        self.policy = policy;
        self
    }

    pub fn with_restart(mut self, restart: Restart) -> Self {
        self.policy.restart = restart;
        self
    }

    pub fn with_shutdown(mut self, shutdown: Shutdown) -> Self {
        self.policy.shutdown = shutdown;
        self
    }

    pub fn significant(mut self, significant: bool) -> Self {
        self.policy.significant = significant;
        self
    }
}

/// Validate a complete supervisor tree before any child starts.
///
/// Checks intensity, period, child count, nesting depth, name length and
/// uniqueness, graceful deadlines and the OTP significant-child rules at
/// every level. Returns `InvalidOptions` or `DuplicateName`.
pub fn validate<T: Template>(flags: &Flags, children: &[ChildSpec<T>]) -> Result<(), Error> {
    validate_tree(flags, children, 1)
}

/// Validate one supervisor level. Recursion is bounded by `MAX_DEPTH`
/// because deeper trees are rejected before descending further.
fn validate_tree<T: Template>(
    flags: &Flags,
    children: &[ChildSpec<T>],
    depth: usize,
) -> Result<(), Error> {
    if depth > MAX_DEPTH {
        return Err(Error::InvalidOptions);
    }
    validate_flags(flags)?;
    if children.len() > MAX_CHILDREN {
        return Err(Error::InvalidOptions);
    }
    let mut names = BTreeSet::new();
    for spec in children {
        validate_spec(flags, spec)?;
        if !names.insert(spec.name.as_str()) {
            return Err(Error::DuplicateName);
        }
    }
    for spec in children {
        validate_child(flags, spec, depth)?;
    }
    Ok(())
}

/// Validate one child and any nested definition it carries. `depth` is the
/// nesting depth of the supervisor that owns the child; the root is 1.
pub fn validate_child<T: Template>(
    flags: &Flags,
    spec: &ChildSpec<T>,
    depth: usize,
) -> Result<(), Error> {
    validate_spec(flags, spec)?;
    if let Some((nested_flags, nested_children)) = spec.template.nested() {
        validate_tree(nested_flags, nested_children, depth.saturating_add(1))?;
    }
    Ok(())
}

/// Validate supervisor-wide options.
pub fn validate_flags(flags: &Flags) -> Result<(), Error> {
    if flags.intensity > MAX_INTENSITY
        || flags.period_seconds == 0
        || flags.period_seconds > MAX_PERIOD_SECONDS
    {
        return Err(Error::InvalidOptions);
    }
    Ok(())
}

/// Validate one child specification against its supervisor's flags.
pub fn validate_spec<T>(flags: &Flags, spec: &ChildSpec<T>) -> Result<(), Error> {
    if spec.name.len() > MAX_NAME_BYTES {
        return Err(Error::InvalidOptions);
    }
    if let Shutdown::Graceful(milliseconds) = spec.policy.shutdown
        && milliseconds > MAX_GRACEFUL_MILLISECONDS
    {
        return Err(Error::InvalidOptions);
    }
    if spec.policy.significant
        && (spec.policy.restart == Restart::Permanent || flags.auto_shutdown == AutoShutdown::Never)
    {
        return Err(Error::InvalidOptions);
    }
    Ok(())
}
