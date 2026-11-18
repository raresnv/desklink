//! Bounded, view-only frame-rate measurement against a locally approved host.
//! Read the private invitation from stdin; never print or save screen contents.
use anyhow::{bail, ensure, Result};
use desklink::{
    invitation::{Destination, Invitation},
    session::{Viewer, ViewerEvent},
};
use std::{
    io, thread,
    time::{Duration, Instant},
};

fn main() -> Result<()> {
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    let invitation = Invitation::parse(line.trim())?;
    let vm_gateway = std::env::args().any(|arg| arg == "--vm-gateway");
    let local = match &invitation.destination {
        Destination::Direct(address) => {
            address.ip().is_loopback()
                || (vm_gateway && address.ip() == std::net::Ipv4Addr::new(10, 0, 2, 2))
        }
        Destination::Relay { endpoint, .. } => endpoint.as_str().starts_with("ws://"),
    };
    ensure!(
        local,
        "Use a loopback address, local VM port forward, or loopback relay"
    );
    let viewer = Viewer::connect(invitation, "Frame rate verification".into(), false)?;
    let deadline = Instant::now() + Duration::from_secs(70);
    let mut connected = None;
    let mut first = None;
    let mut frames = 0u64;
    let mut bytes = 0u64;
    let mut size = (0, 0);
    let mut changed = 0u64;
    let mut previous = None;
    let mut window_start = None;
    let mut window_frames = 0u64;
    let mut window_changed = 0u64;
    loop {
        for event in viewer.events.try_iter() {
            match event {
                ViewerEvent::Connected { .. } => {
                    connected = Some(Instant::now());
                    println!("Approved; warming up for 3 seconds, measuring for 30 seconds");
                }
                ViewerEvent::Ended(end) => bail!("Session ended: {}", end.reason),
                _ => {}
            }
        }
        if connected.is_none() && Instant::now() > deadline {
            bail!("Approval timed out");
        }
        let frame = viewer.latest.lock().ok().and_then(|mut slot| slot.take());
        if let Some(frame) = frame {
            if connected.is_some_and(|start| start.elapsed() >= Duration::from_secs(3)) {
                first.get_or_insert_with(Instant::now);
                window_start.get_or_insert_with(Instant::now);
                frames += 1;
                window_frames += 1;
                let hash = frame.rgb.iter().step_by(97).fold(0u64, |hash, value| {
                    hash.wrapping_mul(31).wrapping_add(u64::from(*value))
                });
                if previous != Some(hash) {
                    changed += 1;
                    window_changed += 1;
                    previous = Some(hash);
                }
                bytes += frame.bytes as u64;
                size = (frame.width, frame.height);
            }
        }
        if let Some(start) = first {
            let elapsed = start.elapsed();
            if window_start.is_some_and(|start: Instant| start.elapsed() >= Duration::from_secs(10))
            {
                let interval = window_start.unwrap().elapsed().as_secs_f64();
                println!(
                    "window fps={:.2}, changed_fps={:.2}",
                    window_frames as f64 / interval,
                    window_changed as f64 / interval
                );
                window_start = Some(Instant::now());
                window_frames = 0;
                window_changed = 0;
            }
            if elapsed >= Duration::from_secs(30) {
                let seconds = elapsed.as_secs_f64();
                println!("{{\"frames\":{frames},\"changed\":{changed},\"seconds\":{seconds:.3},\"fps\":{:.2},\"changed_fps\":{:.2},\"mbps\":{:.2},\"width\":{},\"height\":{}}}", frames as f64 / seconds, changed as f64 / seconds, bytes as f64 * 8.0 / seconds / 1_000_000.0, size.0, size.1);
                viewer.disconnect();
                return Ok(());
            }
        }
        if connected.is_some_and(|start| start.elapsed() > Duration::from_secs(45)) {
            bail!("No usable stream");
        }
        thread::sleep(Duration::from_millis(1));
    }
}
