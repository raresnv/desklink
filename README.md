# DeskLink

Native, consent-based remote desktop for macOS, Windows and Linux, written in
Rust and released under the MIT license. This is an independent project with
original branding and protocol; it is not affiliated with AnyDesk.

Each computer automatically connects to the public relay and receives a stable
12-digit ID. Enter the other computer's ID and request access. Its host explicitly
approves viewing or keyboard/mouse control for every session.

[Website and preview downloads](https://desklink.161.97.168.233.nip.io/)

## Release status

Mac and Windows downloads are now **0.3.4 previews** with the linked-screen
DeskLink icon. Mac app/DMG are Developer ID signed by Overtex Core Solutions SRL,
Apple notarized and stapled. Windows x64 and experimental ARM64 installers, apps
and uninstallers remain unsigned; portable ZIPs are available. Linux remains
0.3.1. The SignPath Foundation application was not approved and Windows publisher
signing is not configured.

The 0.3.4 Mac build passed 49 library tests and Apple trust checks. Mac Finder
and native Windows file-icon checks verified the custom artwork; all public
files were downloaded back and hash/version checked. The running Windows 0.3.3
installation was preserved. Full 0.3.4 screen sharing, input and fresh permission
onboarding were not retested. See the [icon release record](docs/APP-ICON-0.3.4.md).
Earlier records: [Mac DMG](docs/MAC-DMG-0.3.3.md),
[Windows installers](docs/WINDOWS-INSTALLER-0.3.3.md), and
[stream quality](docs/SHARP-VIDEO-0.3.3.md). These checks do not establish
production-scale capacity or every network/display configuration.

## Stream quality

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
the image and open DeskLink from Applications. On Windows, run the
setup EXE and open DeskLink from the Start menu. Portable ZIP downloads can
also be extracted and run with desklink.exe. On Linux,
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
