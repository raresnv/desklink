//! A bounded rendezvous service. The service never receives device private keys.
mod devices;
use super::{decode_capability, ws_config, Hello, Reply, CHUNK, SETUP_LIMIT};
use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use futures_util::{SinkExt, StreamExt};
use rand::{rngs::OsRng, RngCore};
use std::{
    collections::HashMap,
    future::Future,
    net::IpAddr,
    sync::{Arc, Mutex},
    time::Duration,
};
use subtle::ConstantTimeEq;
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{oneshot, OwnedSemaphorePermit, Semaphore},
    task::JoinSet,
    time::{timeout, Instant},
};
use tokio_tungstenite::{
    tungstenite::{
        handshake::server::{ErrorResponse, Request, Response},
        Message,
    },
    WebSocketStream,
};
use zeroize::Zeroize;

pub struct RelayConfig {
    pub access_code: Option<String>,
    pub allow_public_enrollment: bool,
    pub new_devices_per_ip_per_hour: usize,
    pub new_devices_per_hour: usize,
    pub directory_path: Option<std::path::PathBuf>,
    pub proxy_token: Option<String>,
    pub max_devices: usize,
    pub requests_per_minute: usize,
    pub allow_unauthenticated_loopback: bool,
    pub max_connections: usize,
    pub max_per_ip: usize,
    pub max_rooms: usize,
    pub room_ttl: Duration,
    pub setup_timeout: Duration,
    pub idle_timeout: Duration,
    pub session_timeout: Duration,
    pub max_session_bytes: u64,
}
impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            access_code: None,
            allow_public_enrollment: false,
            new_devices_per_ip_per_hour: 30,
            new_devices_per_hour: 300,
            directory_path: None,
            proxy_token: None,
            max_devices: 10_000,
            requests_per_minute: 6,
            allow_unauthenticated_loopback: false,
            max_connections: 128,
            max_per_ip: 16,
            max_rooms: 64,
            room_ttl: Duration::from_secs(600),
            setup_timeout: Duration::from_secs(8),
            idle_timeout: Duration::from_secs(90),
            session_timeout: Duration::from_secs(7200),
            max_session_bytes: 8 * 1024 * 1024 * 1024,
        }
    }
}
struct Admission {
    ip: IpAddr,
    counted: bool,
    addresses: Arc<Mutex<HashMap<IpAddr, usize>>>,
    _permit: OwnedSemaphorePermit,
}
impl Drop for Admission {
    fn drop(&mut self) {
        if !self.counted {
            return;
        }
        if let Ok(mut addresses) = self.addresses.lock() {
            if let Some(count) = addresses.get_mut(&self.ip) {
                *count -= 1;
                if *count == 0 {
                    addresses.remove(&self.ip);
                }
            }
        }
    }
}
impl Admission {
    fn count_source(&mut self, source: IpAddr, maximum: usize) -> bool {
        if self.counted {
            return true;
        }
        let Ok(mut addresses) = self.addresses.lock() else {
            return false;
        };
        let count = addresses.entry(source).or_default();
        if *count >= maximum {
            return false;
        }
        *count += 1;
        self.ip = source;
        self.counted = true;
        true
    }
}
struct Peer {
    socket: WebSocketStream<TcpStream>,
    _admission: Admission,
}
type Rooms = Arc<Mutex<HashMap<String, oneshot::Sender<Peer>>>>;

pub async fn serve(
    listener: TcpListener,
    config: RelayConfig,
    shutdown: impl Future<Output = ()>,
) -> Result<()> {
    ensure!(
        config.max_connections > 0
            && config.max_connections <= 4096
            && config.max_per_ip > 0
            && config.max_per_ip <= config.max_connections
            && config.max_rooms > 0
            && config.max_rooms <= config.max_connections,
        "Invalid relay resource limits"
    );
    ensure!(
        !config.room_ttl.is_zero()
            && config.room_ttl <= Duration::from_secs(600)
            && !config.setup_timeout.is_zero()
            && config.setup_timeout <= Duration::from_secs(30)
            && !config.idle_timeout.is_zero()
            && !config.session_timeout.is_zero()
            && config.max_session_bytes > 0,
        "Invalid relay deadlines or byte limit"
    );
    match &config.access_code {
        Some(code) => ensure!(
            (32..=256).contains(&code.len()),
            "Relay access code must contain 32 to 256 bytes"
        ),
        None => ensure!(
            config.allow_public_enrollment
                || (config.allow_unauthenticated_loopback
                    && listener.local_addr()?.ip().is_loopback()),
            "Configure a relay access code or explicitly enable public device enrollment"
        ),
    }
    ensure!(
        config.new_devices_per_ip_per_hour > 0
            && config.new_devices_per_hour > 0
            && config.new_devices_per_hour <= 10_000,
        "Invalid device enrollment limits"
    );
    ensure!(
        (1..=60).contains(&config.requests_per_minute),
        "Invalid request rate limit"
    );
    if let Some(token) = &config.proxy_token {
        ensure!(
            (32..=256).contains(&token.len()),
            "Proxy token must contain 32 to 256 bytes"
        );
    }
    let devices = devices::Service::new(&config)?;
    let config = Arc::new(config);
    let rooms: Rooms = Arc::default();
    let addresses: Arc<Mutex<HashMap<IpAddr, usize>>> = Arc::default();
    let permits = Arc::new(Semaphore::new(config.max_connections));
    let mut workers = JoinSet::new();
    let mut maintenance = tokio::time::interval(Duration::from_secs(1));
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            _ = maintenance.tick() => devices.reap(),
            Some(_) = workers.join_next(), if !workers.is_empty() => {},
            accepted = listener.accept() => {
                let (stream, address) = accepted?;
                let Ok(permit) = permits.clone().try_acquire_owned() else { continue; };
                let counted = config.proxy_token.is_none();
                let allowed = if !counted { true } else {
                    let mut counts = addresses.lock().map_err(|_| anyhow::anyhow!("Admission unavailable"))?;
                    let count = counts.entry(address.ip()).or_default();
                    if *count >= config.max_per_ip { false } else { *count += 1; true }
                };
                if !allowed { continue; }
                let admission = Admission { ip: address.ip(), counted, addresses: addresses.clone(), _permit: permit };
                let config = config.clone(); let rooms = rooms.clone(); let devices = devices.clone();
                workers.spawn(async move {
                    // Do not log protocol errors: they may contain peer-controlled secrets.
                    let _ = handle(stream, admission, config, rooms, devices).await;
                });
            }
        }
    }
    devices.shutdown().await;
    workers.abort_all();
    while workers.join_next().await.is_some() {}
    Ok(())
}

#[allow(clippy::result_large_err)] // The WebSocket callback fixes this return type.
fn check_request(
    request: &Request,
    response: Response,
) -> std::result::Result<Response, ErrorResponse> {
    if matches!(request.uri().path(), "/v1" | "/v2") && request.uri().query().is_none() {
        return Ok(response);
    }
    Err(tokio_tungstenite::tungstenite::http::Response::builder()
        .status(404)
        .body(Some("Not found".into()))
        .expect("static response"))
}
#[allow(clippy::result_large_err)]
fn proxy_source(
    request: &Request,
    direct: IpAddr,
    token: Option<&str>,
) -> std::result::Result<IpAddr, ErrorResponse> {
    let Some(expected) = token else {
        return Ok(direct);
    };
    let supplied = request
        .headers()
        .get("x-desklink-proxy-token")
        .and_then(|header| header.to_str().ok())
        .unwrap_or("");
    let address = request
        .headers()
        .get("x-desklink-client-ip")
        .and_then(|header| header.to_str().ok())
        .and_then(|text| text.parse::<IpAddr>().ok());
    if bool::from(expected.as_bytes().ct_eq(supplied.as_bytes())) {
        if let Some(address) = address {
            if !address.is_unspecified() && !address.is_multicast() {
                return Ok(address);
            }
        }
    }
    Err(tokio_tungstenite::tungstenite::http::Response::builder()
        .status(403)
        .body(Some("Trusted proxy required".into()))
        .expect("static response"))
}
async fn send(socket: &mut WebSocketStream<TcpStream>, reply: Reply) -> Result<()> {
    timeout(
        Duration::from_secs(5),
        socket.send(Message::Text(serde_json::to_string(&reply)?.into())),
    )
    .await??;
    Ok(())
}
#[allow(clippy::result_large_err)] // The WebSocket callback fixes its error type.
async fn handle(
    stream: TcpStream,
    mut admission: Admission,
    config: Arc<RelayConfig>,
    rooms: Rooms,
    devices: Arc<devices::Service>,
) -> Result<()> {
    stream.set_nodelay(true)?;
    let (mut socket, text, addressed, source_ip) = timeout(config.setup_timeout, async {
        let mut addressed = false;
        let mut source_ip = admission.ip;
        let mut socket = tokio_tungstenite::accept_hdr_async_with_config(
            stream,
            |request: &Request, response| {
                addressed = request.uri().path() == "/v2";
                source_ip = proxy_source(request, admission.ip, config.proxy_token.as_deref())?;
                if !admission.count_source(source_ip, config.max_per_ip) {
                    return Err(tokio_tungstenite::tungstenite::http::Response::builder()
                        .status(429)
                        .body(Some("Too many connections from this network".into()))
                        .expect("static response"));
                }
                check_request(request, response)
            },
            Some(ws_config()),
        )
        .await?;
        let message = socket.next().await.context("Disconnected")??;
        let Message::Text(text) = message else {
            bail!("Expected relay setup");
        };
        ensure!(
            text.len()
                <= if addressed {
                    crate::relay::addressed::LIMIT
                } else {
                    SETUP_LIMIT
                },
            "Setup message too large"
        );
        Ok::<_, anyhow::Error>((socket, text, addressed, source_ip))
    })
    .await??;
    if addressed {
        return devices::handle(socket, &text, admission, source_ip, config, devices).await;
    }
    let hello: Hello = serde_json::from_str(&text)?;
    match hello {
        Hello::Host {
            version,
            mut access_code,
        } => {
            let authorized = version == 1
                && match &config.access_code {
                    Some(expected) => bool::from(expected.as_bytes().ct_eq(access_code.as_bytes())),
                    None => config.allow_unauthenticated_loopback && access_code.is_empty(),
                };
            access_code.zeroize();
            if !authorized {
                send(
                    &mut socket,
                    Reply::Error {
                        reason: "Host registration denied".into(),
                    },
                )
                .await?;
                return Ok(());
            }
            let mut capability = [0; 32];
            OsRng.fill_bytes(&mut capability);
            let room = URL_SAFE_NO_PAD.encode(capability);
            let (sender, mut receiver) = oneshot::channel();
            {
                let mut waiting = rooms
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Rooms unavailable"))?;
                ensure!(waiting.len() < config.max_rooms, "Relay is full");
                waiting.insert(room.clone(), sender);
            }
            // Every exit path removes the room, including a failed ready response.
            let paired = async {
                send(&mut socket, Reply::Ready { room: room.clone(), expires_in: config.room_ttl.as_secs().max(1) }).await?;
                let expiry = tokio::time::sleep(config.room_ttl); tokio::pin!(expiry);
                let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
                heartbeat.tick().await;
                loop {
                    tokio::select! {
                        _ = &mut expiry => bail!("Invitation expired"),
                        peer = &mut receiver => return Ok(peer?),
                        _ = heartbeat.tick() => { timeout(Duration::from_secs(5), socket.send(Message::Ping(Vec::new().into()))).await??; },
                        message = socket.next() => match message {
                            Some(Ok(Message::Ping(_) | Message::Pong(_))) => { timeout(Duration::from_secs(5), socket.flush()).await??; },
                            _ => bail!("Waiting host disconnected"),
                        }
                    }
                }
            }.await;
            rooms
                .lock()
                .map_err(|_| anyhow::anyhow!("Rooms unavailable"))?
                .remove(&room);
            let mut viewer = match paired {
                Ok(viewer) => viewer,
                Err(_) => {
                    let _ = send(
                        &mut socket,
                        Reply::Error {
                            reason: "Invitation expired or sharing stopped".into(),
                        },
                    )
                    .await;
                    return Ok(());
                }
            };
            send(&mut socket, Reply::Paired).await?;
            send(&mut viewer.socket, Reply::Paired).await?;
            let _host_admission = admission;
            timeout(
                config.session_timeout,
                forward(
                    socket,
                    viewer.socket,
                    config.idle_timeout,
                    config.max_session_bytes,
                ),
            )
            .await??;
        }
        Hello::Viewer { version, room } => {
            let sender = if version == 1 && decode_capability(&room).is_ok() {
                rooms
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Rooms unavailable"))?
                    .remove(&room)
            } else {
                None
            };
            if let Some(sender) = sender {
                let _ = sender.send(Peer {
                    socket,
                    _admission: admission,
                });
            } else {
                send(
                    &mut socket,
                    Reply::Error {
                        reason: "Invitation unavailable, already used, or expired".into(),
                    },
                )
                .await?;
            }
        }
    }
    Ok(())
}

async fn forward(
    host: WebSocketStream<TcpStream>,
    viewer: WebSocketStream<TcpStream>,
    idle: Duration,
    max_bytes: u64,
) -> Result<()> {
    let (host_write, host_read) = host.split();
    let (viewer_write, viewer_read) = viewer.split();
    let budget = Arc::new(Mutex::new(max_bytes));
    async fn direction(
        mut read: impl futures_util::Stream<
                Item = std::result::Result<Message, tokio_tungstenite::tungstenite::Error>,
            > + Unpin,
        mut write: impl futures_util::Sink<Message, Error = tokio_tungstenite::tungstenite::Error>
            + Unpin,
        idle: Duration,
        budget: Arc<Mutex<u64>>,
    ) -> Result<()> {
        let mut deadline = Instant::now() + idle;
        loop {
            let message = tokio::time::timeout_at(deadline, read.next())
                .await?
                .context("Peer disconnected")??;
            match message {
                Message::Binary(bytes) => {
                    ensure!(
                        !bytes.is_empty() && bytes.len() <= CHUNK,
                        "Invalid tunnel chunk"
                    );
                    {
                        let mut left = budget
                            .lock()
                            .map_err(|_| anyhow::anyhow!("Budget unavailable"))?;
                        *left = left
                            .checked_sub(bytes.len() as u64)
                            .context("Session traffic budget exceeded")?;
                    }
                    timeout(Duration::from_secs(5), write.send(Message::Binary(bytes))).await??;
                    deadline = Instant::now() + idle;
                }
                Message::Ping(_) | Message::Pong(_) => {}
                _ => bail!("Peer closed or sent invalid tunnel data"),
            }
        }
    }
    tokio::select! {
        result = direction(host_read, viewer_write, idle, budget.clone()) => result,
        result = direction(viewer_read, host_write, idle, budget) => result,
    }
}
