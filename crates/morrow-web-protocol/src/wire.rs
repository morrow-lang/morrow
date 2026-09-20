use serde::{Deserialize, Deserializer, Serialize, Serializer, de::DeserializeOwned};
use std::fmt;

/// Wire version; unsupported versions require an explicit upgrade response.
pub const VERSION: u8 = 1;
/// Hard envelope cap, checked before decoding or publishing output.
pub const MAX_FRAME_BYTES: usize = 65_536;
/// Maximum UTF-8 label and local draft size for the preview.
pub const MAX_LABEL_BYTES: usize = 256;

/// Full-width Morrow integer represented by a canonical JSON decimal string.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Decimal(pub i64);
impl Serialize for Decimal {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}
impl<'de> Deserialize<'de> for Decimal {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        let value = text.parse::<i64>().map_err(serde::de::Error::custom)?;
        if text != value.to_string() {
            return Err(serde::de::Error::custom("noncanonical integer"));
        }
        Ok(Self(value))
    }
}

/// Stable failures; transport maps these to explicit errors, never retries blindly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Error {
    Malformed,
    FrameTooLarge,
    VersionMismatch,
    InvalidIdentity,
    InvalidLabel,
    InvalidLimits,
    RoomLimit,
    NamespaceLimit,
    ConnectionLimit,
    NamespaceExpired,
    ConnectionExpired,
    Unauthorized,
    IncarnationMismatch,
    PayloadMismatch,
    SequenceGap,
    Exhausted,
    TimeRegression,
    Offline,
    Pending,
    ResyncRequired,
    UnexpectedOutcome,
    StaleSnapshot,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}

/// Decode a bounded, closed typed schema. Serde rejects duplicate struct fields.
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, Error> {
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(Error::FrameTooLarge);
    }
    serde_json::from_slice(bytes).map_err(|_| Error::Malformed)
}

/// Serialize through a capped writer so rejected output never allocates without bound.
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, Error> {
    struct Capped(Vec<u8>);
    impl std::io::Write for Capped {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > MAX_FRAME_BYTES - self.0.len() {
                return Err(std::io::Error::other("frame limit"));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut output = Capped(Vec::new());
    serde_json::to_writer(&mut output, value).map_err(|_| Error::FrameTooLarge)?;
    Ok(output.0)
}

/// A supported mutation. IDs are room-scoped and never reused within an incarnation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Mutation {
    Add { label: String },
    SetDone { id: Decimal, done: bool },
    Remove { id: Decimal },
}
impl Mutation {
    pub(crate) fn validate(&self, label_limit: usize) -> Result<(), Error> {
        match self {
            Self::Add { label }
                if label.trim().is_empty()
                    || label.len() > label_limit
                    || label.chars().any(char::is_control) =>
            {
                Err(Error::InvalidLabel)
            }
            Self::SetDone { id, .. } | Self::Remove { id } if id.0 <= 0 => Err(Error::Malformed),
            _ => Ok(()),
        }
    }
}

/// One mutation, scoped to resource incarnation and resumable command namespace.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub version: u8,
    pub incarnation: String,
    pub namespace: String,
    pub sequence: Decimal,
    pub expected_revision: Decimal,
    pub mutation: Mutation,
}

/// Confirmed task state; local input drafts do not enter this structure.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    pub id: Decimal,
    pub label: String,
    pub done: bool,
}

/// Complete authoritative state. Complete snapshots may skip revisions.
/// `viewers` is the room's live physical connection count at publication; it
/// changes without a revision step and is absent from older persisted records.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub version: u8,
    pub room: String,
    pub incarnation: String,
    pub revision: Decimal,
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub viewers: Decimal,
}

/// Command results retained independently from replaceable snapshots.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Applied,
    Conflict,
    NotFound,
    Capacity,
    Unknown,
}

/// Acknowledgement identifies the exact command namespace and resource incarnation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    pub version: u8,
    pub incarnation: String,
    pub namespace: String,
    pub sequence: Decimal,
    pub revision: Decimal,
    pub status: Status,
}

/// A newly authorized physical connection, with a fresh snapshot and next sequence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connected {
    pub version: u8,
    pub connection: String,
    pub namespace: String,
    pub next_sequence: Decimal,
    pub snapshot: Snapshot,
    pub resumed: bool,
}

/// Client wire envelope. Authorization comes exclusively from the transport.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ClientMessage {
    Join {
        room: String,
        resume_namespace: Option<String>,
    },
    Command(Command),
}

/// Server wire envelope; reset is explicit rather than inferred from stale traffic.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ServerMessage {
    Connected(Connected),
    Snapshot(Snapshot),
    Reset(Snapshot),
    Outcome(Outcome),
    Error(Error),
}

pub(crate) fn identity(value: &str) -> Result<(), Error> {
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        Err(Error::InvalidIdentity)
    } else {
        Ok(())
    }
}
