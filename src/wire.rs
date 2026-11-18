//! Blocking session I/O over either LAN TCP or a bounded outbound relay tunnel.
use std::{
    io::{self, Read, Write},
    net::{Shutdown, TcpStream},
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{mpsc as asynchronous, watch};

#[derive(Clone)]
pub struct StopToken(watch::Sender<bool>);
impl Default for StopToken {
    fn default() -> Self {
        Self(watch::channel(false).0)
    }
}
impl StopToken {
    pub fn cancel(&self) {
        self.0.send_replace(true);
    }
    pub fn stopped(&self) -> bool {
        *self.0.borrow()
    }
    pub async fn cancelled(&self) {
        let mut receiver = self.0.subscribe();
        let _ = receiver.wait_for(|stopped| *stopped).await;
    }
}

pub enum Wire {
    Tcp(TcpStream),
    Relay(Tunnel),
}
impl From<TcpStream> for Wire {
    fn from(stream: TcpStream) -> Self {
        Self::Tcp(stream)
    }
}
struct Incoming {
    receiver: mpsc::Receiver<Vec<u8>>,
    bytes: Vec<u8>,
    position: usize,
}
pub(crate) struct Outgoing {
    pub bytes: Vec<u8>,
    pub sent: mpsc::SyncSender<()>,
}

pub struct Tunnel {
    incoming: Arc<Mutex<Incoming>>,
    outgoing: asynchronous::Sender<Outgoing>,
    stop: StopToken,
    read_timeout: Duration,
    write_timeout: Duration,
}
impl Tunnel {
    pub(crate) fn new(
        incoming: mpsc::Receiver<Vec<u8>>,
        outgoing: asynchronous::Sender<Outgoing>,
        stop: StopToken,
    ) -> Self {
        Self {
            incoming: Arc::new(Mutex::new(Incoming {
                receiver: incoming,
                bytes: Vec::new(),
                position: 0,
            })),
            outgoing,
            stop,
            read_timeout: Duration::from_secs(15),
            write_timeout: Duration::from_secs(5),
        }
    }
}
impl Wire {
    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(match self {
            Self::Tcp(stream) => Self::Tcp(stream.try_clone()?),
            Self::Relay(t) => Self::Relay(Tunnel {
                incoming: t.incoming.clone(),
                outgoing: t.outgoing.clone(),
                stop: t.stop.clone(),
                read_timeout: t.read_timeout,
                write_timeout: t.write_timeout,
            }),
        })
    }
    pub fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        match self {
            Self::Tcp(s) => s.shutdown(how),
            Self::Relay(t) => {
                t.stop.cancel();
                Ok(())
            }
        }
    }
    pub fn set_nonblocking(&self, value: bool) -> io::Result<()> {
        match self {
            Self::Tcp(s) => s.set_nonblocking(value),
            Self::Relay(_) if !value => Ok(()),
            _ => Err(io::Error::other("Relay requires blocking session I/O")),
        }
    }
    pub fn set_nodelay(&self, value: bool) -> io::Result<()> {
        match self {
            Self::Tcp(s) => s.set_nodelay(value),
            Self::Relay(_) => Ok(()),
        }
    }
    pub fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        match self {
            Self::Tcp(s) => s.set_read_timeout(timeout),
            Self::Relay(t) => {
                t.read_timeout = timeout.unwrap_or(Duration::from_secs(15));
                Ok(())
            }
        }
    }
    pub fn set_write_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        match self {
            Self::Tcp(s) => s.set_write_timeout(timeout),
            Self::Relay(t) => {
                t.write_timeout = timeout.unwrap_or(Duration::from_secs(5));
                Ok(())
            }
        }
    }
}
impl Read for Wire {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let Self::Relay(t) = self else {
            if let Self::Tcp(s) = self {
                return s.read(output);
            }
            unreachable!()
        };
        if output.is_empty() {
            return Ok(0);
        }
        if t.stop.stopped() {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "Relay disconnected",
            ));
        }
        let mut incoming = t
            .incoming
            .lock()
            .map_err(|_| io::Error::other("Relay unavailable"))?;
        if incoming.position == incoming.bytes.len() {
            match incoming.receiver.recv_timeout(t.read_timeout) {
                Ok(bytes) => {
                    incoming.bytes = bytes;
                    incoming.position = 0;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    return Err(io::Error::new(io::ErrorKind::TimedOut, "Relay timed out"))
                }
                Err(_) => return Ok(0),
            }
        }
        let count = output.len().min(incoming.bytes.len() - incoming.position);
        output[..count]
            .copy_from_slice(&incoming.bytes[incoming.position..incoming.position + count]);
        incoming.position += count;
        Ok(count)
    }
}
impl Write for Wire {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let Self::Relay(t) = self else {
            if let Self::Tcp(s) = self {
                return s.write(bytes);
            }
            unreachable!()
        };
        if bytes.is_empty() {
            return Ok(0);
        }
        let count = bytes.len().min(crate::relay::CHUNK);
        let (sent, acknowledgment) = mpsc::sync_channel(1);
        let mut chunk = Outgoing {
            bytes: bytes[..count].to_vec(),
            sent,
        };
        let deadline = Instant::now() + t.write_timeout;
        loop {
            if t.stop.stopped() {
                return Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    "Relay disconnected",
                ));
            }
            match t.outgoing.try_send(chunk) {
                Ok(()) => {
                    acknowledgment
                        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                        .map_err(|error| match error {
                            mpsc::RecvTimeoutError::Timeout => {
                                io::Error::new(io::ErrorKind::TimedOut, "Relay write timed out")
                            }
                            mpsc::RecvTimeoutError::Disconnected => {
                                io::Error::new(io::ErrorKind::BrokenPipe, "Relay disconnected")
                            }
                        })?;
                    return Ok(count);
                }
                Err(asynchronous::error::TrySendError::Closed(_)) => {
                    return Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "Relay disconnected",
                    ))
                }
                Err(asynchronous::error::TrySendError::Full(bytes)) => chunk = bytes,
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Relay write timed out",
                ));
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Tcp(s) => s.flush(),
            Self::Relay(_) => Ok(()),
        }
    }
}
