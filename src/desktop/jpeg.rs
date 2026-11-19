use crate::protocol::{Message, MAX_MESSAGE};
use crate::video::scaled_dimensions;
use anyhow::{bail, ensure, Context, Result};

// Message::validate reserves 128 bytes and requires a strict upper bound.
pub(super) const BYTE_LIMIT: usize = MAX_MESSAGE - 129;

fn skip_marker_fill(mut bytes: &[u8]) -> &[u8] {
    while let Some(rest) = bytes.strip_prefix(&[0xff]) {
        bytes = rest;
    }
    bytes
}

/// Screen encoders use one baseline scan. The pinned decoder streams those
/// rows; progressive and component-separated scans need full coefficient planes.
/// Validate the entire marker sequence before constructing that decoder.
pub(super) fn validate_baseline(jpeg: &[u8]) -> Result<()> {
    const INVALID: &str = "Unsupported or malformed baseline screen JPEG";
    let mut input = jpeg.strip_prefix(&[0xff, 0xd8]).context(INVALID)?;
    let mut components: &[u8] = &[];
    loop {
        ensure!(input.first() == Some(&0xff), INVALID);
        input = skip_marker_fill(input);
        let (&marker, rest) = input.split_first().context(INVALID)?;
        let (&[hi, lo], _) = rest.split_first_chunk::<2>().context(INVALID)?;
        let length = usize::from(u16::from_be_bytes([hi, lo]));
        let segment = rest.get(2..length).context(INVALID)?;
        input = &rest[length..];
        // The bundled encoders emit short headers. Bound decoder metadata and
        // table storage separately from the permitted entropy-coded byte count.
        ensure!(jpeg.len() - input.len() <= 256 * 1024, INVALID);
        match marker {
            0xc0 => {
                ensure!(
                    components.is_empty() && segment.len() >= 6 && segment[0] == 8,
                    INVALID
                );
                components = &segment[6..];
                let count = usize::from(segment[5]);
                ensure!(
                    matches!(count, 1 | 3) && components.len() == 3 * count,
                    INVALID
                );
                for (index, component) in components.chunks_exact(3).enumerate() {
                    let sampling = component[1];
                    ensure!(
                        sampling == 0x11
                            || (index == 0 && count == 3 && matches!(sampling, 0x12 | 0x21 | 0x22)),
                        INVALID
                    );
                }
            }
            0xda => {
                let count = components.len() / 3;
                ensure!(
                    count > 0 && segment.len() == 4 + count * 2 && usize::from(segment[0]) == count,
                    INVALID
                );
                ensure!(segment[1 + count * 2..] == [0, 63, 0], INVALID);
                for (selector, component) in segment[1..1 + count * 2]
                    .chunks_exact(2)
                    .zip(components.chunks_exact(3))
                {
                    ensure!(selector[0] == component[0], INVALID);
                }
                // Only escaped FF, restart markers and EOI may follow the scan.
                while let Some(pos) = input.iter().position(|&byte| byte == 0xff) {
                    let (&code, tail) = skip_marker_fill(&input[pos..])
                        .split_first()
                        .context(INVALID)?;
                    input = tail;
                    match code {
                        0x00 | 0xd0..=0xd7 => {}
                        0xd9 if input.is_empty() => return Ok(()),
                        _ => bail!(INVALID),
                    }
                }
                bail!(INVALID);
            }
            0xc4 | 0xdb | 0xdd | 0xe0..=0xef | 0xfe => {}
            _ => bail!(INVALID),
        }
    }
}

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
