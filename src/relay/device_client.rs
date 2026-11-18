use super::{
    addressed::{Command, ConnectionOffer, Event, Hello, Role, LIMIT},
    client::{runtime, tunnel, Socket},
    ws_config, RelayEndpoint,
};
use crate::{
    device::{answer_challenge, ComputerId, Identity},
    wire::{StopToken, Wire},
};
use anyhow::{bail, ensure, Context, Result};
use futures_util::{SinkExt, StreamExt};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::Duration,
};
use tokio::{sync::mpsc as asynchronous, time::timeout};
use tokio_tungstenite::tungstenite::Message;

#[derive(Debug)]
pub enum DeviceEvent {
    Ready(ComputerId),
    Incoming(ConnectionOffer),
    Outgoing(ConnectionOffer),
    RequestFailed { request: [u8; 32], reason: String },
    SessionEnded([u8; 32]),
    Offline(String),
}
pub struct DevicePresence {
    pub events: mpsc::Receiver<DeviceEvent>,
    commands: asynchronous::Sender<Command>,
    stop: StopToken,
    online: Arc<AtomicBool>,
}
impl DevicePresence {
    pub fn connect(
        endpoint: RelayEndpoint,
        identity: Arc<Identity>,
        access_code: String,
        name: String,
    ) -> Result<Self> {
        ensure!(
            access_code.len() <= 256 && !name.is_empty() && name.len() <= 80,
            "Invalid relay code or computer name"
        );
        let (commands, mut incoming) = asynchronous::channel(16);
        let (events, receiver) = mpsc::sync_channel(32);
        let stop = StopToken::default();
        let worker_stop = stop.clone();
        let online = Arc::new(AtomicBool::new(false));
        let worker_online = online.clone();
        runtime().spawn(async move {
            let mut backoff=1;
            while !worker_stop.stopped() {
                let result=presence(&endpoint,&identity,&access_code,&name,&mut incoming,&events,&worker_online,&worker_stop).await;
                worker_online.store(false,Ordering::Release);
                while incoming.try_recv().is_ok() {}
                if worker_stop.stopped() { break; }
                let reason=result.err().map(|error|format!("{error:#}")).unwrap_or_else(||"Relay disconnected".into());
                if events.try_send(DeviceEvent::Offline(reason)).is_err() { break; }
                tokio::select! { _=worker_stop.cancelled()=>break, _=tokio::time::sleep(Duration::from_secs(backoff))=>{} }
                backoff=(backoff*2).min(30);
            }
        });
        Ok(Self {
            events: receiver,
            commands,
            stop,
            online,
        })
    }
    pub fn request(&self, target: ComputerId, control: bool) -> Result<[u8; 32]> {
        ensure!(
            self.online.load(Ordering::Acquire),
            "Connect to the relay before requesting access"
        );
        use rand::RngCore;
        let mut request = [0; 32];
        rand::rngs::OsRng.fill_bytes(&mut request);
        self.commands
            .try_send(Command::Request {
                target,
                control,
                request,
            })
            .map_err(|_| anyhow::anyhow!("Too many pending actions"))?;
        Ok(request)
    }
    pub fn cancel_pending(&self) {
        if self.commands.try_send(Command::CancelPending).is_err() {
            self.stop.cancel();
        }
    }
    pub fn cancel_request(&self, request: [u8; 32]) {
        if self.commands.try_send(Command::Cancel { request }).is_err() {
            self.stop.cancel();
        }
    }
    pub fn set_available(&self, available: bool) {
        if self
            .commands
            .try_send(Command::Available { available })
            .is_err()
        {
            self.stop.cancel();
        }
    }
}
impl Drop for DevicePresence {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
async fn send(socket: &mut Socket, value: &impl serde::Serialize) -> Result<()> {
    let text = serde_json::to_string(value)?;
    ensure!(text.len() <= LIMIT, "Device control message too large");
    timeout(
        Duration::from_secs(5),
        socket.send(Message::Text(text.into())),
    )
    .await??;
    Ok(())
}
async fn receive(socket: &mut Socket) -> Result<Event> {
    loop {
        match socket.next().await.context("Relay disconnected")?? {
            Message::Text(text) if text.len() <= LIMIT => {
                return serde_json::from_str(&text).context("Invalid device relay response")
            }
            Message::Ping(_) | Message::Pong(_) => {
                timeout(Duration::from_secs(5), socket.flush()).await??;
            }
            _ => bail!("Invalid device relay response"),
        }
    }
}
async fn open(endpoint: &RelayEndpoint) -> Result<Socket> {
    let (socket, _) = tokio_tungstenite::connect_async_with_config(
        endpoint.device_url(),
        Some(ws_config()),
        true,
    )
    .await
    .context("Could not connect to the device relay; check its address and TLS certificate")?;
    Ok(socket)
}
#[allow(clippy::too_many_arguments)]
async fn presence(
    endpoint: &RelayEndpoint,
    identity: &Arc<Identity>,
    code: &str,
    name: &str,
    commands: &mut asynchronous::Receiver<Command>,
    events: &mpsc::SyncSender<DeviceEvent>,
    online: &AtomicBool,
    stop: &StopToken,
) -> Result<()> {
    let setup = async {
        let mut socket = open(endpoint).await?;
        send(
            &mut socket,
            &Hello::Register {
                version: 2,
                public_key: *identity.public_key(),
                access_code: code.to_owned(),
                name: name.to_owned(),
            },
        )
        .await?;
        let Event::Challenge { challenge } = receive(&mut socket).await? else {
            bail!("Device registration denied");
        };
        send(
            &mut socket,
            &Command::Proof {
                answer: answer_challenge(identity, &challenge)?,
            },
        )
        .await?;
        let id = match receive(&mut socket).await? {
            Event::Registered { id } => id,
            Event::Error { reason } => bail!("{reason}"),
            _ => bail!("Invalid registration response"),
        };
        let worker_identity = identity.clone();
        let realm = endpoint.identity_realm();
        let saved = id.clone();
        tokio::task::spawn_blocking(move || worker_identity.remember_own_id(&realm, &saved))
            .await??;
        Ok::<_, anyhow::Error>((socket, id))
    };
    let (mut socket, id) = tokio::select! {_=stop.cancelled()=>bail!("Disconnected"),result=timeout(Duration::from_secs(12),setup)=>result.context("Device registration timed out")??};
    online.store(true, Ordering::Release);
    events.try_send(DeviceEvent::Ready(id.clone()))?;
    let mut requested: Option<(ComputerId, [u8; 32])> = None;
    let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
    heartbeat.tick().await;
    loop {
        tokio::select! {
            _=stop.cancelled()=>return Ok(()),
            _=heartbeat.tick()=>{timeout(Duration::from_secs(5),socket.send(Message::Ping(Vec::new().into()))).await??;},
            command=commands.recv()=>{
                let Some(command)=command else{return Ok(());};
                if let Command::Request{target,request,..}=&command {if target==&id { events.try_send(DeviceEvent::RequestFailed{request:*request,reason:"You cannot connect to this computer itself".into()})?; continue; }requested=Some((target.clone(),*request));}
                if matches!(command,Command::CancelPending) { requested=None; }
                send(&mut socket,&command).await?;
            },
            message=receive(&mut socket)=>match message? {
                Event::Offer(offer)=>{
                    ensure!(offer.context.host!=offer.context.viewer && offer.context.request!=[0;32],"Invalid session context");
                    if offer.role==Role::Viewer && requested.as_ref().is_none_or(|(_,request)|request!=&offer.context.request) { send(&mut socket,&Command::Cancel{request:offer.context.request}).await?; continue; }
                    let event=if offer.role==Role::Host {
                        ensure!(offer.context.host==id && offer.context.host_key==*identity.public_key(),"Offer is for a different host identity");
                        DeviceEvent::Incoming(ConnectionOffer{offer,endpoint:endpoint.clone()})
                    } else {
                        ensure!(offer.context.viewer==id && offer.context.viewer_key==*identity.public_key() && requested.as_ref().is_some_and(|(host,request)|host==&offer.context.host&&request==&offer.context.request),"Unexpected remote computer in access response");
                        requested=None;DeviceEvent::Outgoing(ConnectionOffer{offer,endpoint:endpoint.clone()})
                    };
                    events.try_send(event)?;
                },
                Event::Rejected{request,reason}=>{if requested.as_ref().is_some_and(|(_,active)|active==&request){requested=None;events.try_send(DeviceEvent::RequestFailed{request,reason})?;}},
                Event::Error{reason}=>bail!("{reason}"),
                Event::Ended{request}=>{if requested.as_ref().is_some_and(|(_,active)|active==&request){requested=None;}events.try_send(DeviceEvent::SessionEnded(request))?;},
                _=>bail!("Unexpected device control message"),
            }
        }
    }
}
pub fn join_device(offer: &ConnectionOffer, stop: &StopToken) -> Result<Wire> {
    runtime().block_on(async {
        let join=async {
            let mut socket=open(&offer.endpoint).await?;
            send(&mut socket,&Hello::Join{version:2,ticket:offer.offer.ticket.clone()}).await?;
            ensure!(matches!(receive(&mut socket).await?,Event::Paired),"Session request expired or was cancelled");
            Ok(tunnel(socket,stop.clone()))
        };
        tokio::select! {_=stop.cancelled()=>bail!("Session cancelled"),result=timeout(Duration::from_secs(15),join)=>result.context("Session pairing timed out")?}
    })
}
