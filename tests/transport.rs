use desklink::{protocol::Message, transport::SecureConnection};
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
        let (mut reader, mut writer) = SecureConnection::accept(stream, &[42; 32])
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
    let stream = TcpStream::connect_timeout(&address, Duration::from_secs(2)).unwrap();
    let (mut reader, mut writer) = SecureConnection::connect(stream, &[42; 32])
        .unwrap()
        .split()
        .unwrap();
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
