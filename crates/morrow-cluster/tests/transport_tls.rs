//! Real sockets and fresh certificates validate the transport boundary, not domain commits.
use morrow_cluster::{
    BootId, ClusterId, Config, Frame, Hello, IoLimits, LinkId, Member, NodeId, PROTOCOL_VERSION,
    Security, accept, connect,
};
use morrow_web_protocol::{Command, Decimal, Mutation, Outcome, ServerMessage, Status};
use rcgen::{
    BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::net::TcpListener;

struct Credential {
    certificate: Vec<u8>,
    key: Vec<u8>,
}
struct Fixture {
    ca: Vec<u8>,
    a: Credential,
    b: Credential,
    listener: TcpListener,
    config_a: Config,
    config_b: Config,
}
impl Fixture {
    async fn new() -> Self {
        let ca_key = KeyPair::generate().unwrap();
        let mut parameters = CertificateParams::new(vec!["morrow-test-ca.invalid".into()]).unwrap();
        parameters.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        parameters.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let ca = parameters.self_signed(&ca_key).unwrap().der().to_vec();
        let issuer = Issuer::new(parameters, ca_key);
        let credential = |name: &str| {
            let key = KeyPair::generate().unwrap();
            let mut parameters = CertificateParams::new(vec![name.into()]).unwrap();
            parameters.extended_key_usages = vec![
                ExtendedKeyUsagePurpose::ServerAuth,
                ExtendedKeyUsagePurpose::ClientAuth,
            ];
            parameters.key_usages = vec![KeyUsagePurpose::DigitalSignature];
            Credential {
                certificate: parameters.signed_by(&key, &issuer).unwrap().der().to_vec(),
                key: key.serialize_der(),
            }
        };
        let a = credential("node-a.morrow.invalid");
        let b = credential("node-b.morrow.invalid");
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let members = vec![
            Member {
                id: node("a"),
                endpoint: "127.0.0.1:1".into(),
                tls_name: "node-a.morrow.invalid".into(),
                certificate_sha256: Sha256::digest(&a.certificate).into(),
            },
            Member {
                id: node("b"),
                endpoint: listener.local_addr().unwrap().to_string(),
                tls_name: "node-b.morrow.invalid".into(),
                certificate_sha256: Sha256::digest(&b.certificate).into(),
            },
        ];
        let config_a =
            Config::new(ClusterId::new("test").unwrap(), node("a"), members.clone()).unwrap();
        let config_b = Config::new(ClusterId::new("test").unwrap(), node("b"), members).unwrap();
        Self {
            ca,
            a,
            b,
            listener,
            config_a,
            config_b,
        }
    }
    fn security(&self, credential: &Credential) -> Security {
        Security::from_der(&self.ca, &credential.certificate, &credential.key).unwrap()
    }
}
fn node(name: &str) -> NodeId {
    NodeId::new(name).unwrap()
}
fn hello(config: &Config, byte: u8) -> Hello {
    Hello {
        version: PROTOCOL_VERSION,
        cluster: config.cluster().clone(),
        node: config.local().clone(),
        manifest: config.manifest(),
        boot: BootId::new([byte; 16]).unwrap(),
        link: LinkId::new([byte.wrapping_add(1); 16]).unwrap(),
    }
}

#[tokio::test]
async fn ten_thousand_real_tls_messages_preserve_identity_unicode_and_full_width_ids() {
    let fixture = Fixture::new().await;
    let security_a = fixture.security(&fixture.a);
    let security_b = fixture.security(&fixture.b);
    let server = async {
        let (socket, _) = fixture.listener.accept().await.unwrap();
        let mut peer = accept(
            socket,
            &fixture.config_b,
            &security_b,
            hello(&fixture.config_b, 9),
            IoLimits::default(),
        )
        .await
        .unwrap();
        assert_eq!(peer.remote.node, node("a"));
        for expected in 1..=10_000_i64 {
            let Some(Frame::Command(command)) = peer.reader.read().await.unwrap() else {
                panic!("missing command")
            };
            assert_eq!(command.sequence, Decimal(expected));
            assert_eq!(command.expected_revision, Decimal(i64::MAX - expected));
            assert_eq!(
                command.mutation,
                Mutation::Add {
                    label: format!("雪🦀-{expected}")
                }
            );
            peer.writer
                .write(&Frame::Event(ServerMessage::Outcome(Outcome {
                    version: 1,
                    incarnation: command.incarnation,
                    namespace: command.namespace,
                    sequence: command.sequence,
                    revision: command.expected_revision,
                    status: Status::Applied,
                })))
                .await
                .unwrap();
        }
        assert_eq!(peer.reader.read().await.unwrap(), Some(Frame::Close));
    };
    let client = async {
        let mut peer = connect(
            &fixture.config_a,
            &node("b"),
            &security_a,
            hello(&fixture.config_a, 3),
            IoLimits::default(),
        )
        .await
        .unwrap();
        assert_eq!(peer.remote.node, node("b"));
        for sequence in 1..=10_000_i64 {
            peer.writer
                .write(&Frame::Command(Command {
                    version: 1,
                    incarnation: "room-boot".into(),
                    namespace: "stream-command".into(),
                    sequence: Decimal(sequence),
                    expected_revision: Decimal(i64::MAX - sequence),
                    mutation: Mutation::Add {
                        label: format!("雪🦀-{sequence}"),
                    },
                }))
                .await
                .unwrap();
            let Some(Frame::Event(ServerMessage::Outcome(outcome))) =
                peer.reader.read().await.unwrap()
            else {
                panic!("missing outcome")
            };
            assert_eq!(outcome.sequence, Decimal(sequence));
            assert_eq!(outcome.revision, Decimal(i64::MAX - sequence));
        }
        peer.writer.write(&Frame::Close).await.unwrap();
    };
    tokio::time::timeout(Duration::from_secs(30), async {
        tokio::join!(client, server);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn unknown_cluster_ca_cannot_publish_an_authenticated_stream() {
    let fixture = Fixture::new().await;
    let foreign = Fixture::new().await;
    let security_a =
        Security::from_der(&foreign.ca, &fixture.a.certificate, &fixture.a.key).unwrap();
    let security_b = fixture.security(&fixture.b);
    let limits = IoLimits {
        handshake_timeout: Duration::from_secs(1),
        ..Default::default()
    };
    let target = node("b");
    let client = connect(
        &fixture.config_a,
        &target,
        &security_a,
        hello(&fixture.config_a, 3),
        limits,
    );
    let server = async {
        let (socket, _) = fixture.listener.accept().await.unwrap();
        accept(
            socket,
            &fixture.config_b,
            &security_b,
            hello(&fixture.config_b, 9),
            limits,
        )
        .await
    };
    let (client, server) = tokio::join!(client, server);
    assert!(client.is_err());
    assert!(server.is_err());
}

#[tokio::test]
async fn local_certificate_identity_mismatch_fails_before_dial() {
    let fixture = Fixture::new().await;
    let wrong = fixture.security(&fixture.b);
    assert!(
        connect(
            &fixture.config_a,
            &node("b"),
            &wrong,
            hello(&fixture.config_a, 3),
            IoLimits::default()
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn tcp_peer_that_never_starts_tls_exhausts_only_its_bounded_handshake() {
    let fixture = Fixture::new().await;
    let security = fixture.security(&fixture.b);
    let held = tokio::net::TcpStream::connect(fixture.listener.local_addr().unwrap())
        .await
        .unwrap();
    let (socket, _) = fixture.listener.accept().await.unwrap();
    let limits = IoLimits {
        handshake_timeout: Duration::from_millis(50),
        ..Default::default()
    };
    let failure = accept(
        socket,
        &fixture.config_b,
        &security,
        hello(&fixture.config_b, 9),
        limits,
    )
    .await
    .err()
    .unwrap();
    assert_eq!(failure.kind(), std::io::ErrorKind::TimedOut);
    drop(held);
}

async fn raw_tls_client(
    fixture: &Fixture,
    credential: Option<&Credential>,
    claimed: Option<Hello>,
) {
    raw_tls_client_with_alpn(fixture, credential, claimed, morrow_cluster::ALPN).await;
}
async fn raw_tls_client_with_alpn(
    fixture: &Fixture,
    credential: Option<&Credential>,
    claimed: Option<Hello>,
    alpn: &[u8],
) {
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName};
    use tokio::io::AsyncWriteExt;
    let mut roots = rustls::RootCertStore::empty();
    roots.add(CertificateDer::from(fixture.ca.clone())).unwrap();
    let builder = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])
    .unwrap()
    .with_root_certificates(roots);
    let mut config = if let Some(credential) = credential {
        builder
            .with_client_auth_cert(
                vec![CertificateDer::from(credential.certificate.clone())],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(credential.key.clone())),
            )
            .unwrap()
    } else {
        builder.with_no_client_auth()
    };
    config.alpn_protocols = vec![alpn.to_vec()];
    let tcp = tokio::net::TcpStream::connect(fixture.listener.local_addr().unwrap())
        .await
        .unwrap();
    if let Ok(mut tls) = tokio_rustls::TlsConnector::from(std::sync::Arc::new(config))
        .connect(ServerName::try_from("node-b.morrow.invalid").unwrap(), tcp)
        .await
    {
        if let Some(hello) = claimed {
            let bytes = hello.encode().unwrap();
            let _ = tls.write_all(&(bytes.len() as u32).to_be_bytes()).await;
            let _ = tls.write_all(&bytes).await;
            let _ = tls.flush().await;
        }
        let mut sink = [0u8; 4096];
        let _ = tokio::io::AsyncReadExt::read(&mut tls, &mut sink).await;
    }
}
#[tokio::test]
async fn trusted_certificate_cannot_claim_another_members_identity() {
    let fixture = Fixture::new().await;
    let security = fixture.security(&fixture.b);
    let client = raw_tls_client(
        &fixture,
        Some(&fixture.b),
        Some(hello(&fixture.config_a, 3)),
    );
    let server = async {
        let (socket, _) = fixture.listener.accept().await.unwrap();
        accept(
            socket,
            &fixture.config_b,
            &security,
            hello(&fixture.config_b, 9),
            IoLimits::default(),
        )
        .await
    };
    let (_, result) = tokio::time::timeout(Duration::from_secs(3), async {
        tokio::join!(client, server)
    })
    .await
    .unwrap();
    assert!(result.is_err());
    assert!(
        result
            .err()
            .unwrap()
            .to_string()
            .contains("does not identify claimed node")
    );
}
#[tokio::test]
async fn certificate_is_mandatory_even_for_a_client_that_trusts_the_cluster_ca() {
    let fixture = Fixture::new().await;
    let security = fixture.security(&fixture.b);
    let client = raw_tls_client(&fixture, None, None);
    let server = async {
        let (socket, _) = fixture.listener.accept().await.unwrap();
        accept(
            socket,
            &fixture.config_b,
            &security,
            hello(&fixture.config_b, 9),
            IoLimits::default(),
        )
        .await
    };
    let (_, result) = tokio::time::timeout(Duration::from_secs(3), async {
        tokio::join!(client, server)
    })
    .await
    .unwrap();
    assert!(result.is_err());
}

#[tokio::test]
async fn legacy_json_alpn_is_rejected_before_identity_or_application_frames() {
    let fixture = Fixture::new().await;
    let security = fixture.security(&fixture.b);
    let client = raw_tls_client_with_alpn(
        &fixture,
        Some(&fixture.a),
        Some(hello(&fixture.config_a, 3)),
        b"morrow.peer.v1",
    );
    let server = async {
        let (socket, _) = fixture.listener.accept().await.unwrap();
        accept(
            socket,
            &fixture.config_b,
            &security,
            hello(&fixture.config_b, 9),
            IoLimits::default(),
        )
        .await
    };
    let (_, result) = tokio::time::timeout(Duration::from_secs(3), async {
        tokio::join!(client, server)
    })
    .await
    .unwrap();
    assert!(result.is_err());
    let error = result.err().unwrap();
    assert!(matches!(
        error
            .get_ref()
            .and_then(|error| error.downcast_ref::<rustls::Error>()),
        Some(rustls::Error::NoApplicationProtocol)
    ));
}
#[tokio::test]
async fn old_handshake_version_is_rejected_even_on_the_binary_alpn() {
    let fixture = Fixture::new().await;
    let security = fixture.security(&fixture.b);
    let mut obsolete = hello(&fixture.config_a, 3);
    obsolete.version = 1;
    let client = raw_tls_client(&fixture, Some(&fixture.a), Some(obsolete));
    let server = async {
        let (socket, _) = fixture.listener.accept().await.unwrap();
        accept(
            socket,
            &fixture.config_b,
            &security,
            hello(&fixture.config_b, 9),
            IoLimits::default(),
        )
        .await
    };
    let (_, result) = tokio::time::timeout(Duration::from_secs(3), async {
        tokio::join!(client, server)
    })
    .await
    .unwrap();
    assert!(result.is_err());
    assert!(
        result
            .err()
            .unwrap()
            .to_string()
            .contains("VersionMismatch")
    );
}
