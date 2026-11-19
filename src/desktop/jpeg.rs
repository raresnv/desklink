use crate::protocol::{Message, MAX_MESSAGE};
use crate::video::scaled_dimensions;
use anyhow::{bail, Result};

// Message::validate reserves 128 bytes and requires a strict upper bound.
pub(super) const BYTE_LIMIT: usize = MAX_MESSAGE - 129;

/// A full encoder or I/O error is terminal. Only a completed over-budget
/// attempt (or a writer stopped at the limit) asks for a smaller JPEG.
pub(super) fn encode_bounded(
    dimensions: (u32, u32),
    quality: u8,
    mut attempt: impl FnMut((u32, u32), u8) -> Result<Option<Vec<u8>>>,
) -> Result<Message> {
    let (width, height) = dimensions;
    let edge = width.max(height);
    let quality = quality.clamp(40, 90);
    // Keep all pixels through quality 40. Two emergency sizes cap the total
    // at five encodes, with a final long edge of at most 1024 pixels.
    let candidates = [
        (edge, quality),
        (edge, (quality + 40) / 2),
        (edge, 40),
        ((edge / 2).max(1), 40),
        ((edge / 4).max(1), 40),
    ];
    let mut previous = None;
    for candidate in candidates {
        if previous == Some(candidate) {
            continue;
        }
        previous = Some(candidate);
        let (edge, quality) = candidate;
        let size = scaled_dimensions(width, height, edge)?;
        if let Some(jpeg) = attempt(size, quality)? {
            return Ok(Message::Frame {
                width: size.0,
                height: size.1,
                jpeg,
            });
        }
    }
    bail!("JPEG exceeds the frame budget at the smallest emergency size")
}

#[cfg(any(test, not(target_os = "windows")))]
pub(super) fn encode_rgb(image: &image::RgbImage, quality: u8, budget: usize) -> Result<Message> {
    use image::codecs::jpeg::JpegEncoder;
    use std::io::{self, BufWriter, Write};

    struct BoundedBytes {
        bytes: Vec<u8>,
        budget: usize,
        full: bool,
    }
    impl Write for BoundedBytes {
        fn write(&mut self, data: &[u8]) -> io::Result<usize> {
            if data.len() > self.budget - self.bytes.len() {
                self.full = true;
                return Err(io::Error::other("JPEG byte limit reached"));
            }
            self.bytes.extend_from_slice(data);
            Ok(data.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    encode_bounded(image.dimensions(), quality, |size, quality| {
        let smaller;
        let source = if size == image.dimensions() {
            image
        } else {
            smaller = image::imageops::thumbnail(image, size.0, size.1);
            &smaller
        };
        let mut output = BoundedBytes {
            bytes: Vec::new(),
            budget,
            full: false,
        };
        let encoded = (|| -> Result<()> {
            let mut writer = BufWriter::new(&mut output);
            JpegEncoder::new_with_quality(&mut writer, quality).encode_image(source)?;
            writer.flush()?;
            Ok(())
        })();
        if output.full {
            return Ok(None);
        }
        encoded?;
        Ok(Some(output.bytes))
    })
}
