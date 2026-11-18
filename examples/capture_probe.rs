//! Bounded profiling of capture/resize/JPEG stages. Outputs timings, never screen pixels.
use anyhow::Result;
use image::{codecs::jpeg::JpegEncoder, DynamicImage};
use std::time::Instant;
fn main() -> Result<()> {
    #[cfg(target_os = "windows")]
    unsafe {
        windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        )?;
    }

    if std::env::args().any(|arg| arg == "--app") {
        return app_probe();
    }
    let monitor = xcap::Monitor::all()?.into_iter().next().unwrap();
    let mut timings = Vec::new();
    for _ in 0..40 {
        let start = Instant::now();
        let rgba = monitor.capture_image()?;
        let capture = start.elapsed().as_secs_f64();
        let rgb = DynamicImage::ImageRgba8(rgba)
            .thumbnail(1600, 1600)
            .to_rgb8();
        let resize = start.elapsed().as_secs_f64() - capture;
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, 75).encode_image(&rgb)?;
        let encode = start.elapsed().as_secs_f64() - capture - resize;
        let total = start.elapsed().as_secs_f64();
        let decode_start = Instant::now();
        let _ = desklink::desktop::decode_frame(rgb.width(), rgb.height(), jpeg)?;
        timings.push([
            capture,
            resize,
            encode,
            total,
            decode_start.elapsed().as_secs_f64(),
        ]);
    }
    let mut average = [0.0; 5];
    for row in &timings[5..] {
        for i in 0..5 {
            average[i] += row[i] / 35.0;
        }
    }
    println!(
        "{}",
        serde_json::json!({"stage":"baseline","capture_ms":average[0]*1000.0,"resize_ms":average[1]*1000.0,"encode_ms":average[2]*1000.0,"total_ms":average[3]*1000.0,"decode_ms":average[4]*1000.0,"unpaced_fps":1.0/average[3]})
    );
    if std::env::args().any(|arg| arg == "--assert30") {
        anyhow::ensure!(1.0 / average[3] >= 30.0, "Capture pipeline is below 30 fps");
    }
    Ok(())
}

fn app_probe() -> Result<()> {
    std::env::set_var("DESKLINK_CAPTURE_DIAGNOSTICS", "1");
    let display = desklink::desktop::displays()?.into_iter().next().unwrap();
    let capture = desklink::desktop::Capturer::open(display.id)?;
    let backend = capture.backend_name();
    let mut samples = Vec::new();
    for _ in 0..90 {
        let start = Instant::now();
        let Some(desklink::protocol::Message::Frame {
            width,
            height,
            jpeg,
        }) = capture.frame()?
        else {
            continue;
        };
        let elapsed = start.elapsed().as_secs_f64();
        let size = jpeg.len();
        let decode_start = Instant::now();
        let _ = desklink::desktop::decode_frame(width, height, jpeg)?;
        samples.push((
            elapsed,
            decode_start.elapsed().as_secs_f64(),
            size,
            start.elapsed().as_secs_f64(),
        ));
    }
    anyhow::ensure!(samples.len() >= 20, "Too few captured frames");
    let count = (samples.len() - 5) as f64;
    let capture = samples[5..].iter().map(|v| v.0).sum::<f64>() / count;
    let decode = samples[5..].iter().map(|v| v.1).sum::<f64>() / count;
    let size = samples[5..].iter().map(|v| v.2).sum::<usize>() as f64 / count;
    let total = samples[5..].iter().map(|v| v.3).sum::<f64>() / count;
    println!(
        "{}",
        serde_json::json!({"stage":"native","backend":backend,"capture_encode_ms":capture*1000.0,"decode_ms":decode*1000.0,"jpeg_bytes":size,"unpaced_fps":1.0/capture,"consumer_fps":1.0/total})
    );
    if std::env::args().any(|arg| arg == "--assert30") {
        anyhow::ensure!(1.0 / total >= 30.0, "Capture pipeline is below 30 fps");
    }
    Ok(())
}
