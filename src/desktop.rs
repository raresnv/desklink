use crate::protocol::{DesktopInfo, InputEvent, Key, Message, MouseButton};
use crate::video::{VideoProfile, MAX_FRAME_EDGE, MAX_FRAME_PIXELS};
use anyhow::{bail, ensure, Context, Result};
use enigo::{Axis, Direction, Enigo, Keyboard, Mouse, Settings};
#[cfg(not(target_os = "macos"))]
use enigo::{Button, Coordinate};
#[cfg(test)]
use image::codecs::jpeg::JpegEncoder;
use image::{codecs::jpeg::JpegDecoder, DynamicImage, ImageDecoder, Limits};
#[cfg(target_os = "macos")]
use std::collections::HashMap;
use std::{collections::HashSet, io::Cursor};
use xcap::Monitor;

#[derive(Clone, Debug)]
pub struct Display {
    pub id: u32,
    pub info: DesktopInfo,
}

mod jpeg;
pub mod permissions;

pub fn displays() -> Result<Vec<Display>> {
    Monitor::all()
        .context("Could not list displays")?
        .into_iter()
        .map(|monitor| {
            let info = DesktopInfo {
                name: monitor.name()?.chars().take(80).collect(),
                width: monitor.width()?,
                height: monitor.height()?,
                x: monitor.x()?,
                y: monitor.y()?,
                // XCap's macOS bounds and Enigo's pointer coordinates both use
                // logical points. Captured pixel dimensions can differ on Retina.
                scale: 1.0,
            };
            info.validate()?;
            Ok(Display {
                id: monitor.id()?,
                info,
            })
        })
        .collect()
}

#[cfg(any(test, not(any(target_os = "macos", target_os = "windows"))))]
fn validate_x11_source(width: u32, height: u32) -> Result<()> {
    ensure!(
        width > 0 && height > 0 && u64::from(width) * u64::from(height) <= MAX_FRAME_PIXELS,
        "The selected display exceeds the 16-megapixel native X11 capture limit"
    );
    Ok(())
}

/// Request a fixed X11 region so a concurrent mode change cannot enlarge the
/// pixel allocation between checking the dimensions and asking the X server.
#[cfg(any(test, not(any(target_os = "macos", target_os = "windows"))))]
pub fn capture_x11_monitor(monitor: &Monitor) -> Result<image::RgbaImage> {
    // Mirrors XCap's backend selection. Its Wayland portal may capture all
    // monitors even for a region request, which has no enforceable byte bound.
    let wayland = std::env::var_os("XDG_SESSION_TYPE").is_some_and(|v| v == "wayland")
        || std::env::var_os("WAYLAND_DISPLAY")
            .is_some_and(|v| v.to_string_lossy().to_lowercase().contains("wayland"));
    ensure!(
        !wayland,
        "Bounded screen capture requires an X11 display session"
    );
    let (width, height) = (monitor.width()?, monitor.height()?);
    validate_x11_source(width, height)?;
    let image = monitor
        .capture_region(0, 0, width, height)
        .context("X11 display capture failed")?;
    ensure!(
        image.dimensions() == (width, height),
        "Display dimensions changed during capture"
    );
    Ok(image)
}

pub struct Capturer {
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    monitor: Monitor,
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    max_edge: u32,
    #[cfg(target_os = "windows")]
    capture: windows_capture::ScreenCapture,
    #[cfg(target_os = "macos")]
    capture: macos_capture::ScreenCapture,
}
impl Capturer {
    pub fn open(id: u32) -> Result<Self> {
        Self::open_with_max_edge(id, VideoProfile::Balanced.max_edge())
    }
    pub fn open_with_max_edge(id: u32, max_edge: u32) -> Result<Self> {
        ensure!(
            (1..=MAX_FRAME_EDGE).contains(&max_edge),
            "Invalid frame edge limit"
        );
        check_capture_permission()?;
        #[cfg(target_os = "macos")]
        return Ok(Self {
            capture: macos_capture::ScreenCapture::open(id, max_edge)?,
        });
        #[cfg(target_os = "windows")]
        return Ok(Self {
            capture: windows_capture::ScreenCapture::open(
                displays()?
                    .into_iter()
                    .find(|d| d.id == id)
                    .context("The selected display was disconnected")?,
                max_edge,
            )?,
        });
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let monitor = Monitor::all()?
                .into_iter()
                .find(|m| m.id().ok() == Some(id))
                .context("The selected display was disconnected")?;
            Ok(Self { monitor, max_edge })
        }
    }

    pub fn backend_name(&self) -> &'static str {
        #[cfg(target_os = "windows")]
        {
            self.capture.backend_name()
        }
        #[cfg(target_os = "macos")]
        {
            "ScreenCaptureKit"
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            "XCap"
        }
    }
    pub fn frame(&self) -> Result<Option<Message>> {
        self.frame_with_quality(VideoProfile::Balanced.max_quality())
    }
    pub fn frame_with_quality(&self, quality: u8) -> Result<Option<Message>> {
        check_capture_permission()?;
        #[cfg(target_os = "windows")]
        return self.capture.frame(quality);
        #[cfg(not(target_os = "windows"))]
        {
            #[cfg(target_os = "macos")]
            let image = match self.capture.frame(std::time::Duration::from_millis(100))? {
                Some(frame) => frame,
                None => return Ok(None), // A static desktop need not generate duplicate frames.
            };
            #[cfg(not(target_os = "macos"))]
            let image = {
                let rgba = capture_x11_monitor(&self.monitor)?;
                let (width, height) =
                    crate::video::scaled_dimensions(rgba.width(), rgba.height(), self.max_edge)?;
                let image = DynamicImage::ImageRgba8(rgba);
                if (width, height) == (image.width(), image.height()) {
                    image.into_rgb8()
                } else {
                    image.thumbnail_exact(width, height).into_rgb8()
                }
            };
            jpeg::encode_rgb(&image, quality, jpeg::BYTE_LIMIT).map(Some)
        }
    }
}

#[cfg(target_os = "windows")]
mod windows_capture;
#[cfg(target_os = "windows")]
mod windows_duplication;

pub struct RemoteFrame {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
    pub bytes: usize,
}

pub fn decode_frame(width: u32, height: u32, jpeg: Vec<u8>) -> Result<RemoteFrame> {
    ensure!(
        (1..=MAX_FRAME_EDGE).contains(&width)
            && (1..=MAX_FRAME_EDGE).contains(&height)
            && u64::from(width) * u64::from(height) <= MAX_FRAME_PIXELS,
        "Frame dimensions exceed limit"
    );
    let bytes = jpeg.len();
    ensure!(
        (1..=jpeg::BYTE_LIMIT).contains(&bytes),
        "Invalid frame size"
    );
    jpeg::validate_baseline(&jpeg)?;
    let mut decoder = JpegDecoder::new(Cursor::new(jpeg)).context("Invalid screen frame")?;
    ensure!(
        decoder.dimensions() == (width, height),
        "Frame dimensions do not match its header"
    );
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_FRAME_EDGE);
    limits.max_image_height = Some(MAX_FRAME_EDGE);
    limits.max_alloc = Some(128 * 1024 * 1024);
    // A baseline scan with up to three components and 2x2 sampling uses
    // row-sized scratch. Allow 32 MiB for tables, metadata and MCU rows,
    // plus both compressed input buffers and any grayscale-to-RGB conversion.
    limits.reserve(32 * 1024 * 1024 + 2 * bytes as u64)?;
    if decoder.color_type() == image::ColorType::L8 {
        limits.reserve(3 * u64::from(width) * u64::from(height))?;
    }
    limits.reserve(decoder.total_bytes())?;
    decoder.set_limits(limits)?;
    let image = DynamicImage::from_decoder(decoder)
        .context("Invalid screen frame")?
        .into_rgb8();
    Ok(RemoteFrame {
        width,
        height,
        rgb: image.into_raw(),
        bytes,
    })
}

pub struct InputController {
    enigo: Enigo,
    desktop: DesktopInfo,
    keys: HashSet<Key>,
    buttons: HashSet<MouseButton>,
    #[cfg(target_os = "macos")]
    keycodes: HashMap<char, u16>,
    #[cfg(target_os = "macos")]
    implicit_fn: bool,
    #[cfg(target_os = "macos")]
    pointer: macos_pointer::Pointer,
}

fn check_capture_permission() -> Result<()> {
    #[cfg(target_os = "macos")]
    require_capture_permission(core_graphics::access::ScreenCaptureAccess.preflight())?;
    Ok(())
}

#[cfg(any(test, target_os = "macos"))]
fn require_capture_permission(allowed: bool) -> Result<()> {
    ensure!(allowed, "Screen Recording permission is missing or no longer matches this build. Enable DeskLink in macOS Privacy & Security > Screen & System Audio Recording, then restart. If its switch is already on after a rebuild, remove DeskLink's old entry and add the current app again");
    Ok(())
}

#[cfg(target_os = "macos")]
mod macos_capture;
#[cfg(target_os = "macos")]
mod macos_pointer;

#[cfg(target_os = "macos")]
pub fn resolve_character_keycode(character: char) -> Result<u16> {
    // Carbon's TSMGetInputSourceProperty traps when a GUI app calls it from
    // a worker on current macOS. Resolve on the running native main loop;
    // raw key posting remains on the session worker.
    dispatch2::run_on_main(move |_| u16::try_from(enigo::Key::Unicode(character)))
        .map_err(|_| anyhow::anyhow!("Could not resolve the keyboard layout"))
}

impl InputController {
    pub fn new(desktop: DesktopInfo) -> Result<Self> {
        let settings = Settings {
            open_prompt_to_get_permissions: false,
            ..Settings::default()
        };
        let enigo = Enigo::new(&settings).context("Remote control unavailable. Grant Accessibility permission on macOS or use an X11 session on Linux")?;
        Ok(Self {
            enigo,
            desktop,
            keys: HashSet::new(),
            buttons: HashSet::new(),
            #[cfg(target_os = "macos")]
            keycodes: HashMap::new(),
            #[cfg(target_os = "macos")]
            implicit_fn: false,
            #[cfg(target_os = "macos")]
            pointer: macos_pointer::Pointer::new()?,
        })
    }

    pub fn apply(&mut self, input: InputEvent) -> Result<()> {
        match input {
            InputEvent::Move { x, y } => {
                let (x, y) = self.desktop.map_pointer(x, y)?;
                #[cfg(target_os = "macos")]
                self.pointer.move_to(x, y, &self.buttons, &self.keys)?;
                #[cfg(not(target_os = "macos"))]
                self.enigo.move_mouse(x, y, Coordinate::Abs)?;
            }
            InputEvent::Button { button, down } => {
                if down {
                    self.buttons.insert(button);
                } else {
                    self.buttons.remove(&button);
                }
                self.send_button(button, down)?;
            }
            InputEvent::Scroll {
                horizontal,
                vertical,
            } => {
                if horizontal != 0 {
                    self.enigo
                        .scroll(horizontal.clamp(-100, 100), Axis::Horizontal)?;
                }
                if vertical != 0 {
                    self.enigo
                        .scroll(vertical.clamp(-100, 100), Axis::Vertical)?;
                }
            }
            InputEvent::Key { key, down } => {
                if down {
                    ensure!(
                        self.keys.len() < 64 || self.keys.contains(&key),
                        "Too many held keys"
                    );
                    self.keys.insert(key);
                } else {
                    self.keys.remove(&key);
                }
                self.send_key(key, down)?;
            }
            InputEvent::Text(text) => {
                ensure!(text.len() <= 8192, "Text is too long");
                type_text(&mut self.enigo, &text)?;
            }
            InputEvent::ReleaseAll => self.release_all(),
        }
        Ok(())
    }

    pub fn release_all(&mut self) {
        let keys: Vec<_> = self.keys.drain().collect();
        for key in keys {
            let _ = self.send_key(key, false);
        }
        #[cfg(target_os = "macos")]
        let _ = release_macos_fn(&mut self.enigo, &mut self.implicit_fn);
        let buttons: Vec<_> = self.buttons.drain().collect();
        for button in buttons {
            let _ = self.send_button(button, false);
        }
    }

    fn send_button(&mut self, button: MouseButton, down: bool) -> Result<()> {
        #[cfg(target_os = "macos")]
        self.pointer.button(button, down, &self.keys)?;
        #[cfg(not(target_os = "macos"))]
        self.enigo.button(map_button(button), direction(down))?;
        Ok(())
    }

    fn send_key(&mut self, key: Key, down: bool) -> Result<()> {
        #[cfg(target_os = "macos")]
        if let Key::Character(character) = key {
            // Keep the press/release pair on the same physical key even if
            // the host changes keyboard layout while a key is held. Releases
            // never dispatch to the UI, including during app shutdown.
            let keycode = if let Some(code) = self.keycodes.get(&character) {
                *code
            } else if down {
                ensure!(
                    self.keycodes.len() < 256,
                    "Too many distinct shortcut keys in one session"
                );
                let code = resolve_character_keycode(character)?;
                self.keycodes.insert(character, code);
                code
            } else {
                return Ok(());
            };
            self.enigo.raw(keycode, direction(down))?;
            return Ok(());
        }
        #[cfg(target_os = "macos")]
        send_macos_named_key(&mut self.enigo, &mut self.implicit_fn, key, down)?;
        #[cfg(not(target_os = "macos"))]
        self.enigo.key(map_key(key)?, direction(down))?;
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn macos_key_uses_fn(key: Key) -> bool {
    matches!(
        key,
        Key::Function(_)
            | Key::Insert
            | Key::Delete
            | Key::Up
            | Key::Down
            | Key::Left
            | Key::Right
            | Key::Home
            | Key::End
            | Key::PageUp
            | Key::PageDown
    )
}
#[cfg(target_os = "macos")]
fn send_macos_named_key(
    keyboard: &mut impl Keyboard,
    fn_active: &mut bool,
    key: Key,
    down: bool,
) -> Result<()> {
    let uses_fn = macos_key_uses_fn(key);
    let mapped = map_key(key)?;
    // The backend also inserts SecondaryFn on key-up, including duplicate ups.
    if uses_fn {
        *fn_active = true;
    }
    let sent = keyboard.key(mapped, direction(down));
    // Enigo keeps the SecondaryFn flag after navigation/function-key releases.
    // Release only the implicit modifier created by our own events, including
    // when the preceding key-up reports a partial failure.
    let cleanup = if !down && uses_fn {
        release_macos_fn(keyboard, fn_active)
    } else {
        Ok(())
    };
    sent?;
    cleanup?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn release_macos_fn(keyboard: &mut impl Keyboard, fn_active: &mut bool) -> Result<()> {
    if *fn_active {
        keyboard.raw(63, Direction::Release)?;
        *fn_active = false;
    }
    Ok(())
}

fn type_text(keyboard: &mut impl Keyboard, text: &str) -> Result<()> {
    if text.is_empty() {
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    let typed = type_windows_text(keyboard, text);
    #[cfg(not(target_os = "windows"))]
    let typed = keyboard.text(text);
    // Enigo's macOS fast-text path posts Unicode key-down events on code 0
    // without a matching key-up. Release that key even after a partial error.
    #[cfg(target_os = "macos")]
    let released = keyboard.raw(0, Direction::Release);
    typed?;
    #[cfg(target_os = "macos")]
    released?;
    Ok(())
}

#[cfg(any(test, target_os = "windows"))]
fn type_windows_text(keyboard: &mut impl Keyboard, text: &str) -> Result<()> {
    ensure!(!text.contains('\0'), "Text contains a null character");
    // Enigo 0.3 sends both a virtual key and a Unicode character for LF/Tab
    // on Windows. Keep those separators out of its text path, and normalize
    // CRLF so a pasted line ending produces exactly one Return.
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((index, character)) = chars.next() {
        let key = match character {
            '\r' | '\n' => enigo::Key::Return,
            '\t' => enigo::Key::Tab,
            _ => continue,
        };
        if start < index {
            keyboard.text(&text[start..index])?;
        }
        start = index + character.len_utf8();
        if character == '\r' && chars.peek().is_some_and(|(_, ch)| *ch == '\n') {
            start = chars.next().expect("peeked line feed").0 + 1;
        }
        keyboard.key(key, Direction::Click)?;
    }
    if start < text.len() {
        keyboard.text(&text[start..])?;
    }
    Ok(())
}

impl Drop for InputController {
    fn drop(&mut self) {
        self.release_all();
    }
}
fn direction(down: bool) -> Direction {
    if down {
        Direction::Press
    } else {
        Direction::Release
    }
}
#[cfg(not(target_os = "macos"))]
fn map_button(button: MouseButton) -> Button {
    match button {
        MouseButton::Left => Button::Left,
        MouseButton::Right => Button::Right,
        MouseButton::Middle => Button::Middle,
    }
}
fn map_key(key: Key) -> Result<enigo::Key> {
    use enigo::Key as E;
    Ok(match key {
        Key::Character(c) => E::Unicode(c),
        Key::Enter => E::Return,
        Key::Tab => E::Tab,
        Key::Escape => E::Escape,
        Key::Backspace => E::Backspace,
        Key::Delete => E::Delete,
        Key::Insert => {
            #[cfg(not(target_os = "macos"))]
            {
                E::Insert
            }
            #[cfg(target_os = "macos")]
            {
                E::Help
            }
        }
        Key::Up => E::UpArrow,
        Key::Down => E::DownArrow,
        Key::Left => E::LeftArrow,
        Key::Right => E::RightArrow,
        Key::Home => E::Home,
        Key::End => E::End,
        Key::PageUp => E::PageUp,
        Key::PageDown => E::PageDown,
        Key::Shift => E::Shift,
        Key::Control => E::Control,
        Key::Alt => E::Alt,
        Key::Meta => E::Meta,
        Key::Function(n) => match n {
            1 => E::F1,
            2 => E::F2,
            3 => E::F3,
            4 => E::F4,
            5 => E::F5,
            6 => E::F6,
            7 => E::F7,
            8 => E::F8,
            9 => E::F9,
            10 => E::F10,
            11 => E::F11,
            12 => E::F12,
            _ => bail!("Unsupported function key"),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct WindowsTextKeyboard(Vec<String>);

    impl Keyboard for WindowsTextKeyboard {
        fn text(&mut self, text: &str) -> enigo::InputResult<()> {
            self.0.push(format!("text:{text}"));
            Ok(())
        }
        fn fast_text(&mut self, _: &str) -> enigo::InputResult<Option<()>> {
            unreachable!()
        }
        fn key(&mut self, key: enigo::Key, direction: Direction) -> enigo::InputResult<()> {
            assert!(matches!(direction, Direction::Click));
            self.0.push(format!("key:{key:?}"));
            Ok(())
        }
        fn raw(&mut self, _: u16, _: Direction) -> enigo::InputResult<()> {
            unreachable!()
        }
    }

    #[test]
    fn sharing_permissions_are_requested_only_for_the_approved_scope() {
        use super::permissions::{Permission, SharingPermissions};
        let permissions = SharingPermissions {
            screen: false,
            control: false,
        };
        assert_eq!(
            permissions.missing(false),
            Some(Permission::ScreenRecording)
        );
        let viewing = SharingPermissions {
            screen: true,
            control: false,
        };
        assert_eq!(viewing.missing(false), None);
        assert_eq!(viewing.missing(true), Some(Permission::Accessibility));
        assert_eq!(
            SharingPermissions {
                screen: true,
                control: true
            }
            .missing(true),
            None
        );
    }

    #[test]
    fn windows_text_injects_each_line_break_and_tab_once() {
        let mut keyboard = WindowsTextKeyboard::default();
        type_windows_text(&mut keyboard, "one😀\r\ntwo\n\rthree\tend").unwrap();
        assert_eq!(
            keyboard.0,
            [
                "text:one😀",
                "key:Return",
                "text:two",
                "key:Return",
                "key:Return",
                "text:three",
                "key:Tab",
                "text:end"
            ]
        );
    }

    #[test]
    fn windows_text_rejects_null_before_injecting_anything() {
        let mut keyboard = WindowsTextKeyboard::default();
        assert!(type_windows_text(&mut keyboard, "prefix\n\0suffix").is_err());
        assert!(keyboard.0.is_empty());
        type_windows_text(&mut keyboard, "").unwrap();
        assert!(keyboard.0.is_empty());
    }

    #[cfg(target_os = "macos")]
    #[derive(Default)]
    struct TextKeyboard {
        held: bool,
        fail: bool,
        events: Vec<&'static str>,
    }

    #[cfg(target_os = "macos")]
    impl Keyboard for TextKeyboard {
        fn fast_text(&mut self, _text: &str) -> enigo::InputResult<Option<()>> {
            self.held = true;
            self.events.push("text");
            if self.fail {
                Err(enigo::InputError::Simulate("partial text failure"))
            } else {
                Ok(Some(()))
            }
        }

        fn key(&mut self, _key: enigo::Key, _direction: Direction) -> enigo::InputResult<()> {
            unreachable!("fast text does not use layout keys")
        }

        fn raw(&mut self, keycode: u16, direction: Direction) -> enigo::InputResult<()> {
            assert_eq!(keycode, 0);
            assert!(matches!(direction, Direction::Release));
            self.held = false;
            self.events.push("release");
            Ok(())
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_text_releases_the_unicode_event_key() {
        let mut keyboard = TextKeyboard::default();
        type_text(&mut keyboard, "ăâîșț").unwrap();
        assert!(!keyboard.held, "Unicode injection left virtual key 0 held");
        assert_eq!(keyboard.events, ["text", "release"]);
        keyboard.events.clear();
        type_text(&mut keyboard, "").unwrap();
        assert!(keyboard.events.is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_partial_text_failure_still_releases_the_event_key() {
        let mut keyboard = TextKeyboard {
            fail: true,
            ..Default::default()
        };
        let error = type_text(&mut keyboard, "partial").unwrap_err();
        assert!(error.to_string().contains("partial text failure"));
        assert!(!keyboard.held, "Failed text injection skipped cleanup");
        assert_eq!(keyboard.events, ["text", "release"]);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn navigation_cleanup_releases_implicit_fn_even_after_partial_key_up() {
        #[derive(Default)]
        struct NavigationKeyboard {
            held_fn: bool,
            fail_up: bool,
            fail_cleanup: bool,
            released: usize,
        }
        impl Keyboard for NavigationKeyboard {
            fn fast_text(&mut self, _: &str) -> enigo::InputResult<Option<()>> {
                unreachable!()
            }
            fn key(&mut self, _: enigo::Key, direction: Direction) -> enigo::InputResult<()> {
                self.held_fn = true;
                if self.fail_up && matches!(direction, Direction::Release) {
                    Err(enigo::InputError::Simulate("partial key-up"))
                } else {
                    Ok(())
                }
            }
            fn raw(&mut self, code: u16, direction: Direction) -> enigo::InputResult<()> {
                assert_eq!(code, 63);
                assert!(matches!(direction, Direction::Release));
                if self.fail_cleanup {
                    return Err(enigo::InputError::Simulate("cleanup failure"));
                }
                self.held_fn = false;
                self.released += 1;
                Ok(())
            }
        }
        for fail_up in [false, true] {
            let mut keyboard = NavigationKeyboard {
                fail_up,
                ..Default::default()
            };
            let mut owned = false;
            send_macos_named_key(&mut keyboard, &mut owned, Key::Left, true).unwrap();
            assert!(owned && keyboard.held_fn);
            let result = send_macos_named_key(&mut keyboard, &mut owned, Key::Left, false);
            assert_eq!(result.is_err(), fail_up);
            assert!(!keyboard.held_fn && !owned);
            assert_eq!(keyboard.released, 1);
        }
        let mut keyboard = NavigationKeyboard::default();
        let mut owned = false;
        send_macos_named_key(&mut keyboard, &mut owned, Key::Left, true).unwrap();
        send_macos_named_key(&mut keyboard, &mut owned, Key::Right, true).unwrap();
        send_macos_named_key(&mut keyboard, &mut owned, Key::Left, false).unwrap();
        send_macos_named_key(&mut keyboard, &mut owned, Key::Right, false).unwrap();
        assert!(
            !keyboard.held_fn && !owned,
            "Overlapping key releases left Fn held"
        );
        // Repeated key-up can also reassert Fn inside the native backend.
        send_macos_named_key(&mut keyboard, &mut owned, Key::Right, false).unwrap();
        assert!(!keyboard.held_fn && !owned, "Repeated key-up left Fn held");
        let mut keyboard = NavigationKeyboard {
            fail_cleanup: true,
            ..Default::default()
        };
        let mut owned = false;
        send_macos_named_key(&mut keyboard, &mut owned, Key::Left, true).unwrap();
        assert!(send_macos_named_key(&mut keyboard, &mut owned, Key::Left, false).is_err());
        assert!(
            keyboard.held_fn && owned,
            "Failed cleanup must retain ownership for teardown"
        );
        keyboard.fail_cleanup = false;
        release_macos_fn(&mut keyboard, &mut owned).unwrap();
        assert!(!keyboard.held_fn && !owned);
    }
    #[test]
    fn capture_denial_is_explicit_instead_of_sharing_only_own_windows() {
        let error = require_capture_permission(false).unwrap_err().to_string();
        assert!(error.contains("Screen Recording"));
        assert!(require_capture_permission(true).is_ok());
    }
    #[test]
    fn decodes_retina_width_and_rejects_both_axes_over_the_limit() {
        for (width, height) in [(3600, 32), (32, 4096), (4097, 1), (1, 4097)] {
            let mut jpeg = Vec::new();
            JpegEncoder::new(&mut jpeg)
                .encode_image(&image::RgbImage::new(width, height))
                .unwrap();
            let frame = decode_frame(width, height, jpeg);
            if width <= 4096 && height <= 4096 {
                assert_eq!(frame.unwrap().rgb.len(), (width * height * 3) as usize);
            } else {
                assert!(frame.is_err());
            }
        }
    }
    #[test]
    fn noisy_frames_fit_byte_budget_and_advertise_the_actual_dimensions() {
        use rand::{RngCore, SeedableRng};
        let mut rgb = image::RgbImage::new(256, 192);
        rand::rngs::StdRng::seed_from_u64(42).fill_bytes(rgb.as_mut());
        for quality in [40, 75, 85, 90] {
            let Message::Frame { jpeg, .. } =
                jpeg::encode_rgb(&rgb, quality, jpeg::BYTE_LIMIT).unwrap()
            else {
                panic!("expected a frame");
            };
            let mut expected = Vec::new();
            JpegEncoder::new_with_quality(&mut expected, quality)
                .encode_image(&rgb)
                .unwrap();
            assert_eq!(
                jpeg, expected,
                "unconstrained frame must preserve quality {quality}"
            );
        }
        for budget in [80_000, 45_000, 5000] {
            let Message::Frame {
                width,
                height,
                jpeg,
            } = jpeg::encode_rgb(&rgb, 90, budget).unwrap()
            else {
                panic!("expected a frame");
            };
            assert!(jpeg.len() <= budget);
            if budget == 5000 {
                assert!(width < 256 && height < 192);
            } else {
                assert_eq!((width, height), (256, 192));
            }
            assert_eq!(width * 192, height * 256);
            decode_frame(width, height, jpeg).unwrap();
        }
    }
    #[test]
    #[ignore = "full-size synthetic decoder memory test"]
    fn maximum_baseline_frame_decodes_at_original_size() {
        let mut jpeg = Vec::new();
        JpegEncoder::new(&mut jpeg)
            .encode_image(&image::RgbImage::new(MAX_FRAME_EDGE, MAX_FRAME_EDGE))
            .unwrap();
        let decoded = decode_frame(MAX_FRAME_EDGE, MAX_FRAME_EDGE, jpeg).unwrap();
        assert_eq!(decoded.rgb.len() as u64, MAX_FRAME_PIXELS * 3);
    }
    #[test]
    #[ignore = "full-size synthetic entropy stress test"]
    fn maximum_original_noise_frame_fits_wire_budget_and_round_trips() {
        use rand::{RngCore, SeedableRng};
        let mut rgb = image::RgbImage::new(MAX_FRAME_EDGE, MAX_FRAME_EDGE);
        rand::rngs::StdRng::seed_from_u64(2026).fill_bytes(rgb.as_mut());
        let frame = jpeg::encode_rgb(&rgb, 90, jpeg::BYTE_LIMIT).unwrap();
        frame.validate().unwrap();
        let Message::Frame {
            width,
            height,
            jpeg,
        } = frame
        else {
            panic!("expected frame")
        };
        eprintln!(
            "4096x4096 entropy fixture encoded as {width}x{height} in {} bytes",
            jpeg.len()
        );
        decode_frame(width, height, jpeg).unwrap();
    }
    #[test]
    fn jpeg_dimensions_must_match_protocol_header() {
        let mut jpeg = Vec::new();
        JpegEncoder::new(&mut jpeg)
            .encode_image(&image::RgbImage::new(16, 10))
            .unwrap();
        assert!(decode_frame(32, 10, jpeg.clone()).is_err());
        let frame = decode_frame(16, 10, jpeg).unwrap();
        assert_eq!(frame.rgb.len(), 16 * 10 * 3);
    }
    #[test]
    fn screen_decoder_requires_single_scan_baseline_before_pixel_decode() {
        let mut jpeg = Vec::new();
        JpegEncoder::new(&mut jpeg)
            .encode_image(&image::RgbImage::new(8, 8))
            .unwrap();
        let frame_header = jpeg.windows(2).position(|s| s == [0xff, 0xc0]).unwrap();
        let scan = jpeg.windows(2).position(|s| s == [0xff, 0xda]).unwrap();
        for (index, val) in [(frame_header + 1, 0xc2), (scan + 4, 1)] {
            let mut variant = jpeg.clone();
            variant[index] = val;
            let error = decode_frame(8, 8, variant)
                .err()
                .expect("unsupported encoding");
            assert!(error.to_string().contains("baseline"), "{error}");
        }
        decode_frame(8, 8, jpeg).unwrap();
    }

    #[test]
    fn linux_native_source_limit_permits_5k_and_bounds_full_regions() {
        assert!(validate_x11_source(5120, 2880).is_ok());
        assert!(validate_x11_source(4096, 4096).is_ok());
        assert!(validate_x11_source(8192, 4320).is_err());
        assert!(validate_x11_source(u32::MAX, u32::MAX).is_err());
        assert!(validate_x11_source(0, 1024).is_err());
    }

    #[test]
    fn rejects_invalid_images_and_large_dimensions() {
        assert!(decode_frame(4097, 1, vec![1]).is_err());
        assert!(decode_frame(0, 1, vec![1]).is_err());
        assert!(decode_frame(1, 1, vec![0; 16]).is_err());
    }
}
