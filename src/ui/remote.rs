use super::{theme, DeskLinkApp};
use crate::{
    protocol::{InputEvent, Key, MouseButton},
    session::Viewer,
};
use eframe::egui::{self, Color32, Event, Modifiers, RichText};
use std::collections::HashSet;

#[derive(Default)]
pub(super) struct RemoteInput {
    active: bool,
    keys: HashSet<Key>,
    modifiers: HashSet<Key>,
    buttons: HashSet<MouseButton>,
    wheel: egui::Vec2,
}

impl DeskLinkApp {
    pub(super) fn remote_view(&mut self, ctx: &egui::Context) {
        let mut disconnect = false;
        egui::TopBottomPanel::top("session-toolbar")
            .frame(egui::Frame::new().fill(Color32::WHITE).inner_margin(14.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    theme::logo(ui, 24.0, theme::TEAL);
                    if let Some(remote) = &self.remote {
                        ui.label(RichText::new(&remote.name).strong().size(16.0));
                    }
                    ui.label(RichText::new("Encrypted").color(theme::TEAL).size(12.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(egui::Button::new(
                                RichText::new("Disconnect").color(Color32::from_rgb(166, 54, 46)),
                            ))
                            .clicked()
                        {
                            disconnect = true;
                        }
                        if self.allowed_control {
                            ui.checkbox(&mut self.control_enabled, "Remote control");
                        } else {
                            ui.label(RichText::new("View only").color(theme::MUTED));
                        }
                    });
                });
            });
        egui::TopBottomPanel::bottom("session-status")
            .frame(egui::Frame::new().fill(Color32::WHITE).inner_margin(10.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(if self.control_enabled {
                            "Click to control · Ctrl+Alt+Esc releases input"
                        } else {
                            "Viewing the remote screen"
                        })
                        .size(12.0)
                        .color(theme::MUTED),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let seconds = self
                            .connected_at
                            .map(|t| t.elapsed().as_secs())
                            .unwrap_or(0);
                        ui.label(
                            RichText::new(format!(
                                "{}:{:02}  ·  {} fps  ·  {} ms  ·  {} KB/frame",
                                seconds / 60,
                                seconds % 60,
                                self.fps,
                                self.latency,
                                self.frame_bytes / 1024
                            ))
                            .size(12.0)
                            .color(theme::MUTED)
                            .monospace(),
                        );
                    });
                });
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(19, 28, 30))
                    .inner_margin(16.0),
            )
            .show(ctx, |ui| {
                if let Some(texture) = &self.texture {
                    let available = ui.available_size();
                    let texture_size = texture.size_vec2();
                    let scale = (available.x / texture_size.x).min(available.y / texture_size.y);
                    let size = texture_size * scale;
                    let rect = egui::Rect::from_center_size(ui.max_rect().center(), size);
                    let response = ui.put(
                        rect,
                        egui::Image::new((texture.id(), texture_size))
                            .fit_to_exact_size(size)
                            .sense(egui::Sense::click_and_drag()),
                    );
                    focus_remote_screen(&response);
                    // Letterbox and toolbar clicks always release keyboard focus.
                    let click_outside = ctx.input(|i| {
                        i.pointer.any_pressed()
                            && i.pointer.interact_pos().is_some_and(|p| !rect.contains(p))
                    });
                    let release_shortcut = ctx.input_mut(|input| {
                        input.consume_shortcut(&egui::KeyboardShortcut::new(
                            Modifiers::CTRL | Modifiers::ALT,
                            egui::Key::Escape,
                        ))
                    });
                    if click_outside || release_shortcut {
                        response.surrender_focus();
                    }
                    let focused = self.control_enabled
                        && self.allowed_control
                        && response.has_focus()
                        && ctx.input(|i| i.focused)
                        && self.pending.is_none();
                    if let Some(viewer) = &self.viewer {
                        if focused {
                            lock_remote_keyboard(ctx, &response);
                            self.input.active = true;
                            self.input.forward(ctx, rect, |event| viewer.input(event));
                        } else {
                            self.input.release(viewer);
                        }
                    }
                    if self.control_enabled && !response.has_focus() {
                        let label_rect =
                            egui::Rect::from_center_size(rect.center(), egui::vec2(220.0, 38.0));
                        ui.painter()
                            .rect_filled(label_rect, 7.0, Color32::from_black_alpha(190));
                        ui.painter().text(
                            label_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "Click to control this screen",
                            egui::FontId::proportional(14.0),
                            Color32::WHITE,
                        );
                    }
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label(
                            RichText::new("Waiting for the first screen frame…")
                                .color(Color32::WHITE),
                        );
                    });
                }
            });
        if disconnect {
            self.end_viewer();
        }
    }
}

impl RemoteInput {
    pub(super) fn release(&mut self, viewer: &Viewer) {
        self.release_to(|event| viewer.input(event));
    }

    fn release_to(&mut self, mut emit: impl FnMut(InputEvent)) {
        if self.active {
            emit(InputEvent::ReleaseAll);
        }
        *self = Self::default();
    }

    fn sync_modifiers(&mut self, emit: &mut impl FnMut(InputEvent), modifiers: Modifiers) {
        for (key, down) in [
            (Key::Shift, modifiers.shift),
            (Key::Control, modifiers.ctrl),
            (Key::Alt, modifiers.alt),
            (Key::Meta, modifiers.mac_cmd),
        ] {
            if down != self.modifiers.contains(&key) {
                emit(InputEvent::Key { key, down });
                if down {
                    self.modifiers.insert(key);
                } else {
                    self.modifiers.remove(&key);
                }
            }
        }
    }

    fn forward(&mut self, ctx: &egui::Context, rect: egui::Rect, mut emit: impl FnMut(InputEvent)) {
        let (events, modifiers, pointer) =
            ctx.input(|i| (i.events.clone(), i.modifiers, i.pointer.hover_pos()));
        for event in events {
            match event {
                Event::PointerMoved(pos) if rect.contains(pos) || !self.buttons.is_empty() => {
                    emit(InputEvent::Move {
                        x: (pos.x - rect.left()) / rect.width(),
                        y: (pos.y - rect.top()) / rect.height(),
                    });
                }
                Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    modifiers,
                } => {
                    if let Some(button) = map_button(button) {
                        if (pressed && rect.contains(pos))
                            || (!pressed && self.buttons.contains(&button))
                        {
                            self.sync_modifiers(&mut emit, modifiers);
                            emit(InputEvent::Move {
                                x: (pos.x - rect.left()) / rect.width(),
                                y: (pos.y - rect.top()) / rect.height(),
                            });
                            emit(InputEvent::Button {
                                button,
                                down: pressed,
                            });
                            if pressed {
                                self.buttons.insert(button);
                            } else {
                                self.buttons.remove(&button);
                            }
                        }
                    }
                }
                Event::Text(text) if !modifiers.ctrl && !modifiers.mac_cmd => {
                    self.sync_modifiers(&mut emit, Modifiers::NONE);
                    if text.len() <= 8192 {
                        emit(InputEvent::Text(text));
                    }
                }
                Event::Paste(text) => {
                    // Paste is explicit text input; the clipboard is never
                    // synchronized or read in the background.
                    self.sync_modifiers(&mut emit, Modifiers::NONE);
                    let mut chunk = String::new();
                    for ch in text.chars().take(65_536) {
                        if chunk.len() + ch.len_utf8() > 4096 {
                            emit(InputEvent::Text(std::mem::take(&mut chunk)));
                        }
                        chunk.push(ch);
                    }
                    if !chunk.is_empty() {
                        emit(InputEvent::Text(chunk));
                    }
                }
                Event::Copy | Event::Cut => {
                    let ch = if matches!(event, Event::Copy) {
                        'c'
                    } else {
                        'x'
                    };
                    self.sync_modifiers(&mut emit, modifiers);
                    emit(InputEvent::Key {
                        key: Key::Character(ch),
                        down: true,
                    });
                    emit(InputEvent::Key {
                        key: Key::Character(ch),
                        down: false,
                    });
                }
                Event::Key {
                    key,
                    pressed,
                    modifiers,
                    ..
                } => {
                    if let Some(key) = map_key(key) {
                        let shortcut = modifiers.ctrl
                            || modifiers.mac_cmd
                            || (modifiers.alt && !cfg!(target_os = "macos"));
                        let printable = matches!(key, Key::Character(_));
                        if !pressed && self.keys.remove(&key) {
                            emit(InputEvent::Key { key, down: false });
                        } else if pressed && (!printable || shortcut) {
                            self.sync_modifiers(&mut emit, modifiers);
                            emit(InputEvent::Key { key, down: true });
                            self.keys.insert(key);
                        }
                    }
                }
                Event::MouseWheel {
                    delta,
                    unit,
                    modifiers,
                } if pointer.is_some_and(|p| rect.contains(p)) => {
                    self.sync_modifiers(&mut emit, modifiers);
                    let multiplier = match unit {
                        egui::MouseWheelUnit::Point => 1.0 / 30.0,
                        egui::MouseWheelUnit::Line => 1.0,
                        egui::MouseWheelUnit::Page => 10.0,
                    };
                    self.wheel -= delta * multiplier;
                    let x = self.wheel.x.trunc().clamp(-100.0, 100.0) as i32;
                    let y = self.wheel.y.trunc().clamp(-100.0, 100.0) as i32;
                    if x != 0 || y != 0 {
                        emit(InputEvent::Scroll {
                            horizontal: x,
                            vertical: y,
                        });
                        self.wheel -= egui::vec2(x as f32, y as f32);
                    }
                }
                Event::WindowFocused(false) | Event::PointerGone => {
                    self.release_to(&mut emit);
                    return;
                }
                _ => {}
            }
        }
        // Command is forwarded with a real key/pointer chord, not merely an
        // idle modifier snapshot. Native paste consumes Cmd+V into Event::Paste;
        // sending Command alone before/after it opens Windows Start on release.
        let mut held = modifiers;
        held.mac_cmd &= self.modifiers.contains(&Key::Meta)
            || !self.buttons.is_empty()
            || !self.keys.is_empty();
        self.sync_modifiers(&mut emit, held);
    }
}

fn focus_remote_screen(response: &egui::Response) {
    // Focus on the down event so the first click is sent as a complete
    // press/release pair, rather than being consumed to activate the image.
    if (response.is_pointer_button_down_on() || response.clicked())
        && response.ctx.input(|input| input.pointer.any_pressed())
    {
        response.request_focus();
    }
}

fn lock_remote_keyboard(ctx: &egui::Context, response: &egui::Response) {
    if response.has_focus() {
        ctx.memory_mut(|memory| {
            memory.set_focus_lock_filter(
                response.id,
                egui::EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: true,
                },
            )
        });
    }
}

fn map_button(button: egui::PointerButton) -> Option<MouseButton> {
    match button {
        egui::PointerButton::Primary => Some(MouseButton::Left),
        egui::PointerButton::Secondary => Some(MouseButton::Right),
        egui::PointerButton::Middle => Some(MouseButton::Middle),
        _ => None,
    }
}

fn map_key(key: egui::Key) -> Option<Key> {
    use egui::Key as E;
    Some(match key {
        E::Enter => Key::Enter,
        E::Escape => Key::Escape,
        E::Tab => Key::Tab,
        E::Backspace => Key::Backspace,
        E::Delete => Key::Delete,
        E::Insert => Key::Insert,
        E::ArrowUp => Key::Up,
        E::ArrowDown => Key::Down,
        E::ArrowLeft => Key::Left,
        E::ArrowRight => Key::Right,
        E::Home => Key::Home,
        E::End => Key::End,
        E::PageUp => Key::PageUp,
        E::PageDown => Key::PageDown,
        E::Space => Key::Character(' '),
        E::F1 => Key::Function(1),
        E::F2 => Key::Function(2),
        E::F3 => Key::Function(3),
        E::F4 => Key::Function(4),
        E::F5 => Key::Function(5),
        E::F6 => Key::Function(6),
        E::F7 => Key::Function(7),
        E::F8 => Key::Function(8),
        E::F9 => Key::Function(9),
        E::F10 => Key::Function(10),
        E::F11 => Key::Function(11),
        E::F12 => Key::Function(12),
        key => {
            let name = key.name();
            let mut chars = name.chars();
            let ch = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            Key::Character(ch.to_ascii_lowercase())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn forward_test_frame(
        input: &mut RemoteInput,
        modifiers: Modifiers,
        events: Vec<Event>,
    ) -> Vec<InputEvent> {
        let ctx = egui::Context::default();
        let mut sent = Vec::new();
        let _ = ctx.run(
            egui::RawInput {
                modifiers,
                events,
                ..Default::default()
            },
            |ctx| {
                input.forward(
                    ctx,
                    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(200.0, 100.0)),
                    |event| sent.push(event),
                );
            },
        );
        sent
    }

    #[test]
    fn paste_does_not_send_a_standalone_meta_tap_across_frames() {
        let mut input = RemoteInput {
            active: true,
            ..Default::default()
        };
        let command = Modifiers {
            mac_cmd: true,
            command: true,
            ..Modifiers::NONE
        };
        let mut sent = forward_test_frame(&mut input, command, vec![]);
        sent.extend(forward_test_frame(
            &mut input,
            command,
            vec![Event::Paste("Relay paste — ăîșț 😀".into())],
        ));
        sent.extend(forward_test_frame(&mut input, command, vec![]));
        sent.extend(forward_test_frame(&mut input, Modifiers::NONE, vec![]));
        assert_eq!(
            serde_json::to_value(sent).unwrap(),
            serde_json::to_value(vec![InputEvent::Text("Relay paste — ăîșț 😀".into())]).unwrap(),
            "A clipboard paste must not open the host's Start menu"
        );
    }

    #[test]
    fn command_pressed_during_a_held_drag_reaches_the_host() {
        let mut input = RemoteInput {
            active: true,
            ..Default::default()
        };
        let button = |pressed| Event::PointerButton {
            pos: egui::pos2(50.0, 50.0),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        forward_test_frame(&mut input, Modifiers::NONE, vec![button(true)]);
        let command = Modifiers {
            mac_cmd: true,
            command: true,
            ..Modifiers::NONE
        };
        let sent = forward_test_frame(&mut input, command, vec![]);
        assert!(
            sent.iter().any(|e| matches!(
                e,
                InputEvent::Key {
                    key: Key::Meta,
                    down: true
                }
            )),
            "Command pressed during an existing drag must reach the host before button release"
        );
        let released = forward_test_frame(&mut input, Modifiers::NONE, vec![]);
        assert!(released.iter().any(|e| matches!(
            e,
            InputEvent::Key {
                key: Key::Meta,
                down: false
            }
        )));
        forward_test_frame(&mut input, Modifiers::NONE, vec![button(false)]);
        assert!(input.buttons.is_empty() && input.modifiers.is_empty());
    }

    #[test]
    fn command_chord_still_forwards_meta_before_the_key() {
        let mut input = RemoteInput {
            active: true,
            ..Default::default()
        };
        let command = Modifiers {
            mac_cmd: true,
            command: true,
            ..Modifiers::NONE
        };
        let mut sent = forward_test_frame(&mut input, command, vec![]);
        for pressed in [true, false] {
            sent.extend(forward_test_frame(
                &mut input,
                command,
                vec![Event::Key {
                    key: egui::Key::R,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: command,
                }],
            ));
        }
        sent.extend(forward_test_frame(&mut input, Modifiers::NONE, vec![]));
        assert_eq!(
            serde_json::to_value(sent).unwrap(),
            serde_json::to_value(vec![
                InputEvent::Key {
                    key: Key::Meta,
                    down: true
                },
                InputEvent::Key {
                    key: Key::Character('r'),
                    down: true
                },
                InputEvent::Key {
                    key: Key::Character('r'),
                    down: false
                },
                InputEvent::Key {
                    key: Key::Meta,
                    down: false
                }
            ])
            .unwrap()
        );
    }

    #[test]
    fn initial_tap_focuses_when_press_and_release_share_a_frame() {
        let ctx = egui::Context::default();
        let id = egui::Id::new("remote-quick-tap-test");
        let show = |ctx: &egui::Context| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let response = ui.interact(
                    egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(200.0, 100.0)),
                    id,
                    egui::Sense::click_and_drag(),
                );
                focus_remote_screen(&response);
            });
        };
        let _ = ctx.run(egui::RawInput::default(), show);
        let _ = ctx.run(
            egui::RawInput {
                events: vec![
                    Event::PointerMoved(egui::pos2(50.0, 50.0)),
                    Event::PointerButton {
                        pos: egui::pos2(50.0, 50.0),
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Modifiers::NONE,
                    },
                    Event::PointerButton {
                        pos: egui::pos2(50.0, 50.0),
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: Modifiers::NONE,
                    },
                ],
                ..Default::default()
            },
            show,
        );
        assert!(
            ctx.memory(|memory| memory.has_focus(id)),
            "A quick tap was consumed instead of activating remote input"
        );
    }

    #[test]
    fn first_remote_pointer_press_focuses_before_release() {
        let ctx = egui::Context::default();
        let id = egui::Id::new("remote-first-click-test");
        let show = |ctx: &egui::Context| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let response = ui.interact(
                    egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(200.0, 100.0)),
                    id,
                    egui::Sense::click_and_drag(),
                );
                focus_remote_screen(&response);
            });
        };
        let _ = ctx.run(egui::RawInput::default(), show);
        assert!(!ctx.memory(|memory| memory.has_focus(id)));
        let _ = ctx.run(
            egui::RawInput {
                events: vec![
                    Event::PointerMoved(egui::pos2(50.0, 50.0)),
                    Event::PointerButton {
                        pos: egui::pos2(50.0, 50.0),
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Modifiers::NONE,
                    },
                ],
                ..Default::default()
            },
            show,
        );
        assert!(
            ctx.memory(|memory| memory.has_focus(id)),
            "The initial button press must reach the host before its release"
        );
        ctx.memory_mut(|memory| memory.surrender_focus(id));
        let _ = ctx.run(egui::RawInput::default(), show);
        assert!(
            !ctx.memory(|memory| memory.has_focus(id)),
            "A held button must not recapture input after focus is released"
        );
        let _ = ctx.run(
            egui::RawInput {
                events: vec![Event::PointerButton {
                    pos: egui::pos2(50.0, 50.0),
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Modifiers::NONE,
                }],
                ..Default::default()
            },
            show,
        );
        assert!(!ctx.memory(|memory| memory.has_focus(id)));
    }

    #[test]
    fn focused_remote_screen_keeps_navigation_keys() {
        let ctx = egui::Context::default();
        let id = egui::Id::new("remote-keyboard-test");
        let show = |ctx: &egui::Context| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let response = ui.interact(
                    egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(200.0, 100.0)),
                    id,
                    egui::Sense::click(),
                );
                lock_remote_keyboard(ctx, &response);
            });
        };
        ctx.memory_mut(|memory| memory.request_focus(id));
        let _ = ctx.run(egui::RawInput::default(), show);
        for key in [
            egui::Key::Tab,
            egui::Key::ArrowLeft,
            egui::Key::ArrowDown,
            egui::Key::Escape,
        ] {
            let input = egui::RawInput {
                events: vec![Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                }],
                ..Default::default()
            };
            let _ = ctx.run(input, show);
            assert!(
                ctx.memory(|memory| memory.has_focus(id)),
                "{key:?} moved focus away from the remote screen"
            );
        }
    }
}
