//! ScreenCaptureKit owns the continuous capture. Its serial callback copies
//! only the latest bounded RGB frame; compression stays on the session worker.
use anyhow::{anyhow, ensure, Context, Result};
use block2::RcBlock;
use core_graphics::display::CGDisplay;
use dispatch2::{DispatchQueue, DispatchQueueAttr};
use image::RgbImage;
use objc2::{
    define_class, msg_send, rc::Retained, runtime::ProtocolObject, AllocAnyThread, DefinedClass,
};
use objc2_core_media::{CMSampleBuffer, CMTime};
use objc2_core_video::*;
use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol};
use objc2_screen_capture_kit::{
    SCContentFilter, SCDisplay, SCShareableContent, SCStream, SCStreamConfiguration,
    SCStreamDelegate, SCStreamOutput, SCStreamOutputType,
};
use std::{
    sync::{mpsc, Arc, Condvar, Mutex},
    time::Duration,
};

fn stream_dimensions(
    mode_pixels: Option<(u64, u64)>,
    display_points: (isize, isize),
    max_edge: u32,
) -> Result<(u32, u32)> {
    let (points_w, points_h) = display_points;
    let pixels = mode_pixels.and_then(|(w, h)| {
        let (w, h) = (u32::try_from(w).ok()?, u32::try_from(h).ok()?);
        if w == 0 || h == 0 {
            return None;
        }
        // Display modes can report the panel's unrotated pixel dimensions.
        if points_w > 0 && points_h > 0 && points_w != points_h && (points_w > points_h) != (w > h)
        {
            Some((h, w))
        } else {
            Some((w, h))
        }
    });
    let (w, h) = match pixels {
        Some(size) => size,
        None => (u32::try_from(points_w)?, u32::try_from(points_h)?),
    };
    crate::video::scaled_dimensions(w, h, max_edge)
}

fn copy_bgra(width: usize, height: usize, stride: usize, data: &[u8]) -> Result<RgbImage> {
    ensure!(
        width > 0 && height > 0 && width.max(height) <= crate::video::MAX_FRAME_EDGE as usize,
        "Invalid stream dimensions"
    );
    ensure!(
        (width * 4..=width * 4 + 4096).contains(&stride)
            && stride
                .checked_mul(height)
                .is_some_and(|len| len <= data.len()),
        "Truncated stream pixels"
    );
    let mut rgb = Vec::with_capacity(width * height * 3);
    for row in data.chunks_exact(stride).take(height) {
        for pixel in row[..width * 4].as_chunks::<4>().0 {
            rgb.extend_from_slice(&[pixel[2], pixel[1], pixel[0]]);
        }
    }
    RgbImage::from_raw(width as u32, height as u32, rgb).context("Invalid RGB stream frame")
}

#[derive(Default)]
struct State {
    frame: Option<RgbImage>,
    error: Option<String>,
}
struct Mailbox {
    state: Mutex<State>,
    ready: Condvar,
    max_edge: usize,
}
impl Mailbox {
    fn new(max_edge: u32) -> Self {
        Self {
            state: Mutex::default(),
            ready: Condvar::new(),
            max_edge: max_edge as usize,
        }
    }
    fn publish(&self, frame: RgbImage) {
        if let Ok(mut state) = self.state.lock() {
            if state.error.is_none() {
                state.frame = Some(frame);
            }
            self.ready.notify_one();
        }
    }
    fn fail(&self, error: String) {
        if let Ok(mut state) = self.state.lock() {
            state.error = Some(error);
            state.frame = None;
            self.ready.notify_all();
        }
    }
    fn take(&self, timeout: Duration) -> Result<Option<RgbImage>> {
        let state = self
            .state
            .lock()
            .map_err(|_| anyhow!("Capture state unavailable"))?;
        let (mut state, _) = self
            .ready
            .wait_timeout_while(state, timeout, |s| s.frame.is_none() && s.error.is_none())
            .map_err(|_| anyhow!("Capture state unavailable"))?;
        if let Some(error) = &state.error {
            return Err(anyhow!("Screen capture stopped: {error}"));
        }
        Ok(state.frame.take())
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "DeskLinkScreenOutput"]
    #[ivars = Arc<Mailbox>]
    struct Output;
    unsafe impl NSObjectProtocol for Output {}
    unsafe impl SCStreamOutput for Output {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        unsafe fn output(&self, _stream: &SCStream, sample: &CMSampleBuffer, kind: SCStreamOutputType) {
            if kind != SCStreamOutputType::Screen || !sample.is_valid() { return; }
            let Some(buffer) = sample.image_buffer() else { return; };
            let flags = CVPixelBufferLockFlags::ReadOnly;
            if CVPixelBufferLockBaseAddress(&buffer, flags) != 0 { self.ivars().fail("Could not lock stream pixels".into()); return; }
            let result = (|| -> Result<RgbImage> {
                let width = CVPixelBufferGetWidth(&buffer);
                let height = CVPixelBufferGetHeight(&buffer);
                let stride = CVPixelBufferGetBytesPerRow(&buffer);
                ensure!(CVPixelBufferGetPixelFormatType(&buffer) == kCVPixelFormatType_32BGRA, "Unsupported capture pixel format");
                ensure!(width > 0 && height > 0 && width.max(height) <= self.ivars().max_edge, "Unexpected stream dimensions");
                ensure!((width * 4..=width * 4 + 4096).contains(&stride), "Invalid stream row stride");
                // Up to 4096 rows of 4096 BGRA pixels plus one page of row padding.
                let len = stride * height;
                let base = CVPixelBufferGetBaseAddress(&buffer);
                ensure!(!base.is_null() && len <= CVPixelBufferGetDataSize(&buffer), "Invalid stream pixel storage");
                let data = std::slice::from_raw_parts(base.cast::<u8>(), len);
                copy_bgra(width, height, stride, data)
            })();
            CVPixelBufferUnlockBaseAddress(&buffer, flags);
            match result { Ok(frame) => self.ivars().publish(frame), Err(error) => self.ivars().fail(error.to_string()) }
        }
    }
    unsafe impl SCStreamDelegate for Output {
        #[unsafe(method(stream:didStopWithError:))]
        unsafe fn stopped(&self, _stream: &SCStream, error: &NSError) { self.ivars().fail(error.to_string()); }
    }
);

// SCDisplay is immutable display metadata. Ownership is handed from Apple's
// completion queue to the session worker; no object is mutated across threads.
struct DisplayMetadata(Retained<SCDisplay>);
unsafe impl Send for DisplayMetadata {}

pub(super) struct ScreenCapture {
    stream: Retained<SCStream>,
    _output: Retained<Output>,
    mailbox: Arc<Mailbox>,
}
impl ScreenCapture {
    pub fn open(id: u32, max_edge: u32) -> Result<Self> {
        let (tx, rx) = mpsc::sync_channel(1);
        let completion = RcBlock::new(
            move |content: *mut SCShareableContent, error: *mut NSError| {
                let result = unsafe {
                    if let Some(error) = error.as_ref() {
                        Err(error.to_string())
                    } else if let Some(content) = content.as_ref() {
                        content
                            .displays()
                            .iter()
                            .find(|display| display.displayID() == id)
                            .map(DisplayMetadata)
                            .ok_or_else(|| "The selected display was disconnected".into())
                    } else {
                        Err("No shareable displays returned".into())
                    }
                };
                let _ = tx.try_send(result);
            },
        );
        unsafe {
            SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(false, true, &completion);
        }
        let display = rx
            .recv_timeout(Duration::from_secs(8))
            .context("Display enumeration timed out")?
            .map_err(|error| anyhow!(error))?
            .0;
        let mailbox = Arc::new(Mailbox::new(max_edge));
        let output: Retained<Output> =
            unsafe { msg_send![super(Output::alloc().set_ivars(mailbox.clone())), init] };
        let queue = DispatchQueue::new("dev.desklink.screen-output", DispatchQueueAttr::SERIAL);
        let stream = unsafe {
            let mode_pixels = CGDisplay::new(id)
                .display_mode()
                .map(|mode| (mode.pixel_width(), mode.pixel_height()));
            let (width, height) =
                stream_dimensions(mode_pixels, (display.width(), display.height()), max_edge)?;
            let filter = SCContentFilter::initWithDisplay_excludingWindows(
                SCContentFilter::alloc(),
                &display,
                &NSArray::new(),
            );
            let config = SCStreamConfiguration::new();
            config.setWidth(width as usize);
            config.setHeight(height as usize);
            config.setMinimumFrameInterval(CMTime::new(1, 60));
            config.setQueueDepth(3);
            config.setPixelFormat(kCVPixelFormatType_32BGRA);
            config.setShowsCursor(true);
            // Audio capture defaults to off; register only the screen output.
            let stream = SCStream::initWithFilter_configuration_delegate(
                SCStream::alloc(),
                &filter,
                &config,
                Some(ProtocolObject::from_ref(&*output)),
            );
            stream
                .addStreamOutput_type_sampleHandlerQueue_error(
                    ProtocolObject::from_ref(&*output),
                    SCStreamOutputType::Screen,
                    Some(&queue),
                )
                .map_err(|error| anyhow!(error.to_string()))?;
            stream
        };
        let capture = Self {
            stream,
            _output: output,
            mailbox,
        };
        let (tx, rx) = mpsc::sync_channel(1);
        let started = RcBlock::new(move |error: *mut NSError| {
            let _ = tx.try_send(unsafe { error.as_ref() }.map(ToString::to_string));
        });
        unsafe {
            capture
                .stream
                .startCaptureWithCompletionHandler(Some(&started));
        }
        if let Some(error) = rx
            .recv_timeout(Duration::from_secs(8))
            .context("Screen stream startup timed out")?
        {
            return Err(anyhow!("Screen stream failed: {error}"));
        }
        Ok(capture)
    }
    pub fn frame(&self, timeout: Duration) -> Result<Option<RgbImage>> {
        self.mailbox.take(timeout)
    }
}
impl Drop for ScreenCapture {
    fn drop(&mut self) {
        self.mailbox.fail("Capture closed".into());
        let (tx, rx) = mpsc::sync_channel(1);
        let stopped = RcBlock::new(move |_error: *mut NSError| {
            let _ = tx.try_send(());
        });
        unsafe {
            self.stream.stopCaptureWithCompletionHandler(Some(&stopped));
        }
        let _ = rx.recv_timeout(Duration::from_secs(2));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_dimensions_preserve_aspect_without_upscaling() {
        let retina = Some((3600, 2338));
        assert_eq!(
            stream_dimensions(retina, (1800, 1169), 1600).unwrap(),
            (1600, 1039)
        );
        assert_eq!(
            stream_dimensions(retina, (1800, 1169), 2560).unwrap(),
            (2560, 1663)
        );
        assert_eq!(
            stream_dimensions(retina, (1800, 1169), 4096).unwrap(),
            (3600, 2338)
        );
        assert_eq!(
            stream_dimensions(retina, (1169, 1800), 4096).unwrap(),
            (2338, 3600)
        );
        assert_eq!(
            stream_dimensions(None, (1440, 900), 4096).unwrap(),
            (1440, 900)
        );
        assert_eq!(
            stream_dimensions(Some((0, 100)), (900, 1800), 1600).unwrap(),
            (800, 1600)
        );
        assert!(stream_dimensions(None, (-1, 100), 1600).is_err());
    }

    #[test]
    fn retina_bgra_storage_allows_padded_rows_and_rejects_invalid_sizes() {
        let width = 3600;
        let stride = width * 4 + 64;
        assert_eq!(
            copy_bgra(width, 2, stride, &vec![0; stride * 2])
                .unwrap()
                .width(),
            3600
        );
        assert!(copy_bgra(4097, 1, 4097 * 4, &vec![0; 4097 * 4]).is_err());
        assert!(copy_bgra(1, 1, 65536, &vec![0; 65536]).is_err());
        assert!(copy_bgra(2, 2, usize::MAX, &[]).is_err());
    }

    #[test]
    fn packed_pixels_handle_padding_and_bgra_channel_order() {
        let bytes = [
            10, 20, 30, 255, 40, 50, 60, 255, 99, 99, 99, 99, 70, 80, 90, 255, 100, 110, 120, 255,
            99, 99, 99, 99,
        ];
        let image = copy_bgra(2, 2, 12, &bytes).unwrap();
        assert_eq!(
            image.into_raw(),
            [30, 20, 10, 60, 50, 40, 90, 80, 70, 120, 110, 100]
        );
        assert!(copy_bgra(2, 2, 7, &bytes).is_err());
        assert!(copy_bgra(2, 2, 12, &bytes[..15]).is_err());
    }

    #[test]
    fn newest_frame_replaces_old_frame_and_errors_take_priority() {
        let mailbox = Mailbox::new(1600);
        mailbox.publish(image::RgbImage::new(2, 1));
        mailbox.publish(image::RgbImage::new(3, 1));
        assert_eq!(
            mailbox
                .take(std::time::Duration::ZERO)
                .unwrap()
                .unwrap()
                .width(),
            3
        );
        assert!(mailbox.take(std::time::Duration::ZERO).unwrap().is_none());
        mailbox.publish(image::RgbImage::new(2, 1));
        mailbox.fail("Stream was stopped".into());
        assert!(mailbox
            .take(std::time::Duration::ZERO)
            .unwrap_err()
            .to_string()
            .contains("stopped"));
    }
}
