use eframe::egui::{self, Color32, RichText, Stroke};

pub const INK: Color32 = Color32::from_rgb(30, 43, 47);
pub const MUTED: Color32 = Color32::from_rgb(100, 115, 119);
pub const TEAL: Color32 = Color32::from_rgb(0, 108, 95);
pub const CANVAS: Color32 = Color32::from_rgb(245, 247, 245);
pub const BORDER: Color32 = Color32::from_rgb(219, 226, 222);
pub const NAV: Color32 = Color32::from_rgb(24, 42, 43);

pub fn setup(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::light();
    style.visuals.override_text_color = Some(INK);
    style.visuals.panel_fill = CANVAS;
    style.visuals.window_fill = Color32::WHITE;
    style.visuals.selection.bg_fill = Color32::from_rgb(190, 225, 214);
    style.visuals.selection.stroke = Stroke::new(1.0_f32, TEAL);
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(240, 244, 241);
    style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(240, 244, 241);
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(225, 238, 232);
    style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(225, 238, 232);
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, TEAL);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(209, 229, 220);
    style.visuals.widgets.active.weak_bg_fill = Color32::from_rgb(209, 229, 220);
    style.spacing.item_spacing = egui::vec2(10.0, 10.0);
    style.spacing.button_padding = egui::vec2(15.0, 10.0);
    style.spacing.interact_size.y = 36.0;
    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Heading, egui::FontId::proportional(25.0));
    ctx.set_style(style);
}

pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::WHITE)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .corner_radius(12)
        .inner_margin(24.0)
}

pub fn primary(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(Color32::WHITE).strong())
            .fill(TEAL)
            .corner_radius(7),
    )
}

pub fn eyebrow(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(11.0).strong().color(MUTED));
}

pub fn logo(ui: &mut egui::Ui, size: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let painter = ui.painter();
    let a = egui::Rect::from_min_size(
        rect.min + egui::vec2(0.0, size * 0.18),
        egui::vec2(size * 0.65, size * 0.48),
    );
    let b = a.translate(egui::vec2(size * 0.33, size * 0.25));
    painter.rect_stroke(
        a,
        3.0,
        Stroke::new(2.0_f32, color),
        egui::StrokeKind::Inside,
    );
    painter.rect_filled(b, 3.0, color);
    painter.line_segment(
        [
            b.left_center() + egui::vec2(4.0, 0.0),
            b.right_center() - egui::vec2(4.0, 0.0),
        ],
        Stroke::new(2.0_f32, NAV),
    );
}
