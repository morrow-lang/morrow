//! Single-pass outer protobuf envelope; borrow fields before bounded typed decoding.
//! The permanent field/variant registry is protocol/peer.proto.
use crate::wire::invalid;
use crate::{
    BootId, ClusterId, Frame, Hello, LinkId, MAX_LEASE_MS, MAX_PEER_FRAME_BYTES, ManifestId, NodeId,
};
use morrow_web_protocol::binary;
use std::io;
const MAX_HELLO_BYTES: usize = 256;

impl Hello {
    /// Encode the explicit protobuf peer handshake; TLS authenticates its identities.
    pub fn encode(&self) -> io::Result<Vec<u8>> {
        let mut out = Encoder::new(MAX_HELLO_BYTES);
        out.number(1, self.version.into())?;
        out.bytes(2, self.cluster.as_str().as_bytes())?;
        out.bytes(3, self.node.as_str().as_bytes())?;
        out.bytes(4, self.boot.as_bytes())?;
        out.bytes(5, self.link.as_bytes())?;
        out.bytes(6, self.manifest.as_bytes())?;
        Ok(out.bytes)
    }
    /// Decode one bounded closed handshake; configuration/TLS agreement is separate.
    pub fn decode(bytes: &[u8]) -> io::Result<Self> {
        let fields = Fields::parse(bytes, MAX_HELLO_BYTES, [0, 2, 2, 2, 2, 2])?;
        fields.require(0b11_1111)?;
        Ok(Self {
            version: u8::try_from(fields.number(1)?)
                .map_err(|_| invalid("peer version overflow"))?,
            cluster: ClusterId::new(fields.text(2, 64)?)
                .map_err(|_| invalid("invalid peer cluster"))?,
            node: NodeId::new(fields.text(3, 64)?).map_err(|_| invalid("invalid peer node"))?,
            boot: BootId::new(
                fields
                    .bytes(4)?
                    .try_into()
                    .map_err(|_| invalid("invalid boot width"))?,
            )
            .map_err(|_| invalid("zero peer boot"))?,
            link: LinkId::new(
                fields
                    .bytes(5)?
                    .try_into()
                    .map_err(|_| invalid("invalid link width"))?,
            )
            .map_err(|_| invalid("zero peer link"))?,
            manifest: ManifestId::new(
                fields
                    .bytes(6)?
                    .try_into()
                    .map_err(|_| invalid("invalid manifest width"))?,
            ),
        })
    }
}
impl Frame {
    /// Encode once into the bounded protobuf envelope; no nested JSON or size re-encode.
    pub fn encode(&self) -> io::Result<Vec<u8>> {
        let mut out = Encoder::new(MAX_PEER_FRAME_BYTES);
        match self {
            Self::Join { room, lease_ms } => {
                join(room, *lease_ms)?;
                out.number(1, 1)?;
                out.bytes(2, room.as_bytes())?;
                out.number(3, (*lease_ms).into())?;
            }
            Self::Command(command) => {
                let command = binary::encode_command(command)
                    .map_err(|_| invalid("invalid forwarded command"))?;
                out.number(1, 2)?;
                out.bytes(4, &command)?;
            }
            Self::Event(event) => {
                let event =
                    binary::encode_server(event).map_err(|_| invalid("invalid forwarded event"))?;
                out.number(1, 3)?;
                out.bytes(5, &event)?;
            }
            Self::Ping { nonce } => {
                out.number(1, 4)?;
                out.number(6, *nonce)?;
            }
            Self::Pong { nonce } => {
                out.number(1, 5)?;
                out.number(6, *nonce)?;
            }
            Self::Close => out.number(1, 6)?,
        }
        Ok(out.bytes)
    }
    /// Decode a strict outer envelope, then a shared bounded command/server payload.
    pub fn decode(bytes: &[u8]) -> io::Result<Self> {
        let fields = Fields::parse(bytes, MAX_PEER_FRAME_BYTES, [0, 2, 0, 2, 2, 0])?;
        Ok(match fields.number(1)? {
            1 => {
                fields.require(0b00_0111)?;
                let room = fields.text(2, 128)?;
                let lease_ms =
                    u32::try_from(fields.number(3)?).map_err(|_| invalid("peer lease overflow"))?;
                join(room, lease_ms)?;
                Self::Join {
                    room: room.into(),
                    lease_ms,
                }
            }
            2 => {
                fields.require(0b00_1001)?;
                Self::Command(
                    binary::decode_command(fields.bytes(4)?)
                        .map_err(|_| invalid("invalid forwarded command"))?,
                )
            }
            3 => {
                fields.require(0b01_0001)?;
                Self::Event(
                    binary::decode_server(fields.bytes(5)?)
                        .map_err(|_| invalid("invalid forwarded event"))?,
                )
            }
            4 | 5 => {
                fields.require(0b10_0001)?;
                let nonce = fields.number(6)?;
                if fields.number(1)? == 4 {
                    Self::Ping { nonce }
                } else {
                    Self::Pong { nonce }
                }
            }
            6 => {
                fields.require(1)?;
                Self::Close
            }
            _ => return Err(invalid("unknown peer frame kind")),
        })
    }
}
fn join(room: &str, lease_ms: u32) -> io::Result<()> {
    if room.is_empty() || room.len() > 128 || room.chars().any(char::is_control) {
        return Err(invalid("invalid forwarded room"));
    }
    if !(1..=MAX_LEASE_MS).contains(&lease_ms) {
        return Err(invalid("invalid forwarded capability lease"));
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Value<'a> {
    Number(u64),
    Bytes(&'a [u8]),
}
struct Fields<'a> {
    values: [Option<Value<'a>>; 6],
    seen: u8,
}
impl<'a> Fields<'a> {
    fn parse(bytes: &'a [u8], limit: usize, types: [u8; 6]) -> io::Result<Self> {
        if bytes.is_empty() || bytes.len() > limit {
            return Err(invalid("invalid protobuf peer payload size"));
        }
        let mut input = bytes;
        let mut result = Self {
            values: [None; 6],
            seen: 0,
        };
        while !input.is_empty() {
            let key = varint(&mut input)?;
            let tag = key >> 3;
            if !(1..=6).contains(&tag) {
                return Err(invalid("unknown peer protobuf field"));
            }
            let index = tag as usize - 1;
            if result.seen & (1 << index) != 0 {
                return Err(invalid("duplicate peer protobuf field"));
            }
            if key & 7 != u64::from(types[index]) {
                return Err(invalid("wrong peer protobuf wire type"));
            }
            let value = if types[index] == 0 {
                Value::Number(varint(&mut input)?)
            } else {
                let length = usize::try_from(varint(&mut input)?)
                    .map_err(|_| invalid("peer field length overflow"))?;
                if length > input.len() {
                    return Err(invalid("truncated peer field"));
                }
                let (value, rest) = input.split_at(length);
                input = rest;
                Value::Bytes(value)
            };
            result.values[index] = Some(value);
            result.seen |= 1 << index;
        }
        Ok(result)
    }
    fn require(&self, fields: u8) -> io::Result<()> {
        if self.seen != fields {
            return Err(invalid("missing or irrelevant peer variant field"));
        }
        Ok(())
    }
    fn number(&self, tag: usize) -> io::Result<u64> {
        match self.values[tag - 1] {
            Some(Value::Number(value)) => Ok(value),
            _ => Err(invalid("required peer integer missing")),
        }
    }
    fn bytes(&self, tag: usize) -> io::Result<&'a [u8]> {
        match self.values[tag - 1] {
            Some(Value::Bytes(value)) => Ok(value),
            _ => Err(invalid("required peer bytes missing")),
        }
    }
    fn text(&self, tag: usize, max: usize) -> io::Result<&'a str> {
        let bytes = self.bytes(tag)?;
        if bytes.len() > max {
            return Err(invalid("peer text too long"));
        }
        std::str::from_utf8(bytes).map_err(|_| invalid("invalid peer UTF-8"))
    }
}
fn varint(input: &mut &[u8]) -> io::Result<u64> {
    let mut value = 0;
    for index in 0..10 {
        let (&byte, rest) = input
            .split_first()
            .ok_or_else(|| invalid("truncated peer varint"))?;
        *input = rest;
        if index == 9 && byte > 1 {
            return Err(invalid("peer varint overflow"));
        }
        value |= u64::from(byte & 127) << (index * 7);
        if byte & 128 == 0 {
            if index > 0 && byte == 0 {
                return Err(invalid("overlong peer varint"));
            }
            return Ok(value);
        }
    }
    Err(invalid("peer varint overflow"))
}
struct Encoder {
    bytes: Vec<u8>,
    limit: usize,
}
impl Encoder {
    fn new(limit: usize) -> Self {
        Self {
            // Control frames fit here; large application fields reserve exactly once.
            bytes: Vec::with_capacity(128.min(limit)),
            limit,
        }
    }
    fn extend(&mut self, bytes: &[u8]) -> io::Result<()> {
        if bytes.len() > self.limit - self.bytes.len() {
            return Err(invalid("peer frame exceeds wire budget"));
        }
        self.bytes.reserve_exact(bytes.len());
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }
    fn varint(&mut self, mut value: u64) -> io::Result<()> {
        let mut bytes = [0u8; 10];
        let mut length = 0;
        loop {
            bytes[length] = (value & 127) as u8;
            value >>= 7;
            if value != 0 {
                bytes[length] |= 128;
            }
            length += 1;
            if value == 0 {
                break;
            }
        }
        self.extend(&bytes[..length])
    }
    fn number(&mut self, tag: u8, value: u64) -> io::Result<()> {
        self.varint(u64::from(tag) << 3)?;
        self.varint(value)
    }
    fn bytes(&mut self, tag: u8, value: &[u8]) -> io::Result<()> {
        self.varint((u64::from(tag) << 3) | 2)?;
        self.varint(value.len() as u64)?;
        self.extend(value)
    }
}
