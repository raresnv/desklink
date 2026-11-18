use super::{forward, Admission, Peer, RelayConfig};
use crate::{
    device::{make_challenge, registry::Directory, verify_answer, ComputerId},
    relay::{
        addressed::{Command, Event, Hello, Offer, Role, SessionContext, LIMIT},
        decode_capability,
    },
    wire::StopToken,
};
use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use futures_util::{SinkExt, StreamExt};
use rand::{rngs::OsRng, RngCore};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::Duration,
};
use subtle::ConstantTimeEq;
use tokio::{
    net::TcpStream,
    sync::{mpsc, oneshot},
    task::JoinSet,
    time::{timeout, Instant},
};
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};
use zeroize::Zeroize;

type Socket = WebSocketStream<TcpStream>;
struct Online {
    key: [u8; 32],
    name: String,
    events: mpsc::Sender<Event>,
    generation: [u8; 32],
    available: bool,
    busy: bool,
    ip: std::net::IpAddr,
}
struct Pending {
    host: ComputerId,
    viewer: ComputerId,
    stop: StopToken,
    tickets: [String; 2],
}
#[derive(Default)]
struct State {
    peers: HashMap<ComputerId, Online>,
    requests: HashMap<[u8; 32], Pending>,
    tickets: HashMap<String, oneshot::Sender<Peer>>,
    device_limits: HashMap<ComputerId, VecDeque<Instant>>,
    address_limits: HashMap<std::net::IpAddr, VecDeque<Instant>>,
}
pub(super) struct Service {
    directory: Arc<Directory>,
    state: Mutex<State>,
    jobs: Mutex<JoinSet<()>>,
    next_directory_sweep: Mutex<Instant>,
}
impl Service {
    pub fn new(config: &RelayConfig) -> Result<Arc<Self>> {
        Ok(Arc::new(Self {
            directory: Arc::new(Directory::open(
                config.directory_path.as_deref(),
                config.max_devices,
            )?),
            state: Mutex::new(State::default()),
            jobs: Mutex::new(JoinSet::new()),
            next_directory_sweep: Mutex::new(Instant::now()),
        }))
    }
    pub async fn shutdown(&self) {
        let mut jobs = {
            let mut guard = self.jobs.lock().expect("device jobs");
            std::mem::take(&mut *guard)
        };
        jobs.abort_all();
        while jobs.join_next().await.is_some() {}
    }
    pub fn reap(&self) {
        if let Ok(mut state) = self.state.lock() {
            let now = Instant::now();
            state.address_limits.retain(|_, times| {
                expire(times, now);
                !times.is_empty()
            });
            state.device_limits.retain(|_, times| {
                expire(times, now);
                !times.is_empty()
            });
        }
        if let Ok(mut jobs) = self.jobs.lock() {
            if let Ok(mut next) = self.next_directory_sweep.lock() {
                if Instant::now() >= *next {
                    *next = Instant::now() + Duration::from_secs(60);
                    let directory = self.directory.clone();
                    jobs.spawn_blocking(move || {
                        let _ = directory.prune_expired_allocations();
                    });
                }
            }
            while jobs.try_join_next().is_some() {}
        }
    }
    fn finish(&self, request: &[u8; 32]) {
        if let Ok(mut state) = self.state.lock() {
            if let Some(pending) = state.requests.remove(request) {
                pending.stop.cancel();
                for ticket in pending.tickets {
                    state.tickets.remove(&ticket);
                }
                for id in [pending.host, pending.viewer] {
                    if let Some(peer) = state.peers.get_mut(&id) {
                        peer.busy = false;
                        let _ = peer.events.try_send(Event::Ended { request: *request });
                    }
                }
            }
        }
    }
    fn offline(&self, id: &ComputerId, generation: &[u8; 32]) {
        let requests = if let Ok(mut state) = self.state.lock() {
            if !state
                .peers
                .get(id)
                .is_some_and(|peer| &peer.generation == generation)
            {
                return;
            }
            state.peers.remove(id);
            state
                .requests
                .iter()
                .filter(|(_, p)| &p.host == id || &p.viewer == id)
                .map(|(request, _)| *request)
                .collect::<Vec<_>>()
        } else {
            return;
        };
        for request in requests {
            self.finish(&request);
        }
    }
    fn request(
        self: &Arc<Self>,
        viewer: &ComputerId,
        target: ComputerId,
        control: bool,
        request: [u8; 32],
        config: &Arc<RelayConfig>,
    ) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("Device service unavailable"))?;
        let caller = state.peers.get(viewer).context("Device is offline")?;
        let ip = caller.ip;
        let viewer_key = caller.key;
        let viewer_name = caller.name.clone();
        let viewer_events = caller.events.clone();
        let busy = caller.busy;
        let now = Instant::now();
        if !state.address_limits.contains_key(&ip) {
            ensure!(
                state.address_limits.len() < config.max_connections * 4,
                "Request limiter is busy; retry later"
            );
        }
        let address = state.address_limits.entry(ip).or_default();
        expire(address, now);
        ensure!(
            address.len() < 30,
            "Too many access requests from this network; try again later"
        );
        address.push_back(now);
        let attempts = state.device_limits.entry(viewer.clone()).or_default();
        expire(attempts, now);
        ensure!(
            attempts.len() < config.requests_per_minute,
            "Too many access requests; try again later"
        );
        attempts.push_back(now);
        ensure!(
            &target != viewer,
            "You cannot connect to this computer itself"
        );
        ensure!(
            !busy,
            "This computer already has an active request or session"
        );
        ensure!(
            request != [0; 32] && !state.requests.contains_key(&request),
            "Duplicate or invalid access request"
        );
        let host = state
            .peers
            .get(&target)
            .filter(|host| host.available && !host.busy)
            .context("Computer is offline or busy")?;
        let host_key = host.key;
        let host_name = host.name.clone();
        let host_events = host.events.clone();
        ensure!(
            state.requests.len() < config.max_rooms,
            "Relay is busy; try again later"
        );
        let host_ticket = URL_SAFE_NO_PAD.encode(random());
        let viewer_ticket = URL_SAFE_NO_PAD.encode(random());
        let context = SessionContext {
            host: target.clone(),
            viewer: viewer.clone(),
            host_key,
            viewer_key,
            request,
        };
        let (host_sender, host_receiver) = oneshot::channel();
        let (viewer_sender, viewer_receiver) = oneshot::channel();
        let stop = StopToken::default();
        state.tickets.insert(host_ticket.clone(), host_sender);
        state.tickets.insert(viewer_ticket.clone(), viewer_sender);
        state.requests.insert(
            request,
            Pending {
                host: target.clone(),
                viewer: viewer.clone(),
                stop: stop.clone(),
                tickets: [host_ticket.clone(), viewer_ticket.clone()],
            },
        );
        state.peers.get_mut(&target).expect("checked host").busy = true;
        state.peers.get_mut(viewer).expect("checked viewer").busy = true;
        let offered = host_events
            .try_send(Event::Offer(Offer {
                role: Role::Host,
                context: context.clone(),
                ticket: host_ticket,
                name: viewer_name,
                control,
            }))
            .is_ok()
            && viewer_events
                .try_send(Event::Offer(Offer {
                    role: Role::Viewer,
                    context,
                    ticket: viewer_ticket,
                    name: host_name,
                    control,
                }))
                .is_ok();
        drop(state);
        if !offered {
            self.finish(&request);
            bail!("Remote computer is busy");
        }
        let service = self.clone();
        let config = config.clone();
        self.jobs
            .lock()
            .map_err(|_| anyhow::anyhow!("Session jobs unavailable"))?
            .spawn(async move {
                let session = async {
                    let (mut host, mut viewer) = timeout(Duration::from_secs(12), async {
                        Ok::<_, anyhow::Error>((host_receiver.await?, viewer_receiver.await?))
                    })
                    .await??;
                    send(&mut host.socket, &Event::Paired).await?;
                    send(&mut viewer.socket, &Event::Paired).await?;
                    // Admission guards live for both sockets through the entire session.
                    let _guards = (host._admission, viewer._admission);
                    timeout(
                        config.session_timeout,
                        forward(
                            host.socket,
                            viewer.socket,
                            config.idle_timeout,
                            config.max_session_bytes,
                        ),
                    )
                    .await??;
                    Ok::<_, anyhow::Error>(())
                };
                tokio::select! {_=stop.cancelled()=>{},_=session=>{}}
                service.finish(&request);
            });
        Ok(())
    }
}
fn expire(times: &mut VecDeque<Instant>, now: Instant) {
    while times
        .front()
        .is_some_and(|time| now.duration_since(*time) >= Duration::from_secs(60))
    {
        times.pop_front();
    }
}
fn random() -> [u8; 32] {
    let mut bytes = [0; 32];
    OsRng.fill_bytes(&mut bytes);
    bytes
}
async fn send(socket: &mut Socket, value: &Event) -> Result<()> {
    let text = serde_json::to_string(value)?;
    ensure!(text.len() <= LIMIT, "Control message too large");
    timeout(
        Duration::from_secs(5),
        socket.send(Message::Text(text.into())),
    )
    .await??;
    Ok(())
}
async fn command(socket: &mut Socket) -> Result<Command> {
    let message = socket.next().await.context("Device disconnected")??;
    let Message::Text(text) = message else {
        bail!("Expected device proof");
    };
    ensure!(text.len() <= LIMIT, "Control message too large");
    Ok(serde_json::from_str(&text)?)
}
pub(super) async fn handle(
    mut socket: Socket,
    first: &str,
    admission: Admission,
    source_ip: std::net::IpAddr,
    config: Arc<RelayConfig>,
    service: Arc<Service>,
) -> Result<()> {
    ensure!(first.len() <= LIMIT, "Device setup too large");
    match serde_json::from_str::<Hello>(first)? {
        Hello::Join { version, ticket } => {
            ensure!(
                version == 2 && decode_capability(&ticket).is_ok(),
                "Invalid session ticket"
            );
            let sender = service
                .state
                .lock()
                .map_err(|_| anyhow::anyhow!("Device service unavailable"))?
                .tickets
                .remove(&ticket)
                .context("Session request expired or was already used")?;
            let _ = sender.send(Peer {
                socket,
                _admission: admission,
            });
            Ok(())
        }
        Hello::Register {
            version,
            public_key,
            mut access_code,
            name,
        } => {
            let authorized = version == 2
                && public_key != [0; 32]
                && !name.is_empty()
                && name.len() <= 80
                && ((config.allow_public_enrollment && access_code.is_empty())
                    || match &config.access_code {
                        Some(expected) => {
                            bool::from(expected.as_bytes().ct_eq(access_code.as_bytes()))
                        }
                        None => access_code.is_empty(),
                    });
            access_code.zeroize();
            if !authorized {
                send(
                    &mut socket,
                    &Event::Error {
                        reason: "Device registration denied; check the relay access code".into(),
                    },
                )
                .await?;
                return Ok(());
            }
            let (state, challenge) = make_challenge(&public_key)?;
            let proof = async {
                send(&mut socket, &Event::Challenge { challenge }).await?;
                let Command::Proof { answer } = command(&mut socket).await? else {
                    bail!("Expected ownership proof");
                };
                verify_answer(state, &answer)
            };
            timeout(config.setup_timeout, proof).await??;
            let directory = service.directory.clone();
            let public = config.allow_public_enrollment;
            let per_ip = config.new_devices_per_ip_per_hour;
            let total = config.new_devices_per_hour;
            let registration = tokio::task::spawn_blocking(move || {
                if public {
                    directory.enroll_limited(&public_key, source_ip, per_ip, total)
                } else {
                    directory.enroll(&public_key)
                }
            })
            .await?;
            let id = match registration {
                Ok(id) => id,
                Err(error) => {
                    send(
                        &mut socket,
                        &Event::Error {
                            reason: error.to_string(),
                        },
                    )
                    .await?;
                    return Ok(());
                }
            };
            let (events, mut replies) = mpsc::channel(16);
            let generation = random();
            {
                let mut state = service
                    .state
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Device service unavailable"))?;
                ensure!(
                    !state.peers.contains_key(&id),
                    "This computer ID is already connected"
                );
                state.peers.insert(
                    id.clone(),
                    Online {
                        key: public_key,
                        name,
                        events,
                        generation,
                        available: true,
                        busy: false,
                        ip: source_ip,
                    },
                );
            }
            let run=async {
                send(&mut socket,&Event::Registered{id:id.clone()}).await?;
                let mut heartbeat=tokio::time::interval(Duration::from_secs(15));heartbeat.tick().await;
                let mut active=Instant::now();
                loop {tokio::select!{
                    _=heartbeat.tick()=>{ensure!(active.elapsed()<Duration::from_secs(45),"Device heartbeat timed out");timeout(Duration::from_secs(5),socket.send(Message::Ping(Vec::new().into()))).await??;},
                    event=replies.recv()=>{send(&mut socket,&event.context("Device disconnected")?).await?;},
                    message=socket.next()=>{
                        match message.context("Device disconnected")??{
                            Message::Ping(_)|Message::Pong(_)=>{active=Instant::now();timeout(Duration::from_secs(5),socket.flush()).await??;},
                            Message::Text(text) if text.len()<=LIMIT=>{
                                active=Instant::now();
                                let command:Command=serde_json::from_str(&text)?;
                                let result: Result<()> =match command{
                                    Command::Request{target,control,request}=>{if let Err(error)=service.request(&id,target,control,request,&config){send(&mut socket,&Event::Rejected{request,reason:error.to_string()}).await?;}Ok(())},
                                    Command::Cancel{request}=>{let ours=service.state.lock().map_err(|_|anyhow::anyhow!("Device service unavailable"))?.requests.get(&request).is_some_and(|p|p.host==id||p.viewer==id);if ours{service.finish(&request);}Ok(())},
                                    Command::CancelPending=>{let requests:Vec<_>=service.state.lock().map_err(|_|anyhow::anyhow!("Device service unavailable"))?.requests.iter().filter(|(_,p)|p.viewer==id).map(|(request,_)|*request).collect();for request in requests{service.finish(&request);}Ok(())},
                                    Command::Available{available}=>{if let Some(peer)=service.state.lock().map_err(|_|anyhow::anyhow!("Device service unavailable"))?.peers.get_mut(&id){peer.available=available;}Ok(())},
                                    Command::Proof{..}=>bail!("Unexpected repeated ownership proof"),
                                };
                                if let Err(error)=result{send(&mut socket,&Event::Error{reason:error.to_string()}).await?;}
                            },
                            _=>bail!("Invalid device control frame"),
                        }
                    }
                }}
                #[allow(unreachable_code)] Ok::<_,anyhow::Error>(())
            }.await;
            service.offline(&id, &generation);
            drop(admission);
            run
        }
    }
}
