use super::{decode_capability, ws_config, Hello, RelayEndpoint, Reply, SETUP_LIMIT};
use crate::wire::{Outgoing, StopToken, Tunnel, Wire};
use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use futures_util::{SinkExt, StreamExt};
use std::{
    sync::{mpsc, OnceLock},
    time::Duration,
};
use tokio::{net::TcpStream, runtime::Runtime, sync::mpsc as asynchronous, time::timeout};
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

pub(crate) type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;
pub(crate) fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        // Explicit selection also works if a downstream crate enables a second
        // provider. Respect an embedding application's already-installed choice.
        let _ = rustls::crypto::ring::default_provider().install_default();
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("Relay runtime")
    })
}

pub struct PendingHost {
    room: [u8; 32],
    expires_in: u64,
    socket: Socket,
}
impl PendingHost {
    pub fn room(&self) -> &[u8; 32] {
        &self.room
    }
    pub fn expires_in(&self) -> u64 {
        self.expires_in
    }
    pub fn wait(mut self, stop: &StopToken) -> Result<Wire> {
        runtime().block_on(async {
            tokio::select! {
                _ = stop.cancelled() => bail!("Sharing stopped"),
                result = timeout(Duration::from_secs(self.expires_in + 5), reply(&mut self.socket)) => {
                    ensure!(matches!(result.context("Invitation expired")??, Reply::Paired), "Expected relay pairing");
                }
            }
            Ok(tunnel(self.socket, stop.clone()))
        })
    }
}

async fn reply(socket: &mut Socket) -> Result<Reply> {
    loop {
        match socket.next().await.context("Relay disconnected")?? {
            Message::Text(text) if text.len() <= SETUP_LIMIT => {
                let reply: Reply = serde_json::from_str(&text).context("Invalid relay response")?;
                if let Reply::Error { reason } = &reply {
                    bail!("Relay: {reason}");
                }
                return Ok(reply);
            }
            Message::Ping(_) | Message::Pong(_) => {
                socket.flush().await?;
            }
            _ => bail!("Invalid relay response"),
        }
    }
}
async fn open(endpoint: &RelayEndpoint, hello: Hello, stop: &StopToken) -> Result<(Socket, Reply)> {
    let setup = async {
        let (mut socket, _) = tokio_tungstenite::connect_async_with_config(
            endpoint.as_str(),
            Some(ws_config()),
            true,
        )
        .await
        .context(
            "Could not connect securely to the relay; check its address and TLS certificate",
        )?;
        socket
            .send(Message::Text(serde_json::to_string(&hello)?.into()))
            .await?;
        let answer = reply(&mut socket).await?;
        Ok((socket, answer))
    };
    tokio::select! {
        _ = stop.cancelled() => bail!("Connection cancelled"),
        result = timeout(Duration::from_secs(12), setup) => result.context("Relay connection timed out")?,
    }
}
pub fn register_host(
    endpoint: &RelayEndpoint,
    access_code: &str,
    stop: &StopToken,
) -> Result<PendingHost> {
    ensure!(access_code.len() <= 256, "Relay access code is too long");
    runtime().block_on(async {
        let (socket, response) = open(
            endpoint,
            Hello::Host {
                version: 1,
                access_code: access_code.to_owned(),
            },
            stop,
        )
        .await?;
        let Reply::Ready { room, expires_in } = response else {
            bail!("Expected relay invitation");
        };
        ensure!(
            (1..=600).contains(&expires_in),
            "Invalid relay invitation lifetime"
        );
        Ok(PendingHost {
            room: decode_capability(&room)?,
            expires_in,
            socket,
        })
    })
}
pub fn connect_viewer(endpoint: &RelayEndpoint, room: &[u8; 32], stop: &StopToken) -> Result<Wire> {
    runtime().block_on(async {
        let (socket, reply) = open(
            endpoint,
            Hello::Viewer {
                version: 1,
                room: URL_SAFE_NO_PAD.encode(room),
            },
            stop,
        )
        .await?;
        ensure!(matches!(reply, Reply::Paired), "Expected relay pairing");
        Ok(tunnel(socket, stop.clone()))
    })
}
pub(crate) fn tunnel(socket: Socket, stop: StopToken) -> Wire {
    let (to_session, incoming) = mpsc::sync_channel(4);
    let (outgoing, mut from_session) = asynchronous::channel::<Outgoing>(4);
    let result = Wire::Relay(Tunnel::new(incoming, outgoing, stop.clone()));
    runtime().spawn(async move {
        let (mut sink, mut stream) = socket.split();
        let read = async {
            while let Some(Ok(message)) = stream.next().await {
                match message {
                    Message::Binary(bytes) if !bytes.is_empty() => {
                        let mut chunk = bytes.to_vec();
                        loop {
                            match to_session.try_send(chunk) {
                                Ok(()) => break,
                                Err(mpsc::TrySendError::Full(bytes)) => chunk = bytes,
                                Err(_) => return,
                            }
                            tokio::time::sleep(Duration::from_millis(2)).await;
                        }
                    }
                    Message::Ping(_) | Message::Pong(_) => {}
                    _ => return,
                }
            }
        };
        let write = async {
            while let Some(chunk) = from_session.recv().await {
                if !matches!(
                    timeout(
                        Duration::from_secs(5),
                        sink.send(Message::Binary(chunk.bytes.into()))
                    )
                    .await,
                    Ok(Ok(()))
                ) {
                    break;
                }
                let _ = chunk.sent.try_send(());
            }
        };
        tokio::select! { _ = stop.cancelled() => {}, _ = read => {}, _ = write => {} }
    });
    result
}
