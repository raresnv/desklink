//! Loopback-only viewer test fixture. Shares generated pixels and records
//! event counts. Never captures a display or injects operating-system input.
use anyhow::Result;
use desklink::{
    invitation::Invitation,
    protocol::{DesktopInfo, Message},
    transport::SecureConnection,
};
use image::{codecs::jpeg::JpegEncoder, Rgb, RgbImage};
use std::{
    net::TcpListener,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

fn main() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let invitation = Invitation::new(listener.local_addr()?);
    println!("Synthetic desktop fixture. No real screen capture or input injection.");
    println!("{}", invitation.encode());
    let (stream, _) = listener.accept()?;
    let (mut reader, mut writer) =
        SecureConnection::accept(stream, invitation.secret())?.split()?;
    let Message::Request { control, .. } = reader.receive()? else {
        anyhow::bail!("Expected request")
    };
    writer.send(&Message::Approved {
        desktop: DesktopInfo {
            name: "Synthetic test desktop".into(),
            width: 1280,
            height: 800,
            x: 0,
            y: 0,
            scale: 1.0,
        },
        control,
    })?;
    println!("Synthetic viewer connected");
    let (tx, rx) = mpsc::sync_channel(128);
    thread::spawn(move || {
        let mut inputs = 0u64;
        while let Ok(message) = reader.receive() {
            match message {
                Message::Ping(n) => {
                    if tx.try_send(Message::Pong(n)).is_err() {
                        break;
                    }
                }
                Message::Input(_) => {
                    inputs += 1;
                    println!("Input events received: {inputs}");
                }
                Message::Disconnect => break,
                _ => {}
            }
        }
        println!("Synthetic viewer disconnected; received {inputs} input events");
    });
    let start = Instant::now();
    loop {
        while let Ok(message) = rx.try_recv() {
            writer.send(&message)?;
        }
        let mut image = RgbImage::from_pixel(1280, 800, Rgb([242, 245, 240]));
        fill(&mut image, 0, 0, 210, 800, [24, 42, 43]);
        fill(&mut image, 30, 36, 150, 28, [133, 217, 181]);
        for row in 0..5 {
            fill(&mut image, 30, 125 + row * 48, 130, 14, [94, 133, 120]);
        }
        fill(&mut image, 254, 50, 570, 30, [30, 55, 50]);
        fill(&mut image, 254, 110, 410, 12, [143, 158, 147]);
        fill(&mut image, 254, 180, 980, 370, [255, 255, 255]);
        let phase = (start.elapsed().as_millis() / 50) as u32;
        for bar in 0..12 {
            let height = 80 + ((bar * 31 + phase) % 190);
            fill(
                &mut image,
                292 + bar * 74,
                510 - height,
                45,
                height,
                [0, 108, 95],
            );
        }
        for card in 0..3 {
            fill(&mut image, 254 + card * 336, 590, 308, 150, [220, 234, 225]);
        }
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, 75).encode_image(&image)?;
        if writer
            .send(&Message::Frame {
                width: 1280,
                height: 800,
                jpeg,
            })
            .is_err()
        {
            break;
        }
        thread::sleep(Duration::from_millis(83));
    }
    Ok(())
}

fn fill(image: &mut RgbImage, x: u32, y: u32, width: u32, height: u32, color: [u8; 3]) {
    for py in y..(y + height).min(image.height()) {
        for px in x..(x + width).min(image.width()) {
            image.put_pixel(px, py, Rgb(color));
        }
    }
}
