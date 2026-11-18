//! Quartz pointer events use logical desktop coordinates throughout. In
//! particular, do not invert NSEvent's point coordinate using pixel height.
use crate::protocol::{Key, MouseButton};
use anyhow::{Context, Result};
use core_graphics::{
    event::{CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, CGMouseButton, EventField},
    event_source::{CGEventSource, CGEventSourceStateID},
    geometry::CGPoint,
};
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

pub(super) struct Pointer {
    source: CGEventSource,
    position: CGPoint,
    last_click: Option<(MouseButton, Instant, i64)>,
}

impl Pointer {
    pub(super) fn new() -> Result<Self> {
        let source = CGEventSource::new(CGEventSourceStateID::Private)
            .ok()
            .context("Could not create macOS pointer source")?;
        let position = CGEvent::new(source.clone())
            .ok()
            .context("Could not read macOS pointer position")?
            .location();
        Ok(Self {
            source,
            position,
            last_click: None,
        })
    }

    pub(super) fn move_to(
        &mut self,
        x: i32,
        y: i32,
        buttons: &HashSet<MouseButton>,
        keys: &HashSet<Key>,
    ) -> Result<()> {
        self.position = CGPoint::new(x as f64, y as f64);
        let (kind, button) = motion_kind(buttons);
        self.post(kind, button, 0, keys)
    }

    pub(super) fn button(
        &mut self,
        button: MouseButton,
        down: bool,
        keys: &HashSet<Key>,
    ) -> Result<()> {
        let now = Instant::now();
        let count = if down {
            let count = match self.last_click {
                Some((previous, time, count))
                    if previous == button
                        && now.duration_since(time) < Duration::from_millis(500) =>
                {
                    (count + 1).min(3)
                }
                _ => 1,
            };
            self.last_click = Some((button, now, count));
            count
        } else {
            self.last_click.map(|(_, _, count)| count).unwrap_or(1)
        };
        let (kind, button) = match (button, down) {
            (MouseButton::Left, true) => (CGEventType::LeftMouseDown, CGMouseButton::Left),
            (MouseButton::Left, false) => (CGEventType::LeftMouseUp, CGMouseButton::Left),
            (MouseButton::Right, true) => (CGEventType::RightMouseDown, CGMouseButton::Right),
            (MouseButton::Right, false) => (CGEventType::RightMouseUp, CGMouseButton::Right),
            (MouseButton::Middle, true) => (CGEventType::OtherMouseDown, CGMouseButton::Center),
            (MouseButton::Middle, false) => (CGEventType::OtherMouseUp, CGMouseButton::Center),
        };
        self.post(kind, button, count, keys)
    }

    fn post(
        &self,
        kind: CGEventType,
        button: CGMouseButton,
        count: i64,
        keys: &HashSet<Key>,
    ) -> Result<()> {
        let event = CGEvent::new_mouse_event(self.source.clone(), kind, self.position, button)
            .ok()
            .context("Could not create macOS pointer event")?;
        let mut flags = CGEventFlags::empty();
        for (key, flag) in [
            (Key::Shift, CGEventFlags::CGEventFlagShift),
            (Key::Control, CGEventFlags::CGEventFlagControl),
            (Key::Alt, CGEventFlags::CGEventFlagAlternate),
            (Key::Meta, CGEventFlags::CGEventFlagCommand),
        ] {
            if keys.contains(&key) {
                flags.insert(flag);
            }
        }
        event.set_flags(flags);
        event.set_integer_value_field(EventField::MOUSE_EVENT_CLICK_STATE, count);
        event.set_integer_value_field(
            EventField::EVENT_SOURCE_USER_DATA,
            enigo::EVENT_MARKER as i64,
        );
        event.post(CGEventTapLocation::HID);
        Ok(())
    }
}

fn motion_kind(buttons: &HashSet<MouseButton>) -> (CGEventType, CGMouseButton) {
    if buttons.contains(&MouseButton::Left) {
        (CGEventType::LeftMouseDragged, CGMouseButton::Left)
    } else if buttons.contains(&MouseButton::Right) {
        (CGEventType::RightMouseDragged, CGMouseButton::Right)
    } else if buttons.contains(&MouseButton::Middle) {
        (CGEventType::OtherMouseDragged, CGMouseButton::Center)
    } else {
        (CGEventType::MouseMoved, CGMouseButton::Left)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drag_events_follow_remote_button_state_instead_of_physical_mouse_state() {
        assert_eq!(
            motion_kind(&HashSet::new()).0 as u32,
            CGEventType::MouseMoved as u32
        );
        for (button, expected) in [
            (MouseButton::Left, CGEventType::LeftMouseDragged),
            (MouseButton::Right, CGEventType::RightMouseDragged),
            (MouseButton::Middle, CGEventType::OtherMouseDragged),
        ] {
            assert_eq!(
                motion_kind(&HashSet::from([button])).0 as u32,
                expected as u32
            );
        }
    }
}
