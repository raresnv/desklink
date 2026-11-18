//! Opt-in smoke check of persistent ID enrollment and encrypted packaged relay traffic.
use desklink::{
    device::Identity,
    protocol::Message,
    relay::{self, DeviceEvent, DevicePresence, RelayEndpoint},
    transport::SecureConnection,
    wire::StopToken,
};
use std::{sync::Arc, thread, time::Duration};

fn configured_endpoint() -> RelayEndpoint {
    let endpoint = RelayEndpoint::parse(
        &std::env::var("DESKLINK_RELAY_TEST_ENDPOINT").expect("local endpoint"),
    )
    .unwrap();
    assert!(
        endpoint.as_str().starts_with("ws://")
            || std::env::var("DESKLINK_RELAY_TEST_ALLOW_PUBLIC").is_ok_and(|v| v == "1"),
        "Public relay tests require DESKLINK_RELAY_TEST_ALLOW_PUBLIC=1"
    );
    endpoint
}

#[test]
#[ignore = "requires an explicitly configured relay; see docs/INTERNET-RELAY.md"]
fn packaged_relay_supports_an_encrypted_session() {
    let endpoint = configured_endpoint();
    let code = std::env::var("DESKLINK_RELAY_TEST_CODE").expect("test enrollment code");
    let saved = std::env::var_os("DESKLINK_ID_TEST_STATE");
    let root = saved
        .clone()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("desklink-id-container-{}", rand::random::<u64>()))
        });
    let host = Arc::new(Identity::load_or_create(&root.join("host")).unwrap());
    let viewer = Arc::new(Identity::load_or_create(&root.join("viewer")).unwrap());
    let host_presence = DevicePresence::connect(
        endpoint.clone(),
        host.clone(),
        code.clone(),
        "Packaged host".into(),
    )
    .unwrap();
    let viewer_presence = DevicePresence::connect(
        endpoint.clone(),
        viewer.clone(),
        code,
        "Packaged viewer".into(),
    )
    .unwrap();
    let DeviceEvent::Ready(id) = host_presence
        .events
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
    else {
        panic!("host not registered")
    };
    host.remember_own_id(&endpoint.identity_realm(), &id)
        .unwrap();
    let DeviceEvent::Ready(viewer_id) = viewer_presence
        .events
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
    else {
        panic!("viewer not registered")
    };
    viewer
        .remember_own_id(&endpoint.identity_realm(), &viewer_id)
        .unwrap();
    viewer_presence.request(id, true).unwrap();
    let DeviceEvent::Incoming(incoming) = host_presence
        .events
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
    else {
        panic!("no host offer")
    };
    let DeviceEvent::Outgoing(outgoing) = viewer_presence
        .events
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
    else {
        panic!("no viewer offer")
    };
    let worker = thread::spawn(move || {
        let wire = relay::join_device(&incoming, &StopToken::default()).unwrap();
        let (_, mut writer) = SecureConnection::accept_device(
            wire,
            &host,
            incoming.peer_key(),
            &incoming.context().prologue(),
        )
        .unwrap()
        .split()
        .unwrap();
        writer
            .send(&Message::Denied("Packaged ID relay verified".into()))
            .unwrap();
    });
    let wire = relay::join_device(&outgoing, &StopToken::default()).unwrap();
    let (mut reader, _) = SecureConnection::connect_device(
        wire,
        &viewer,
        outgoing.peer_key(),
        &outgoing.context().prologue(),
    )
    .unwrap()
    .split()
    .unwrap();
    assert!(
        matches!(reader.receive().unwrap(),Message::Denied(reason) if reason=="Packaged ID relay verified")
    );
    worker.join().unwrap();
    drop(host_presence);
    drop(viewer_presence);
    if saved.is_none() {
        let _ = std::fs::remove_dir_all(root);
    }
}

#[test]
#[ignore = "requires an explicitly configured relay with enrollment authentication"]
fn configured_relay_rejects_wrong_enrollment_code() {
    let root = std::env::temp_dir().join(format!("desklink-rejected-{}", rand::random::<u64>()));
    let identity = Arc::new(Identity::load_or_create(&root).unwrap());
    let presence = DevicePresence::connect(
        configured_endpoint(),
        identity,
        "deliberately-wrong-enrollment-code-123456".into(),
        "Authentication rejection probe".into(),
    )
    .unwrap();
    match presence
        .events
        .recv_timeout(Duration::from_secs(20))
        .unwrap()
    {
        DeviceEvent::Offline(reason) => {
            assert!(reason.contains("Device registration denied"), "{reason}")
        }
        event => panic!("Unexpected event for wrong code: {event:?}"),
    }
    drop(presence);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires an explicitly configured relay; synthetic full-duplex load"]
fn bulk_stream_keeps_reverse_heartbeats_live() {
    use std::{
        io::{Read, Write},
        sync::atomic::{AtomicUsize, Ordering},
        time::Instant,
    };
    let endpoint = configured_endpoint();
    let root = std::path::PathBuf::from(
        std::env::var_os("DESKLINK_ID_TEST_STATE").expect("persistent stress test identities"),
    );
    let code = std::env::var("DESKLINK_RELAY_TEST_CODE").unwrap_or_default();
    let host = Arc::new(Identity::load_or_create(&root.join("duplex-host")).unwrap());
    let viewer = Arc::new(Identity::load_or_create(&root.join("duplex-viewer")).unwrap());
    let hp = DevicePresence::connect(
        endpoint.clone(),
        host,
        code.clone(),
        "Synthetic duplex host".into(),
    )
    .unwrap();
    let vp =
        DevicePresence::connect(endpoint, viewer, code, "Synthetic duplex viewer".into()).unwrap();
    let event = hp.events.recv_timeout(Duration::from_secs(15)).unwrap();
    let DeviceEvent::Ready(id) = event else {
        panic!("host not ready: {event:?}")
    };
    assert!(matches!(
        vp.events.recv_timeout(Duration::from_secs(15)).unwrap(),
        DeviceEvent::Ready(_)
    ));
    vp.request(id, true).unwrap();
    let DeviceEvent::Incoming(offer) = hp.events.recv_timeout(Duration::from_secs(10)).unwrap()
    else {
        panic!("host offer missing")
    };
    let DeviceEvent::Outgoing(other) = vp.events.recv_timeout(Duration::from_secs(10)).unwrap()
    else {
        panic!("viewer offer missing")
    };
    let hs = StopToken::default();
    let vs = StopToken::default();
    let hstop = hs.clone();
    let join = thread::spawn(move || relay::join_device(&offer, &hstop).unwrap());
    let mut v = relay::join_device(&other, &vs).unwrap();
    let mut h = join.join().unwrap();
    let mut hr = h.try_clone().unwrap();
    let mut vr = v.try_clone().unwrap();
    hr.set_read_timeout(Some(Duration::from_secs(4))).unwrap();
    let pings = Arc::new(AtomicUsize::new(0));
    let count = pings.clone();
    let stop = hs.clone();
    let reader = thread::spawn(move || {
        let mut bytes = [0; 8];
        while !stop.stopped() {
            if let Err(e) = hr.read_exact(&mut bytes) {
                return Err(format!(
                    "reverse heartbeat stalled after {} pings: {e}",
                    count.load(Ordering::Relaxed)
                ));
            }
            count.fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    });
    let pause = std::env::var("DESKLINK_RELAY_TEST_DRAIN_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    let stop = vs.clone();
    let drain = thread::spawn(move || {
        let mut b = vec![0; 65536];
        while !stop.stopped() {
            if vr.read(&mut b).is_err() {
                break;
            }
            if pause > 0 {
                thread::sleep(Duration::from_millis(pause));
            }
        }
    });
    let stop = hs.clone();
    let writer = thread::spawn(move || {
        let frame = vec![0x6d; 180_000];
        let began = Instant::now();
        let mut frames = 0;
        while !stop.stopped() && began.elapsed() < Duration::from_secs(22) {
            let t = Instant::now();
            h.write_all(&frame).map_err(|e| e.to_string())?;
            frames += 1;
            if let Some(delay) = Duration::from_millis(16).checked_sub(t.elapsed()) {
                thread::sleep(delay);
            }
        }
        Ok::<_, String>(frames)
    });
    let began = Instant::now();
    let mut sent: usize = 0;
    while began.elapsed() < Duration::from_secs(20) {
        if v.write_all(&[0x50; 8]).is_err() {
            break;
        }
        sent += 1;
        thread::sleep(Duration::from_millis(200));
    }
    let received = pings.load(Ordering::Relaxed);
    hs.cancel();
    vs.cancel();
    let reverse = reader.join().unwrap();
    let _ = drain.join();
    let frames = writer.join().unwrap();
    println!("bulk frames: {frames:?}, reverse heartbeats sent: {sent}, received: {received}, result: {reverse:?}");
    assert!(
        received >= sent.saturating_sub(5),
        "reverse direction starved: sent {sent}, received {received}, {reverse:?}"
    );
    assert!(received >= 20, "too few heartbeat observations");
}

#[test]
#[ignore = "requires the deployed trusted proxy; synthetic header-spoofing check"]
fn public_edge_replaces_untrusted_proxy_headers() {
    use desklink::device::answer_challenge;
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message as WebMessage};
    let endpoint = configured_endpoint();
    let root = std::path::PathBuf::from(
        std::env::var_os("DESKLINK_ID_TEST_STATE").expect("persistent identities"),
    );
    let identity = Identity::load_or_create(&root.join("untrusted-header-probe")).unwrap();
    let _ = rustls::crypto::ring::default_provider().install_default();
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        tokio::time::timeout(Duration::from_secs(15), async {
            let mut request = endpoint.device_url().into_client_request().unwrap();
            request.headers_mut().insert(
                "x-desklink-proxy-token",
                "untrusted-client-value".parse().unwrap(),
            );
            request
                .headers_mut()
                .insert("x-desklink-client-ip", "not-an-ip".parse().unwrap());
            let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
            let hello = serde_json::json!({
                "action": "register", "version": 2,
                "public_key": identity.public_key(),
                "access_code": std::env::var("DESKLINK_RELAY_TEST_CODE").unwrap_or_default(),
                "name": "Proxy boundary probe"
            });
            socket
                .send(WebMessage::Text(
                    serde_json::to_string(&hello).unwrap().into(),
                ))
                .await
                .unwrap();
            let Some(Ok(WebMessage::Text(text))) = socket.next().await else {
                panic!("missing ownership challenge")
            };
            let challenge: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert_eq!(challenge["event"], "challenge", "registration denied");
            let bytes: Vec<u8> = serde_json::from_value(challenge["challenge"].clone()).unwrap();
            let proof = serde_json::json!({
                "action": "proof", "answer": answer_challenge(&identity, &bytes).unwrap()
            });
            socket
                .send(WebMessage::Text(
                    serde_json::to_string(&proof).unwrap().into(),
                ))
                .await
                .unwrap();
            let Some(Ok(WebMessage::Text(text))) = socket.next().await else {
                panic!("missing registration result")
            };
            let registered: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert_eq!(registered["event"], "registered");
        })
        .await
        .unwrap();
    });
}
