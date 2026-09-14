use crate::{BootId, ClusterId, Error, LinkId, ManifestId, NodeId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Exact initial peer schema version; incompatible peers fail before forwarding.
pub const PROTOCOL_VERSION: u8 = 1;
const OWNER_ALGORITHM: &[u8] = b"fern-room-owner-v1\0";
/// Operator-controlled member record. Endpoints are never learned from peer frames.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Member {
    /// Stable logical node name.
    pub id: NodeId,
    /// Configured TCP host:port; the transport performs bounded resolution/dialing.
    pub endpoint: String,
    /// DNS identity required during ordinary TLS certificate verification.
    pub tls_name: String,
    /// SHA-256 of the verified leaf DER certificate, binding this member's identity.
    pub certificate_sha256: [u8; 32],
}
/// A validated, sorted, fixed routing configuration for at most sixteen nodes.
#[derive(Clone, Debug)]
pub struct Config {
    cluster: ClusterId,
    local: NodeId,
    members: Vec<Member>,
    manifest: ManifestId,
    placement: ManifestId,
}
impl Config {
    /// Validate membership and hash a canonical length-delimited manifest.
    /// Local identity is excluded from the digest so all members obtain the same value.
    pub fn new(cluster: ClusterId, local: NodeId, mut members: Vec<Member>) -> Result<Self, Error> {
        if members.is_empty() || members.len() > 16 {
            return Err(Error::InvalidConfig);
        }
        members.sort_by(|a, b| a.id.cmp(&b.id));
        for (index, member) in members.iter().enumerate() {
            validate_member(member)?;
            if index > 0 && members[index - 1].id == member.id {
                return Err(Error::DuplicateNode);
            }
            if members[..index]
                .iter()
                .any(|m| m.certificate_sha256 == member.certificate_sha256)
            {
                return Err(Error::DuplicateCertificate);
            }
        }
        if !members.iter().any(|m| m.id == local) {
            return Err(Error::UnknownNode);
        }
        let mut hash = Sha256::new();
        hash.update(b"fern-routing-v1\0");
        field(&mut hash, cluster.as_str().as_bytes());
        hash.update((members.len() as u32).to_be_bytes());
        for member in &members {
            field(&mut hash, member.id.as_str().as_bytes());
            field(&mut hash, member.endpoint.as_bytes());
            field(&mut hash, member.tls_name.as_bytes());
            hash.update(member.certificate_sha256);
        }
        let mut placement = Sha256::new();
        placement.update(b"fern-room-placement-v1\0");
        field(&mut placement, OWNER_ALGORITHM);
        field(&mut placement, cluster.as_str().as_bytes());
        placement.update((members.len() as u32).to_be_bytes());
        for member in &members {
            field(&mut placement, member.id.as_str().as_bytes());
        }
        Ok(Self {
            cluster,
            local,
            members,
            manifest: ManifestId(hash.finalize().into()),
            placement: ManifestId(placement.finalize().into()),
        })
    }
    /// Stable cluster identity used in peer handshakes.
    pub fn cluster(&self) -> &ClusterId {
        &self.cluster
    }
    /// Configured identity of this process.
    pub fn local(&self) -> &NodeId {
        &self.local
    }
    /// Exact routing configuration fingerprint. It supplies agreement, not fencing.
    pub fn manifest(&self) -> ManifestId {
        self.manifest
    }
    /// Checkpoint ownership fingerprint: routing algorithm, cluster, and sorted node IDs.
    /// Credential renewal and address changes preserve this value; membership changes
    /// do not. Peer authentication must still compare the complete [`Self::manifest`].
    pub fn placement(&self) -> ManifestId {
        self.placement
    }
    /// Canonically sorted membership records.
    pub fn members(&self) -> &[Member] {
        &self.members
    }
    /// Look up an allowlisted peer without allocating or accepting a remote endpoint.
    pub fn member(&self, id: &NodeId) -> Option<&Member> {
        self.members
            .binary_search_by(|m| m.id.cmp(id))
            .ok()
            .map(|i| &self.members[i])
    }
    /// Stable rendezvous owner for a valid UTF-8 room ID, independent of liveness.
    /// Highest SHA-256 score wins; sorted node ID breaks the theoretical digest tie.
    pub fn owner(&self, room: &str) -> Result<&NodeId, Error> {
        if room.is_empty() || room.len() > 128 || room.chars().any(char::is_control) {
            return Err(Error::InvalidRoom);
        }
        let mut winner: Option<([u8; 32], &NodeId)> = None;
        for member in &self.members {
            let mut hash = Sha256::new();
            hash.update(OWNER_ALGORITHM);
            field(&mut hash, self.cluster.as_str().as_bytes());
            field(&mut hash, room.as_bytes());
            field(&mut hash, member.id.as_str().as_bytes());
            let score: [u8; 32] = hash.finalize().into();
            if winner.as_ref().is_none_or(|(best, _)| score > *best) {
                winner = Some((score, &member.id));
            }
        }
        Ok(winner.expect("validated nonempty membership").1)
    }
    /// Validate protocol and membership agreement; TLS identity binding remains mandatory.
    /// This method does not authenticate a certificate or a claimed node by itself.
    pub fn validate_hello(&self, hello: &Hello) -> Result<(), Error> {
        if hello.version != PROTOCOL_VERSION {
            return Err(Error::VersionMismatch);
        }
        if hello.cluster != self.cluster {
            return Err(Error::ClusterMismatch);
        }
        if hello.manifest != self.manifest {
            return Err(Error::ManifestMismatch);
        }
        if hello.node == self.local {
            return Err(Error::SelfConnection);
        }
        if self.member(&hello.node).is_none() {
            return Err(Error::UnknownNode);
        }
        Ok(())
    }
    /// Validate agreement and require the identity authenticated independently by TLS.
    pub fn validate_authenticated_hello(&self, peer: &NodeId, hello: &Hello) -> Result<(), Error> {
        self.validate_hello(hello)?;
        if &hello.node != peer {
            return Err(Error::UnknownNode);
        }
        Ok(())
    }
}
fn field(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u32).to_be_bytes());
    hash.update(bytes);
}
fn validate_member(member: &Member) -> Result<(), Error> {
    if member.endpoint.is_empty()
        || member.endpoint.len() > 320
        || member
            .endpoint
            .bytes()
            .any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
        || member.certificate_sha256 == [0; 32]
    {
        return Err(Error::InvalidConfig);
    }
    let Some((host, port)) = member.endpoint.rsplit_once(':') else {
        return Err(Error::InvalidConfig);
    };
    if host.is_empty() || port.parse::<u16>().ok().filter(|port| *port != 0).is_none() {
        return Err(Error::InvalidConfig);
    }
    if member.tls_name.is_empty()
        || member.tls_name.len() > 253
        || !member.tls_name.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
    {
        return Err(Error::InvalidConfig);
    }
    Ok(())
}
/// First typed frame after mutually authenticated TLS; contains no forwarded mutation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hello {
    /// Must equal `PROTOCOL_VERSION`.
    pub version: u8,
    /// Fixed cluster identity.
    pub cluster: ClusterId,
    /// Claimed node, checked against the verified leaf's configured identity.
    pub node: NodeId,
    /// Process incarnation, supplied from OS randomness by the transport owner.
    pub boot: BootId,
    /// Fresh physical forwarding-stream identity.
    pub link: LinkId,
    /// Exact fixed-membership manifest digest.
    pub manifest: ManifestId,
}
