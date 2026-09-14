//! Standard certificate validation plus configured leaf pinning; no permissive verifier.
use rustls::{
    ClientConfig, RootCertStore, ServerConfig,
    pki_types::{CertificateDer, PrivateKeyDer},
};
use sha2::{Digest, Sha256};
use std::{io, sync::Arc};

/// Explicit binary peer protocol; JSON peers cannot negotiate this transport.
pub const ALPN: &[u8] = b"fern.peer.protobuf.v1";
#[derive(Clone)]
pub struct Security {
    pub(crate) client: Arc<ClientConfig>,
    pub(crate) server: Arc<ServerConfig>,
    pub(crate) leaf_fingerprint: [u8; 32],
}
impl Security {
    /// Load one cluster trust root and this node's leaf/key from bounded DER records.
    /// Certificates must authorize both server and client authentication.
    pub fn from_der(ca: &[u8], certificate: &[u8], private_key: &[u8]) -> io::Result<Self> {
        if ca.is_empty()
            || ca.len() > 16_384
            || certificate.is_empty()
            || certificate.len() > 16_384
            || private_key.is_empty()
            || private_key.len() > 8192
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid TLS credential size",
            ));
        }
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut roots = RootCertStore::empty();
        roots
            .add(CertificateDer::from(ca.to_vec()))
            .map_err(io::Error::other)?;
        let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
            Arc::new(roots.clone()),
            provider.clone(),
        )
        .build()
        .map_err(io::Error::other)?;
        let chain = vec![CertificateDer::from(certificate.to_vec())];
        let key = PrivateKeyDer::try_from(private_key.to_vec()).map_err(io::Error::other)?;
        let mut server = ServerConfig::builder_with_provider(provider.clone())
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(io::Error::other)?
            .with_client_cert_verifier(verifier)
            .with_single_cert(chain.clone(), key.clone_key())
            .map_err(io::Error::other)?;
        server.alpn_protocols = vec![ALPN.to_vec()];
        server.max_early_data_size = 0;
        let mut client = ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(io::Error::other)?
            .with_root_certificates(roots)
            .with_client_auth_cert(chain, key)
            .map_err(io::Error::other)?;
        client.alpn_protocols = vec![ALPN.to_vec()];
        client.enable_early_data = false;
        Ok(Self {
            client: Arc::new(client),
            server: Arc::new(server),
            leaf_fingerprint: fingerprint(certificate),
        })
    }
}
pub(crate) fn fingerprint(certificate: &[u8]) -> [u8; 32] {
    Sha256::digest(certificate).into()
}
