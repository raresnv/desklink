use desklink::{
    protocol::Message,
    relay::{
        self,
        server::{serve, RelayConfig},
        RelayEndpoint,
    },
    transport::SecureConnection,
    wire::StopToken,
};
use std::{sync::mpsc, thread, time::Duration};
use tokio::sync::oneshot;

struct Relay {
    endpoint: RelayEndpoint,
    stop: Option<oneshot::Sender<()>>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Relay {
    fn start(ttl: Duration) -> Self {
        Self::with_config(RelayConfig {
            access_code: Some("a private deployment code with 32 bytes".into()),
            room_ttl: ttl,
            ..RelayConfig::default()
        })
    }
    fn with_config(config: RelayConfig) -> Self {
        let (ready, receive) = mpsc::channel();
        let (stop, shutdown) = oneshot::channel();
        let worker = thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                    ready.send(listener.local_addr().unwrap()).unwrap();
                    serve(listener, config, async {
                        let _ = shutdown.await;
                    })
                    .await
                    .unwrap();
                });
        });
        let address = receive.recv().unwrap();
        Self {
            endpoint: RelayEndpoint::parse(&format!("ws://{address}/v1")).unwrap(),
            stop: Some(stop),
            worker: Some(worker),
        }
    }
    fn host(&self) -> relay::PendingHost {
        relay::register_host(
            &self.endpoint,
            "a private deployment code with 32 bytes",
            &StopToken::default(),
        )
        .unwrap()
    }
}
impl Drop for Relay {
    fn drop(&mut self) {
        let _ = self.stop.take().unwrap().send(());
        self.worker.take().unwrap().join().unwrap();
    }
}

#[test]
fn relay_transfers_authenticated_frames_and_input_and_consumes_room() {
    let relay = Relay::start(Duration::from_secs(10));
    let host = relay.host();
    let room = *host.room();
    let worker = thread::spawn(move || {
        let wire = host.wait(&StopToken::default()).unwrap();
        let (mut reader, mut writer) = SecureConnection::accept(wire, &[42; 32])
            .unwrap()
            .split()
            .unwrap();
        assert!(matches!(reader.receive().unwrap(), Message::Request { .. }));
        writer
            .send(&Message::Frame {
                width: 800,
                height: 600,
                jpeg: vec![123; 240_000],
            })
            .unwrap();
        assert!(matches!(reader.receive().unwrap(), Message::Disconnect));
    });
    let wire = relay::connect_viewer(&relay.endpoint, &room, &StopToken::default()).unwrap();
    let (mut reader, mut writer) = SecureConnection::connect(wire, &[42; 32])
        .unwrap()
        .split()
        .unwrap();
    writer
        .send(&Message::Request {
            name: "Internet viewer".into(),
            control: true,
        })
        .unwrap();
    match reader.receive().unwrap() {
        Message::Frame { jpeg, .. } => assert_eq!(jpeg, vec![123; 240_000]),
        _ => panic!("missing frame"),
    }
    writer.send(&Message::Disconnect).unwrap();
    worker.join().unwrap();
    assert!(relay::connect_viewer(&relay.endpoint, &room, &StopToken::default()).is_err());
}

#[test]
fn relay_wrong_key_is_rejected_end_to_end() {
    let relay = Relay::start(Duration::from_secs(10));
    let host = relay.host();
    let room = *host.room();
    let worker = thread::spawn(move || {
        assert!(
            SecureConnection::accept(host.wait(&StopToken::default()).unwrap(), &[42; 32]).is_err()
        );
    });
    let wire = relay::connect_viewer(&relay.endpoint, &room, &StopToken::default()).unwrap();
    assert!(SecureConnection::connect(wire, &[43; 32]).is_err());
    worker.join().unwrap();
}

#[test]
fn host_access_code_and_room_expiry_are_enforced() {
    let relay = Relay::start(Duration::from_millis(150));
    assert!(relay::register_host(&relay.endpoint, "wrong", &StopToken::default()).is_err());
    let host = relay.host();
    let room = *host.room();
    assert!(host.wait(&StopToken::default()).is_err());
    assert!(relay::connect_viewer(&relay.endpoint, &room, &StopToken::default()).is_err());
}

#[test]
fn stopping_an_outbound_host_cancels_waiting_and_closes_the_room() {
    let relay = Relay::start(Duration::from_secs(10));
    let stop = StopToken::default();
    let host = relay::register_host(
        &relay.endpoint,
        "a private deployment code with 32 bytes",
        &stop,
    )
    .unwrap();
    stop.cancel();
    let before = std::time::Instant::now();
    assert!(host.wait(&stop).is_err());
    assert!(before.elapsed() < Duration::from_secs(2));
}

#[test]
fn final_denial_is_delivered_before_host_cancellation() {
    let relay = Relay::start(Duration::from_secs(10));
    let stop = StopToken::default();
    let host = relay::register_host(
        &relay.endpoint,
        "a private deployment code with 32 bytes",
        &stop,
    )
    .unwrap();
    let room = *host.room();
    let worker = thread::spawn(move || {
        let (_, mut writer) = SecureConnection::accept(host.wait(&stop).unwrap(), &[42; 32])
            .unwrap()
            .split()
            .unwrap();
        writer
            .send(&Message::Denied("Host declined".into()))
            .unwrap();
        stop.cancel();
    });
    let wire = relay::connect_viewer(&relay.endpoint, &room, &StopToken::default()).unwrap();
    let (mut reader, _) = SecureConnection::connect(wire, &[42; 32])
        .unwrap()
        .split()
        .unwrap();
    assert!(
        matches!(reader.receive().unwrap(), Message::Denied(reason) if reason == "Host declined")
    );
    worker.join().unwrap();
}

#[test]
fn room_limit_does_not_displace_a_registered_host() {
    let relay = Relay::with_config(RelayConfig {
        access_code: Some("a private deployment code with 32 bytes".into()),
        max_rooms: 1,
        ..RelayConfig::default()
    });
    let host = relay.host();
    assert!(relay::register_host(
        &relay.endpoint,
        "a private deployment code with 32 bytes",
        &StopToken::default()
    )
    .is_err());
    let room = *host.room();
    let wire = relay::connect_viewer(&relay.endpoint, &room, &StopToken::default()).unwrap();
    assert!(host.wait(&StopToken::default()).is_ok());
    drop(wire);
}

#[test]
fn stalled_handshake_is_closed_and_connection_capacity_is_reclaimed() {
    use std::io::Read;
    let relay = Relay::with_config(RelayConfig {
        access_code: Some("a private deployment code with 32 bytes".into()),
        max_per_ip: 1,
        setup_timeout: Duration::from_millis(100),
        ..RelayConfig::default()
    });
    let address = relay
        .endpoint
        .as_str()
        .strip_prefix("ws://")
        .unwrap()
        .strip_suffix("/v1")
        .unwrap();
    let mut stalled = std::net::TcpStream::connect(address).unwrap();
    stalled
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    assert_eq!(stalled.read(&mut [0; 1]).unwrap(), 0);
    assert!(relay::register_host(
        &relay.endpoint,
        "a private deployment code with 32 bytes",
        &StopToken::default()
    )
    .is_ok());
}

#[tokio::test]
async fn rejects_unknown_protocol_versions_and_extra_registration_fields() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message as Ws;
    let relay = Relay::start(Duration::from_secs(10));
    for message in [
        r#"{"role":"host","version":2,"access_code":"a private deployment code with 32 bytes"}"#,
        r#"{"role":"host","version":1,"access_code":"a private deployment code with 32 bytes","room":"replace-existing-host"}"#,
    ] {
        let (mut socket, _) = tokio_tungstenite::connect_async(relay.endpoint.as_str())
            .await
            .unwrap();
        socket.send(Ws::Text(message.into())).await.unwrap();
        let response = tokio::time::timeout(Duration::from_secs(2), socket.next())
            .await
            .unwrap();
        assert!(!matches!(response, Some(Ok(Ws::Text(text))) if text.contains("ready")));
    }
}

#[tokio::test]
async fn relay_backend_requires_a_deployment_code_by_default() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    assert!(
        serve(listener, RelayConfig::default(), std::future::pending())
            .await
            .is_err()
    );
}

#[test]
fn wss_verifies_certificates_and_tls_proxy_carries_bidirectional_data() {
    use futures_util::{SinkExt, StreamExt};
    use std::sync::Arc;
    use tokio_rustls::{
        rustls::{self, pki_types::PrivatePkcs8KeyDer},
        TlsAcceptor,
    };
    use tokio_tungstenite::{tungstenite::Message as Ws, Connector};
    let relay = Relay::start(Duration::from_secs(10));
    let backend = relay
        .endpoint
        .as_str()
        .strip_prefix("ws://")
        .unwrap()
        .strip_suffix("/v1")
        .unwrap()
        .to_owned();
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
    let certificate = cert.der().clone();
    let server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![certificate.clone()],
            PrivatePkcs8KeyDer::from(signing_key.serialize_der()).into(),
        )
        .unwrap();
    let (ready, address) = mpsc::channel();
    let stop = StopToken::default();
    let worker_stop = stop.clone();
    let proxy = thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            ready.send(listener.local_addr().unwrap()).unwrap();
            let acceptor = TlsAcceptor::from(Arc::new(server_config));
            let mut tasks = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    _ = worker_stop.cancelled() => break,
                    accepted = listener.accept() => {
                        let (socket, _) = accepted.unwrap(); let acceptor = acceptor.clone(); let backend = backend.clone();
                        tasks.spawn(async move {
                            if let Ok(mut tls) = acceptor.accept(socket).await {
                                let mut upstream = tokio::net::TcpStream::connect(backend).await.unwrap();
                                let _ = tokio::io::copy_bidirectional(&mut tls, &mut upstream).await;
                            }
                        });
                    }
                }
            }
            tasks.abort_all(); while tasks.join_next().await.is_some() {}
        });
    });
    let endpoint = RelayEndpoint::parse(&format!("wss://{}/v1", address.recv().unwrap())).unwrap();
    let error = relay::register_host(
        &endpoint,
        "a private deployment code with 32 bytes",
        &StopToken::default(),
    )
    .err()
    .expect("untrusted TLS certificate was accepted");
    assert!(format!("{error:#}").to_lowercase().contains("certificate"));
    // A test-only trust store verifies the successful TLS route without adding
    // any certificate bypass to the desktop app or the public client API.
    let mut roots = rustls::RootCertStore::empty();
    roots.add(certificate).unwrap();
    let connector = Connector::Rustls(Arc::new(
        rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    ));
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        let (mut host, _) = tokio_tungstenite::connect_async_tls_with_config(endpoint.as_str(), None, true, Some(connector.clone())).await.unwrap();
        host.send(Ws::Text(r#"{"role":"host","version":1,"access_code":"a private deployment code with 32 bytes"}"#.into())).await.unwrap();
        let Ws::Text(ready) = host.next().await.unwrap().unwrap() else { panic!("no room"); };
        let room: serde_json::Value = serde_json::from_str(&ready).unwrap();
        let (mut viewer, _) = tokio_tungstenite::connect_async_tls_with_config(endpoint.as_str(), None, true, Some(connector)).await.unwrap();
        viewer.send(Ws::Text(serde_json::json!({"role":"viewer","version":1,"room":room["room"]}).to_string().into())).await.unwrap();
        assert!(matches!(host.next().await.unwrap().unwrap(), Ws::Text(_)));
        assert!(matches!(viewer.next().await.unwrap().unwrap(), Ws::Text(_)));
        host.send(Ws::Binary(vec![7; 60_000].into())).await.unwrap();
        assert_eq!(viewer.next().await.unwrap().unwrap().into_data(), vec![7; 60_000]);
        viewer.send(Ws::Binary(vec![9; 128].into())).await.unwrap();
        assert_eq!(host.next().await.unwrap().unwrap().into_data(), vec![9; 128]);
    });
    stop.cancel();
    proxy.join().unwrap();
}

#[tokio::test]
async fn rejects_oversized_setup_and_nonbinary_session_messages() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message as Ws;
    let relay = Relay::start(Duration::from_secs(10));
    let (mut oversized, _) = tokio_tungstenite::connect_async(relay.endpoint.as_str())
        .await
        .unwrap();
    oversized
        .send(Ws::Text("x".repeat(2048).into()))
        .await
        .unwrap();
    assert!(!matches!(
        tokio::time::timeout(Duration::from_secs(2), oversized.next())
            .await
            .unwrap(),
        Some(Ok(Ws::Text(_)))
    ));
    let (mut host, _) = tokio_tungstenite::connect_async(relay.endpoint.as_str())
        .await
        .unwrap();
    host.send(Ws::Text(
        r#"{"role":"host","version":1,"access_code":"a private deployment code with 32 bytes"}"#
            .into(),
    ))
    .await
    .unwrap();
    let Ws::Text(ready) = host.next().await.unwrap().unwrap() else {
        panic!("missing room");
    };
    let ready: serde_json::Value = serde_json::from_str(&ready).unwrap();
    let (mut viewer, _) = tokio_tungstenite::connect_async(relay.endpoint.as_str())
        .await
        .unwrap();
    viewer
        .send(Ws::Text(
            serde_json::json!({"role":"viewer","version":1,"room":ready["room"]})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    host.next().await.unwrap().unwrap();
    viewer.next().await.unwrap().unwrap();
    viewer
        .send(Ws::Text("invalid session payload".into()))
        .await
        .unwrap();
    assert!(!matches!(
        tokio::time::timeout(Duration::from_secs(2), host.next())
            .await
            .unwrap(),
        Some(Ok(Ws::Text(_)))
    ));
}

#[test]
fn paired_session_byte_budget_and_idle_timeout_are_enforced() {
    use std::io::{Read, Write};
    for (budget, idle) in [
        (3, Duration::from_secs(2)),
        (1024, Duration::from_millis(100)),
    ] {
        let relay = Relay::with_config(RelayConfig {
            access_code: Some("a private deployment code with 32 bytes".into()),
            max_session_bytes: budget,
            idle_timeout: idle,
            ..RelayConfig::default()
        });
        let host = relay.host();
        let room = *host.room();
        let mut viewer =
            relay::connect_viewer(&relay.endpoint, &room, &StopToken::default()).unwrap();
        let mut host = host.wait(&StopToken::default()).unwrap();
        host.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        if budget == 3 {
            let _ = viewer.write_all(&[1, 2, 3, 4]);
        }
        assert!(matches!(host.read(&mut [0; 8]), Ok(0) | Err(_)));
    }
}
