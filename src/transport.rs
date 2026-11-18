use crate::protocol::{self, Message, MAX_MESSAGE, VERSION};
use crate::wire::Wire;
use anyhow::{ensure, Context, Result};
use snow::{Builder, TransportState};
use std::{
    io::{self, Read, Write},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const PATTERN: &str = "Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s";
const CHUNK: usize = 60_000;
const TAG: usize = 16;
type Cipher = Arc<Mutex<TransportState>>;

pub struct SecureConnection {
    stream: Wire,
    cipher: Cipher,
}
pub struct SecureReader {
    stream: Wire,
    cipher: Cipher,
    timeout: Duration,
}
pub struct SecureWriter {
    stream: Wire,
    cipher: Cipher,
}

impl SecureConnection {
    pub fn connect(stream: impl Into<Wire>, key: &[u8; 32]) -> Result<Self> {
        Self::handshake(stream.into(), key, true)
    }
    pub fn accept(stream: impl Into<Wire>, key: &[u8; 32]) -> Result<Self> {
        Self::handshake(stream.into(), key, false)
    }

    pub fn connect_device(
        stream: impl Into<Wire>,
        identity: &crate::device::Identity,
        remote: &[u8; 32],
        context: &[u8],
    ) -> Result<Self> {
        Self::device_handshake(stream.into(), identity, remote, context, true)
    }
    pub fn accept_device(
        stream: impl Into<Wire>,
        identity: &crate::device::Identity,
        remote: &[u8; 32],
        context: &[u8],
    ) -> Result<Self> {
        Self::device_handshake(stream.into(), identity, remote, context, false)
    }
    fn device_handshake(
        stream: Wire,
        identity: &crate::device::Identity,
        remote: &[u8; 32],
        context: &[u8],
        initiator: bool,
    ) -> Result<Self> {
        ensure!(
            context.len() <= 256 && remote != &[0; 32],
            "Invalid computer identity context"
        );
        let builder = Builder::new("Noise_IK_25519_ChaChaPoly_BLAKE2s".parse()?)
            .local_private_key(identity.private_key())
            .prologue(context);
        let noise = if initiator {
            builder.remote_public_key(remote).build_initiator()?
        } else {
            builder.build_responder()?
        };
        Self::exchange(stream, noise, initiator, Some(remote))
    }
    fn handshake(stream: Wire, key: &[u8; 32], initiator: bool) -> Result<Self> {
        let builder = Builder::new(PATTERN.parse()?).prologue(VERSION).psk(0, key);
        let noise = if initiator {
            builder.build_initiator()?
        } else {
            builder.build_responder()?
        };
        Self::exchange(stream, noise, initiator, None)
    }
    fn exchange(
        mut stream: Wire,
        mut noise: snow::HandshakeState,
        initiator: bool,
        remote: Option<&[u8; 32]>,
    ) -> Result<Self> {
        // Accepted sockets inherit O_NONBLOCK on macOS. The session worker
        // uses blocking I/O with explicit deadlines on every platform.
        stream.set_nonblocking(false)?;
        stream.set_nodelay(true)?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        let mut output = [0; 4096];
        let deadline = Instant::now() + Duration::from_secs(8);
        if initiator {
            let n = noise.write_message(&[], &mut output)?;
            write_packet(&mut stream, &output[..n])?;
            let packet = read_packet(
                &mut DeadlineReader {
                    stream: &mut stream,
                    deadline,
                },
                4096,
            )?;
            noise.read_message(&packet, &mut output).context(
                "Host authentication failed; verify the computer identity or invitation",
            )?;
        } else {
            let packet = read_packet(
                &mut DeadlineReader {
                    stream: &mut stream,
                    deadline,
                },
                4096,
            )?;
            noise
                .read_message(&packet, &mut output)
                .context("Invalid pairing key or computer identity")?;
            if let Some(expected) = remote {
                ensure!(
                    noise.get_remote_static() == Some(expected.as_slice()),
                    "Request came from a different computer identity"
                );
            }
            let n = noise.write_message(&[], &mut output)?;
            write_packet(&mut stream, &output[..n])?;
        }
        ensure!(noise.is_handshake_finished(), "Incomplete Noise handshake");
        if let Some(expected) = remote {
            ensure!(
                noise.get_remote_static() == Some(expected.as_slice()),
                "Remote computer identity does not match"
            );
        }
        Ok(Self {
            stream,
            cipher: Arc::new(Mutex::new(noise.into_transport_mode()?)),
        })
    }

    pub fn split(self) -> Result<(SecureReader, SecureWriter)> {
        Ok((
            SecureReader {
                stream: self.stream.try_clone()?,
                cipher: self.cipher.clone(),
                timeout: Duration::from_secs(15),
            },
            SecureWriter {
                stream: self.stream,
                cipher: self.cipher,
            },
        ))
    }
}

struct DeadlineReader<'a> {
    stream: &'a mut Wire,
    deadline: Instant,
}
impl Read for DeadlineReader<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "Peer timed out"));
        }
        self.stream.set_read_timeout(Some(remaining))?;
        self.stream.read(bytes)
    }
}

impl SecureReader {
    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }
    pub fn receive(&mut self) -> Result<Message> {
        let deadline = Instant::now() + self.timeout;
        let mut reader = DeadlineReader {
            stream: &mut self.stream,
            deadline,
        };
        let bytes = receive_encrypted(&mut reader, &self.cipher)?;
        protocol::decode(&bytes)
    }
}

impl SecureWriter {
    pub fn send(&mut self, message: &Message) -> Result<()> {
        send_encrypted(&mut self.stream, &self.cipher, &protocol::encode(message)?)
    }
}

fn write_packet(writer: &mut impl Write, bytes: &[u8]) -> Result<()> {
    ensure!(
        !bytes.is_empty() && bytes.len() <= u16::MAX as usize,
        "Invalid record size"
    );
    // A relay Write is a complete flushed WebSocket message. Keep the record
    // header with its ciphertext to avoid a tiny extra network write per chunk.
    let mut packet = Vec::with_capacity(bytes.len() + 2);
    packet.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
    packet.extend_from_slice(bytes);
    writer.write_all(&packet)?;
    Ok(())
}

fn read_packet(reader: &mut impl Read, maximum: usize) -> Result<Vec<u8>> {
    let mut header = [0; 2];
    reader.read_exact(&mut header)?;
    let size = u16::from_be_bytes(header) as usize;
    ensure!(size > 0 && size <= maximum, "Record exceeds protocol limit");
    let mut bytes = vec![0; size];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn send_encrypted(writer: &mut impl Write, cipher: &Cipher, bytes: &[u8]) -> Result<()> {
    ensure!(
        !bytes.is_empty() && bytes.len() <= MAX_MESSAGE,
        "Message exceeds protocol limit"
    );
    let mut first = true;
    for chunk in bytes.chunks(CHUNK) {
        let mut plain = Vec::with_capacity(CHUNK + 4);
        if first {
            plain.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
            first = false;
        }
        plain.extend_from_slice(chunk);
        let mut encrypted = vec![0; plain.len() + TAG];
        let n = cipher
            .lock()
            .map_err(|_| anyhow::anyhow!("Cipher unavailable"))?
            .write_message(&plain, &mut encrypted)?;
        write_packet(writer, &encrypted[..n])?;
    }
    Ok(())
}

fn receive_encrypted(reader: &mut impl Read, cipher: &Cipher) -> Result<Vec<u8>> {
    let mut result = Vec::new();
    let mut expected = None;
    loop {
        let encrypted = read_packet(reader, CHUNK + TAG + 4)?;
        ensure!(encrypted.len() > TAG, "Truncated encrypted record");
        let mut plain = vec![0; encrypted.len()];
        let n = cipher
            .lock()
            .map_err(|_| anyhow::anyhow!("Cipher unavailable"))?
            .read_message(&encrypted, &mut plain)
            .context("Message authentication failed")?;
        let data = if expected.is_none() {
            ensure!(n > 4, "Missing message header");
            let size = u32::from_be_bytes(plain[..4].try_into()?) as usize;
            ensure!(
                size > 0 && size <= MAX_MESSAGE,
                "Message exceeds protocol limit"
            );
            expected = Some(size);
            &plain[4..n]
        } else {
            &plain[..n]
        };
        ensure!(
            !data.is_empty() && result.len() + data.len() <= expected.unwrap_or(0),
            "Invalid message chunk"
        );
        result.extend_from_slice(data);
        if result.len() == expected.unwrap_or(0) {
            return Ok(result);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn pair() -> (Cipher, Cipher) {
        let key = [7; 32];
        let mut a = Builder::new(PATTERN.parse().unwrap())
            .psk(0, &key)
            .build_initiator()
            .unwrap();
        let mut b = Builder::new(PATTERN.parse().unwrap())
            .psk(0, &key)
            .build_responder()
            .unwrap();
        let mut buf = [0; 1024];
        let mut out = [0; 1024];
        let n = a.write_message(&[], &mut buf).unwrap();
        b.read_message(&buf[..n], &mut out).unwrap();
        let n = b.write_message(&[], &mut buf).unwrap();
        a.read_message(&buf[..n], &mut out).unwrap();
        (
            Arc::new(Mutex::new(a.into_transport_mode().unwrap())),
            Arc::new(Mutex::new(b.into_transport_mode().unwrap())),
        )
    }

    #[test]
    fn record_header_and_ciphertext_use_one_write_without_changing_bytes() {
        #[derive(Default)]
        struct Recorder {
            writes: Vec<Vec<u8>>,
        }
        impl std::io::Write for Recorder {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.writes.push(bytes.to_vec());
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut wire = Recorder::default();
        write_packet(&mut wire, &[7, 8, 9]).unwrap();
        assert_eq!(wire.writes.concat(), vec![0, 3, 7, 8, 9]);
        assert_eq!(
            wire.writes.len(),
            1,
            "Tiny record headers must not incur a separate relay flush"
        );
    }

    #[test]
    fn tampered_records_are_rejected() {
        let (a, b) = pair();
        let mut wire = Vec::new();
        send_encrypted(&mut wire, &a, b"hello").unwrap();
        *wire.last_mut().unwrap() ^= 1;
        assert!(receive_encrypted(&mut Cursor::new(wire), &b).is_err());
    }

    #[test]
    fn forged_large_record_is_rejected_before_allocation() {
        assert!(read_packet(&mut Cursor::new([255, 255]), 4096).is_err());
    }

    #[test]
    fn authenticated_oversized_message_header_is_rejected() {
        let (a, b) = pair();
        let mut plain = ((MAX_MESSAGE + 1) as u32).to_be_bytes().to_vec();
        plain.push(1);
        let mut encrypted = vec![0; plain.len() + TAG];
        let n = a
            .lock()
            .unwrap()
            .write_message(&plain, &mut encrypted)
            .unwrap();
        let mut wire = Vec::new();
        write_packet(&mut wire, &encrypted[..n]).unwrap();
        assert!(receive_encrypted(&mut Cursor::new(wire), &b).is_err());
    }
}
