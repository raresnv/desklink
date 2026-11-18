use anyhow::{ensure, Context, Result};
use bincode::Options;
use serde::{Deserialize, Serialize};

pub const MAX_MESSAGE: usize = 8 * 1024 * 1024;
pub const MAX_FRAME_PIXELS: u64 = 1600 * 1600;
pub const VERSION: &[u8] = b"DeskLink-v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesktopInfo {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    /// Physical pixels per input coordinate. Only macOS uses logical points.
    pub scale: f32,
}

impl DesktopInfo {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1..=32768).contains(&self.width) && (1..=32768).contains(&self.height),
            "Invalid display dimensions"
        );
        ensure!(
            self.scale.is_finite() && (0.25..=8.0).contains(&self.scale),
            "Invalid display scale"
        );
        ensure!(
            self.x.abs_diff(0) <= 131072 && self.y.abs_diff(0) <= 131072,
            "Invalid display origin"
        );
        ensure!(self.name.len() <= 256, "Display name is too long");
        Ok(())
    }

    pub fn map_pointer(&self, x: f32, y: f32) -> Result<(i32, i32)> {
        self.validate()?;
        ensure!(
            x.is_finite() && y.is_finite(),
            "Invalid pointer coordinates"
        );
        let width = (self.width as f32 / self.scale).round().max(1.0) as i32;
        let height = (self.height as f32 / self.scale).round().max(1.0) as i32;
        Ok((
            self.x + (x.clamp(0.0, 1.0) * (width - 1) as f32).round() as i32,
            self.y + (y.clamp(0.0, 1.0) * (height - 1) as f32).round() as i32,
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Key {
    Character(char),
    Enter,
    Tab,
    Escape,
    Backspace,
    Delete,
    Insert,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Shift,
    Control,
    Alt,
    Meta,
    Function(u8),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InputEvent {
    Move { x: f32, y: f32 },
    Button { button: MouseButton, down: bool },
    Scroll { horizontal: i32, vertical: i32 },
    Key { key: Key, down: bool },
    Text(String),
    ReleaseAll,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    Request {
        name: String,
        control: bool,
    },
    Approved {
        desktop: DesktopInfo,
        control: bool,
    },
    Denied(String),
    Frame {
        width: u32,
        height: u32,
        jpeg: Vec<u8>,
    },
    Input(InputEvent),
    Ping(u64),
    Pong(u64),
    Error(String),
    Disconnect,
}

impl Message {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Request { name, .. } => ensure!(
                !name.trim().is_empty() && name.len() <= 80 && !name.chars().any(char::is_control),
                "Viewer name must be 1–80 characters without control characters"
            ),
            Self::Approved { desktop, .. } => desktop.validate()?,
            Self::Frame {
                width,
                height,
                jpeg,
            } => {
                ensure!(
                    *width > 0
                        && *height > 0
                        && u64::from(*width) * u64::from(*height) <= MAX_FRAME_PIXELS,
                    "Invalid frame dimensions"
                );
                ensure!(
                    !jpeg.is_empty() && jpeg.len() < MAX_MESSAGE - 128,
                    "Invalid frame size"
                );
            }
            Self::Input(InputEvent::Move { x, y }) => ensure!(
                x.is_finite() && y.is_finite(),
                "Invalid pointer coordinates"
            ),
            Self::Input(InputEvent::Text(text)) => {
                ensure!(text.len() <= 8192, "Text input is too long")
            }
            Self::Input(InputEvent::Key {
                key: Key::Function(n),
                ..
            }) => ensure!((1..=12).contains(n), "Unsupported function key"),
            Self::Input(InputEvent::Scroll {
                horizontal,
                vertical,
            }) => ensure!(
                horizontal.abs_diff(0) <= 100 && vertical.abs_diff(0) <= 100,
                "Scroll is too large"
            ),
            Self::Error(text) | Self::Denied(text) => {
                ensure!(text.len() <= 1024, "Status message is too long")
            }
            _ => {}
        }
        Ok(())
    }
}

fn codec() -> impl Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_limit(MAX_MESSAGE as u64)
        .reject_trailing_bytes()
}

pub fn encode(message: &Message) -> Result<Vec<u8>> {
    message.validate()?;
    codec()
        .serialize(message)
        .context("Could not encode message")
}

pub fn decode(bytes: &[u8]) -> Result<Message> {
    ensure!(
        !bytes.is_empty() && bytes.len() <= MAX_MESSAGE,
        "Invalid message length"
    );
    let message: Message = codec()
        .deserialize(bytes)
        .context("Invalid protocol message")?;
    message.validate()?;
    Ok(message)
}

#[derive(Debug, Default, Clone, Copy)]
pub struct AccessPolicy {
    approved: bool,
    control: bool,
}

impl AccessPolicy {
    pub fn approve(&mut self, control: bool) {
        self.approved = true;
        self.control = control;
    }
    pub fn revoke(&mut self) {
        self.approved = false;
        self.control = false;
    }
    pub fn can_view(&self) -> bool {
        self.approved
    }
    pub fn can_control(&self) -> bool {
        self.approved && self.control
    }
}
