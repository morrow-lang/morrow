//! Isolated measurements of actual Morrow messages; no production codec selection.
mod adapter;
pub mod browser;
pub mod corpus;
pub mod schema;
mod validate;
pub use morrow_web_protocol as protocol;
use prost::Message as _;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    Client(protocol::ClientMessage),
    Server(protocol::ServerMessage),
}
impl Message {
    pub fn is_client(&self) -> bool {
        matches!(self, Self::Client(_))
    }
}
#[derive(Clone, Copy, Debug)]
pub enum Codec {
    Json,
    Cbor,
    Protobuf,
}
pub fn encode(codec: Codec, value: &Message) -> Result<Vec<u8>, String> {
    let codec = browser::selected_codec(codec);
    match codec {
        Codec::Json => match value {
            Message::Client(v) => protocol::encode(v),
            Message::Server(v) => protocol::encode(v),
        }
        .map_err(|e| e.to_string()),
        _ => encode_prepared(codec, &schema::Wire::from(value)),
    }
}
pub fn encode_prepared(codec: Codec, wire: &schema::Wire) -> Result<Vec<u8>, String> {
    let codec = browser::selected_codec(codec);
    let bytes = match codec {
        Codec::Protobuf => {
            if wire.encoded_len() > protocol::MAX_FRAME_BYTES {
                return Err("frame limit".into());
            }
            wire.encode_to_vec()
        }
        Codec::Cbor => {
            let mut writer = Capped(Vec::new());
            minicbor::encode(wire, &mut writer).map_err(|e| e.to_string())?;
            writer.0
        }
        Codec::Json => return Err("JSON uses actual message types".into()),
    };
    Ok(bytes)
}
pub fn decode_prepared(codec: Codec, bytes: &[u8]) -> Result<schema::Wire, String> {
    let codec = browser::selected_codec(codec);
    if bytes.len() > protocol::MAX_FRAME_BYTES {
        return Err("frame limit".into());
    }
    validate::check(codec, bytes)?;
    match codec {
        Codec::Cbor => minicbor::decode(bytes).map_err(|e| e.to_string()),
        Codec::Protobuf => schema::Wire::decode(bytes).map_err(|e| e.to_string()),
        Codec::Json => Err("JSON uses actual message types".into()),
    }
}
pub fn decode(codec: Codec, bytes: &[u8], client: bool) -> Result<Message, String> {
    let codec = browser::selected_codec(codec);
    if matches!(codec, Codec::Json) {
        return if client {
            protocol::decode(bytes).map(Message::Client)
        } else {
            protocol::decode(bytes).map(Message::Server)
        }
        .map_err(|e| e.to_string());
    }
    let message = Message::try_from(decode_prepared(codec, bytes)?)?;
    if message.is_client() != client {
        return Err("wrong direction".into());
    }
    Ok(message)
}
struct Capped(Vec<u8>);
impl minicbor::encode::Write for Capped {
    type Error = &'static str;
    fn write_all(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        if bytes.len() > protocol::MAX_FRAME_BYTES - self.0.len() {
            return Err("frame limit");
        }
        self.0.extend_from_slice(bytes);
        Ok(())
    }
}
