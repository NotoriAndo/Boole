use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::{Duration, Instant};

use boole_p2p::{Frame, FrameError, PeerId, TlsIdentity, TlsTransport, Transport};

fn identity() -> TlsIdentity {
    let secret = TlsIdentity::generate_pkcs8().unwrap();
    TlsIdentity::from_pkcs8(&secret).unwrap()
}

#[test]
fn two_pinned_nodes_authenticate_each_other_and_exchange_existing_frames() {
    let alice = identity();
    let bob = identity();
    let alice_id = alice.peer_id();
    let bob_id = bob.peer_id();
    let mut listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = TlsTransport::new(alice, vec![("127.0.0.1:1".parse().unwrap(), bob_id)]).unwrap();
    let client = TlsTransport::new(bob, vec![(addr, alice_id)]).unwrap();
    let task = std::thread::spawn(move || {
        let (mut conn, _) = server.accept(&mut listener).unwrap();
        assert_eq!(conn.peer_id(), bob_id);
        assert_eq!(
            server.recv_frame(&mut conn).unwrap(),
            Frame::GetBlocks { from: 1, to: 3 }
        );
        server
            .send_frame(
                &mut conn,
                &Frame::Blocks {
                    blocks: vec![serde_json::json!({"secret": "native-frame"})],
                },
            )
            .unwrap();
    });
    let mut conn = client.connect(&addr).unwrap();
    assert_eq!(conn.peer_id(), alice_id);
    client
        .send_frame(&mut conn, &Frame::GetBlocks { from: 1, to: 3 })
        .unwrap();
    assert_eq!(
        client.recv_frame(&mut conn).unwrap(),
        Frame::Blocks {
            blocks: vec![serde_json::json!({"secret": "native-frame"})]
        }
    );
    task.join().unwrap();
}

#[test]
fn an_unapproved_server_or_client_key_cannot_exchange_frames() {
    for wrong_server in [false, true] {
        let alice = identity();
        let bob = identity();
        let stranger = identity();
        let alice_id = alice.peer_id();
        let bob_id = bob.peer_id();
        let stranger_id = stranger.peer_id();
        let mut listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = TlsTransport::new(
            alice,
            vec![(
                "127.0.0.1:1".parse().unwrap(),
                if wrong_server { bob_id } else { stranger_id },
            )],
        )
        .unwrap();
        let client = TlsTransport::new(
            bob,
            vec![(address, if wrong_server { stranger_id } else { alice_id })],
        )
        .unwrap();
        let task = std::thread::spawn(move || assert!(server.accept(&mut listener).is_err()));
        if let Ok(mut connection) = client.connect(&address) {
            assert!(client.recv_frame(&mut connection).is_err());
        }
        task.join().unwrap();
    }
}

#[test]
fn plaintext_and_trickled_handshakes_fail_within_one_absolute_deadline() {
    for plaintext in [true, false] {
        let alice = identity();
        let bob = identity();
        let server =
            TlsTransport::new(alice, vec![("127.0.0.1:1".parse().unwrap(), bob.peer_id())])
                .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let task = std::thread::spawn(move || {
            let (socket, _) = listener.accept().unwrap();
            let started = Instant::now();
            let result = server.accept_stream_until(socket, started + Duration::from_millis(150));
            assert!(result.is_err());
            assert!(started.elapsed() < Duration::from_secs(2));
        });
        let mut socket = TcpStream::connect(address).unwrap();
        if plaintext {
            socket
                .write_all(b"{\"type\":\"getBlocks\",\"from\":1,\"to\":1}\n")
                .unwrap();
        } else {
            socket.write_all(&[0x16, 0x03, 0x01, 0x10, 0x00]).unwrap();
            for _ in 0..40 {
                if socket.write_all(&[0]).is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        task.join().unwrap();
    }
}

#[test]
fn counted_frame_budget_is_enforced_and_a_failed_connection_is_fenced() {
    let alice = identity();
    let bob = identity();
    let alice_id = alice.peer_id();
    let bob_id = bob.peer_id();
    let mut listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = TlsTransport::new(alice, vec![("127.0.0.1:1".parse().unwrap(), bob_id)]).unwrap();
    let client = TlsTransport::new(bob, vec![(address, alice_id)]).unwrap();
    let task = std::thread::spawn(move || {
        let (mut connection, _) = server.accept(&mut listener).unwrap();
        assert!(matches!(
            server.recv_frame_counted_until(
                &mut connection,
                20,
                Instant::now() + Duration::from_secs(2)
            ),
            Err(FrameError::FrameBudgetExceeded { cap: 20, .. })
        ));
        assert!(server
            .recv_frame(&mut connection)
            .unwrap_err()
            .to_string()
            .contains("cannot be reused"));
    });
    let mut connection = client.connect(&address).unwrap();
    // Application messages stay behind the same codec cap, after authentication.
    let _ = client.send_frame(
        &mut connection,
        &Frame::Blocks {
            blocks: vec![serde_json::json!({"payload": "x".repeat(256)})],
        },
    );
    task.join().unwrap();
}

#[test]
fn an_authenticated_but_silent_peer_does_not_renew_the_read_deadline() {
    let alice = identity();
    let bob = identity();
    let alice_id = alice.peer_id();
    let bob_id = bob.peer_id();
    let mut listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = TlsTransport::new(alice, vec![("127.0.0.1:1".parse().unwrap(), bob_id)]).unwrap();
    let client = TlsTransport::new(bob, vec![(address, alice_id)]).unwrap();
    let task = std::thread::spawn(move || {
        let (mut connection, _) = server.accept(&mut listener).unwrap();
        let started = Instant::now();
        assert!(server
            .recv_frame_counted_until(&mut connection, 1024, started + Duration::from_millis(100))
            .is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
    });
    let _connection = client.connect(&address).unwrap();
    task.join().unwrap();
}

#[test]
fn private_material_is_not_debug_output_and_unsafe_peer_configuration_is_rejected() {
    let secret = TlsIdentity::generate_pkcs8().unwrap();
    let alice = TlsIdentity::from_pkcs8(&secret).unwrap();
    assert!(!format!("{alice:?}").contains(&hex::encode(&secret[..])));
    let bob = identity();
    let address = "127.0.0.1:1".parse().unwrap();
    assert!(TlsTransport::new(alice.clone(), vec![(address, alice.peer_id())]).is_err());
    assert!(TlsTransport::new(
        alice.clone(),
        vec![(address, bob.peer_id()), (address, bob.peer_id())]
    )
    .is_err());
    assert!(PeerId::from_hex(&"00".repeat(32)).is_err());
    assert!(PeerId::from_hex(&bob.peer_id().to_hex().to_uppercase()).is_err());
    assert!(TlsIdentity::from_pkcs8(&[0; 5000]).is_err());
    let transport = TlsTransport::new(alice, vec![]).unwrap();
    assert!(transport.connect(&address).is_err());
}

#[derive(Debug)]
struct TestServerPin(Vec<u8>);
impl rustls::client::danger::ServerCertVerifier for TestServerPin {
    fn verify_server_cert(
        &self,
        entity: &rustls::pki_types::CertificateDer<'_>,
        intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _: &rustls::pki_types::ServerName<'_>,
        _: &[u8],
        _: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        if entity.as_ref() != self.0 || !intermediates.is_empty() {
            return Err(rustls::Error::General("wrong test server".into()));
        }
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &rustls::pki_types::CertificateDer<'_>,
        _: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General("TLS 1.3 only".into()))
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        key: &rustls::pki_types::CertificateDer<'_>,
        signature: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature_with_raw_key(
            message,
            &rustls::pki_types::SubjectPublicKeyInfoDer::from(key.as_ref()),
            signature,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![rustls::SignatureScheme::ED25519]
    }
    fn requires_raw_public_keys(&self) -> bool {
        true
    }
}

#[test]
fn presenting_an_approved_public_key_without_its_private_key_is_rejected() {
    let alice_secret = TlsIdentity::generate_pkcs8().unwrap();
    let alice = TlsIdentity::from_pkcs8(&alice_secret).unwrap();
    let victim_secret = TlsIdentity::generate_pkcs8().unwrap();
    let victim = TlsIdentity::from_pkcs8(&victim_secret).unwrap();
    let attacker_secret = TlsIdentity::generate_pkcs8().unwrap();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let load = |bytes: &[u8]| {
        provider
            .key_provider
            .load_private_key(rustls::pki_types::PrivatePkcs8KeyDer::from(bytes.to_vec()).into())
            .unwrap()
    };
    let alice_spki = load(&alice_secret).public_key().unwrap().as_ref().to_vec();
    let victim_spki = load(&victim_secret).public_key().unwrap().as_ref().to_vec();
    let wrong_signing_key = load(&attacker_secret);
    let forged = rustls::sign::CertifiedKey::new(
        vec![rustls::pki_types::CertificateDer::from(victim_spki)],
        wrong_signing_key,
    );
    let mut config = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(TestServerPin(alice_spki)))
        .with_client_cert_resolver(Arc::new(
            rustls::client::AlwaysResolvesClientRawPublicKeys::new(Arc::new(forged)),
        ));
    config.alpn_protocols = vec![b"boole-transport/1".to_vec()];
    let mut listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = TlsTransport::new(
        alice,
        vec![("127.0.0.1:1".parse().unwrap(), victim.peer_id())],
    )
    .unwrap();
    let task = std::thread::spawn(move || assert!(server.accept(&mut listener).is_err()));
    let socket = TcpStream::connect(address).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    socket
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let connection = rustls::ClientConnection::new(
        Arc::new(config),
        rustls::pki_types::ServerName::from(address.ip()),
    )
    .unwrap();
    let mut stream = rustls::StreamOwned::new(connection, socket);
    assert!(stream.read(&mut [0; 1]).is_err());
    task.join().unwrap();
}

#[test]
fn anonymous_clients_and_wrong_application_protocols_cannot_enter_the_transport() {
    for anonymous in [true, false] {
        let alice_secret = TlsIdentity::generate_pkcs8().unwrap();
        let alice = TlsIdentity::from_pkcs8(&alice_secret).unwrap();
        let bob_secret = TlsIdentity::generate_pkcs8().unwrap();
        let bob = TlsIdentity::from_pkcs8(&bob_secret).unwrap();
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let alice_spki = provider
            .key_provider
            .load_private_key(
                rustls::pki_types::PrivatePkcs8KeyDer::from(alice_secret.to_vec()).into(),
            )
            .unwrap()
            .public_key()
            .unwrap()
            .as_ref()
            .to_vec();
        let builder = rustls::ClientConfig::builder_with_provider(provider.clone())
            .with_protocol_versions(&[&rustls::version::TLS13])
            .unwrap()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(TestServerPin(alice_spki)));
        let mut config = if anonymous {
            builder.with_no_client_auth()
        } else {
            let key = provider
                .key_provider
                .load_private_key(
                    rustls::pki_types::PrivatePkcs8KeyDer::from(bob_secret.to_vec()).into(),
                )
                .unwrap();
            let certificate = rustls::pki_types::CertificateDer::from(
                key.public_key().unwrap().as_ref().to_vec(),
            );
            builder.with_client_cert_resolver(Arc::new(
                rustls::client::AlwaysResolvesClientRawPublicKeys::new(Arc::new(
                    rustls::sign::CertifiedKey::new(vec![certificate], key),
                )),
            ))
        };
        config.alpn_protocols = vec![if anonymous {
            b"boole-transport/1".to_vec()
        } else {
            b"other-protocol/1".to_vec()
        }];
        let mut listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server =
            TlsTransport::new(alice, vec![("127.0.0.1:1".parse().unwrap(), bob.peer_id())])
                .unwrap();
        let task = std::thread::spawn(move || assert!(server.accept(&mut listener).is_err()));
        let socket = TcpStream::connect(address).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        socket
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let connection = rustls::ClientConnection::new(
            Arc::new(config),
            rustls::pki_types::ServerName::from(address.ip()),
        )
        .unwrap();
        let mut stream = rustls::StreamOwned::new(connection, socket);
        assert!(stream.read(&mut [0; 1]).is_err());
        task.join().unwrap();
    }
}

#[test]
fn typed_codec_does_not_erase_duplicate_json_fields_before_validation() {
    #[derive(Debug, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Strict {
        #[serde(rename = "nonce")]
        _nonce: u64,
    }
    struct Duplicate;
    impl serde::Serialize for Duplicate {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            use serde::ser::SerializeMap;
            let mut map = serializer.serialize_map(Some(2))?;
            map.serialize_entry("nonce", &1u64)?;
            map.serialize_entry("nonce", &2u64)?;
            map.end()
        }
    }
    let alice = identity();
    let bob = identity();
    let mut listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let client = TlsTransport::new(bob.clone(), vec![(address, alice.peer_id())]).unwrap();
    let server =
        TlsTransport::new(alice, vec![("127.0.0.1:1".parse().unwrap(), bob.peer_id())]).unwrap();
    let task = std::thread::spawn(move || {
        let (mut connection, _) = server.accept(&mut listener).unwrap();
        let error = server
            .recv_json_counted_until::<Strict>(
                &mut connection,
                1024,
                Instant::now() + Duration::from_secs(2),
            )
            .unwrap_err();
        assert!(error.to_string().contains("duplicate field"));
    });
    let mut connection = client.connect(&address).unwrap();
    client
        .send_json_counted_until(
            &mut connection,
            &Duplicate,
            1024,
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
    task.join().unwrap();
}

#[test]
fn intercepted_tcp_bytes_do_not_expose_application_plaintext() {
    const SECRET: &str = "closed-local-test-payload-not-visible-on-the-wire-147293";
    let alice = identity();
    let bob = identity();
    let mut listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let server_address = listener.local_addr().unwrap();
    let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy_address = proxy.local_addr().unwrap();
    let client = TlsTransport::new(bob.clone(), vec![(proxy_address, alice.peer_id())]).unwrap();
    let server =
        TlsTransport::new(alice, vec![("127.0.0.1:1".parse().unwrap(), bob.peer_id())]).unwrap();
    let peer = std::thread::spawn(move || {
        let (mut connection, _) = server.accept(&mut listener).unwrap();
        let message = server.recv_frame(&mut connection).unwrap();
        server.send_frame(&mut connection, &message).unwrap();
    });
    let relay = std::thread::spawn(move || {
        fn copy(mut from: TcpStream, mut to: TcpStream) -> Vec<u8> {
            from.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            to.set_write_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut captured = Vec::new();
            let mut buffer = [0; 4096];
            loop {
                let count = from.read(&mut buffer).unwrap();
                if count == 0 {
                    break;
                }
                captured.extend_from_slice(&buffer[..count]);
                assert!(captured.len() < 65_536);
                to.write_all(&buffer[..count]).unwrap();
            }
            let _ = to.shutdown(std::net::Shutdown::Write);
            captured
        }
        let (socket, _) = proxy.accept().unwrap();
        let upstream = TcpStream::connect(server_address).unwrap();
        let back = upstream.try_clone().unwrap();
        let out = socket.try_clone().unwrap();
        let task = std::thread::spawn(move || copy(socket, upstream));
        let mut captured = copy(back, out);
        captured.extend(task.join().unwrap());
        captured
    });
    let mut connection = client.connect(&proxy_address).unwrap();
    let message = Frame::Blocks {
        blocks: vec![serde_json::json!({"payload": SECRET})],
    };
    client.send_frame(&mut connection, &message).unwrap();
    assert_eq!(client.recv_frame(&mut connection).unwrap(), message);
    drop(connection);
    peer.join().unwrap();
    let captured = relay.join().unwrap();
    assert!(captured.len() > SECRET.len());
    assert!(!captured
        .windows(SECRET.len())
        .any(|bytes| bytes == SECRET.as_bytes()));
}
