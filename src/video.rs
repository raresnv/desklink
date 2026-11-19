use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};

pub const MAX_FRAME_EDGE: u32 = 4096;
pub const MAX_FRAME_PIXELS: u64 = MAX_FRAME_EDGE as u64 * MAX_FRAME_EDGE as u64;

/// Authenticated v1 promise: expanded frame bounds and live profile changes.
pub const VIDEO_CAPABILITY_MARKER: &[u8] = b"DLVID001";

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VideoProfile {
    Balanced,
    #[default]
    Sharp,
    Original,
}

impl VideoProfile {
    pub fn max_edge(self) -> u32 {
        match self {
            Self::Balanced => 1600,
            Self::Sharp => 2560,
            Self::Original => MAX_FRAME_EDGE,
        }
    }

    pub fn max_quality(self) -> u8 {
        match self {
            Self::Balanced => 75,
            Self::Sharp => 85,
            Self::Original => 90,
        }
    }

    pub fn min_quality(self) -> u8 {
        match self {
            Self::Balanced => 40,
            Self::Sharp => 55,
            Self::Original => 65,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Balanced => "Balanced",
            Self::Sharp => "Sharp",
            Self::Original => "Original / Retina",
        }
    }
}

/// Fits inside the edge limit, rounding to the nearest pixel on the short axis.
pub fn scaled_dimensions(width: u32, height: u32, max_edge: u32) -> Result<(u32, u32)> {
    ensure!(width > 0 && height > 0, "Invalid source dimensions");
    ensure!(
        (1..=MAX_FRAME_EDGE).contains(&max_edge),
        "Invalid frame edge limit"
    );
    let longest = width.max(height);
    if longest <= max_edge {
        return Ok((width, height));
    }
    let longest = u64::from(longest);
    let scale = |axis: u32| {
        let numerator = u64::from(axis) * u64::from(max_edge);
        ((numerator + longest / 2) / longest).max(1) as u32
    };
    Ok((scale(width), scale(height)))
}
