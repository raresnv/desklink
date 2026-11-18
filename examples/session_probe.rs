//! Interactive loopback-only verifier for a real DeskLink GUI host.
//! All capture and injection happen through the production encrypted session.
use anyhow::{ensure, Context, Result};
use desklink::{
    invitation::{Destination, Invitation},
    protocol::{InputEvent, Key, MouseButton},
    session::{Viewer, ViewerEvent},
};
use std::{
    io::{self, BufRead},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

fn main() -> Result<()> {
    println!("Paste a loopback host invitation (the host must approve locally).");
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    let invite = Invitation::parse(&line)?;
    let local = match &invite.destination {
        Destination::Direct(address) => address.ip().is_loopback(),
        Destination::Relay { endpoint, .. } => endpoint.as_str().starts_with("ws://"),
    };
    ensure!(
        local,
        "This verifier only connects through loopback TCP or a loopback relay"
    );
    let viewer = Viewer::connect(invite, "Local macOS verification".into(), true)?;
    let (tx, rx) = mpsc::sync_channel(32);
    thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            let Ok(line) = line else {
                break;
            };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let mut count = 0;
    let mut changed = 0;
    let mut previous = 0u64;
    let mut last = None;
    let start = Instant::now();
    let mut report = Instant::now();
    loop {
        for event in viewer.events.try_iter() {
            match event {
                ViewerEvent::Waiting => println!("Waiting for local approval"),
                ViewerEvent::Connected { desktop, control } => {
                    println!("Connected: {desktop:?}; control={control}")
                }
                ViewerEvent::Ended(end) => {
                    println!("Ended: {}; failed={}", end.reason, end.failed);
                    return Ok(());
                }
                ViewerEvent::Latency(_) => {}
            }
        }
        if let Some(frame) = viewer.latest.lock().ok().and_then(|mut slot| slot.take()) {
            count += 1;
            let fingerprint = frame.rgb.iter().step_by(97).fold(0u64, |hash, byte| {
                hash.wrapping_mul(31).wrapping_add(u64::from(*byte))
            });
            if fingerprint != previous {
                changed += 1;
                previous = fingerprint;
            }
            last = Some(frame);
        }
        while let Ok(command) = rx.try_recv() {
            let (verb, rest) = command.split_once(' ').unwrap_or((&command, ""));
            match verb {
                "wait" => {
                    let milliseconds: u64 = rest.parse()?;
                    ensure!(milliseconds <= 30_000, "Wait is limited to 30 seconds");
                    thread::sleep(Duration::from_millis(milliseconds));
                    if let Some(frame) = viewer.latest.lock().ok().and_then(|mut slot| slot.take()) {
                        last = Some(frame);
                    }
                }
                "move" => {
                    let mut values = rest.split_whitespace();
                    viewer.input(InputEvent::Move { x: values.next().context("x required")?.parse()?, y: values.next().context("y required")?.parse()? });
                }
                "click" | "right" | "down" | "up" => {
                    let button = if verb == "right" { MouseButton::Right } else { MouseButton::Left };
                    if verb != "up" { viewer.input(InputEvent::Button { button, down: true }); }
                    if verb == "click" || verb == "right" { thread::sleep(Duration::from_millis(40)); }
                    if verb != "down" { viewer.input(InputEvent::Button { button, down: false }); }
                }
                "text" => viewer.input(InputEvent::Text(rest.into())),
                "key" => {
                    let mut values = rest.split_whitespace();
                    let name = values.next().context("key required")?;
                    let key = match name {
                        "meta" => Key::Meta, "ctrl" => Key::Control, "alt" => Key::Alt, "shift" => Key::Shift,
                        "enter" => Key::Enter, "tab" => Key::Tab, "escape" => Key::Escape,
                        "backspace" => Key::Backspace, "left" => Key::Left, "right" => Key::Right,
                        "up" => Key::Up, "down" => Key::Down,
                        name if name.chars().count() == 1 => Key::Character(name.chars().next().unwrap()),
                        _ => anyhow::bail!("Unknown key"),
                    };
                    match values.next() {
                        Some("down") => viewer.input(InputEvent::Key { key, down: true }),
                        Some("up") => viewer.input(InputEvent::Key { key, down: false }),
                        _ => { viewer.input(InputEvent::Key { key, down: true }); viewer.input(InputEvent::Key { key, down: false }); },
                    }
                }
                "scroll" => viewer.input(InputEvent::Scroll { horizontal: 0, vertical: rest.parse()? }),
                "release" => viewer.input(InputEvent::ReleaseAll),
                "save" => {
                    let frame = last.as_ref().context("No frame yet")?;
                    let mut output = std::fs::File::create(rest)?;
                    let rgb = image::RgbImage::from_raw(frame.width, frame.height, frame.rgb.clone()).context("Invalid frame")?;
                    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, 90).encode_image(&rgb)?;
                    println!("Saved real host frame to {rest}");
                }
                "quit" => { viewer.disconnect(); return Ok(()); },
                "status" => println!("frames={count}; changed={changed}; seconds={:.1}", start.elapsed().as_secs_f32()),
                "" => {},
                _ => anyhow::bail!("Commands: wait MS, move x y, click, right, down, up, text VALUE, key NAME [down|up], scroll N, release, save PATH, status, quit"),
            }
        }
        if report.elapsed() >= Duration::from_secs(10) {
            println!("frames={count}; changed={changed}");
            report = Instant::now();
        }
        thread::sleep(Duration::from_millis(10));
    }
}
