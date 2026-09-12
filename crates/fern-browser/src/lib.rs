//! Rust browser host for the compiled Fern checklist preview.
#![forbid(unsafe_code)]
#[cfg(target_arch = "wasm32")]
mod browser;
#[cfg(target_arch = "wasm32")]
pub use browser::{mount, unmount};

use fern_web_protocol::{
    Client, Connected, Decimal, Error, MAX_FRAME_BYTES, MAX_LABEL_BYTES, Snapshot, VERSION,
};
use serde::{Deserialize, Serialize};

/// Maximum persisted offline record size. Records contain no authentication material.
pub const MAX_SAVED_BYTES: usize = MAX_FRAME_BYTES;
/// Browser socket output admission threshold; commands are never queued indefinitely.
pub const MAX_OUTPUT_BYTES: usize = 262_144;

/// Bounded offline state: cached confirmed data, editable draft and uncertainty notice.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Saved {
    pub draft: String,
    pub snapshot: Option<Snapshot>,
    pub had_pending: bool,
}
impl Saved {
    /// Restore untrusted origin-local storage without restoring mutation replay authority.
    pub fn decode(text: &str) -> Result<Self, Error> {
        let saved: Self = fern_web_protocol::decode(text.as_bytes())?;
        saved.validate()?;
        Ok(saved)
    }
    /// Encode a validated bounded record for persistence.
    pub fn encode(&self) -> Result<String, Error> {
        self.validate()?;
        String::from_utf8(fern_web_protocol::encode(self)?).map_err(|_| Error::Malformed)
    }
    fn validate(&self) -> Result<(), Error> {
        if self.draft.len() > MAX_LABEL_BYTES {
            return Err(Error::InvalidLabel);
        }
        if let Some(snapshot) = &self.snapshot {
            Client::new(Connected {
                version: VERSION,
                connection: "offline".into(),
                namespace: "offline".into(),
                next_sequence: Decimal(1),
                snapshot: snapshot.clone(),
                resumed: false,
            })?;
        }
        Ok(())
    }
}

/// Capped exponential reconnect delay with injected jitter for deterministic tests.
pub fn reconnect_delay_ms(attempt: u32, jitter_ms: u32) -> u32 {
    500_u32.saturating_mul(1_u32 << attempt.min(6)).min(30_000) + jitter_ms.min(999)
}

/// Reject before appending to an already full browser WebSocket output buffer.
pub fn can_send(buffered_bytes: usize, frame_bytes: usize) -> bool {
    frame_bytes <= MAX_FRAME_BYTES && buffered_bytes <= MAX_OUTPUT_BYTES.saturating_sub(frame_bytes)
}
