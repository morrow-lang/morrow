//! One authenticated forwarding stream; callers own admission, revocation and application order.
use crate::tls::{ALPN, fingerprint};
use crate::wire::invalid;
use crate::{
    Config, FrameReader, FrameWriter, Hello, IoLimits, NodeId, PROTOCOL_VERSION, Security,
};
use rustls::pki_types::ServerName;
use std::io;
use tokio::{
    io::{ReadHalf, WriteHalf},
    net::TcpStream,
};
use tokio_rustls::{TlsAcceptor, TlsConnector, TlsStream};

pub type PeerReader = FrameReader<ReadHalf<TlsStream<TcpStream>>>;
pub type PeerWriter = FrameWriter<WriteHalf<TlsStream<TcpStream>>>;

pub struct PeerStream {
    /// Configured certificate-bound identity and fresh remote boot/link incarnation.
    pub remote: Hello,
    pub local: Hello,
    pub reader: PeerReader,
    pub writer: PeerWriter,
}

/// Dial one configured owner. No request or mutation is retried by this function.
/// Caller must own a process-wide admission permit before retaining this future.
pub async fn connect(
    config: &Config,
    target: &NodeId,
    security: &Security,
    local: Hello,
    limits: IoLimits,
) -> io::Result<PeerStream> {
    let limits = limits.validate()?;
    validate_local(config, security, &local)?;
    if target == config.local() {
        return Err(invalid("cannot forward to the local node"));
    }
    let member = config
        .member(target)
        .ok_or_else(|| invalid("unknown peer node"))?;
    let name = ServerName::try_from(member.tls_name.clone())
        .map_err(|_| invalid("invalid peer TLS name"))?;
    let deadline = tokio::time::Instant::now() + limits.handshake_timeout;
    let result = tokio::time::timeout_at(deadline, async {
        let socket = TcpStream::connect(&member.endpoint).await?;
        socket.set_nodelay(true)?;
        let stream: TlsStream<TcpStream> = TlsConnector::from(security.client.clone())
            .connect(name, socket)
            .await?
            .into();
        handshake(stream, config, Some(target), local, limits).await
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "peer connect/handshake deadline"))??;
    finish_before(deadline, result)
}

/// Authenticate one admitted inbound socket before any room or capability is created.
/// Caller retains the accepted-stream permit for the entire returned stream lifetime.
pub async fn accept(
    socket: TcpStream,
    config: &Config,
    security: &Security,
    local: Hello,
    limits: IoLimits,
) -> io::Result<PeerStream> {
    let limits = limits.validate()?;
    validate_local(config, security, &local)?;
    socket.set_nodelay(true)?;
    let deadline = tokio::time::Instant::now() + limits.handshake_timeout;
    let result = tokio::time::timeout_at(deadline, async {
        let stream: TlsStream<TcpStream> = TlsAcceptor::from(security.server.clone())
            .accept(socket)
            .await?
            .into();
        handshake(stream, config, None, local, limits).await
    })
    .await
    .map_err(|_| {
        io::Error::new(
            io::ErrorKind::TimedOut,
            "peer TLS/identity handshake deadline",
        )
    })??;
    finish_before(deadline, result)
}

fn finish_before(deadline: tokio::time::Instant, result: PeerStream) -> io::Result<PeerStream> {
    if tokio::time::Instant::now() >= deadline {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "peer handshake deadline",
        ))
    } else {
        Ok(result)
    }
}

fn validate_local(config: &Config, security: &Security, local: &Hello) -> io::Result<()> {
    if local.version != PROTOCOL_VERSION
        || &local.node != config.local()
        || &local.cluster != config.cluster()
        || local.manifest != config.manifest()
    {
        return Err(invalid("local peer handshake differs from configuration"));
    }
    let member = config
        .member(config.local())
        .ok_or_else(|| invalid("local node absent from configuration"))?;
    if member.certificate_sha256 != security.leaf_fingerprint {
        return Err(invalid(
            "local certificate does not identify the configured node",
        ));
    }
    Ok(())
}

async fn handshake(
    stream: TlsStream<TcpStream>,
    config: &Config,
    expected: Option<&NodeId>,
    local: Hello,
    limits: IoLimits,
) -> io::Result<PeerStream> {
    let connection = stream.get_ref().1;
    if connection.alpn_protocol() != Some(ALPN) {
        return Err(invalid("peer ALPN mismatch"));
    }
    let leaf = connection
        .peer_certificates()
        .and_then(|chain| chain.first())
        .ok_or_else(|| invalid("peer certificate is required"))?;
    let peer_fingerprint = fingerprint(leaf.as_ref());
    let (reader, writer) = tokio::io::split(stream);
    let mut reader = FrameReader::new(reader, limits)?;
    let mut writer = FrameWriter::new(writer, limits)?;
    // Both peers write a small bounded Hello, flush, then read. No application data precedes validation.
    writer.write_json(&local).await?;
    let remote: Hello = reader.read_json().await?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "peer closed before identity handshake",
        )
    })?;
    config.validate_hello(&remote).map_err(io::Error::other)?;
    if expected.is_some_and(|expected| expected != &remote.node) {
        return Err(invalid(
            "connected peer identity differs from requested owner",
        ));
    }
    let member = config
        .member(&remote.node)
        .ok_or_else(|| invalid("unknown authenticated node"))?;
    if member.certificate_sha256 != peer_fingerprint {
        return Err(invalid("peer certificate does not identify claimed node"));
    }
    Ok(PeerStream {
        remote,
        local,
        reader,
        writer,
    })
}
