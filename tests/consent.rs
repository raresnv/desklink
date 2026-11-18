#![cfg(feature = "desktop")]

use desklink::{
    desktop::Display,
    protocol::DesktopInfo,
    session::{Decision, Host, HostEvent, Viewer, ViewerEvent},
};
use std::{
    net::{IpAddr, Ipv4Addr},
    time::{Duration, Instant},
};

fn host() -> Host {
    // An intentionally nonexistent monitor proves declined sessions never
    // attempt screen capture or initialize platform input permissions.
    Host::start(
        Display {
            id: u32::MAX,
            info: DesktopInfo {
                name: "Unavailable test display".into(),
                width: 1920,
                height: 1080,
                x: 0,
                y: 0,
                scale: 1.0,
            },
        },
        0,
        IpAddr::V4(Ipv4Addr::LOCALHOST),
    )
    .unwrap()
}

#[test]
fn declined_session_never_opens_a_display_and_viewer_sees_reason() {
    let host = host();
    let viewer = Viewer::connect(host.invitation.clone().unwrap(), "Alice".into(), true).unwrap();
    match host.events.recv_timeout(Duration::from_secs(3)).unwrap() {
        HostEvent::Pending {
            name,
            control,
            reply,
            ..
        } => {
            assert_eq!(name, "Alice");
            assert!(control);
            reply.send(Decision::Reject).unwrap();
        }
        event => panic!("expected a local approval request, got {event:?}"),
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match viewer
            .events
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap()
        {
            ViewerEvent::Waiting => {}
            ViewerEvent::Ended(end) => {
                assert!(end.reason.contains("declined"), "{end:?}");
                break;
            }
            _ => panic!("declined viewer was connected"),
        }
    }
    // The listener survives a declined request.
    let _second_viewer =
        Viewer::connect(host.invitation.clone().unwrap(), "Bob".into(), false).unwrap();
    assert!(matches!(
        host.events.recv_timeout(Duration::from_secs(3)).unwrap(),
        HostEvent::Ended(_)
    ));
    assert!(matches!(
        host.events.recv_timeout(Duration::from_secs(3)).unwrap(),
        HostEvent::Pending { control: false, .. }
    ));
}

#[test]
fn stopping_host_cancels_a_pending_request_promptly() {
    let host = host();
    let viewer = Viewer::connect(host.invitation.clone().unwrap(), "Alice".into(), false).unwrap();
    let event = host.events.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(matches!(event, HostEvent::Pending { .. }), "{event:?}");
    host.stop();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match viewer
            .events
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap()
        {
            ViewerEvent::Ended(_) => break,
            ViewerEvent::Waiting => {}
            _ => panic!("unapproved viewer was connected"),
        }
    }
}

#[test]
fn approved_capture_failure_is_reported_as_an_error() {
    let host = host();
    let viewer = Viewer::connect(host.invitation.clone().unwrap(), "Alice".into(), true).unwrap();
    let HostEvent::Pending { reply, .. } =
        host.events.recv_timeout(Duration::from_secs(3)).unwrap()
    else {
        panic!("expected approval request");
    };
    reply.send(Decision::ViewOnly).unwrap();
    let HostEvent::Ended(end) = host.events.recv_timeout(Duration::from_secs(3)).unwrap() else {
        panic!("expected capture failure");
    };
    assert!(end.failed, "capture failure appeared successful: {end:?}");
    assert!(!end.reason.is_empty(), "{end:?}");
    while let Ok(event) = viewer.events.recv_timeout(Duration::from_secs(3)) {
        if let ViewerEvent::Ended(viewer_end) = event {
            assert!(viewer_end.failed, "{viewer_end:?}");
            assert_eq!(viewer_end.reason, end.reason);
            return;
        }
    }
    panic!("viewer was not disconnected after capture failure");
}

#[test]
fn internet_host_requests_local_approval_and_delivers_rejection() {
    use desklink::{
        relay::{
            server::{serve, RelayConfig},
            RelayEndpoint,
        },
        wire::StopToken,
    };
    let (ready, address) = std::sync::mpsc::channel();
    let shutdown = StopToken::default();
    let worker_shutdown = shutdown.clone();
    let worker = std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                ready.send(listener.local_addr().unwrap()).unwrap();
                serve(
                    listener,
                    RelayConfig {
                        access_code: Some("a deployment access code over 32 bytes".into()),
                        ..RelayConfig::default()
                    },
                    worker_shutdown.cancelled(),
                )
                .await
                .unwrap();
            });
    });
    let endpoint = RelayEndpoint::parse(&format!("ws://{}/v1", address.recv().unwrap())).unwrap();
    let host = Host::start_internet(
        Display {
            id: u32::MAX,
            info: DesktopInfo {
                name: "Missing monitor".into(),
                width: 800,
                height: 600,
                x: 0,
                y: 0,
                scale: 1.0,
            },
        },
        endpoint,
        "a deployment access code over 32 bytes".into(),
    )
    .unwrap();
    let HostEvent::Ready(invitation) = host.events.recv_timeout(Duration::from_secs(3)).unwrap()
    else {
        panic!("missing ready invitation");
    };
    let viewer = Viewer::connect(invitation, "Internet guest".into(), true).unwrap();
    let HostEvent::Pending {
        name,
        address,
        reply,
        ..
    } = host.events.recv_timeout(Duration::from_secs(3)).unwrap()
    else {
        panic!("missing host approval");
    };
    assert_eq!(name, "Internet guest");
    assert!(address.contains("relay"));
    reply.send(Decision::Reject).unwrap();
    loop {
        match viewer.events.recv_timeout(Duration::from_secs(3)).unwrap() {
            ViewerEvent::Waiting => {}
            ViewerEvent::Ended(end) => {
                assert!(end.reason.contains("declined"), "{end:?}");
                break;
            }
            _ => panic!("unapproved viewer connected"),
        }
    }
    shutdown.cancel();
    worker.join().unwrap();
}

#[test]
fn id_requests_require_approval_without_opening_a_display_when_declined() {
    use desklink::{
        device::Identity,
        relay::{
            server::{serve, RelayConfig},
            DeviceEvent, DevicePresence, RelayEndpoint,
        },
        wire::StopToken,
    };
    use std::{
        sync::{mpsc, Arc},
        thread,
    };
    let root = std::env::temp_dir().join(format!("desklink-id-consent-{}", rand::random::<u64>()));
    let (ready, address) = mpsc::channel();
    let stop = StopToken::default();
    let shutdown = stop.clone();
    let server = thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                ready.send(listener.local_addr().unwrap()).unwrap();
                serve(
                    listener,
                    RelayConfig {
                        allow_unauthenticated_loopback: true,
                        ..RelayConfig::default()
                    },
                    shutdown.cancelled(),
                )
                .await
                .unwrap();
            })
    });
    let endpoint = RelayEndpoint::parse(&format!("ws://{}/v2", address.recv().unwrap())).unwrap();
    let host_identity = Arc::new(Identity::load_or_create(&root.join("host")).unwrap());
    let viewer_identity = Arc::new(Identity::load_or_create(&root.join("viewer")).unwrap());
    let host_presence = DevicePresence::connect(
        endpoint.clone(),
        host_identity.clone(),
        String::new(),
        "Host".into(),
    )
    .unwrap();
    let viewer_presence = DevicePresence::connect(
        endpoint,
        viewer_identity.clone(),
        String::new(),
        "Viewer".into(),
    )
    .unwrap();
    let DeviceEvent::Ready(id) = host_presence
        .events
        .recv_timeout(Duration::from_secs(3))
        .unwrap()
    else {
        panic!("host not online")
    };
    assert!(matches!(
        viewer_presence
            .events
            .recv_timeout(Duration::from_secs(3))
            .unwrap(),
        DeviceEvent::Ready(_)
    ));
    viewer_presence.request(id, true).unwrap();
    let DeviceEvent::Incoming(host_offer) = host_presence
        .events
        .recv_timeout(Duration::from_secs(3))
        .unwrap()
    else {
        panic!("no incoming request")
    };
    let DeviceEvent::Outgoing(viewer_offer) = viewer_presence
        .events
        .recv_timeout(Duration::from_secs(3))
        .unwrap()
    else {
        panic!("no outgoing request")
    };
    let host = Host::start_device(
        Display {
            id: u32::MAX,
            info: DesktopInfo {
                name: "Missing display".into(),
                width: 800,
                height: 600,
                x: 0,
                y: 0,
                scale: 1.0,
            },
        },
        host_offer,
        host_identity,
    )
    .unwrap();
    let viewer =
        Viewer::connect_device(viewer_offer, viewer_identity, "Viewer".into(), true).unwrap();
    let HostEvent::Pending { address, reply, .. } =
        host.events.recv_timeout(Duration::from_secs(3)).unwrap()
    else {
        panic!("no local approval")
    };
    assert!(address.starts_with("Computer ID "));
    reply.send(Decision::Reject).unwrap();
    loop {
        match viewer.events.recv_timeout(Duration::from_secs(3)).unwrap() {
            ViewerEvent::Ended(end) => {
                assert!(end.reason.contains("declined"));
                break;
            }
            ViewerEvent::Waiting => {}
            _ => panic!("declined viewer was connected"),
        }
    }
    drop(viewer);
    drop(host);
    drop(host_presence);
    drop(viewer_presence);
    stop.cancel();
    server.join().unwrap();
    let _ = std::fs::remove_dir_all(root);
}
