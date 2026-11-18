mod video;
use video::FrameWindow;

use crate::{
    desktop::{self, Capturer, Display, InputController, RemoteFrame},
    device::Identity,
    invitation::{Destination, Invitation},
    protocol::{AccessPolicy, DesktopInfo, InputEvent, Message},
    relay::addressed::ConnectionOffer,
    relay::{self, RelayEndpoint},
    transport::{SecureConnection, SecureReader, SecureWriter},
    wire::{StopToken, Wire},
};
use anyhow::{bail, ensure, Context, Result};
use std::{
    net::{IpAddr, Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

pub type LatestFrame = Arc<Mutex<Option<RemoteFrame>>>;
type LatestMessage = Arc<Mutex<Option<Message>>>;
type FrameWake = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct FrameDelivery {
    latest: LatestFrame,
    wake: Mutex<Option<FrameWake>>,
}
impl FrameDelivery {
    fn publish(&self, frame: RemoteFrame) {
        if let Ok(mut slot) = self.latest.lock() {
            *slot = Some(frame);
        }
        let wake = self.wake.lock().ok().and_then(|wake| wake.clone());
        if let Some(wake) = wake {
            wake();
        }
    }
}

// Budget includes capture and JPEG encoding, so processing time is not added
// to the target interval. Slow peers still retain only the newest frame.
#[cfg(any(target_os = "macos", target_os = "windows"))]
const FRAME_INTERVAL: Duration = Duration::from_nanos(1_000_000_000 / 60);
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const FRAME_INTERVAL: Duration = Duration::from_nanos(1_000_000_000 / 30);

#[derive(Debug)]
pub enum Decision {
    ViewOnly,
    Control,
    Reject,
}
#[derive(Debug)]
pub enum HostEvent {
    Ready(Invitation),
    Offline,
    Pending {
        name: String,
        address: String,
        control: bool,
        reply: SyncSender<Decision>,
    },
    Connected {
        name: String,
        control: bool,
    },
    Ended(SessionEnd),
    Notice(String),
}
#[derive(Debug)]
pub enum ViewerEvent {
    Waiting,
    Connected { desktop: DesktopInfo, control: bool },
    Latency(u64),
    Ended(SessionEnd),
}

#[derive(Debug)]
pub struct SessionEnd {
    pub reason: String,
    pub failed: bool,
}

impl SessionEnd {
    fn from_result(result: Result<()>, cancelled: bool, normal: &str) -> Self {
        match result {
            Ok(()) => Self {
                reason: normal.into(),
                failed: false,
            },
            Err(_) if cancelled => Self {
                reason: "Disconnected".into(),
                failed: false,
            },
            Err(error) if is_peer_closed(&error) => Self {
                reason: "Peer disconnected".into(),
                failed: false,
            },
            Err(error) => Self {
                reason: format!("{error:#}"),
                failed: true,
            },
        }
    }
}

fn is_peer_closed(error: &anyhow::Error) -> bool {
    error.downcast_ref::<std::io::Error>().is_some_and(|error| {
        matches!(
            error.kind(),
            std::io::ErrorKind::UnexpectedEof
                | std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::BrokenPipe
                | std::io::ErrorKind::ConnectionAborted
        )
    })
}

#[derive(Clone, Default)]
struct Cancellation(Arc<CancelState>);
#[derive(Default)]
struct CancelState {
    stopped: AtomicBool,
    requested: AtomicBool,
    socket: Mutex<Option<Wire>>,
    relay_stop: StopToken,
}
impl Cancellation {
    fn end_result(&self, result: Result<()>, normal: &str) -> SessionEnd {
        SessionEnd::from_result(result, self.0.requested.load(Ordering::Acquire), normal)
    }

    fn request_stop(&self) {
        self.0.requested.store(true, Ordering::Release);
        self.cancel();
    }
    fn stopped(&self) -> bool {
        self.0.stopped.load(Ordering::Acquire)
    }
    fn attach(&self, stream: &Wire) -> Result<()> {
        let mut socket = self
            .0
            .socket
            .lock()
            .map_err(|_| anyhow::anyhow!("Connection state unavailable"))?;
        *socket = Some(stream.try_clone()?);
        if self.stopped() {
            let _ = stream.shutdown(Shutdown::Both);
        }
        Ok(())
    }
    fn cancel(&self) {
        self.0.stopped.store(true, Ordering::Release);
        self.0.relay_stop.cancel();
        if let Ok(socket) = self.0.socket.lock() {
            if let Some(socket) = socket.as_ref() {
                let _ = socket.shutdown(Shutdown::Both);
            }
        }
    }
}

pub struct Host {
    pub invitation: Option<Invitation>,
    pub events: Receiver<HostEvent>,
    stop: Cancellation,
    active: Arc<Mutex<Option<Cancellation>>>,
}

impl Host {
    pub fn start(display: Display, port: u16, advertised: IpAddr) -> Result<Self> {
        ensure!(
            !advertised.is_unspecified() && !advertised.is_multicast(),
            "Enter this computer's LAN IP address"
        );
        let bind = if advertised.is_ipv4() {
            "0.0.0.0".parse::<IpAddr>()?
        } else {
            "::".parse()?
        };
        let listener = TcpListener::bind(SocketAddr::new(bind, port))
            .context("Could not listen. Try another port")?;
        listener.set_nonblocking(true)?;
        let invitation =
            Invitation::new(SocketAddr::new(advertised, listener.local_addr()?.port()));
        let (tx, events) = mpsc::sync_channel(32);
        let stop = Cancellation::default();
        let active = Arc::new(Mutex::new(None));
        let worker_stop = stop.clone();
        let worker_active = active.clone();
        let invite = invitation.clone();
        thread::spawn(move || {
            while !worker_stop.stopped() {
                match listener.accept() {
                    Ok((stream, address)) => {
                        let cancel = Cancellation::default();
                        if let Ok(mut active) = worker_active.lock() {
                            *active = Some(cancel.clone());
                        }
                        if worker_stop.stopped() {
                            cancel.cancel();
                        }
                        let result = host_peer(
                            stream.into(),
                            address.to_string(),
                            &invite,
                            &display,
                            &tx,
                            &cancel,
                        );
                        cancel.cancel();
                        if let Ok(mut active) = worker_active.lock() {
                            *active = None;
                        }
                        let end = cancel.end_result(result, "Session ended");
                        let _ = tx.try_send(HostEvent::Ended(end));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(40))
                    }
                    Err(e) => {
                        let _ = tx.try_send(HostEvent::Notice(format!("Listener error: {e}")));
                        break;
                    }
                }
            }
        });
        Ok(Self {
            invitation: Some(invitation),
            events,
            stop,
            active,
        })
    }

    /// Outbound-only, single-use internet hosting. Setup occurs off the UI thread.
    pub fn start_internet(
        display: Display,
        endpoint: RelayEndpoint,
        access_code: String,
    ) -> Result<Self> {
        ensure!(access_code.len() <= 256, "Relay access code is too long");
        let (tx, events) = mpsc::sync_channel(32);
        let stop = Cancellation::default();
        let active = Arc::new(Mutex::new(None));
        let worker_stop = stop.clone();
        let worker_active = active.clone();
        thread::spawn(move || {
            let result = (|| -> Result<()> {
                let pending =
                    relay::register_host(&endpoint, &access_code, &worker_stop.0.relay_stop)?;
                let invitation = Invitation::for_relay(endpoint, *pending.room())?;
                tx.try_send(HostEvent::Ready(invitation.clone()))?;
                let stream = pending.wait(&worker_stop.0.relay_stop)?;
                let cancel = Cancellation::default();
                cancel.attach(&stream)?;
                if let Ok(mut active) = worker_active.lock() {
                    *active = Some(cancel.clone());
                }
                if worker_stop.stopped() {
                    cancel.cancel();
                }
                let result = host_peer(
                    stream,
                    "Internet relay · peer address unavailable".into(),
                    &invitation,
                    &display,
                    &tx,
                    &cancel,
                );
                cancel.cancel();
                result
            })();
            let end = worker_stop.end_result(
                result,
                "Internet invitation used. Start sharing for a new invitation.",
            );
            let _ = tx.try_send(HostEvent::Ended(end));
            let _ = tx.try_send(HostEvent::Offline);
        });
        Ok(Self {
            invitation: None,
            events,
            stop,
            active,
        })
    }

    pub fn start_device(
        display: Display,
        offer: ConnectionOffer,
        identity: Arc<Identity>,
    ) -> Result<Self> {
        ensure!(offer.is_host(), "Expected an incoming access request");
        let (tx, events) = mpsc::sync_channel(32);
        let stop = Cancellation::default();
        let active = Arc::new(Mutex::new(Some(stop.clone())));
        let cancel = stop.clone();
        thread::spawn(move || {
            let result = (|| -> Result<()> {
                identity.check_peer(
                    &offer.endpoint.identity_realm(),
                    offer.peer_id(),
                    offer.peer_key(),
                )?;
                let stream = relay::join_device(&offer, &cancel.0.relay_stop)?;
                cancel.attach(&stream)?;
                let secure = SecureConnection::accept_device(
                    stream,
                    &identity,
                    offer.peer_key(),
                    &offer.context().prologue(),
                )?;
                let pin = PeerPin {
                    identity,
                    realm: offer.endpoint.identity_realm(),
                    id: offer.peer_id().clone(),
                    key: *offer.peer_key(),
                };
                serve_host(
                    secure,
                    format!("Computer ID {}", offer.peer_id()),
                    &display,
                    &tx,
                    &cancel,
                    Some(pin),
                )
            })();
            cancel.cancel();
            let _ = tx.try_send(HostEvent::Ended(cancel.end_result(result, "Session ended")));
            let _ = tx.try_send(HostEvent::Offline);
        });
        Ok(Self {
            invitation: None,
            events,
            stop,
            active,
        })
    }

    pub fn disconnect(&self) {
        if let Ok(active) = self.active.lock() {
            if let Some(active) = active.as_ref() {
                active.request_stop();
            }
        }
    }
    pub fn stop(&self) {
        self.stop.request_stop();
        self.disconnect();
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        self.stop.cancel();
        if let Ok(active) = self.active.lock() {
            if let Some(active) = active.as_ref() {
                active.cancel();
            }
        }
    }
}

pub struct Viewer {
    pub events: Receiver<ViewerEvent>,
    pub latest: LatestFrame,
    outgoing: SyncSender<Message>,
    cancel: Cancellation,
    delivery: Arc<FrameDelivery>,
}

impl Viewer {
    pub fn connect(invite: Invitation, name: String, control: bool) -> Result<Self> {
        Self::connect_target(ViewerTarget::Legacy(invite), name, control)
    }
    pub fn connect_device(
        offer: ConnectionOffer,
        identity: Arc<Identity>,
        name: String,
        control: bool,
    ) -> Result<Self> {
        ensure!(!offer.is_host(), "Expected an outgoing access request");
        Self::connect_target(ViewerTarget::Device { offer, identity }, name, control)
    }
    fn connect_target(target: ViewerTarget, name: String, control: bool) -> Result<Self> {
        Message::Request {
            name: name.clone(),
            control,
        }
        .validate()?;
        let (events_tx, events) = mpsc::sync_channel(32);
        let (outgoing, incoming) = mpsc::sync_channel(128);
        let delivery = Arc::new(FrameDelivery::default());
        let latest = delivery.latest.clone();
        let cancel = Cancellation::default();
        let worker_cancel = cancel.clone();
        let worker_delivery = delivery.clone();
        thread::spawn(move || {
            let result = viewer_peer(
                target,
                name,
                control,
                &events_tx,
                incoming,
                &worker_delivery,
                &worker_cancel,
            );
            let end = worker_cancel.end_result(result, "Host ended the session");
            worker_cancel.cancel();
            let _ = events_tx.try_send(ViewerEvent::Ended(end));
        });
        Ok(Self {
            events,
            latest,
            outgoing,
            cancel,
            delivery,
        })
    }
    pub fn set_frame_waker(&self, wake: impl Fn() + Send + Sync + 'static) {
        if let Ok(mut slot) = self.delivery.wake.lock() {
            *slot = Some(Arc::new(wake));
        }
    }
    pub fn input(&self, event: InputEvent) {
        // Fail closed on saturation: losing a release event must never leave
        // a key held on the host.
        if self.outgoing.try_send(Message::Input(event)).is_err() {
            self.cancel.cancel();
        }
    }
    pub fn disconnect(&self) {
        self.cancel.request_stop();
    }
}
impl Drop for Viewer {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

pub fn local_ip() -> IpAddr {
    local_ip_address::local_ip().unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST))
}

fn host_peer(
    stream: Wire,
    address: String,
    invite: &Invitation,
    display: &Display,
    events: &SyncSender<HostEvent>,
    cancel: &Cancellation,
) -> Result<()> {
    cancel.attach(&stream)?;
    let secure = SecureConnection::accept(stream, invite.secret())?;
    serve_host(secure, address, display, events, cancel, None)
}
struct PeerPin {
    identity: Arc<Identity>,
    realm: String,
    id: crate::device::ComputerId,
    key: [u8; 32],
}
impl PeerPin {
    fn remember(&self) -> Result<()> {
        self.identity
            .remember_peer(&self.realm, &self.id, &self.key)
    }
}
fn serve_host(
    secure: SecureConnection,
    address: String,
    display: &Display,
    events: &SyncSender<HostEvent>,
    cancel: &Cancellation,
    pin: Option<PeerPin>,
) -> Result<()> {
    let (mut reader, mut writer) = secure.split()?;
    let Message::Request {
        name,
        control: requested,
    } = reader.receive()?
    else {
        bail!("Expected an access request");
    };
    let (reply, answer) = mpsc::sync_channel(1);
    events
        .try_send(HostEvent::Pending {
            name: name.clone(),
            address,
            control: requested,
            reply,
        })
        .map_err(|_| anyhow::anyhow!("Host is busy"))?;
    let deadline = Instant::now() + Duration::from_secs(60);
    let decision = loop {
        if cancel.stopped() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            break Decision::Reject;
        }
        match answer.recv_timeout(Duration::from_millis(100)) {
            Ok(answer) => break answer,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => break Decision::Reject,
        }
    };
    let mut policy = AccessPolicy::default();
    match decision {
        Decision::Reject => {
            writer.send(&Message::Denied(
                "The host declined or the request expired".into(),
            ))?;
            return Ok(());
        }
        Decision::ViewOnly => policy.approve(false),
        Decision::Control => policy.approve(requested),
    }
    ensure!(!cancel.stopped(), "Session cancelled");
    if let Some(pin) = pin {
        pin.remember()?;
    }
    // OS integrations are created only after local consent.
    let capturer = match Capturer::open(display.id) {
        Ok(capturer) => capturer,
        Err(error) => {
            let _ = writer.send(&Message::Error(format!("{error:#}")));
            return Err(error);
        }
    };
    let first_frame = match (|| -> Result<Message> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            ensure!(!cancel.stopped(), "Session cancelled");
            if let Some(frame) = capturer.frame()? {
                return Ok(frame);
            }
            ensure!(Instant::now() < deadline, "No screen frames arrived");
        }
    })() {
        Ok(frame) => frame,
        Err(e) => {
            let _ = writer.send(&Message::Error(format!("{e:#}")));
            return Err(e);
        }
    };
    drop(capturer); // Stop the startup stream before creating the capture worker's stream.
    let mut input = if policy.can_control() {
        match InputController::new(display.info.clone()) {
            Ok(input) => Some(input),
            Err(e) => {
                let _ = writer.send(&Message::Error(format!("{e:#}")));
                return Err(e);
            }
        }
    } else {
        None
    };
    writer.send(&Message::Approved {
        desktop: display.info.clone(),
        control: policy.can_control(),
    })?;
    let video = Arc::new(Mutex::new(FrameWindow::default()));
    let ticket = video.lock().unwrap().reserve().unwrap();
    writer.send(&first_frame)?;
    writer.send(&Message::Ping(ticket))?;
    let _ = events.try_send(HostEvent::Connected {
        name,
        control: policy.can_control(),
    });
    let latest: LatestMessage = Arc::new(Mutex::new(None));
    let (outgoing, incoming) = mpsc::sync_channel(128);
    let writer_thread = spawn_writer(
        writer,
        incoming,
        latest.clone(),
        cancel.clone(),
        Some(video.clone()),
    );
    let capture_stop = cancel.clone();
    let capture_outgoing = outgoing.clone();
    let display_id = display.id;
    let capture_video = video.clone();
    let capture_thread = thread::spawn(move || {
        let result = (|| -> Result<()> {
            let capture = Capturer::open(display_id)?;
            while !capture_stop.stopped() {
                let start = Instant::now();
                let quality = capture_video
                    .lock()
                    .map(|window| window.quality())
                    .unwrap_or(60);
                if let Some(frame) = capture.frame_with_quality(quality)? {
                    if let Ok(mut slot) = latest.lock() {
                        *slot = Some(frame);
                    }
                }
                if let Some(remaining) = FRAME_INTERVAL.checked_sub(start.elapsed()) {
                    thread::sleep(remaining);
                }
            }
            Ok(())
        })();
        if let Err(e) = result {
            let _ = capture_outgoing.try_send(Message::Error(format!("{e:#}")));
            thread::sleep(Duration::from_millis(60));
            capture_stop.cancel();
        }
    });
    let result = read_host_messages(&mut reader, &outgoing, &mut input, policy, cancel, &video);
    cancel.cancel();
    drop(input); // Release all held inputs before waiting on other workers.
    let _ = writer_thread.join();
    let _ = capture_thread.join();
    result
}

fn read_host_messages(
    reader: &mut SecureReader,
    outgoing: &SyncSender<Message>,
    input: &mut Option<InputController>,
    policy: AccessPolicy,
    cancel: &Cancellation,
    video: &Arc<Mutex<FrameWindow>>,
) -> Result<()> {
    while !cancel.stopped() {
        match reader.receive()? {
            Message::Input(event) if policy.can_control() => {
                if let Some(input) = input.as_mut() {
                    input.apply(event)?;
                }
            }
            Message::Input(_) => {} // A view-only peer can never escalate control.
            Message::Ping(value) => {
                outgoing
                    .try_send(Message::Pong(value))
                    .context("Output queue is full")?;
            }
            Message::Pong(ticket) => {
                if let Ok(mut window) = video.lock() {
                    window.acknowledge(ticket);
                }
            }
            Message::Disconnect => return Ok(()),
            _ => bail!("Unexpected viewer message"),
        }
    }
    Ok(())
}

enum ViewerTarget {
    Legacy(Invitation),
    Device {
        offer: ConnectionOffer,
        identity: Arc<Identity>,
    },
}

fn viewer_peer(
    target: ViewerTarget,
    name: String,
    requested: bool,
    events: &SyncSender<ViewerEvent>,
    incoming: Receiver<Message>,
    delivery: &FrameDelivery,
    cancel: &Cancellation,
) -> Result<()> {
    let connection_hint = if cfg!(target_os = "macos") {
        "Could not reach host. Check DeskLink in macOS System Settings > Privacy & Security > Local Network, then retry. If permission is enabled after a rebuild, refresh the entry for the current app and restart DeskLink. Also check that both computers are on the same network and the host's firewall allows DeskLink"
    } else {
        "Could not reach host. Check that both computers are on the same network and the host's firewall allows DeskLink"
    };
    let (secure, pin) = match target {
        ViewerTarget::Legacy(invite) => {
            let stream = match &invite.destination {
                Destination::Direct(address) => {
                    TcpStream::connect_timeout(address, Duration::from_secs(8))
                        .context(connection_hint)?
                        .into()
                }
                Destination::Relay { endpoint, room } => {
                    relay::connect_viewer(endpoint, room, &cancel.0.relay_stop)?
                }
            };
            cancel.attach(&stream)?;
            (SecureConnection::connect(stream, invite.secret())?, None)
        }
        ViewerTarget::Device { offer, identity } => {
            let realm = offer.endpoint.identity_realm();
            identity.check_peer(&realm, offer.peer_id(), offer.peer_key())?;
            let stream = relay::join_device(&offer, &cancel.0.relay_stop)?;
            cancel.attach(&stream)?;
            let secure = SecureConnection::connect_device(
                stream,
                &identity,
                offer.peer_key(),
                &offer.context().prologue(),
            )?;
            (
                secure,
                Some(PeerPin {
                    identity,
                    realm,
                    id: offer.peer_id().clone(),
                    key: *offer.peer_key(),
                }),
            )
        }
    };
    let (mut reader, mut writer) = secure.split()?;
    writer.send(&Message::Request {
        name,
        control: requested,
    })?;
    let _ = events.try_send(ViewerEvent::Waiting);
    reader.set_timeout(Duration::from_secs(70));
    let control = match reader.receive()? {
        Message::Approved { desktop, control } => {
            ensure!(!control || requested, "Unexpected control permission");
            if let Some(pin) = &pin {
                pin.remember()?;
            }
            let _ = events.try_send(ViewerEvent::Connected { desktop, control });
            control
        }
        Message::Denied(reason) | Message::Error(reason) => bail!("{reason}"),
        _ => bail!("Expected host approval"),
    };
    reader.set_timeout(Duration::from_secs(15));
    let (outgoing, writer_input) = mpsc::sync_channel(128);
    let writer_thread = spawn_writer(
        writer,
        writer_input,
        Arc::new(Mutex::new(None)),
        cancel.clone(),
        None,
    );
    // Forward UI input through the same single writer as heartbeat replies.
    let forward_stop = cancel.clone();
    let forward_outgoing = outgoing.clone();
    let forward_thread = thread::spawn(move || {
        while !forward_stop.stopped() {
            match incoming.recv_timeout(Duration::from_millis(50)) {
                Ok(message) if control => {
                    if forward_outgoing.try_send(message).is_err() {
                        forward_stop.cancel();
                    }
                }
                Ok(_) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => {
                    forward_stop.cancel();
                    break;
                }
            }
        }
    });
    let result = (|| -> Result<()> {
        while !cancel.stopped() {
            match reader.receive()? {
                Message::Frame {
                    width,
                    height,
                    jpeg,
                } => {
                    let frame = desktop::decode_frame(width, height, jpeg)?;
                    delivery.publish(frame);
                }
                Message::Ping(value) => {
                    outgoing
                        .try_send(Message::Pong(value))
                        .context("Output queue is full")?;
                }
                Message::Pong(sent) => {
                    let _ =
                        events.try_send(ViewerEvent::Latency(now_millis().saturating_sub(sent)));
                }
                Message::Disconnect => return Ok(()),
                Message::Error(reason) => bail!("{reason}"),
                _ => bail!("Unexpected host message"),
            }
        }
        Ok(())
    })();
    cancel.cancel();
    let _ = writer_thread.join();
    let _ = forward_thread.join();
    result
}

fn spawn_writer(
    mut writer: SecureWriter,
    incoming: Receiver<Message>,
    latest: LatestMessage,
    cancel: Cancellation,
    video: Option<Arc<Mutex<FrameWindow>>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut heartbeat = Instant::now() - Duration::from_secs(3);
        let result = (|| -> Result<()> {
            while !cancel.stopped() {
                // A frame may already be waiting after the previous network write.
                // Do not add a polling timeout to every frame in a busy stream.
                let frame_ready = latest.lock().is_ok_and(|slot| slot.is_some())
                    && video
                        .as_ref()
                        .is_none_or(|window| window.lock().is_ok_and(|w| w.has_capacity()));
                let wait = if frame_ready {
                    Duration::ZERO
                } else {
                    Duration::from_millis(8)
                };
                match incoming.recv_timeout(wait) {
                    Ok(message) => writer.send(&message)?,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                // Drain input before a frame to keep interaction responsive.
                for _ in 0..128 {
                    match incoming.try_recv() {
                        Ok(message) => writer.send(&message)?,
                        Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
                    }
                }
                if heartbeat.elapsed() >= Duration::from_secs(2) {
                    writer.send(&Message::Ping(now_millis()))?;
                    heartbeat = Instant::now();
                }
                let frame = latest.lock().ok().and_then(|mut slot| {
                    if slot.is_none() {
                        return None;
                    }
                    let ticket = match &video {
                        Some(window) => Some(window.lock().ok()?.reserve()?),
                        None => None,
                    };
                    slot.take().map(|frame| (frame, ticket))
                });
                if let Some((frame, ticket)) = frame {
                    writer.send(&frame)?;
                    // Existing viewers echo this fence after decoding the frame.
                    if let Some(ticket) = ticket {
                        writer.send(&Message::Ping(ticket))?;
                    }
                }
            }
            Ok(())
        })();
        let _ = result;
        cancel.cancel();
    })
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn writer_keeps_control_live_while_video_waits_for_a_decode_fence() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (send, received) = mpsc::channel();
        let accept = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let secure = SecureConnection::accept(stream, &[7; 32]).unwrap();
            send.send(secure.split().unwrap()).unwrap();
        });
        let secure =
            SecureConnection::connect(TcpStream::connect(address).unwrap(), &[7; 32]).unwrap();
        let (mut peer, _) = secure.split().unwrap();
        let (_reader, writer) = received.recv().unwrap();
        accept.join().unwrap();
        let window = Arc::new(Mutex::new(FrameWindow::default()));
        let ticket = window.lock().unwrap().reserve().unwrap();
        while window.lock().unwrap().reserve().is_some() {}
        let latest = Arc::new(Mutex::new(Some(Message::Frame {
            width: 1,
            height: 1,
            jpeg: vec![1],
        })));
        let stop = Cancellation::default();
        let (outgoing, incoming) = mpsc::sync_channel(8);
        let worker = spawn_writer(writer, incoming, latest, stop.clone(), Some(window.clone()));
        outgoing.send(Message::Pong(123)).unwrap();
        peer.set_timeout(Duration::from_millis(150));
        let mut control = false;
        loop {
            match peer.receive() {
                Ok(Message::Frame { .. }) => panic!("Video escaped its full window"),
                Ok(Message::Pong(123)) => control = true,
                Ok(Message::Ping(_)) => {}
                Err(_) => break,
                other => panic!("Unexpected test message {other:?}"),
            }
        }
        assert!(
            control,
            "Video backpressure blocked control/heartbeat output"
        );
        window.lock().unwrap().acknowledge(ticket);
        peer.set_timeout(Duration::from_secs(1));
        loop {
            match peer.receive().unwrap() {
                Message::Frame { .. } => break,
                Message::Ping(_) => {}
                other => panic!("Unexpected test message {other:?}"),
            }
        }
        stop.cancel();
        worker.join().unwrap();
    }

    #[test]
    fn video_window_bounds_frames_until_the_receiver_acknowledges() {
        let mut window = FrameWindow::default();
        let a = window.reserve().unwrap();
        let b = window.reserve().unwrap();
        let c = window.reserve().unwrap();
        while window.reserve().is_some() {}
        assert!(window.reserve().is_none());
        assert!(!window.acknowledge(42));
        assert!(window.reserve().is_none());
        assert!(window.acknowledge(b));
        assert!(!window.acknowledge(b));
        let d = window.reserve().unwrap();
        assert_ne!(d, a);
        assert_ne!(d, c);
        assert!(window.reserve().is_none());
    }

    #[test]
    fn transport_failure_survives_worker_cleanup_cancellation() {
        let cancel = Cancellation::default();
        cancel.cancel(); // Internal worker teardown, not a user disconnect.
        let end = cancel.end_result(
            Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "Relay timed out").into()),
            "Session ended",
        );
        assert!(end.failed);
        assert!(end.reason.contains("Relay timed out"));
    }

    #[test]
    fn explicit_disconnect_remains_neutral_after_teardown() {
        let cancel = Cancellation::default();
        cancel.request_stop();
        let end = cancel.end_result(
            Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "Relay timed out").into()),
            "Session ended",
        );
        assert!(!end.failed);
        assert_eq!(end.reason, "Disconnected");
    }

    #[test]
    fn frame_delivery_wakes_after_replacing_the_pending_frame() {
        let delivery = Arc::new(FrameDelivery::default());
        let observer = delivery.clone();
        let (tx, rx) = mpsc::channel();
        *delivery.wake.lock().unwrap() = Some(Arc::new(move || {
            tx.send(observer.latest.lock().unwrap().as_ref().unwrap().width)
                .unwrap();
        }));
        for width in [2, 3] {
            delivery.publish(RemoteFrame {
                width,
                height: 1,
                rgb: vec![0; width as usize * 3],
                bytes: 10,
            });
        }
        assert_eq!(rx.try_iter().collect::<Vec<_>>(), [2, 3]);
        assert_eq!(delivery.latest.lock().unwrap().take().unwrap().width, 3);
        // Break the test callback's observer cycle.
        *delivery.wake.lock().unwrap() = None;
    }
    #[test]
    fn permission_failures_keep_their_reason_and_error_severity() {
        let end = SessionEnd::from_result(
            Err(anyhow::anyhow!("Accessibility permission is missing")),
            false,
            "Session ended",
        );
        assert!(end.failed);
        assert!(end.reason.contains("Accessibility"));
    }
    #[test]
    fn peer_close_and_local_stop_are_neutral() {
        let end = SessionEnd::from_result(
            Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof).into()),
            false,
            "Session ended",
        );
        assert!(!end.failed);
        let end = SessionEnd::from_result(Err(anyhow::anyhow!("Cancelled")), true, "Session ended");
        assert!(!end.failed);
    }
}
