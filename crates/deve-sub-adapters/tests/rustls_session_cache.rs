#![allow(clippy::expect_used)]

//! RUSTSEC-2026-0349 regression under the adapter's ring TLS feature set.
//! These in-memory handshakes check when a server preference becomes trusted;
//! they do not claim a post-quantum downgrade is reachable with ring.

use std::io::Cursor;
use std::sync::Arc;

use rustls::client::{ClientSessionMemoryCache, ClientSessionStore, Resumption};
use rustls::pki_types::{PrivateKeyDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore, ServerConfig, ServerConnection};

struct ServerFlight {
    client: ClientConnection,
    server: ServerConnection,
    cache: Arc<ClientSessionMemoryCache>,
    name: ServerName<'static>,
    wire: Vec<u8>,
}

fn server_flight(trust_certificate: bool) -> ServerFlight {
    let name = ServerName::try_from("session-cache.example.test").expect("test server name");
    let cert = rcgen::generate_simple_self_signed(vec!["session-cache.example.test".into()])
        .expect("test certificate");
    let mut roots = RootCertStore::empty();
    if trust_certificate {
        roots.add(cert.cert.der().clone()).expect("test trust root");
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = ClientConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .expect("TLS 1.3 client")
        .with_root_certificates(roots)
        .with_no_client_auth();
    // Match rustls's default capacity; tiny caches in 0.23.45 evict the
    // first entry immediately and would mask the authentication regression.
    let cache = Arc::new(ClientSessionMemoryCache::new(256));
    config.resumption = Resumption::store(cache.clone());
    let mut client = ClientConnection::new(Arc::new(config), name.clone()).expect("client");
    let config = ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .expect("TLS 1.3 server")
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.cert.der().clone()],
            PrivateKeyDer::Pkcs8(cert.key_pair.serialize_der().into()),
        )
        .expect("server certificate");
    let mut server = ServerConnection::new(Arc::new(config)).expect("server");
    let mut hello = Vec::new();
    client.write_tls(&mut hello).expect("write ClientHello");
    server
        .read_tls(&mut Cursor::new(hello))
        .expect("read ClientHello");
    server.process_new_packets().expect("process ClientHello");
    let mut wire = Vec::new();
    server.write_tls(&mut wire).expect("write server flight");
    ServerFlight {
        client,
        server,
        cache,
        name,
        wire,
    }
}

#[test]
fn key_exchange_hint_is_saved_only_after_authenticated_handshake() {
    let mut flight = server_flight(true);
    // WHY: deliver only the plaintext ServerHello record, withholding the
    // authenticated part of the flight. A complete valid handshake is the
    // positive control, so disabling the session cache cannot pass this test.
    assert_eq!(flight.wire.first(), Some(&22), "handshake record");
    assert_eq!(flight.wire.get(5), Some(&2), "ServerHello message");
    let record_end = 5 + usize::from(u16::from_be_bytes([flight.wire[3], flight.wire[4]]));
    assert!(
        record_end < flight.wire.len(),
        "authenticated flight remains"
    );
    flight
        .client
        .read_tls(&mut Cursor::new(&flight.wire[..record_end]))
        .expect("read ServerHello");
    flight
        .client
        .process_new_packets()
        .expect("process ServerHello");
    assert!(flight.client.is_handshaking());
    assert!(flight.client.negotiated_cipher_suite().is_some());
    assert_eq!(
        flight.cache.kx_hint(&flight.name),
        None,
        "untrusted hint must not persist"
    );

    flight
        .client
        .read_tls(&mut Cursor::new(&flight.wire[record_end..]))
        .expect("read authenticated flight");
    flight
        .client
        .process_new_packets()
        .expect("authenticate server");
    assert!(!flight.client.is_handshaking());
    let group = flight
        .client
        .negotiated_key_exchange_group()
        .expect("negotiated group")
        .name();
    assert_eq!(flight.cache.kx_hint(&flight.name), Some(group));

    let mut finished = Vec::new();
    flight
        .client
        .write_tls(&mut finished)
        .expect("write client Finished");
    flight
        .server
        .read_tls(&mut Cursor::new(finished))
        .expect("read client Finished");
    flight
        .server
        .process_new_packets()
        .expect("complete handshake");
    assert!(!flight.server.is_handshaking());
}

#[test]
fn rejected_certificate_does_not_persist_key_exchange_hint() {
    let mut flight = server_flight(false);
    flight
        .client
        .read_tls(&mut Cursor::new(flight.wire))
        .expect("read server flight");
    assert!(matches!(
        flight.client.process_new_packets(),
        Err(rustls::Error::InvalidCertificate(
            rustls::CertificateError::UnknownIssuer
        ))
    ));
    assert_eq!(
        flight.cache.kx_hint(&flight.name),
        None,
        "failed authentication must not poison the cache"
    );
}
