use super::*;

struct Canvas {
    ctx: egui::Context,
    texture: egui::TextureHandle,
    input: RemoteInput,
    full: egui::Rect,
    visible: egui::Rect,
    sent: Vec<InputEvent>,
    time: f64,
    frame_seconds: f64,
}

impl Canvas {
    fn new() -> Self {
        let ctx = egui::Context::default();
        let texture = ctx.load_texture(
            "synthetic",
            egui::ColorImage::new([1200, 600], Color32::GRAY),
            Default::default(),
        );
        Self {
            ctx,
            texture,
            input: RemoteInput::default(),
            full: egui::Rect::NOTHING,
            visible: egui::Rect::NOTHING,
            sent: vec![],
            time: 0.0,
            frame_seconds: 0.2,
        }
    }

    fn frame(&mut self, zoom: Zoom, events: Vec<Event>) {
        self.sent.clear();
        self.time += self.frame_seconds;
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(340.0, 235.0),
            )),
            events,
            time: Some(self.time),
            ..Default::default()
        };
        raw.viewports
            .get_mut(&egui::ViewportId::ROOT)
            .unwrap()
            .native_pixels_per_point = Some(2.0);
        let _ = self.ctx.run(raw, |ctx| {
            egui::TopBottomPanel::top("test-toolbar").show(ctx, |ui| {
                ui.label("Toolbar");
            });
            egui::CentralPanel::default().show(ctx, |ui| {
                show_screen(ui, &self.texture, zoom, |_, response, visible| {
                    self.full = response.rect;
                    self.visible = visible;
                    self.input
                        .interact(response, visible, true, |event| self.sent.push(event));
                });
            });
        });
    }
}

fn button(pos: egui::Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    }
}
fn wheel() -> Event {
    Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -60.0),
        modifiers: Modifiers::NONE,
    }
}

#[test]
fn fit_and_native_zoom_use_actual_transmitted_pixels_and_physical_dpi() {
    let mut canvas = Canvas::new();
    canvas.frame(Zoom::Fit, vec![]);
    assert_eq!(canvas.visible, canvas.full);
    assert!((canvas.full.aspect_ratio() - 2.0).abs() < 0.01);
    assert!(canvas.full.width() < 340.0 && canvas.full.width() > 300.0);
    canvas.frame(Zoom::Native, vec![]);
    assert_eq!(
        canvas.full.size() * canvas.ctx.pixels_per_point(),
        egui::vec2(1200.0, 600.0)
    );
    assert!(canvas.visible.width() < canvas.full.width());
}

#[test]
fn panned_image_maps_full_coordinates_and_routes_remote_drags_wheel_and_toolbar_clicks() {
    let mut canvas = Canvas::new();
    for _ in 0..4 {
        canvas.frame(Zoom::Native, vec![]);
    }
    let anchor = canvas.visible.center();
    canvas.frame(Zoom::Native, vec![Event::PointerMoved(anchor), wheel()]);
    for _ in 0..15 {
        canvas.frame(Zoom::Native, vec![]);
    }
    assert!(
        canvas.full.top() < canvas.visible.top() - 10.0,
        "unfocused wheel pans"
    );
    let origin = canvas.full.min;
    canvas.frame(
        Zoom::Native,
        vec![button(anchor, true), button(anchor, false)],
    );
    let expected = (anchor - origin) / canvas.full.size();
    assert!(canvas.sent.iter().any(|e| matches!(e, InputEvent::Move{x,y} if (*x-expected.x).abs()<0.001 && (*y-expected.y).abs()<0.001)));
    canvas.frame(Zoom::Native, vec![wheel()]);
    assert!(canvas
        .sent
        .iter()
        .any(|e| matches!(e, InputEvent::Scroll { vertical: 2, .. })));
    for _ in 0..15 {
        canvas.frame(Zoom::Native, vec![]);
    }
    assert_eq!(
        canvas.full.min, origin,
        "remote wheel also panned the local viewport"
    );

    canvas.frame(Zoom::Native, vec![button(anchor, true)]);
    let outside = egui::pos2(-50.0, -80.0);
    canvas.frame(
        Zoom::Native,
        vec![Event::PointerMoved(outside), button(outside, false)],
    );
    assert!(canvas
        .sent
        .iter()
        .any(|e| matches!(e, InputEvent::Move{x,y} if *x==0.0 && *y==0.0)));
    assert!(canvas
        .sent
        .iter()
        .any(|e| matches!(e, InputEvent::Button { down: false, .. })));
    assert_eq!(canvas.full.min, origin);

    let toolbar = egui::pos2(anchor.x, 5.0);
    assert!(canvas.full.contains(toolbar) && !canvas.visible.contains(toolbar));
    canvas.frame(Zoom::Native, vec![Event::PointerMoved(toolbar), wheel()]);
    assert!(
        canvas.sent.is_empty(),
        "clipped-away desktop received hover or wheel"
    );
    canvas.frame(
        Zoom::Native,
        vec![Event::PointerMoved(anchor), button(anchor, true)],
    );
    canvas.frame(Zoom::Native, vec![button(toolbar, true)]);
    assert!(matches!(canvas.sent.as_slice(), [InputEvent::ReleaseAll]));
    assert!(!canvas.input.active && canvas.input.buttons.is_empty());
}

#[test]
fn scrollbar_remains_draggable_while_remote_image_is_focused() {
    let mut canvas = Canvas::new();
    for _ in 0..8 {
        canvas.frame(Zoom::Native, vec![]);
    }
    let center = canvas.visible.center();
    canvas.frame(
        Zoom::Native,
        vec![
            Event::PointerMoved(center),
            button(center, true),
            button(center, false),
        ],
    );
    assert!(canvas.input.active);
    let bar = egui::pos2(canvas.visible.right() + 6.0, canvas.visible.top() + 20.0);
    canvas.frame(
        Zoom::Native,
        vec![Event::PointerMoved(bar), button(bar, true)],
    );
    assert!(matches!(canvas.sent.as_slice(), [InputEvent::ReleaseAll]));
    let end = bar + egui::vec2(0.0, 50.0);
    canvas.frame(Zoom::Native, vec![Event::PointerMoved(end)]);
    canvas.frame(Zoom::Native, vec![button(end, false)]);
    assert!(canvas.full.top() < canvas.visible.top() - 10.0);
}

#[test]
fn wheel_targets_the_pointer_location_at_the_time_of_each_event() {
    let mut canvas = Canvas::new();
    for _ in 0..4 {
        canvas.frame(Zoom::Native, vec![]);
    }
    let center = canvas.visible.center();
    let toolbar = egui::pos2(center.x, 5.0);
    canvas.frame(
        Zoom::Native,
        vec![
            Event::PointerMoved(center),
            button(center, true),
            button(center, false),
        ],
    );
    canvas.frame(Zoom::Native, vec![Event::PointerMoved(toolbar)]);
    canvas.frame(Zoom::Native, vec![wheel(), Event::PointerMoved(center)]);
    assert!(!canvas
        .sent
        .iter()
        .any(|e| matches!(e, InputEvent::Scroll { .. })));
    canvas.frame(Zoom::Native, vec![wheel(), Event::PointerMoved(toolbar)]);
    assert!(canvas
        .sent
        .iter()
        .any(|e| matches!(e, InputEvent::Scroll { vertical: 2, .. })));
}

#[test]
fn remote_wheel_followed_by_focus_release_never_pans_locally() {
    // Check both an immediate new local wheel and one after egui's smoothing tail.
    for idle_frames in [0, 45] {
        let mut canvas = Canvas::new();
        canvas.frame_seconds = 1.0 / 60.0;
        for _ in 0..12 {
            canvas.frame(Zoom::Native, vec![]);
        }
        let origin = canvas.full.min;
        let center = canvas.visible.center();
        canvas.frame(
            Zoom::Native,
            vec![
                Event::PointerMoved(center),
                button(center, true),
                button(center, false),
            ],
        );
        canvas.frame(
            Zoom::Native,
            vec![Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: egui::vec2(0.0, -3.0),
                modifiers: Modifiers::NONE,
            }],
        );
        assert!(matches!(
            canvas.sent.as_slice(),
            [InputEvent::Scroll {
                horizontal: 0,
                vertical: 3
            }]
        ));
        canvas.frame(
            Zoom::Native,
            vec![Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::CTRL | Modifiers::ALT,
            }],
        );
        assert!(matches!(canvas.sent.as_slice(), [InputEvent::ReleaseAll]));
        for _ in 0..idle_frames {
            canvas.frame(Zoom::Native, vec![]);
            assert_eq!(
                canvas.full.min, origin,
                "released remote wheel panned locally"
            );
            assert!(canvas.sent.is_empty());
        }
        canvas.frame(
            Zoom::Native,
            vec![Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -5.0),
                modifiers: Modifiers::NONE,
            }],
        );
        for _ in 0..45 {
            canvas.frame(Zoom::Native, vec![]);
            assert!(canvas.sent.is_empty());
        }
        assert_eq!(
            canvas.full.min,
            origin - egui::vec2(0.0, 5.0),
            "only the new local wheel should pan"
        );
    }
}
