//! Closed per-browser forwarding schema. No transport frame means an application commit.
use fern_web_protocol::{Command, ServerMessage};
use serde::{Deserialize, Serialize};
use std::io;

pub const MAX_PEER_FRAME_BYTES: usize = fern_web_protocol::MAX_FRAME_BYTES + 4096;
pub const MAX_LEASE_MS: u32 = 3_600_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Frame {
    Join { room: String, lease_ms: u32 },
    Command(Command),
    Event(ServerMessage),
    Ping { nonce: u64 },
    Pong { nonce: u64 },
    Close,
}
impl Frame {
    /// Validate transport bounds; the owner still authenticates and validates every command.
    pub(crate) fn validate(&self) -> io::Result<()> {
        match self {
            Self::Join { room, lease_ms } => {
                if room.is_empty() || room.len() > 128 || room.chars().any(char::is_control) {
                    return Err(invalid("invalid forwarded room"));
                }
                lease(*lease_ms)
            }
            Self::Command(command) => fern_web_protocol::encode(command)
                .map(|_| ())
                .map_err(|_| invalid("forwarded command exceeds wire budget")),
            Self::Event(event) => fern_web_protocol::encode(event)
                .map(|_| ())
                .map_err(|_| invalid("forwarded event exceeds wire budget")),
            _ => Ok(()),
        }
    }
}
fn lease(value: u32) -> io::Result<()> {
    if !(1..=MAX_LEASE_MS).contains(&value) {
        Err(invalid("invalid forwarded capability lease"))
    } else {
        Ok(())
    }
}
pub(crate) fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
