use super::{theme, DeskLinkApp};
use crate::desktop;
use eframe::egui::{self, Color32, RichText};
use std::time::Instant;

impl DeskLinkApp {
    pub(super) fn dashboard(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("navigation")
            .exact_width(190.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(theme::NAV).inner_margin(22.0))
            .show(ctx, |ui| {
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    theme::logo(ui, 28.0, Color32::from_rgb(151, 223, 196));
                    ui.label(
                        RichText::new("DeskLink")
                            .size(23.0)
                            .strong()
                            .color(Color32::WHITE),
                    );
                });
                ui.add_space(48.0);
                ui.label(
                    RichText::new("WORKSPACE")
                        .size(10.0)
                        .strong()
                        .color(Color32::from_rgb(148, 176, 172)),
                );
                ui.add_space(8.0);
                if ui
                    .add(
                        egui::Button::new(RichText::new("Remote desktop").color(Color32::WHITE))
                            .fill(if !self.help {
                                Color32::from_rgb(45, 74, 71)
                            } else {
                                theme::NAV
                            }),
                    )
                    .clicked()
                {
                    self.help = false;
                }
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("How to connect").color(Color32::from_rgb(186, 204, 198)),
                        )
                        .fill(if self.help {
                            Color32::from_rgb(45, 74, 71)
                        } else {
                            theme::NAV
                        }),
                    )
                    .clicked()
                {
                    self.help = true;
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.label(
                        RichText::new(concat!("Version ", env!("CARGO_PKG_VERSION")))
                            .size(11.0)
                            .color(Color32::from_rgb(148, 176, 172)),
                    );
                    ui.label(
                        RichText::new("Made with Rust")
                            .size(12.0)
                            .color(Color32::from_rgb(186, 204, 198)),
                    );
                    ui.add_space(16.0);
                    ui.separator();
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("Encrypted. Yours.")
                            .size(12.0)
                            .color(Color32::from_rgb(151, 223, 196)),
                    );
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::CANVAS).inner_margin(32.0))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        theme::eyebrow(ui, "YOUR WORKSPACE");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                RichText::new("Remote desktop preview")
                                    .size(12.0)
                                    .color(theme::TEAL),
                            );
                        });
                    });
                    ui.add_space(24.0);
                    ui.label(
                        RichText::new(if self.help {
                            "Two computers. One connection."
                        } else {
                            "Your desk. Within reach."
                        })
                        .size(34.0)
                        .strong(),
                    );
                    ui.label(
                        RichText::new("Share a screen. Lend a hand. Stay in control.")
                            .size(16.0)
                            .color(theme::MUTED),
                    );
                    ui.add_space(26.0);
                    if let Some((message, error)) = &self.status {
                        let message = message.clone();
                        let color = if *error {
                            Color32::from_rgb(161, 52, 42)
                        } else {
                            theme::TEAL
                        };
                        egui::Frame::new()
                            .fill(if *error {
                                Color32::from_rgb(255, 237, 232)
                            } else {
                                Color32::from_rgb(228, 241, 234)
                            })
                            .corner_radius(8)
                            .inner_margin(12.0)
                            .show(ui, |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    ui.add(
                                        egui::Label::new(RichText::new(message).color(color))
                                            .wrap(),
                                    );
                                    if ui.small_button("Dismiss").clicked() {
                                        self.status = None;
                                    }
                                });
                            });
                        ui.add_space(12.0);
                    }
                    if self.help {
                        self.help_panel(ui);
                        return;
                    }
                    let width = ui.available_width();
                    if width >= 720.0 {
                        ui.columns(2, |columns| {
                            self.host_card(&mut columns[0], ctx);
                            self.connect_card(&mut columns[1]);
                        });
                    } else {
                        self.host_card(ui, ctx);
                        ui.add_space(16.0);
                        self.connect_card(ui);
                    }
                    ui.add_space(26.0);
                    theme::eyebrow(ui, "A CONNECTION YOU CHOOSE");
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(
                            "Every session needs your approval. Close the app to end access.",
                        )
                        .color(theme::MUTED),
                    );
                    ui.add_space(12.0);
                    ui.separator();
                    ui.add_space(8.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new("01  Encrypted pairing")
                                .size(12.0)
                                .color(theme::MUTED),
                        );
                        ui.label(
                            RichText::new("02  No account needed")
                                .size(12.0)
                                .color(theme::MUTED),
                        );
                        ui.label(
                            RichText::new("03  Connect by computer ID")
                                .size(12.0)
                                .color(theme::MUTED),
                        );
                    });
                });
            });
    }

    fn host_card(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        theme::card().show(ui,|ui|{
            theme::eyebrow(ui,"THIS COMPUTER");ui.add_space(10.0);ui.heading("Your computer ID");ui.label(RichText::new("Give this ID to someone you want to connect.").color(theme::MUTED));ui.add_space(16.0);
            if let Some(id)=&self.computer_id{
                ui.label(RichText::new(id.to_string()).size(28.0).monospace().strong());
                let copied=self.copied_at.is_some_and(|time|time.elapsed().as_secs()<3);
                if ui.button(if copied{"ID copied"}else{"Copy ID"}).clicked(){ctx.copy_text(id.to_string());self.copied_at=Some(Instant::now());}
            }else{ui.label(RichText::new("Getting your computer ID…").size(20.0));}
            ui.add_space(8.0);ui.label(RichText::new(&self.relay_status).color(if self.relay_online{theme::TEAL}else{theme::MUTED}));
            if self.presence.is_some()&&ui.checkbox(&mut self.accept_requests,"Accept access requests").changed(){if let Some(presence)=&self.presence{presence.set_available(self.accept_requests);}}
            ui.add_space(16.0);theme::eyebrow(ui,"DISPLAY TO SHARE");
            ui.add_enabled_ui(self.host.is_none(),|ui|{
                let label=self.displays.get(self.selected_display).map(|display|format!("{} · {} × {}",display.info.name,display.info.width,display.info.height)).unwrap_or_else(||"No display found".into());
                egui::ComboBox::from_id_salt("display").width(ui.available_width()-12.0).selected_text(label).show_ui(ui,|ui|{for(index,display)in self.displays.iter().enumerate(){ui.selectable_value(&mut self.selected_display,index,&display.info.name);}});
                if ui.small_button("Refresh displays").clicked(){match desktop::displays(){Ok(displays)=>{self.displays=displays;self.selected_display=0;},Err(error)=>self.status=Some((format!("{error:#}"),true))}}
            });
            if let Some((name,control))=&self.host_peer{ui.add_space(12.0);ui.label(format!("{} · {}",name,if *control{"Remote control"}else{"View only"}));if ui.button("Disconnect viewer").clicked(){if let Some(host)=&self.host{host.disconnect();}}}
            ui.add_space(12.0);ui.label(RichText::new("Nothing is shared until you approve each request.").size(12.0).color(theme::MUTED));
            #[cfg(target_os="macos")]
            {
                let permissions=desktop::permissions::snapshot();
                if !permissions.screen || !permissions.control {
                    ui.add_space(12.0);theme::eyebrow(ui,"SHARING PERMISSIONS");
                    ui.label(RichText::new("Needed to share this Mac. You can view another computer without granting them.").size(12.0).color(theme::MUTED));
                    if !permissions.screen && ui.button("Allow screen recording").clicked() {desktop::permissions::request(desktop::permissions::Permission::ScreenRecording);}
                    if !permissions.control && ui.button("Allow keyboard and mouse control").clicked() {desktop::permissions::request(desktop::permissions::Permission::Accessibility);}
                    ui.label(RichText::new("After granting access in System Settings, return here. macOS may require an app restart.").size(12.0).color(theme::MUTED));
                }
            }
            ui.add_space(16.0);
            egui::CollapsingHeader::new("Advanced connection settings").default_open(false).show(ui,|ui|{
                ui.add_enabled_ui(self.presence.is_none(),|ui|{
                    theme::eyebrow(ui,"RELAY ADDRESS");ui.add(egui::TextEdit::singleline(&mut self.relay_url).hint_text("wss://relay.example.com/v2").char_limit(240).desired_width(f32::INFINITY));
                    theme::eyebrow(ui,"RELAY ACCESS CODE");ui.add(egui::TextEdit::singleline(&mut self.relay_access_code).password(true).char_limit(256).desired_width(f32::INFINITY));
                    theme::eyebrow(ui,"COMPUTER NAME");ui.add(egui::TextEdit::singleline(&mut self.name).char_limit(80).desired_width(f32::INFINITY));
                });
                ui.add_space(8.0);
                if self.presence.is_some(){if ui.button("Disconnect from relay").clicked(){self.stop_presence();}}else if theme::primary(ui,"Connect to relay").clicked(){match self.start_presence(){Ok(())=>self.status=None,Err(error)=>self.status=Some((format!("{error:#}"),true))}}
                ui.label(RichText::new("DeskLink connects automatically. These settings are only needed for a custom relay.").size(12.0).color(theme::MUTED));
            });
        });
    }
    fn connect_card(&mut self, ui: &mut egui::Ui) {
        theme::card().show(ui, |ui| {
            theme::eyebrow(ui, "ANOTHER COMPUTER");
            ui.add_space(10.0);
            ui.heading("Connect to a computer");
            ui.label(RichText::new("Enter their ID and request access.").color(theme::MUTED));
            ui.add_space(16.0);
            if self.requesting || self.viewer.is_some() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(if self.requesting {
                        "Sending access request…"
                    } else {
                        &self.viewer_status
                    });
                });
                ui.add_space(12.0);
                if ui.button("Cancel request").clicked() {
                    if self.viewer.is_some() {
                        self.end_viewer();
                    } else {
                        self.finish_request();
                    }
                }
            } else {
                theme::eyebrow(ui, "REMOTE ADDRESS");
                ui.add(
                    egui::TextEdit::singleline(&mut self.remote_address)
                        .hint_text("482 731 906 254")
                        .char_limit(32)
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(8.0);
                ui.checkbox(
                    &mut self.request_control,
                    "Request keyboard and mouse control",
                );
                ui.add_space(16.0);
                ui.add_enabled_ui(
                    self.relay_online
                        && self.host.is_none()
                        && !self.remote_address.trim().is_empty(),
                    |ui| {
                        if theme::primary(ui, "Request access").clicked() {
                            if let Err(error) = self.request_access() {
                                self.status = Some((format!("{error:#}"), true));
                            }
                        }
                    },
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new("The other computer must be online and approve your request.")
                        .size(12.0)
                        .color(theme::MUTED),
                );
            }
        });
    }
    fn help_panel(&self, ui: &mut egui::Ui) {
        theme::card().show(ui,|ui|{
            for(step,title,body)in[
                ("01","Open DeskLink on both computers","Each computer connects automatically and shows its permanent ID."),
                ("02","Enter the remote address","Type the other computer's 12-digit ID and choose Request access."),
                ("03","Approve on the host","The host sees the requesting computer's ID and chooses control, view-only, or decline. Click the remote screen to send input."),
                ("04","End access whenever you want","Disconnect the viewer or close DeskLink. Your computer ID stays the same when you reopen the app."),
            ]{ui.horizontal_top(|ui|{ui.label(RichText::new(step).size(22.0).color(theme::TEAL));ui.vertical(|ui|{ui.label(RichText::new(title).strong());ui.label(body);});});ui.add_space(18.0);}
            ui.separator();ui.label("macOS: Screen Recording is needed for capture and Accessibility for remote control. Linux: use an X11 session. Windows: elevated or secure desktops require appropriate privileges.");
            ui.add_space(8.0);ui.label("Computer IDs are public addresses. Only approve people you know. Short IDs trust your relay's directory on first contact; previously approved computer keys are remembered.");
        });
    }
}
