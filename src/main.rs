#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("DeskLink — Remote desktop")
            .with_inner_size([1160.0, 780.0])
            .with_min_inner_size([900.0, 650.0]),
        ..Default::default()
    };
    eframe::run_native(
        "DeskLink",
        options,
        Box::new(|cc| Ok(Box::new(desklink::ui::DeskLinkApp::new(cc)))),
    )
}
