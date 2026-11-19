//! Bounded DXGI capture with scaled GDI initialization fallback.
//! WIC scales and encodes native BGR rows without Rust pixel transforms.
use super::jpeg;
use super::windows_duplication::{Duplication, RawFrame};
use crate::{
    desktop::Display,
    protocol::Message,
    video::{scaled_dimensions, MAX_FRAME_PIXELS},
};
use anyhow::{ensure, Context, Result};
use std::{ffi::c_void, ptr};
use windows::{
    core::{w, PWSTR},
    Win32::{
        Foundation::HGLOBAL,
        Graphics::{Gdi::*, Imaging::*},
        System::{
            Com::{StructuredStorage::*, *},
            Memory::{GlobalLock, GlobalUnlock},
            Variant::VARIANT,
        },
    },
};

struct Surface {
    screen: HDC,
    memory: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    pixels: *mut c_void,
    width: u32,
    height: u32,
    stride: u32,
    display: Display,
}
impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            if !self.previous.0.is_null() {
                SelectObject(self.memory, self.previous);
            }
            if !self.bitmap.0.is_null() {
                let _ = DeleteObject(self.bitmap.into());
            }
            if !self.memory.0.is_null() {
                let _ = DeleteDC(self.memory);
            }
            if !self.screen.0.is_null() {
                ReleaseDC(None, self.screen);
            }
        }
    }
}
impl Surface {
    fn open_scaled(display: Display, bound: u32) -> Result<Self> {
        unsafe {
            let (width, height) =
                scaled_dimensions(display.info.width, display.info.height, bound)?;
            let stride = width * 4;
            let mut surface = Self {
                screen: GetDC(None),
                memory: HDC::default(),
                bitmap: HBITMAP::default(),
                previous: HGDIOBJ::default(),
                pixels: ptr::null_mut(),
                width,
                height,
                stride,
                display,
            };
            ensure!(
                !surface.screen.0.is_null(),
                "Cannot access the interactive Windows desktop"
            );
            surface.memory = CreateCompatibleDC(Some(surface.screen));
            ensure!(
                !surface.memory.0.is_null(),
                "Cannot allocate capture context"
            );
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width as i32,
                    biHeight: -(height as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            surface.bitmap = CreateDIBSection(
                Some(surface.screen),
                &info,
                DIB_RGB_COLORS,
                &mut surface.pixels,
                None,
                0,
            )?;
            ensure!(!surface.pixels.is_null(), "Capture pixel allocation failed");
            surface.previous = SelectObject(surface.memory, surface.bitmap.into());
            ensure!(
                !surface.previous.0.is_null() && surface.previous.0 as usize != usize::MAX,
                "Cannot select capture bitmap"
            );
            ensure!(
                SetStretchBltMode(surface.memory, COLORONCOLOR) != 0,
                "Cannot configure capture scaling"
            );
            Ok(surface)
        }
    }
    fn capture(&self) -> Result<&[u8]> {
        unsafe {
            let d = &self.display.info;
            let mut monitor = MONITORINFOEXW::default();
            monitor.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
            ensure!(
                GetMonitorInfoW(
                    HMONITOR(self.display.id as usize as *mut c_void),
                    std::ptr::from_mut(&mut monitor).cast()
                )
                .as_bool(),
                "Selected display is no longer available"
            );
            let bounds = monitor.monitorInfo.rcMonitor;
            let name = String::from_utf16_lossy(
                &monitor.szDevice[..monitor
                    .szDevice
                    .iter()
                    .position(|c| *c == 0)
                    .unwrap_or(monitor.szDevice.len())],
            );
            ensure!(
                bounds.left == d.x
                    && bounds.top == d.y
                    && (bounds.right - bounds.left) as u32 == d.width
                    && (bounds.bottom - bounds.top) as u32 == d.height
                    && name == d.name,
                "Display configuration changed; select the display and approve again"
            );
            ensure!(
                StretchBlt(
                    self.memory,
                    0,
                    0,
                    self.width as i32,
                    self.height as i32,
                    Some(self.screen),
                    d.x,
                    d.y,
                    d.width as i32,
                    d.height as i32,
                    SRCCOPY | CAPTUREBLT
                )
                .as_bool(),
                "Windows capture failed; protected desktops cannot be shared"
            );
            ensure!(GdiFlush().as_bool(), "Windows capture did not complete");
            Ok(std::slice::from_raw_parts(
                self.pixels.cast(),
                (self.stride * self.height) as usize,
            ))
        }
    }
}
struct ComApartment;
impl ComApartment {
    fn open() -> Result<Self> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
        Ok(Self)
    }
}
impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() }
    }
}

#[derive(Default)]
struct Mailbox {
    frame: Option<RawFrame>,
    error: Option<String>,
}
pub(super) struct ScreenCapture {
    state: std::sync::Arc<(std::sync::Mutex<Mailbox>, std::sync::Condvar)>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
    duplication_active: std::sync::Arc<std::sync::atomic::AtomicBool>,
    max_edge: u32,
    factory: IWICImagingFactory,
    _apartment: ComApartment,
}
impl ScreenCapture {
    pub fn open(display: Display, max_edge: u32) -> Result<Self> {
        let apartment = ComApartment::open()?;
        let factory =
            unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }?;
        let state = std::sync::Arc::new((
            std::sync::Mutex::new(Mailbox::default()),
            std::sync::Condvar::new(),
        ));
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let duplication_active = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let backend = duplication_active.clone();
        let shared = state.clone();
        let stopped = stop.clone();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("windows-capture".into())
            .spawn(move || {
                let result = (|| -> Result<()> {
                    let duplication = match Duplication::open(&display) {
                        Ok(capture) => Some(capture),
                        Err(error) => {
                            if std::env::var_os("DESKLINK_CAPTURE_DIAGNOSTICS").is_some() {
                                eprintln!("DXGI fallback: {error:#}");
                            }
                            None
                        }
                    };
                    backend.store(duplication.is_some(), std::sync::atomic::Ordering::Release);
                    let fallback = if duplication.is_none() {
                        Some(Surface::open_scaled(display.clone(), max_edge)?)
                    } else {
                        None
                    };
                    let _ = tx.send(Ok::<(), String>(()));
                    while !stopped.load(std::sync::atomic::Ordering::Acquire) {
                        let start = std::time::Instant::now();
                        let frame = if let Some(capture) = &duplication {
                            // Runtime loss can mean a changed monitor or secure desktop.
                            // End this approval rather than reusing stale screen coordinates.
                            match capture.capture()? {
                                Some(frame) => frame,
                                None => continue,
                            }
                        } else {
                            let surface = fallback.as_ref().context("No capture backend")?;
                            RawFrame {
                                width: surface.width,
                                height: surface.height,
                                pixels: surface.capture()?.to_vec(),
                            }
                        };
                        let (lock, ready) = &*shared;
                        let mut slot = lock
                            .lock()
                            .map_err(|_| anyhow::anyhow!("Capture state unavailable"))?;
                        slot.frame = Some(frame);
                        ready.notify_one();
                        drop(slot);
                        if let Some(delay) = std::time::Duration::from_nanos(1_000_000_000 / 60)
                            .checked_sub(start.elapsed())
                        {
                            std::thread::sleep(delay);
                        }
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    let reason = format!("{error:#}");
                    let _ = tx.send(Err(reason.clone()));
                    let (lock, ready) = &*shared;
                    if let Ok(mut slot) = lock.lock() {
                        slot.error = Some(reason);
                        slot.frame = None;
                        ready.notify_all();
                    }
                }
            })?;
        let dimensions = rx.recv_timeout(std::time::Duration::from_secs(5));
        match dimensions {
            Ok(Ok(())) => (),
            other => {
                stop.store(true, std::sync::atomic::Ordering::Release);
                let _ = worker.join();
                anyhow::bail!("Windows capture could not start: {other:?}");
            }
        };
        Ok(Self {
            state,
            stop,
            worker: Some(worker),
            duplication_active,
            max_edge,
            factory,
            _apartment: apartment,
        })
    }
    pub fn backend_name(&self) -> &'static str {
        if self
            .duplication_active
            .load(std::sync::atomic::Ordering::Acquire)
        {
            "DXGI desktop duplication"
        } else {
            "GDI scaled capture"
        }
    }
    pub fn frame(&self, quality: u8) -> Result<Option<Message>> {
        let (lock, ready) = &*self.state;
        let slot = lock
            .lock()
            .map_err(|_| anyhow::anyhow!("Capture state unavailable"))?;
        let (mut slot, _) = ready
            .wait_timeout_while(slot, std::time::Duration::from_millis(100), |s| {
                s.frame.is_none() && s.error.is_none()
            })
            .map_err(|_| anyhow::anyhow!("Capture state unavailable"))?;
        if let Some(error) = &slot.error {
            anyhow::bail!("{error}");
        }
        let Some(raw) = slot.frame.take() else {
            return Ok(None);
        };
        drop(slot);
        let width = raw.width;
        let height = raw.height;
        encode(
            &self.factory,
            width,
            height,
            width * 4,
            &raw.pixels,
            self.max_edge,
            quality,
        )
        .map(Some)
    }
}
impl Drop for ScreenCapture {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        self.state.1.notify_all();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn encode(
    factory: &IWICImagingFactory,
    width: u32,
    height: u32,
    stride: u32,
    pixels: &[u8],
    max_edge: u32,
    quality: u8,
) -> Result<Message> {
    unsafe {
        ensure!(
            width > 0
                && height > 0
                && u64::from(width) * u64::from(height) <= MAX_FRAME_PIXELS
                && stride == width * 4
                && pixels.len() == stride as usize * height as usize,
            "Invalid native JPEG pixels"
        );
        let dimensions = scaled_dimensions(width, height, max_edge)?;
        let bitmap = factory.CreateBitmapFromMemory(
            width,
            height,
            &GUID_WICPixelFormat32bppBGR,
            stride,
            pixels,
        )?;
        let converter = factory.CreateFormatConverter()?;
        converter.Initialize(
            &bitmap,
            &GUID_WICPixelFormat24bppBGR,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )?;
        jpeg::encode_bounded(dimensions, quality, |size, quality| {
            encode_at(factory, &converter, (width, height), size, quality)
        })
    }
}

fn encode_at(
    factory: &IWICImagingFactory,
    converter: &IWICFormatConverter,
    source: (u32, u32),
    size: (u32, u32),
    quality: u8,
) -> Result<Option<Vec<u8>>> {
    unsafe {
        let stream = CreateStreamOnHGlobal(HGLOBAL::default(), true)?;
        let encoder = factory.CreateEncoder(&GUID_ContainerFormatJpeg, ptr::null())?;
        encoder.Initialize(&stream, WICBitmapEncoderNoCache)?;
        let (mut frame, mut options) = (None, None);
        encoder.CreateNewFrame(&mut frame, &mut options)?;
        let frame = frame.context("JPEG encoder did not create a frame")?;
        let options = options.context("JPEG encoder options unavailable")?;
        let quality_option = PROPBAG2 {
            pstrName: PWSTR(w!("ImageQuality").as_ptr() as *mut _),
            ..Default::default()
        };
        options.Write(
            1,
            &quality_option,
            &VARIANT::from(f32::from(quality) / 100.0),
        )?;
        frame.Initialize(&options)?;
        let (encoded_width, encoded_height) = size;
        frame.SetSize(encoded_width, encoded_height)?;
        let mut format = GUID_WICPixelFormat24bppBGR;
        frame.SetPixelFormat(&mut format)?;
        ensure!(
            format == GUID_WICPixelFormat24bppBGR,
            "Unsupported native JPEG pixel format"
        );
        // WIC scales and reads the original bitmap for every candidate.
        if source != size {
            let scaler = factory.CreateBitmapScaler()?;
            scaler.Initialize(
                converter,
                encoded_width,
                encoded_height,
                WICBitmapInterpolationModeNearestNeighbor,
            )?;
            frame.WriteSource(&scaler, ptr::null())?;
        } else {
            frame.WriteSource(converter, ptr::null())?;
        }
        frame.Commit()?;
        encoder.Commit()?;
        let mut stat = STATSTG::default();
        stream.Stat(&mut stat, STATFLAG_NONAME)?;
        ensure!(stat.cbSize > 0, "Empty native JPEG");
        if stat.cbSize > jpeg::BYTE_LIMIT as u64 {
            return Ok(None);
        }
        let handle = GetHGlobalFromStream(&stream)?;
        let data = GlobalLock(handle);
        ensure!(!data.is_null(), "Cannot read encoded JPEG");
        let jpeg = std::slice::from_raw_parts(data.cast::<u8>(), stat.cbSize as usize).to_vec();
        let _ = GlobalUnlock(handle);
        Ok(Some(jpeg))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires a visible test desktop; prints timings only"]
    fn profile_real_desktop_capture_stages() {
        unsafe {
            let _ = windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
                windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
            );
        }

        let _apartment = ComApartment::open().unwrap();
        let factory: IWICImagingFactory =
            unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }
                .unwrap();
        let display = crate::desktop::displays().unwrap().remove(0);
        for bound in [1600, crate::video::MAX_FRAME_EDGE] {
            let surface = Surface::open_scaled(display.clone(), bound).unwrap();
            let mut capture_ms = Vec::new();
            let mut encode_ms = Vec::new();
            for _ in 0..90 {
                let start = std::time::Instant::now();
                let pixels = surface.capture().unwrap();
                capture_ms.push(start.elapsed().as_secs_f64() * 1000.0);
                if bound == 1600 {
                    let start = std::time::Instant::now();
                    let _ = encode(
                        &factory,
                        surface.width,
                        surface.height,
                        surface.stride,
                        pixels,
                        bound,
                        60,
                    )
                    .unwrap();
                    encode_ms.push(start.elapsed().as_secs_f64() * 1000.0);
                }
            }
            let mean = |values: &[f64]| {
                values.iter().skip(5).sum::<f64>() / (values.len().saturating_sub(5).max(1) as f64)
            };
            println!(
                "{{\"bound\":{bound},\"gdi_ms\":{},\"wic_ms\":{}}}",
                mean(&capture_ms),
                mean(&encode_ms)
            );
        }
    }
    #[test]
    fn native_jpeg_scales_large_odd_rows_with_bounded_dimensions() {
        let _apartment = ComApartment::open().unwrap();
        let factory: IWICImagingFactory =
            unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }
                .unwrap();
        let (width, height) = (1701u32, 101u32);
        let mut pixels = vec![0; (width * height * 4) as usize];
        for y in 0..height {
            for x in 0..width {
                let offset = ((y * width + x) * 4) as usize;
                pixels[offset..offset + 4].copy_from_slice(if y < height / 2 {
                    &[0, 0, 255, 0]
                } else {
                    &[255, 0, 0, 0]
                });
            }
        }
        for (bound, quality, expected) in [
            (1600, 75, (1600, 95)),
            (2560, 85, (1701, 101)),
            (4096, 90, (1701, 101)),
        ] {
            let Message::Frame {
                width: w,
                height: h,
                jpeg,
            } = encode(&factory, width, height, width * 4, &pixels, bound, quality).unwrap()
            else {
                panic!("expected a frame");
            };
            assert_eq!((w, h), expected);
            let image = crate::desktop::decode_frame(w, h, jpeg).unwrap();
            assert!(image.rgb[3 * (8 * w + 8) as usize] > 220);
            assert!(image.rgb[3 * ((h - 8) * w + 8) as usize + 2] > 220);
        }
        assert!(encode(&factory, 32768, 32768, 131072, &[], 2560, 85).is_err());
    }
    #[test]
    fn native_jpeg_preserves_channels_odd_dimensions_and_row_order() {
        let _apartment = ComApartment::open().unwrap();
        let factory: IWICImagingFactory =
            unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }
                .unwrap();
        let (width, height) = (65u32, 49u32);
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        for y in 0..height {
            for x in 0..width {
                let offset = ((y * width + x) * 4) as usize;
                let bgr = if y < height / 2 {
                    [0, 0, 255, 0]
                } else {
                    [255, 0, 0, 0]
                };
                pixels[offset..offset + 4].copy_from_slice(&bgr);
            }
        }
        let Message::Frame { jpeg, .. } =
            encode(&factory, width, height, width * 4, &pixels, 4096, 90).unwrap()
        else {
            panic!("expected a frame");
        };
        let frame = crate::desktop::decode_frame(width, height, jpeg).unwrap();
        let top = &frame.rgb[((8 * width + 8) * 3) as usize..][..3];
        let bottom = &frame.rgb[(((height - 8) * width + 8) * 3) as usize..][..3];
        assert!(top[0] > 220 && top[2] < 30, "top red: {top:?}");
        assert!(bottom[2] > 220 && bottom[0] < 30, "bottom blue: {bottom:?}");
        assert!(encode(
            &factory,
            width,
            height,
            width * 4,
            &pixels[..pixels.len() - 1],
            1600,
            75
        )
        .is_err());
    }
}
