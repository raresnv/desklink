use desklink::{protocol::Message, transport::SecureConnection, video::VideoProfile};
use std::{
    net::{TcpListener, TcpStream},
    thread,
    time::Duration,
};

#[test]
fn authenticated_loopback_transfers_large_frames_both_ways() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let connection = SecureConnection::accept(stream, &[42; 32]).unwrap();
        assert!(connection.supports_video_settings());
        let (mut reader, mut writer) = connection.split().unwrap();
        assert!(matches!(reader.receive().unwrap(), Message::Request { .. }));
        writer
            .send(&Message::Frame {
                width: 800,
                height: 600,
                jpeg: vec![123; 240_000],
            })
            .unwrap();
        assert!(matches!(
            reader.receive().unwrap(),
            Message::VideoSettings(VideoProfile::Original)
        ));
        assert!(matches!(reader.receive().unwrap(), Message::Disconnect));
    });
    let stream = TcpStream::connect_timeout(&address, Duration::from_secs(2)).unwrap();
    let connection = SecureConnection::connect(stream, &[42; 32]).unwrap();
    assert!(connection.supports_video_settings());
    let (mut reader, mut writer) = connection.split().unwrap();
    writer
        .send(&Message::Request {
            name: "Test viewer".into(),
            control: true,
        })
        .unwrap();
    match reader.receive().unwrap() {
        Message::Frame { jpeg, .. } => assert_eq!(jpeg, vec![123; 240_000]),
        other => panic!("unexpected {other:?}"),
    }
    writer
        .send(&Message::VideoSettings(VideoProfile::Original))
        .unwrap();
    writer.send(&Message::Disconnect).unwrap();
    server.join().unwrap();
}

#[test]
fn wrong_pairing_key_cannot_complete_handshake() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        assert!(SecureConnection::accept(stream, &[42; 32]).is_err());
    });
    assert!(SecureConnection::connect(TcpStream::connect(address).unwrap(), &[43; 32]).is_err());
    server.join().unwrap();
}

#[path = "support/legacy_handshake.rs"]
mod legacy_handshake;

#[test]
fn missing_and_unknown_legacy_payloads_fall_back_in_both_roles() {
    for legacy_is_initiator in [true, false] {
        for payload in [&b""[..], b"DLVID002", b"DLVID001x", b"DLVID00"] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let client_stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
            let server_stream = listener.accept().unwrap().0;
            let (legacy_stream, current_stream) = if legacy_is_initiator {
                (client_stream, server_stream)
            } else {
                (server_stream, client_stream)
            };
            let legacy = thread::spawn(move || {
                legacy_handshake::exchange(legacy_stream, legacy_is_initiator, payload);
            });
            let connection = if legacy_is_initiator {
                SecureConnection::accept(current_stream, &[42; 32])
            } else {
                SecureConnection::connect(current_stream, &[42; 32])
            }
            .unwrap();
            assert!(!connection.supports_video_settings());
            let (_, mut writer) = connection.split().unwrap();
            assert!(writer
                .send(&Message::VideoSettings(VideoProfile::Sharp))
                .is_err());
            assert!(writer
                .send(&Message::Frame {
                    width: 1601,
                    height: 1,
                    jpeg: vec![1]
                })
                .is_err());
            writer.send(&Message::Disconnect).unwrap();
            legacy.join().unwrap();
        }
    }
}
