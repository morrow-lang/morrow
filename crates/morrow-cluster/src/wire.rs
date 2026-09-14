//! Closed per-browser forwarding schema. No transport frame means an application commit.
use morrow_web_protocol::{Command, ServerMessage};
use std::io;

pub const MAX_PEER_FRAME_BYTES: usize = morrow_web_protocol::MAX_FRAME_BYTES + 4096;
pub const MAX_LEASE_MS: u32 = 3_600_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame {
    Join { room: String, lease_ms: u32 },
    Command(Command),
    Event(ServerMessage),
    Ping { nonce: u64 },
    Pong { nonce: u64 },
    Close,
}
pub(crate) fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
