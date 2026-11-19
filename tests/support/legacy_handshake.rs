// 0.3.2 invitation handshake: two length-prefixed Noise records, no local
// capabilities. Only observe the received payload to verify the new peer's bytes.
use std::{
    io::{Read, Write},
    net::TcpStream,
    time::Duration,
};

fn read_record(stream: &mut TcpStream) -> Vec<u8> {
    let mut header = [0; 2];
    stream.read_exact(&mut header).unwrap();
    let size = usize::from(u16::from_be_bytes(header));
    assert!((1..=4096).contains(&size));
    let mut bytes = vec![0; size];
    stream.read_exact(&mut bytes).unwrap();
    bytes
}

pub fn exchange(mut stream: TcpStream, initiator: bool, local_payload: &[u8]) {
    let timeout = Some(Duration::from_secs(5));
    stream.set_read_timeout(timeout).unwrap();
    stream.set_write_timeout(timeout).unwrap();
    let builder = snow::Builder::new("Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s".parse().unwrap())
        .prologue(b"DeskLink-v1")
        .psk(0, &[42; 32]);
    let mut noise = if initiator {
        builder.build_initiator()
    } else {
        builder.build_responder()
    }
    .unwrap();
    let mut output = [0; 4096];
    for send in [initiator, !initiator] {
        if send {
            let len = noise.write_message(local_payload, &mut output).unwrap();
            stream.write_all(&(len as u16).to_be_bytes()).unwrap();
            stream.write_all(&output[..len]).unwrap();
        } else {
            let len = noise
                .read_message(&read_record(&mut stream), &mut output)
                .unwrap();
            assert_eq!(&output[..len], b"DLVID001");
        }
    }
    let mut transport = noise.into_transport_mode().unwrap();
    let len = transport
        .read_message(&read_record(&mut stream), &mut output)
        .unwrap();
    // Frozen v0.3.2 encrypted single-record Disconnect (four-byte payload).
    assert_eq!(&output[..len], &[0, 0, 0, 4, 8, 0, 0, 0]);
}
