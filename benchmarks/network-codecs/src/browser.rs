//! The same fixture engine runs natively in tests and inside actual browser WASM.
use crate::{Codec, Message, corpus, decode, encode};
use std::hint::black_box;

// Native experiments retain runtime codec selection. A browser module deliberately
// links one codec so its artifact size does not contain the other two candidates.
#[inline(always)]
pub(crate) fn selected_codec(codec: Codec) -> Codec {
    #[cfg(all(target_arch = "wasm32", feature = "browser-json"))]
    {
        let _ = codec;
        Codec::Json
    }
    #[cfg(all(target_arch = "wasm32", feature = "browser-cbor"))]
    {
        let _ = codec;
        Codec::Cbor
    }
    #[cfg(all(target_arch = "wasm32", feature = "browser-protobuf"))]
    {
        let _ = codec;
        Codec::Protobuf
    }
    #[cfg(not(all(
        target_arch = "wasm32",
        any(
            feature = "browser-json",
            feature = "browser-cbor",
            feature = "browser-protobuf"
        )
    )))]
    {
        codec
    }
}

pub struct Engine {
    codec: Codec,
    fixtures: Vec<(String, Message, Vec<u8>)>,
}

impl Engine {
    pub fn new(codec: Codec) -> Result<Self, String> {
        acceptance(codec)?;
        let mut fixtures = Vec::new();
        for (name, message) in corpus::fixtures() {
            let bytes = encode(codec, &message)?;
            if decode(codec, &bytes, message.is_client())? != message {
                return Err(format!("semantic mismatch: {name}"));
            }
            if decode(codec, &bytes, !message.is_client()).is_ok() {
                return Err(format!("accepted wrong direction: {name}"));
            }
            fixtures.push((name, message, bytes));
        }
        Ok(Self { codec, fixtures })
    }

    pub fn len(&self) -> usize {
        self.fixtures.len()
    }
    pub fn is_empty(&self) -> bool {
        self.fixtures.is_empty()
    }
    pub fn name(&self, index: usize) -> Result<String, String> {
        Ok(self.fixture(index)?.0.clone())
    }
    fn fixture(&self, index: usize) -> Result<&(String, Message, Vec<u8>), String> {
        self.fixtures
            .get(index)
            .ok_or_else(|| "invalid fixture".into())
    }
    pub fn encode(&self, index: usize) -> Result<Vec<u8>, String> {
        encode(self.codec, black_box(&self.fixture(index)?.1))
    }
    pub fn decode(&self, index: usize, bytes: &[u8]) -> Result<usize, String> {
        let message = decode(
            self.codec,
            black_box(bytes),
            self.fixture(index)?.1.is_client(),
        )?;
        black_box(message);
        Ok(1)
    }
    pub fn check(&self, index: usize, bytes: &[u8]) -> Result<bool, String> {
        let expected = &self.fixture(index)?.1;
        Ok(decode(self.codec, bytes, expected.is_client())? == *expected)
    }
    pub fn batch(&self, index: usize, iterations: usize, encoding: bool) -> Result<usize, String> {
        if !(1..=100_000).contains(&iterations) {
            return Err("iterations out of range".into());
        }
        let fixture = self.fixture(index)?;
        let mut checksum = 0;
        for _ in 0..iterations {
            checksum += if encoding {
                black_box(self.encode(index)?).len()
            } else {
                self.decode(index, &fixture.2)?
            };
        }
        Ok(black_box(checksum))
    }
}

fn acceptance(codec: Codec) -> Result<(), String> {
    use crate::protocol::{ClientMessage, Decimal, ServerMessage, Snapshot, Task, VERSION};
    let join = Message::Client(ClientMessage::Join {
        room: "r".into(),
        resume_namespace: None,
    });
    let expected: &[u8] = match selected_codec(codec) {
        Codec::Json => br#"{"type":"join","data":{"room":"r","resume_namespace":null}}"#,
        Codec::Cbor => &[0xa2, 1, 1, 2, 0xa1, 1, 0x61, b'r'],
        Codec::Protobuf => &[8, 1, 18, 3, 10, 1, b'r'],
    };
    if encode(codec, &join)? != expected {
        return Err("independent wire golden failed".into());
    }
    for id in [
        i64::MIN,
        -9007199254740993,
        -1,
        0,
        1,
        9007199254740993,
        i64::MAX,
    ] {
        let value = Message::Server(ServerMessage::Snapshot(Snapshot {
            version: VERSION,
            room: "r🌿".into(),
            incarnation: "boot-a".into(),
            revision: Decimal(id),
            tasks: vec![Task {
                id: Decimal(id),
                label: "こんにちは 🌿\"\\".into(),
                done: false,
            }],
        }));
        let bytes = encode(codec, &value)?;
        if decode(codec, &bytes, false)? != value {
            return Err("full-width golden failed".into());
        }
        for length in 0..bytes.len() {
            if decode(codec, &bytes[..length], false).is_ok() {
                return Err("truncation accepted".into());
            }
        }
    }
    let malformed: &[&[u8]] = match selected_codec(codec) {
        Codec::Json => &[
            br#"{"type":"join","data":{}}"#,
            br#"{"type":"join","data":{"room":"r","room":"s"}}"#,
            br#"{"type":"join","data":{"room":"r","extra":1}}"#,
        ],
        Codec::Cbor => &[
            &[0xa2, 1, 1, 2, 0xa0],
            &[0xa3, 1, 1, 1, 1, 2, 0xa1, 1, 0x61, b'r'],
            &[0xa3, 1, 1, 2, 0xa1, 1, 0x61, b'r', 9, 1],
        ],
        Codec::Protobuf => &[
            &[8, 1, 18, 0],
            &[8, 1, 8, 1, 18, 3, 10, 1, b'r'],
            &[8, 1, 18, 3, 10, 1, b'r', 72, 1],
        ],
    };
    if malformed
        .iter()
        .any(|bytes| decode(codec, bytes, true).is_ok())
    {
        return Err("missing/duplicate/unknown-field accepted".into());
    }
    if decode(codec, &vec![0; crate::protocol::MAX_FRAME_BYTES + 1], true).is_ok() {
        return Err("oversized frame accepted".into());
    }
    Ok(())
}

#[cfg(all(
    target_arch = "wasm32",
    any(
        feature = "browser-json",
        feature = "browser-cbor",
        feature = "browser-protobuf"
    )
))]
mod exports {
    use super::*;
    use std::sync::OnceLock;
    use wasm_bindgen::prelude::*;

    #[cfg(any(
        all(feature = "browser-json", feature = "browser-cbor"),
        all(feature = "browser-json", feature = "browser-protobuf"),
        all(feature = "browser-cbor", feature = "browser-protobuf")
    ))]
    compile_error!("select exactly one browser codec");
    #[cfg(feature = "browser-json")]
    const CODEC: Codec = Codec::Json;
    #[cfg(feature = "browser-cbor")]
    const CODEC: Codec = Codec::Cbor;
    #[cfg(feature = "browser-protobuf")]
    const CODEC: Codec = Codec::Protobuf;

    fn engine() -> &'static Engine {
        static ENGINE: OnceLock<Engine> = OnceLock::new();
        ENGINE.get_or_init(|| Engine::new(CODEC).expect("fixture acceptance"))
    }
    #[wasm_bindgen]
    pub fn fixture_count() -> usize {
        engine().len()
    }
    #[wasm_bindgen]
    pub fn fixture_name(index: usize) -> Result<String, String> {
        engine().name(index)
    }
    #[wasm_bindgen]
    pub fn encode_one(index: usize) -> Result<Vec<u8>, String> {
        engine().encode(index)
    }
    #[wasm_bindgen]
    pub fn decode_one(index: usize, bytes: &[u8]) -> Result<usize, String> {
        engine().decode(index, bytes)
    }
    #[wasm_bindgen]
    pub fn check_one(index: usize, bytes: &[u8]) -> Result<bool, String> {
        engine().check(index, bytes)
    }
    #[wasm_bindgen]
    pub fn batch(index: usize, iterations: usize, encoding: bool) -> Result<usize, String> {
        engine().batch(index, iterations, encoding)
    }
    #[wasm_bindgen]
    pub fn copy_in(bytes: &[u8]) -> usize {
        black_box(bytes).len()
    }
    #[wasm_bindgen]
    pub fn copy_out(index: usize) -> Result<Vec<u8>, String> {
        Ok(engine().fixture(index)?.2.clone())
    }
    #[cfg(feature = "browser-json")]
    #[wasm_bindgen]
    pub fn decode_text(index: usize, text: &str) -> Result<usize, String> {
        engine().decode(index, text.as_bytes())
    }
    #[cfg(feature = "browser-json")]
    #[wasm_bindgen]
    pub fn encode_text(index: usize) -> Result<String, String> {
        String::from_utf8(engine().encode(index)?).map_err(|e| e.to_string())
    }
    #[cfg(feature = "browser-json")]
    #[wasm_bindgen]
    pub fn copy_text_in(text: &str) -> usize {
        black_box(text).len()
    }
    #[cfg(feature = "browser-json")]
    #[wasm_bindgen]
    pub fn copy_text_out(index: usize) -> Result<String, String> {
        String::from_utf8(engine().fixture(index)?.2.clone()).map_err(|e| e.to_string())
    }
}
