//! Bounded fixed-membership routing and forwarding-stream state, independent of async IO.
//! No transition replays mutations or changes room ownership after a peer fails.
mod config;
mod identity;
mod state;
pub use config::{Config, Hello, Member, PROTOCOL_VERSION};
pub use identity::{BootId, ClusterId, LinkId, ManifestId, NodeId, RequestId};
pub use state::{Limits, LinkState, Loss, LossReason, Usage};

/// Stable failures from configuration, admission or a stream transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidIdentity,
    InvalidConfig,
    DuplicateNode,
    DuplicateCertificate,
    UnknownNode,
    SelfConnection,
    VersionMismatch,
    ClusterMismatch,
    ManifestMismatch,
    InvalidRoom,
    InvalidLimits,
    InvalidLease,
    TimeRegression,
    TimeOverflow,
    Overloaded,
    Disconnected,
    StaleLink,
    UnknownRequest,
    Exhausted,
    LeaseExpired,
    RequestExpired,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
mod framing;
mod peer_codec;
mod tls;
mod transport;
mod wire;
pub use framing::{FrameReader, FrameWriter, IoLimits};
pub use tls::{ALPN, Security};
pub use transport::{PeerReader, PeerStream, PeerWriter, accept, connect};
pub use wire::{Frame, MAX_LEASE_MS, MAX_PEER_FRAME_BYTES};

mod provision;
mod provision_files;
pub use provision::{NodeSettings, Provisioned, ProvisionedNode, provision};
