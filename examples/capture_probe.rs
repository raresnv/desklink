//! Bounded profiling of capture/resize/JPEG stages. Outputs timings, never screen pixels.
use anyhow::{bail, Context, Result};
use desklink::video::{scaled_dimensions, VideoProfile};
use image::{codecs::jpeg::JpegEncoder, DynamicImage};
use std::{collections::BTreeSet, time::Instant};

fn main() -> Result<()> {
    let mut profile = VideoProfile::Balanced;
    let mut app = false;
    let mut assert_30 = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--app" => app = true,
            "--assert30" => assert_30 = true,
            "--profile" => {
                app = true;
                profile = match args.next().as_deref() {
                    Some("balanced") => VideoProfile::Balanced,
                    Some("sharp") => VideoProfile::Sharp,
                    Some("original") => VideoProfile::Original,
                    _ => bail!("Select --profile balanced, sharp or original"),
                };
            }
            "--help" | "-h" => {
                println!("capture_probe [--app] [--profile balanced|sharp|original] [--assert30]");
                return Ok(());
            }
            _ => bail!("Unknown capture probe argument: {arg}"),
        }
    }
    #[cfg(target_os = "windows")]
    unsafe {
        windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        )?;
    }

    if app {
        return app_probe(profile, assert_30);
    }
    let monitor = xcap::Monitor::all()?
        .into_iter()
        .next()
        .context("No displays")?;
    let mut timings = Vec::new();
    let mut frame_dimensions = BTreeSet::new();
    for _ in 0..40 {
        let start = Instant::now();
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let rgba = monitor.capture_image()?;
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let rgba = desklink::desktop::capture_x11_monitor(&monitor)?;
        let capture = start.elapsed().as_secs_f64();
        let (width, height) = scaled_dimensions(rgba.width(), rgba.height(), profile.max_edge())?;
        frame_dimensions.insert((width, height));
        let rgb = DynamicImage::ImageRgba8(rgba)
            .thumbnail_exact(width, height)
            .into_rgb8();
        let resize = start.elapsed().as_secs_f64() - capture;
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, profile.max_quality()).encode_image(&rgb)?;
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
        serde_json::json!({"stage":"baseline","profile":profile.label(),"frame_dimensions":frame_dimensions,"capture_ms":average[0]*1000.0,"resize_ms":average[1]*1000.0,"encode_ms":average[2]*1000.0,"total_ms":average[3]*1000.0,"decode_ms":average[4]*1000.0,"unpaced_fps":1.0/average[3]})
    );
    if assert_30 {
        anyhow::ensure!(1.0 / average[3] >= 30.0, "Capture pipeline is below 30 fps");
    }
    Ok(())
}

fn observed_fps(frames: usize, total_seconds: f64, warmup_seconds: f64) -> f64 {
    if frames > 5 {
        (frames - 5) as f64 / (total_seconds - warmup_seconds)
    } else {
        frames as f64 / total_seconds
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn delivery_rate_counts_empty_polls_after_warmup() {
        // Five warmup frames in one second, then fifteen updates over five seconds.
        assert_eq!(observed_fps(20, 6.0, 1.0), 3.0);
        assert_eq!(observed_fps(1, 5.0, 0.0), 0.2);
    }
}

fn app_probe(profile: VideoProfile, assert_30: bool) -> Result<()> {
    std::env::set_var("DESKLINK_CAPTURE_DIAGNOSTICS", "1");
    let display = desklink::desktop::displays()?
        .into_iter()
        .next()
        .context("No displays")?;
    let capture = desklink::desktop::Capturer::open_with_max_edge(display.id, profile.max_edge())?;
    let mut frame_dimensions = BTreeSet::new();
    let backend = capture.backend_name();
    let mut samples = Vec::new();
    let window_start = Instant::now();
    let mut warmup_seconds = 0.0;
    for _ in 0..90 {
        let start = Instant::now();
        let Some(desklink::protocol::Message::Frame {
            width,
            height,
            jpeg,
        }) = capture.frame_with_quality(profile.max_quality())?
        else {
            continue;
        };
        let elapsed = start.elapsed().as_secs_f64();
        frame_dimensions.insert((width, height));
        let size = jpeg.len();
        let decode_start = Instant::now();
        let _ = desklink::desktop::decode_frame(width, height, jpeg)?;
        samples.push((
            elapsed,
            decode_start.elapsed().as_secs_f64(),
            size,
            start.elapsed().as_secs_f64(),
        ));
        if samples.len() == 5 {
            warmup_seconds = window_start.elapsed().as_secs_f64();
        }
    }
    let delivery = observed_fps(
        samples.len(),
        window_start.elapsed().as_secs_f64(),
        warmup_seconds,
    );
    anyhow::ensure!(!samples.is_empty(), "No captured frames");
    let measured = if samples.len() > 5 {
        &samples[5..]
    } else {
        &samples[..]
    };
    let count = measured.len() as f64;
    let capture = measured.iter().map(|v| v.0).sum::<f64>() / count;
    let decode = measured.iter().map(|v| v.1).sum::<f64>() / count;
    let size = measured.iter().map(|v| v.2).sum::<usize>() as f64 / count;
    let total = measured.iter().map(|v| v.3).sum::<f64>() / count;
    println!(
        "{}",
        serde_json::json!({
            "stage": "native", "backend": backend, "profile": profile.label(),
            "display_dimensions": [display.info.width, display.info.height],
            "frame_dimensions": frame_dimensions, "captured_frames": samples.len(),
            "capture_encode_ms": capture*1000.0, "decode_ms": decode*1000.0,
            "jpeg_bytes": size, "unpaced_fps": 1.0/capture, "processing_fps": 1.0/total,
            "consumer_fps": delivery,
        })
    );
    if assert_30 {
        anyhow::ensure!(
            samples.len() >= 20,
            "Too few captured frames for a throughput assertion"
        );
        anyhow::ensure!(delivery >= 30.0, "Observed frame delivery is below 30 fps");
    }
    Ok(())
}
