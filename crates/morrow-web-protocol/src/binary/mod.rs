//! Closed, bounded protobuf v1 for live commands and snapshots.
//!
//! This is an explicit transport format. Generic JSON helpers remain available for
//! HTTP and persisted records. Never infer a codec from untrusted payload bytes.
mod adapter;
mod schema;
mod validate;

use crate::{
    ClientMessage, Command, Error, MAX_FRAME_BYTES, MAX_LABEL_BYTES, ServerMessage, Snapshot,
};
use prost::Message as _;

/// Explicit WebSocket protocol; the logical application version remains unchanged.
pub const SUBPROTOCOL: &str = "morrow.live.protobuf.v1";
/// Portable client and Hub snapshot cardinality bound.
pub const MAX_TASKS: usize = 100;
/// Maximum UTF-8 bytes for transport identities, checked before allocation.
pub const MAX_IDENTITY_BYTES: usize = 128;

enum Message<'a> {
    Client(&'a ClientMessage),
    Server(&'a ServerMessage),
}
enum Decoded {
    Client(ClientMessage),
    Server(ServerMessage),
}

/// Encode a client message after checking its allocation and frame bounds.
pub fn encode_client(message: &ClientMessage) -> Result<Vec<u8>, Error> {
    match message {
        ClientMessage::Join {
            room,
            resume_namespace,
        } => {
            identity(room)?;
            if let Some(namespace) = resume_namespace {
                identity(namespace)?;
            }
        }
        ClientMessage::Command(command) => command_bounds(command)?,
    }
    encode_wire(&schema::Wire::from(&Message::Client(message)))
}

/// Decode a client envelope; logical authority and mutation validity stay in Hub.
pub fn decode_client(bytes: &[u8]) -> Result<ClientMessage, Error> {
    match decode_wire(bytes)? {
        Decoded::Client(message) => Ok(message),
        Decoded::Server(_) => Err(Error::Malformed),
    }
}

/// Encode one server envelope through the same stable protobuf field registry.
pub fn encode_server(message: &ServerMessage) -> Result<Vec<u8>, Error> {
    match message {
        ServerMessage::Connected(connected) => {
            identity(&connected.connection)?;
            identity(&connected.namespace)?;
            snapshot_bounds(&connected.snapshot)?;
        }
        ServerMessage::Snapshot(snapshot) | ServerMessage::Reset(snapshot) => {
            snapshot_bounds(snapshot)?
        }
        ServerMessage::Outcome(outcome) => {
            identity(&outcome.incarnation)?;
            identity(&outcome.namespace)?;
        }
        ServerMessage::Error(_) => {}
    }
    encode_wire(&schema::Wire::from(&Message::Server(message)))
}

/// Decode one server envelope; Client retains state/revision validation.
pub fn decode_server(bytes: &[u8]) -> Result<ServerMessage, Error> {
    match decode_wire(bytes)? {
        Decoded::Server(message) => Ok(message),
        Decoded::Client(_) => Err(Error::Malformed),
    }
}

/// Encode a bare Command for a typed peer envelope, without nesting live envelopes.
pub fn encode_command(command: &Command) -> Result<Vec<u8>, Error> {
    command_bounds(command)?;
    encode_wire(&schema::Command::from(command))
}

/// Decode a bare Command carried by the negotiated peer protocol.
pub fn decode_command(bytes: &[u8]) -> Result<Command, Error> {
    validate::check(bytes, validate::COMMAND)?;
    schema::Command::decode(bytes)
        .map_err(|_| Error::Malformed)?
        .try_into()
        .map_err(|_| Error::Malformed)
}

fn encode_wire(message: &impl prost::Message) -> Result<Vec<u8>, Error> {
    // Source strings/cardinality have already been bounded before DTO allocation.
    // The exact length check precedes the encoded output allocation.
    if message.encoded_len() > MAX_FRAME_BYTES {
        return Err(Error::FrameTooLarge);
    }
    Ok(message.encode_to_vec())
}
fn decode_wire(bytes: &[u8]) -> Result<Decoded, Error> {
    // The allocation-free structural pass limits all strings and collections before
    // prost builds owned DTOs. It also preserves closed-schema duplicate semantics.
    validate::check(bytes, validate::ENVELOPE)?;
    schema::Wire::decode(bytes)
        .map_err(|_| Error::Malformed)?
        .try_into()
        .map_err(|_| Error::Malformed)
}
fn identity(value: &str) -> Result<(), Error> {
    bounded(value, MAX_IDENTITY_BYTES)
}
fn bounded(value: &str, limit: usize) -> Result<(), Error> {
    if value.len() > limit {
        Err(Error::Malformed)
    } else {
        Ok(())
    }
}
fn command_bounds(command: &Command) -> Result<(), Error> {
    identity(&command.incarnation)?;
    identity(&command.namespace)?;
    if let crate::Mutation::Add { label } = &command.mutation {
        bounded(label, MAX_LABEL_BYTES)?;
    }
    Ok(())
}
fn snapshot_bounds(snapshot: &Snapshot) -> Result<(), Error> {
    identity(&snapshot.room)?;
    identity(&snapshot.incarnation)?;
    if snapshot.tasks.len() > MAX_TASKS || snapshot.viewers.0 < 0 {
        return Err(Error::Malformed);
    }
    for task in &snapshot.tasks {
        bounded(&task.label, MAX_LABEL_BYTES)?;
    }
    Ok(())
}
