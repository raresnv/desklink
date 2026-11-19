mod dashboard;
mod remote;
mod theme;

use crate::{
    config::RelaySettings,
    desktop::{self, Display},
    device::{ComputerId, Identity},
    protocol::DesktopInfo,
    relay::{DeviceEvent, DevicePresence, RelayEndpoint},
    session::{Decision, Host, HostEvent, Viewer, ViewerEvent},
    video::VideoProfile,
};
use eframe::egui::{self, TextureHandle};
use std::{
    sync::{mpsc::SyncSender, Arc},
    time::{Duration, Instant},
};

struct Pending {
    name: String,
    address: String,
    control: bool,
    reply: SyncSender<Decision>,
    since: Instant,
}

pub struct DeskLinkApp {
    displays: Vec<Display>,
    selected_display: usize,
    identity: Option<Arc<Identity>>,
    presence: Option<DevicePresence>,
    computer_id: Option<ComputerId>,
    relay_online: bool,
    relay_status: String,
    accept_requests: bool,
    requesting: bool,
    current_request: Option<[u8; 32]>,
    relay_url: String,
    relay_access_code: String,
    host: Option<Host>,
    host_peer: Option<(String, bool)>,
    pending: Option<Pending>,
    remote_address: String,
    name: String,
    request_control: bool,
    viewer: Option<Viewer>,
    remote: Option<DesktopInfo>,
    allowed_control: bool,
    control_enabled: bool,
    supports_video_settings: bool,
    video_profile: VideoProfile,
    zoom: remote::Zoom,
    input: remote::RemoteInput,
    texture: Option<TextureHandle>,
    status: Option<(String, bool)>,
    viewer_status: String,
    latency: u64,
    frame_count: u32,
    fps: u32,
    frame_bytes: usize,
    sample_start: Instant,
    connected_at: Option<Instant>,
    help: bool,
    copied_at: Option<Instant>,
}

impl DeskLinkApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let settings = RelaySettings::from_overrides(
            std::env::var("DESKLINK_RELAY_URL").ok().as_deref(),
            std::env::var("DESKLINK_RELAY_ACCESS_CODE").ok().as_deref(),
        );
        theme::setup(&cc.egui_ctx);
        let result = desktop::displays();
        let status = result.as_ref().err().map(|e| (format!("{e:#}"), true));
        let identity_result =
            Identity::default_root().and_then(|root| Identity::load_or_create(&root));
        let status = identity_result
            .as_ref()
            .err()
            .map(|error| (format!("{error:#}"), true))
            .or(status);
        let mut app = Self {
            displays: result.unwrap_or_default(),
            selected_display: 0,
            identity: identity_result.ok().map(Arc::new),
            presence: None,
            computer_id: None,
            relay_online: false,
            relay_status: "Connecting to DeskLink…".into(),
            accept_requests: true,
            requesting: false,
            current_request: None,
            relay_url: settings.url,
            relay_access_code: settings.access_code,
            host: None,
            host_peer: None,
            pending: None,
            remote_address: String::new(),
            name: std::env::var("COMPUTERNAME")
                .or_else(|_| std::env::var("HOSTNAME"))
                .unwrap_or_else(|_| "My computer".into())
                .chars()
                .take(80)
                .collect(),
            request_control: true,
            viewer: None,
            remote: None,
            allowed_control: false,
            control_enabled: false,
            supports_video_settings: false,
            video_profile: VideoProfile::default(),
            zoom: remote::Zoom::Fit,
            input: remote::RemoteInput::default(),
            texture: None,
            status,
            viewer_status: String::new(),
            latency: 0,
            frame_count: 0,
            fps: 0,
            frame_bytes: 0,
            sample_start: Instant::now(),
            connected_at: None,
            help: false,
            copied_at: None,
        };
        if !app.relay_url.is_empty() {
            if let Err(error) = app.start_presence() {
                app.status = Some((format!("{error:#}"), true));
            }
        }
        app
    }

    fn start_presence(&mut self) -> anyhow::Result<()> {
        let endpoint = RelayEndpoint::parse(&self.relay_url)?;
        let identity = self
            .identity
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Device identity is unavailable"))?;
        self.computer_id = identity.own_id(&endpoint.identity_realm())?;
        self.presence = Some(DevicePresence::connect(
            endpoint,
            identity,
            self.relay_access_code.clone(),
            self.name.trim().to_owned(),
        )?);
        self.relay_online = false;
        self.relay_status = "Connecting to relay…".into();
        Ok(())
    }
    fn request_access(&mut self) -> anyhow::Result<()> {
        let target = ComputerId::parse(&self.remote_address)?;
        anyhow::ensure!(
            self.computer_id.as_ref() != Some(&target),
            "Enter the other computer's ID, not your own"
        );
        let request = self
            .presence
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Connect to the relay first"))?
            .request(target, self.request_control)?;
        self.current_request = Some(request);
        self.requesting = true;
        self.status = None;
        Ok(())
    }
    fn finish_request(&mut self) {
        if let Some(request) = self.current_request.take() {
            if let Some(presence) = &self.presence {
                presence.cancel_request(request);
            }
        }
        self.requesting = false;
    }
    fn stop_presence(&mut self) {
        self.end_viewer();
        self.host = None;
        self.pending = None;
        self.host_peer = None;
        self.finish_request();
        self.presence = None;
        self.relay_online = false;
        self.relay_status = "Disconnected from relay".into();
    }
    fn poll_devices(&mut self, ctx: &egui::Context) {
        let events: Vec<_> = self
            .presence
            .as_ref()
            .map(|presence| presence.events.try_iter().collect())
            .unwrap_or_default();
        for event in events {
            match event {
                DeviceEvent::Ready(id) => {
                    self.computer_id = Some(id);
                    self.relay_online = true;
                    self.relay_status = "Online".into();
                    self.status = None;
                    if let Some(presence) = &self.presence {
                        presence.set_available(self.accept_requests);
                    }
                }
                DeviceEvent::Incoming(offer) => {
                    let request = offer.context().request;
                    if !self.accept_requests
                        || self.host.is_some()
                        || self.viewer.is_some()
                        || self.requesting
                    {
                        if let Some(presence) = &self.presence {
                            presence.cancel_request(request);
                        }
                        continue;
                    }
                    let result = (|| -> anyhow::Result<Host> {
                        let identity = self
                            .identity
                            .clone()
                            .ok_or_else(|| anyhow::anyhow!("Identity unavailable"))?;
                        let display = self
                            .displays
                            .get(self.selected_display)
                            .cloned()
                            .ok_or_else(|| {
                                anyhow::anyhow!("Select a display before accepting requests")
                            })?;
                        Host::start_device(display, offer, identity)
                    })();
                    match result {
                        Ok(host) => {
                            self.current_request = Some(request);
                            self.host = Some(host);
                        }
                        Err(error) => {
                            if let Some(presence) = &self.presence {
                                presence.cancel_request(request);
                            }
                            self.status = Some((format!("{error:#}"), true));
                        }
                    }
                }
                DeviceEvent::Outgoing(offer) => {
                    let request = offer.context().request;
                    if !self.requesting || self.current_request != Some(request) {
                        if let Some(presence) = &self.presence {
                            presence.cancel_request(request);
                        }
                        continue;
                    }
                    self.requesting = false;
                    self.current_request = Some(request);
                    let result = self
                        .identity
                        .clone()
                        .ok_or_else(|| anyhow::anyhow!("Identity unavailable"))
                        .and_then(|identity| {
                            Viewer::connect_device(
                                offer,
                                identity,
                                self.name.trim().into(),
                                self.request_control,
                            )
                        });
                    match result {
                        Ok(viewer) => {
                            let context = ctx.clone();
                            viewer.set_frame_waker(move || context.request_repaint());
                            self.viewer = Some(viewer);
                            self.viewer_status = "Waiting for the host to approve…".into();
                        }
                        Err(error) => {
                            self.finish_request();
                            self.status = Some((format!("{error:#}"), true));
                        }
                    }
                }
                DeviceEvent::RequestFailed { request, reason } => {
                    if self.current_request == Some(request) {
                        self.finish_request();
                        self.status = Some((reason, true));
                    }
                }
                DeviceEvent::SessionEnded(request) => {
                    if self.current_request == Some(request) {
                        if self.host.is_some() {
                            let was_active = self.host_peer.is_some();
                            self.pending = None;
                            self.host = None;
                            self.host_peer = None;
                            self.finish_request();
                            self.status = Some((
                                if was_active {
                                    "Session ended"
                                } else {
                                    "Access request cancelled"
                                }
                                .into(),
                                false,
                            ));
                        } else if self.requesting {
                            self.finish_request();
                            self.status = Some(("Access request ended".into(), false));
                        }
                    }
                }
                DeviceEvent::Offline(reason) => {
                    self.relay_online = false;
                    self.relay_status = "Reconnecting…".into();
                    self.end_viewer();
                    self.host = None;
                    self.pending = None;
                    self.host_peer = None;
                    self.finish_request();
                    self.status = Some((reason, true));
                }
            }
        }
    }
    fn poll(&mut self, ctx: &egui::Context) {
        // Drain session errors before control-plane termination notifications.
        let host_events: Vec<_> = self
            .host
            .as_ref()
            .map(|host| host.events.try_iter().collect())
            .unwrap_or_default();
        for event in host_events {
            match event {
                HostEvent::Ready(invitation) => {
                    if let Some(host) = &mut self.host {
                        host.invitation = Some(invitation);
                    }
                    self.status = None;
                }
                HostEvent::Offline => {
                    self.finish_request();
                    self.host = None;
                    self.pending = None;
                    self.host_peer = None;
                    self.copied_at = None;
                }
                HostEvent::Pending {
                    name,
                    address,
                    control,
                    reply,
                } => {
                    self.pending = Some(Pending {
                        name,
                        address: address.to_string(),
                        control,
                        reply,
                        since: Instant::now(),
                    });
                    ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                        egui::UserAttentionType::Informational,
                    ));
                }
                HostEvent::Connected { name, control } => {
                    self.pending = None;
                    self.host_peer = Some((name, control));
                    self.status = None;
                }
                HostEvent::Ended(end) => {
                    self.pending = None;
                    self.host_peer = None;
                    self.status = Some((end.reason, end.failed));
                }
                HostEvent::Notice(reason) => self.status = Some((reason, true)),
            }
        }
        let viewer_events: Vec<_> = self
            .viewer
            .as_ref()
            .map(|viewer| viewer.events.try_iter().collect())
            .unwrap_or_default();
        for event in viewer_events {
            match event {
                ViewerEvent::Waiting => {
                    self.viewer_status = "Waiting for the host to approve…".into()
                }
                ViewerEvent::Connected {
                    desktop,
                    control,
                    supports_video_settings,
                } => {
                    self.supports_video_settings = supports_video_settings;
                    if supports_video_settings {
                        if let Some(viewer) = &self.viewer {
                            viewer.set_video_profile(self.video_profile);
                        }
                    }
                    self.remote = Some(desktop);
                    self.allowed_control = control;
                    self.control_enabled = control;
                    self.connected_at = Some(Instant::now());
                    self.sample_start = Instant::now();
                    self.frame_count = 0;
                    self.status = None;
                }
                ViewerEvent::Latency(ms) => self.latency = ms,
                ViewerEvent::Ended(end) => {
                    self.end_viewer();
                    self.status = Some((end.reason, end.failed));
                }
            }
        }
        self.poll_devices(ctx);
        let frame = self
            .viewer
            .as_ref()
            .and_then(|v| v.latest.lock().ok().and_then(|mut slot| slot.take()));
        if let Some(frame) = frame {
            let image = egui::ColorImage::from_rgb(
                [frame.width as usize, frame.height as usize],
                &frame.rgb,
            );
            if let Some(texture) = &mut self.texture {
                texture.set(image, egui::TextureOptions::LINEAR);
            } else {
                self.texture =
                    Some(ctx.load_texture("remote-screen", image, egui::TextureOptions::LINEAR));
            }
            self.frame_count += 1;
            self.frame_bytes = frame.bytes;
        }
        if self.sample_start.elapsed() >= Duration::from_secs(1) {
            self.fps = (self.frame_count as f64 / self.sample_start.elapsed().as_secs_f64()).round()
                as u32;
            self.frame_count = 0;
            self.sample_start = Instant::now();
        }
    }

    fn end_viewer(&mut self) {
        self.finish_request();
        if let Some(viewer) = self.viewer.take() {
            viewer.disconnect();
        }
        self.remote = None;
        self.texture = None;
        self.connected_at = None;
        self.control_enabled = false;
        self.supports_video_settings = false;
        self.allowed_control = false;
        self.input = remote::RemoteInput::default();
        self.viewer_status.clear();
    }

    fn approval(&mut self, ctx: &egui::Context) {
        let mut decision = None;
        if let Some(pending) = &self.pending {
            egui::Modal::new(egui::Id::new("incoming-request")).show(ctx, |ui| {
                ui.set_width(410.0);
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("INCOMING REQUEST")
                        .size(11.0)
                        .color(theme::TEAL)
                        .strong(),
                );
                ui.add_space(12.0);
                ui.heading("Let them in?");
                ui.add_space(12.0);
                ui.label(egui::RichText::new(&pending.name).size(20.0).strong());
                ui.label(
                    egui::RichText::new(&pending.address)
                        .color(theme::MUTED)
                        .monospace(),
                );
                ui.add_space(12.0);
                ui.label(if pending.control {
                    "This person is asking to see your screen and control your keyboard and mouse."
                } else {
                    "This person is asking to see your screen."
                });
                ui.add_space(8.0);
                ui.label(
                    "Only approve someone you know. Your entire selected display will be visible.",
                );
                if let Some(permission)=desktop::permissions::snapshot().missing(pending.control) {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(match permission {
                        desktop::permissions::Permission::ScreenRecording=>"macOS needs Screen Recording before sharing. Choose Allow to request it, finish the permission step, then return and choose Allow again.",
                        desktop::permissions::Permission::Accessibility=>"macOS needs Accessibility for keyboard and mouse control. Choose Allow to request it, finish the permission step, then return and choose Allow again.",
                    }).color(theme::MUTED).size(12.0));
                }
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    if theme::primary(
                        ui,
                        if pending.control {
                            "Allow control"
                        } else {
                            "Allow viewing"
                        },
                    )
                    .clicked()
                    {
                        decision = Some(if pending.control {
                            Decision::Control
                        } else {
                            Decision::ViewOnly
                        });
                    }
                    if pending.control && ui.button("View only").clicked() {
                        decision = Some(Decision::ViewOnly);
                    }
                    if ui.button("Decline").clicked() {
                        decision = Some(Decision::Reject);
                    }
                });
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new(format!(
                        "Expires in {} seconds",
                        60u64.saturating_sub(pending.since.elapsed().as_secs())
                    ))
                    .size(12.0)
                    .color(theme::MUTED),
                );
            });
        }
        if let Some(decision) = decision {
            let control = matches!(decision, Decision::Control);
            if !matches!(decision, Decision::Reject) {
                if let Some(permission) = desktop::permissions::snapshot().missing(control) {
                    desktop::permissions::request(permission);
                    self.status=Some(("Complete the macOS permission request, then choose Allow again. Viewing needs Screen Recording; control also needs Accessibility.".into(),false));
                    return;
                }
            }
            if let Some(pending) = self.pending.take() {
                let _ = pending.reply.try_send(decision);
            }
        }
    }
}

impl eframe::App for DeskLinkApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
        ctx.request_repaint_after(Duration::from_millis(if self.viewer.is_some() {
            16
        } else {
            100
        }));
        if self.remote.is_some() {
            self.remote_view(ctx);
        } else {
            self.dashboard(ctx);
        }
        self.approval(ctx);
        if !ctx.input(|i| i.focused) {
            if let Some(viewer) = &self.viewer {
                self.input.release(viewer);
            }
        }
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.end_viewer();
        if let Some(host) = self.host.take() {
            host.stop();
        }
    }
}
