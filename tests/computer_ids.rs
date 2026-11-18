use desklink::{
    device::Identity,
    protocol::Message,
    relay::{
        join_device,
        server::{serve, RelayConfig},
        DeviceEvent, DevicePresence, RelayEndpoint,
    },
    transport::SecureConnection,
    wire::StopToken,
};
use std::{
    path::PathBuf,
    sync::{mpsc, Arc},
    thread,
    time::Duration,
};
struct TestRelay {
    endpoint: RelayEndpoint,
    stop: StopToken,
    worker: Option<thread::JoinHandle<()>>,
    root: PathBuf,
}
impl TestRelay {
    fn start() -> Self {
        Self::with_config(RelayConfig {
            access_code: Some("private-computer-id-test-access-code".into()),
            ..RelayConfig::default()
        })
    }
    fn with_config(config: RelayConfig) -> Self {
        let root = std::env::temp_dir().join(format!(
            "desklink-id-relay-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let (send, recv) = mpsc::channel();
        let stop = StopToken::default();
        let shutdown = stop.clone();
        let worker = thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                    send.send(listener.local_addr().unwrap()).unwrap();
                    serve(listener, config, shutdown.cancelled()).await.unwrap();
                })
        });
        Self {
            endpoint: RelayEndpoint::parse(&format!("ws://{}/v2", recv.recv().unwrap())).unwrap(),
            stop,
            worker: Some(worker),
            root,
        }
    }
    fn device(&self, name: &str) -> (Arc<Identity>, DevicePresence, desklink::device::ComputerId) {
        let identity = Arc::new(Identity::load_or_create(&self.root.join(name)).unwrap());
        let presence = DevicePresence::connect(
            self.endpoint.clone(),
            identity.clone(),
            "private-computer-id-test-access-code".into(),
            name.into(),
        )
        .unwrap();
        let event = presence
            .events
            .recv_timeout(Duration::from_secs(4))
            .unwrap();
        let DeviceEvent::Ready(id) = event else {
            panic!("device not online: {event:?}");
        };
        (identity, presence, id)
    }
}
impl Drop for TestRelay {
    fn drop(&mut self) {
        self.stop.cancel();
        self.worker.take().unwrap().join().unwrap();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn offer(presence: &DevicePresence) -> desklink::relay::addressed::ConnectionOffer {
    loop {
        match presence
            .events
            .recv_timeout(Duration::from_secs(4))
            .unwrap()
        {
            DeviceEvent::Incoming(offer) | DeviceEvent::Outgoing(offer) => return offer,
            DeviceEvent::SessionEnded(_) => {}
            event => panic!("unexpected {event:?}"),
        }
    }
}
#[test]
fn numeric_address_connects_authenticated_devices_repeatedly_without_invitations() {
    let relay = TestRelay::start();
    let (host, host_presence, host_id) = relay.device("Host");
    let (viewer, viewer_presence, _) = relay.device("Viewer");
    for _ in 0..2 {
        viewer_presence.request(host_id.clone(), true).unwrap();
        let host_offer = offer(&host_presence);
        let viewer_offer = offer(&viewer_presence);
        let host = host.clone();
        let worker = thread::spawn(move || {
            let wire = join_device(&host_offer, &StopToken::default()).unwrap();
            let (mut reader, mut writer) = SecureConnection::accept_device(
                wire,
                &host,
                host_offer.peer_key(),
                &host_offer.context().prologue(),
            )
            .unwrap()
            .split()
            .unwrap();
            assert!(matches!(reader.receive().unwrap(), Message::Request { .. }));
            writer
                .send(&Message::Denied("Host declined".into()))
                .unwrap();
        });
        let wire = join_device(&viewer_offer, &StopToken::default()).unwrap();
        let (mut reader, mut writer) = SecureConnection::connect_device(
            wire,
            &viewer,
            viewer_offer.peer_key(),
            &viewer_offer.context().prologue(),
        )
        .unwrap()
        .split()
        .unwrap();
        writer
            .send(&Message::Request {
                name: "Viewer".into(),
                control: true,
            })
            .unwrap();
        assert!(matches!(reader.receive().unwrap(), Message::Denied(_)));
        worker.join().unwrap();
        loop {
            if matches!(
                host_presence
                    .events
                    .recv_timeout(Duration::from_secs(3))
                    .unwrap(),
                DeviceEvent::SessionEnded(_)
            ) {
                break;
            }
        }
    }
}
#[test]
fn offline_and_self_addresses_do_not_connect() {
    let relay = TestRelay::start();
    let (_, presence, id) = relay.device("Host");
    presence.request(id, true).unwrap();
    assert!(matches!(
        presence
            .events
            .recv_timeout(Duration::from_secs(3))
            .unwrap(),
        DeviceEvent::RequestFailed { .. }
    ));
}

#[test]
fn offline_busy_and_cancellation_are_bounded_and_host_stays_available() {
    let relay = TestRelay::start();
    let (_, host, host_id) = relay.device("Host");
    let (_, viewer, _) = relay.device("Viewer");
    let (_, other, _) = relay.device("Other");
    viewer
        .request(
            desklink::device::ComputerId::parse("999999999999").unwrap(),
            true,
        )
        .unwrap();
    assert!(
        matches!(viewer.events.recv_timeout(Duration::from_secs(3)).unwrap(),DeviceEvent::RequestFailed{reason,..} if reason.contains("offline"))
    );
    viewer.request(host_id.clone(), true).unwrap();
    let incoming = offer(&host);
    let outgoing = offer(&viewer);
    other.request(host_id.clone(), true).unwrap();
    assert!(
        matches!(other.events.recv_timeout(Duration::from_secs(3)).unwrap(),DeviceEvent::RequestFailed{reason,..} if reason.contains("busy"))
    );
    host.cancel_request(incoming.context().request);
    assert!(matches!(
        host.events.recv_timeout(Duration::from_secs(3)).unwrap(),
        DeviceEvent::SessionEnded(_)
    ));
    assert!(join_device(&outgoing, &StopToken::default()).is_err());
    other.request(host_id, true).unwrap();
    let next = offer(&host);
    let _ = offer(&other);
    host.cancel_request(next.context().request);
}

#[test]
fn static_key_and_request_context_mismatches_fail_before_session_messages() {
    for wrong_key in [true, false] {
        let relay = TestRelay::start();
        let (host, host_presence, host_id) = relay.device("Host");
        let (viewer, viewer_presence, _) = relay.device("Viewer");
        let (attacker, _, _) = relay.device("Attacker");
        viewer_presence.request(host_id, true).unwrap();
        let host_offer = offer(&host_presence);
        let viewer_offer = offer(&viewer_presence);
        let worker = thread::spawn(move || {
            let wire = join_device(&host_offer, &StopToken::default()).unwrap();
            assert!(SecureConnection::accept_device(
                wire,
                &host,
                host_offer.peer_key(),
                &host_offer.context().prologue()
            )
            .is_err());
        });
        let wire = join_device(&viewer_offer, &StopToken::default()).unwrap();
        let mut context = viewer_offer.context().prologue();
        if !wrong_key {
            context[0] ^= 1;
        }
        assert!(SecureConnection::connect_device(
            wire,
            if wrong_key { &attacker } else { &viewer },
            viewer_offer.peer_key(),
            &context
        )
        .is_err());
        worker.join().unwrap();
    }
}

#[test]
fn access_request_throttle_survives_failed_targets() {
    let relay = TestRelay::start();
    let (_, presence, _) = relay.device("Viewer");
    for index in 0..7 {
        presence
            .request(
                desklink::device::ComputerId::parse("999999999999").unwrap(),
                false,
            )
            .unwrap();
        let DeviceEvent::RequestFailed { reason, .. } = presence
            .events
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
        else {
            panic!("missing request error");
        };
        assert_eq!(reason.contains("Too many"), index == 6, "{reason}");
    }
}

#[test]
fn reconnecting_a_device_does_not_reset_its_request_limit() {
    let relay = TestRelay::start();
    let (_, presence, _) = relay.device("Viewer");
    for _ in 0..6 {
        presence
            .request(
                desklink::device::ComputerId::parse("999999999999").unwrap(),
                false,
            )
            .unwrap();
        assert!(matches!(
            presence
                .events
                .recv_timeout(Duration::from_secs(3))
                .unwrap(),
            DeviceEvent::RequestFailed { .. }
        ));
    }
    drop(presence);
    let (_, reconnected, _) = relay.device("Viewer");
    reconnected
        .request(
            desklink::device::ComputerId::parse("999999999999").unwrap(),
            false,
        )
        .unwrap();
    assert!(
        matches!(reconnected.events.recv_timeout(Duration::from_secs(3)).unwrap(),DeviceEvent::RequestFailed{reason,..} if reason.contains("Too many"))
    );
}

#[test]
fn cancelling_then_immediately_requesting_again_keeps_the_new_request() {
    let relay = TestRelay::start();
    let (_, host, host_id) = relay.device("Host");
    let (_, viewer, _) = relay.device("Viewer");
    viewer.request(host_id.clone(), true).unwrap();
    let _ = offer(&host);
    let old = offer(&viewer);
    viewer.cancel_pending();
    let next = viewer.request(host_id, false).unwrap();
    let incoming = offer(&host);
    let outgoing = offer(&viewer);
    assert_eq!(incoming.context().request, next);
    assert_eq!(outgoing.context().request, next);
    assert_ne!(next, old.context().request);
    host.cancel_request(next);
}

#[test]
fn source_network_limit_covers_multiple_device_identities() {
    let relay = TestRelay::start();
    let mut devices = Vec::new();
    for index in 0..6 {
        let (_, presence, _) = relay.device(&format!("Viewer{index}"));
        devices.push(presence);
    }
    let target = desklink::device::ComputerId::parse("999999999999").unwrap();
    for presence in &devices {
        for _ in 0..5 {
            presence.request(target.clone(), false).unwrap();
            assert!(
                matches!(presence.events.recv_timeout(Duration::from_secs(3)).unwrap(),DeviceEvent::RequestFailed{reason,..} if !reason.contains("Too many"))
            );
        }
    }
    devices[0].request(target, false).unwrap();
    assert!(
        matches!(devices[0].events.recv_timeout(Duration::from_secs(3)).unwrap(),DeviceEvent::RequestFailed{reason,..} if reason.contains("from this network"))
    );
}

#[tokio::test]
async fn authenticated_proxy_keeps_network_limits_separate_and_rejects_spoofing() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message as Ws};
    let token = "a-separate-proxy-only-secret-32-bytes";
    let relay = TestRelay::with_config(RelayConfig {
        access_code: Some("private-computer-id-test-access-code".into()),
        proxy_token: Some(token.into()),
        ..RelayConfig::default()
    });
    assert!(tokio_tungstenite::connect_async(relay.endpoint.as_str())
        .await
        .is_err());
    async fn registered(
        relay: &TestRelay,
        token: &str,
        ip: &str,
        name: &str,
    ) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>
    {
        let identity = Identity::load_or_create(&relay.root.join(name)).unwrap();
        let mut request = relay.endpoint.as_str().into_client_request().unwrap();
        request
            .headers_mut()
            .insert("x-desklink-proxy-token", token.parse().unwrap());
        request
            .headers_mut()
            .insert("x-desklink-client-ip", ip.parse().unwrap());
        let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
        socket.send(Ws::Text(serde_json::json!({"action":"register","version":2,"public_key":identity.public_key(),"access_code":"private-computer-id-test-access-code","name":name}).to_string().into())).await.unwrap();
        let Ws::Text(text) = socket.next().await.unwrap().unwrap() else {
            panic!("no challenge")
        };
        let event: serde_json::Value = serde_json::from_str(&text).unwrap();
        let challenge: Vec<u8> = serde_json::from_value(event["challenge"].clone()).unwrap();
        let answer = desklink::device::answer_challenge(&identity, &challenge).unwrap();
        socket
            .send(Ws::Text(
                serde_json::json!({"action":"proof","answer":answer})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        let Ws::Text(text) = socket.next().await.unwrap().unwrap() else {
            panic!("no registration")
        };
        assert!(text.contains("registered"));
        socket
    }
    let mut first = registered(&relay, token, "203.0.113.10", "ProxyA").await;
    for index in 0..31 {
        first.send(Ws::Text(serde_json::json!({"action":"request","target":"999999999999","control":false,"request":rand::random::<[u8;32]>()}).to_string().into())).await.unwrap();
        let Ws::Text(text) = first.next().await.unwrap().unwrap() else {
            panic!("no request response")
        };
        if index == 30 {
            assert!(
                text.contains("from this network"),
                "network limit was not applied: {text}"
            );
        }
    }
    let mut second = registered(&relay, token, "198.51.100.20", "ProxyB").await;
    second.send(Ws::Text(serde_json::json!({"action":"request","target":"999999999999","control":false,"request":rand::random::<[u8;32]>()}).to_string().into())).await.unwrap();
    let Ws::Text(text) = second.next().await.unwrap().unwrap() else {
        panic!("no request response")
    };
    assert!(
        text.contains("offline"),
        "unrelated source was throttled: {text}"
    );
    // Admission must use authenticated source IPs rather than the one proxy IP.
    let mut other_networks = Vec::new();
    for index in 1..=18 {
        other_networks.push(
            registered(
                &relay,
                token,
                &format!("192.0.2.{index}"),
                &format!("Network{index}"),
            )
            .await,
        );
    }
}

#[test]
fn public_registration_is_explicit_and_preserves_owned_ids_across_reconnects() {
    let private = TestRelay::start();
    let identity = Arc::new(Identity::load_or_create(&private.root.join("no-code")).unwrap());
    let denied = DevicePresence::connect(
        private.endpoint.clone(),
        identity,
        String::new(),
        "No code".into(),
    )
    .unwrap();
    assert!(matches!(
        denied.events.recv_timeout(Duration::from_secs(4)).unwrap(),
        DeviceEvent::Offline(_)
    ));
    drop(denied);

    let public = TestRelay::with_config(RelayConfig {
        allow_public_enrollment: true,
        new_devices_per_ip_per_hour: 1,
        ..RelayConfig::default()
    });
    let identity = Arc::new(Identity::load_or_create(&public.root.join("public")).unwrap());
    let presence = DevicePresence::connect(
        public.endpoint.clone(),
        identity.clone(),
        String::new(),
        "Public".into(),
    )
    .unwrap();
    let DeviceEvent::Ready(id) = presence
        .events
        .recv_timeout(Duration::from_secs(4))
        .unwrap()
    else {
        panic!("public registration failed")
    };
    drop(presence);
    let other = Arc::new(Identity::load_or_create(&public.root.join("limited")).unwrap());
    let denied = DevicePresence::connect(
        public.endpoint.clone(),
        other,
        String::new(),
        "Limited".into(),
    )
    .unwrap();
    assert!(matches!(
        denied.events.recv_timeout(Duration::from_secs(4)).unwrap(),
        DeviceEvent::Offline(_)
    ));
    drop(denied);
    let presence = DevicePresence::connect(
        public.endpoint.clone(),
        identity,
        String::new(),
        "Public again".into(),
    )
    .unwrap();
    let DeviceEvent::Ready(again) = presence
        .events
        .recv_timeout(Duration::from_secs(4))
        .unwrap()
    else {
        panic!("known identity was throttled")
    };
    assert_eq!(id, again);
}
