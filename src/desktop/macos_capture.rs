//! ScreenCaptureKit owns the continuous capture. Its serial callback copies
//! only the latest bounded RGB frame; compression stays on the session worker.
use anyhow::{anyhow, ensure, Context, Result};
use block2::RcBlock;
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

pub(super) fn stream_dimensions(width: usize, height: usize) -> Result<(usize, usize)> {
    ensure!(
        width > 0 && height > 0,
        "The selected display has empty bounds"
    );
    let scale = 1600.0 / width.max(height) as f64;
    let scale = scale.min(1.0);
    Ok((
        ((width as f64 * scale).round() as usize).max(1),
        ((height as f64 * scale).round() as usize).max(1),
    ))
}

fn copy_bgra(width: usize, height: usize, stride: usize, data: &[u8]) -> Result<RgbImage> {
    ensure!(
        width > 0 && height > 0 && width <= 1600 && height <= 1600,
        "Invalid stream dimensions"
    );
    ensure!(
        stride >= width * 4
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
#[derive(Default)]
struct Mailbox {
    state: Mutex<State>,
    ready: Condvar,
}
impl Mailbox {
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
                let len = stride.checked_mul(height).context("Stream pixel size overflow")?;
                let base = CVPixelBufferGetBaseAddress(&buffer);
                ensure!(!base.is_null() && len <= 32 * 1024 * 1024, "Invalid stream pixel storage");
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
    pub fn open(id: u32) -> Result<Self> {
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
        let mailbox = Arc::new(Mailbox::default());
        let output: Retained<Output> =
            unsafe { msg_send![super(Output::alloc().set_ivars(mailbox.clone())), init] };
        let queue = DispatchQueue::new("dev.desklink.screen-output", DispatchQueueAttr::SERIAL);
        let stream = unsafe {
            let (width, height) =
                stream_dimensions(display.width() as usize, display.height() as usize)?;
            let filter = SCContentFilter::initWithDisplay_excludingWindows(
                SCContentFilter::alloc(),
                &display,
                &NSArray::new(),
            );
            let config = SCStreamConfiguration::new();
            config.setWidth(width);
            config.setHeight(height);
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
        assert_eq!(stream_dimensions(1800, 1169).unwrap(), (1600, 1039));
        assert_eq!(stream_dimensions(1440, 900).unwrap(), (1440, 900));
        assert_eq!(stream_dimensions(900, 1800).unwrap(), (800, 1600));
        assert!(stream_dimensions(0, 100).is_err());
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
        let mailbox = Mailbox::default();
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
