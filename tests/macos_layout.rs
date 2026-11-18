//! This needs the actual process main thread, rather than Rust's test workers.
//! It resolves layout keycodes without injecting any keyboard/mouse events.
#[cfg(target_os = "macos")]
fn main() {
    use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoop};
    use std::{
        sync::mpsc,
        thread,
        time::{Duration, Instant},
    };
    let main_thread =
        objc2::MainThreadMarker::new().expect("test must start on the process main thread");
    let _app = objc2_app_kit::NSApplication::sharedApplication(main_thread);
    let (tx, rx) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        let codes: Vec<_> = ['a', 'c', 'x', 'z']
            .into_iter()
            .map(|character| desklink::desktop::resolve_character_keycode(character).unwrap())
            .collect();
        tx.send(codes).unwrap();
    });
    let deadline = Instant::now() + Duration::from_secs(8);
    let codes = loop {
        if let Ok(codes) = rx.try_recv() {
            break codes;
        }
        assert!(
            Instant::now() < deadline,
            "Keyboard layout resolution blocked the worker"
        );
        // SAFETY: Apple's constant is immutable and valid for the process lifetime.
        CFRunLoop::run_in_mode(
            unsafe { kCFRunLoopDefaultMode },
            Duration::from_millis(10),
            true,
        );
    };
    worker.join().unwrap();
    assert_eq!(codes.len(), 4);
    assert!(codes.iter().all(|code| *code < 128));
    assert!(codes.windows(2).all(|pair| pair[0] != pair[1]));
    println!("macOS worker-to-main keyboard layout regression: passed");
}

#[cfg(not(target_os = "macos"))]
fn main() {
    println!("macOS keyboard-layout regression skipped on this platform");
}
