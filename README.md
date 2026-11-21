# DeskLink

Native, consent-based remote desktop for macOS, Windows and Linux, written in
Rust and released under the MIT license. This is an independent project with
original branding and protocol; it is not affiliated with AnyDesk.

Each computer automatically connects to the public relay and receives a stable
12-digit ID. Enter the other computer's ID and request access. Its host explicitly
approves viewing or keyboard/mouse control for every session.

[Website and preview downloads](https://desklink.161.97.168.233.nip.io/)

## Release status

Public Mac downloads are now version 0.3.3 preview, Developer ID signed by Overtex
Core Solutions SRL and notarized by Apple with a stapled ticket. The default Mac
download is now a signed, notarized DMG with an Applications shortcut; see the
[DMG release record](docs/MAC-DMG-0.3.3.md). Windows x64 and
experimental ARM64 downloads are also 0.3.3 previews; Linux remains 0.3.1. The
SignPath Foundation application was not approved; Windows publisher signing
is not configured. Existing Windows downloads are unsigned; do not
assume a local build or CI artifact is publisher signed.

The 0.3.1 Mac and Windows previews were exercised for screen sharing and input in
both directions through the public relay using a Mac and Windows 11 ARM VM running
Windows x64 under emulation. The exact downloaded signed Mac 0.3.3 package passes
signature, executable-permission, notarization-ticket and Gatekeeper checks; it opens under downloaded-
file protection and connects to the public relay. Full two-way input and fresh
permission-onboarding checks on this signed build remain pending. Its downloaded viewer passed live quality switching, Fit, 100% zoom and panning through the public relay with generated images. A short real-screen, view-only test of the downloaded Mac host through the public relay to a local client measured about 34 fps in Sharp (2560 × 1663) and 19 fps in Original (3600 × 2338). An updated second physical Mac remains unchecked. These checks do
not establish physical Windows x64, production-scale capacity or every network/
DPI configuration. Linux targets ARM64/X11; Wayland control is unsupported.

Windows 0.3.3 passed 44 native unit tests, ARM64 capture/decoding at all three
quality modes, exact public-download hash/version checks and process launch with
a public-relay TCP connection. These do not establish a full GUI or two-way input
retest. See the [Windows release record](docs/WINDOWS-RELEASE-0.3.3.md).

## Stream quality (source version 0.3.3)

Updated pairs start with Sharp, up to 2560 pixels on the longest edge. The viewer
can switch to Balanced (1600) or Original / Retina (up to 4096), and choose Fit
or 100% zoom with scroll bars. 100% maps transmitted pixels to physical display
pixels. Larger streams use more bandwidth and may lower frame rate; Original
is bounded and does not upscale the host display. Actual received dimensions
appear in the session status. Both apps must support these profiles; older peers
keep legacy quality. Input and per-session host approval are unchanged.

## Connection troubleshooting

Run one DeskLink copy per computer identity. If the app says the computer ID is
already connected, close other DeskLink copies, including older downloads or test
builds. Keep the intended copy open; it will reconnect automatically with the same
ID. A new launch does not need a new identity or weaker network/TLS settings.

## Build and run

A Rust toolchain with the checked-in Cargo.lock is required. Recent local builds
used Rust 1.100.0. Windows GNU builds also need a matching MinGW compiler and
windres; Windows MSVC requires the Windows SDK. Linux desktop builds need the
libraries below. Local builds are unsigned or development signed unless you
configure an authorized distribution signing identity.

```sh
cargo build --locked --release --bin desklink
cargo run --locked --release --bin desklink
cargo test --locked --no-default-features
```

On macOS, build an Apple Silicon application bundle with:

```sh
bash scripts/bundle-macos.sh
```

On Ubuntu 24.04, install desktop development libraries first:

```sh
sudo apt-get install build-essential pkg-config libclang-dev libxcb1-dev \
  libxrandr-dev libx11-dev libxtst-dev libxdo-dev libxkbcommon-dev \
  libxkbcommon-x11-dev libwayland-dev libgbm-dev libpipewire-0.3-dev libegl1-mesa-dev
```

The relay is also in this repository and builds without desktop dependencies:

```sh
cargo build --locked --release --no-default-features --bin desklink-relay
```

Configure a private relay using its documented environment options; generate
fresh server credentials outside source control. Never reuse test fixture access
codes for a public deployment.

## Permissions, installation and removal

On Mac, open the DMG, drag DeskLink.app onto its Applications shortcut, eject
the image and open DeskLink from Applications. On Windows, extract the full
ZIP package and open desklink.exe. On Linux,
use the documented X11 desktop and runtime libraries.

macOS Screen Recording is required to share a display, and Accessibility is
required to allow keyboard/mouse control. Use the app's permission prompts and
finish the OS steps yourself. Viewing another computer requires neither host
permission. Changing an app's signing identity may require macOS to reauthorize
it. Windows supports the ordinary signed-in desktop; secure/UAC/locked desktops
are outside the supported control scope. Public relay connections go outward on
TCP 9443 and require no incoming firewall rule or router forwarding.

The app installs no background service. To uninstall, quit DeskLink and delete
its app/extracted folder. Revoke its macOS permissions in System Settings if
wanted. Device identities and remembered peers are separate local data; removing
them resets the installation's ID/trust records. See [privacy](PRIVACY.md).

## Trust and privacy

The public relay is contacted automatically on launch, before any screen-sharing
approval. It handles public keys, numeric IDs, computer names, presence and
connection metadata. Session screen/input data is encrypted between the paired
apps. Short IDs trust the selected relay's key directory on first contact;
remembered peer key changes are blocked. This is not a claim of relay-independent
first-contact identity verification. Closing the app stops its relay connection.

See [Privacy policy](PRIVACY.md), [Code signing policy](CODE_SIGNING.md),
[Security reporting](SECURITY.md) and [Contributing](CONTRIBUTING.md).
