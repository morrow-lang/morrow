use crate::Error;
use serde::{Deserialize, Deserializer, Serialize};

macro_rules! name {
    ($name:ident,$doc:literal) => {
        #[doc=$doc]
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);
        impl $name {
            /// Validate a nonempty ASCII identifier of at most 64 bytes.
            pub fn new(value: impl Into<String>) -> Result<Self, Error> {
                let value = value.into();
                if value.is_empty()
                    || value.len() > 64
                    || !value
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
                {
                    return Err(Error::InvalidIdentity);
                }
                Ok(Self(value))
            }
            /// Canonical identifier bytes, suitable for stable routing input.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                Self::new(String::deserialize(d)?).map_err(serde::de::Error::custom)
            }
        }
    };
}
name!(
    NodeId,
    "Configured stable node identity; never a native actor address."
);
name!(ClusterId, "Configured stable cluster identity.");
macro_rules! nonce {
    ($name:ident,$doc:literal) => {
        #[doc=$doc]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name([u8; 16]);
        impl $name {
            /// Accept an externally generated random, nonzero 128-bit identity.
            /// This constructor validates representation; callers own randomness.
            pub fn new(value: [u8; 16]) -> Result<Self, Error> {
                if value == [0; 16] {
                    Err(Error::InvalidIdentity)
                } else {
                    Ok(Self(value))
                }
            }
            /// Return the exact identity bytes without numeric conversion.
            pub fn as_bytes(&self) -> &[u8; 16] {
                &self.0
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                Self::new(<[u8; 16]>::deserialize(d)?).map_err(serde::de::Error::custom)
            }
        }
    };
}
nonce!(
    BootId,
    "Random process incarnation, regenerated at every node startup."
);
nonce!(
    LinkId,
    "Random forwarding stream incarnation; reconnect uses a fresh value."
);
/// SHA-256 identity of the canonical fixed routing manifest; not a consensus epoch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ManifestId(pub(crate) [u8; 32]);
impl ManifestId {
    /// Construct from a received digest; authenticity comes from the peer handshake.
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
    /// The digest bytes used for exact manifest agreement.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}
/// A monotonic sequence scoped to one physical forwarding stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RequestId {
    link: LinkId,
    sequence: u64,
}
impl RequestId {
    /// Sequence zero is reserved; identities are never carried as JSON numbers.
    pub fn new(link: LinkId, sequence: u64) -> Result<Self, Error> {
        if sequence == 0 {
            Err(Error::InvalidIdentity)
        } else {
            Ok(Self { link, sequence })
        }
    }
    /// The physical stream generation that admitted this request.
    pub fn link(self) -> LinkId {
        self.link
    }
    /// The stream-local nonzero sequence.
    pub fn sequence(self) -> u64 {
        self.sequence
    }
}
