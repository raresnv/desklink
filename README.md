# DeskLink

Native, consent-based remote desktop for macOS, Windows and Linux, written in
Rust and released under the MIT license. This is an independent project with
original branding and protocol; it is not affiliated with AnyDesk.

Each computer automatically connects to the public relay and receives a stable
12-digit ID. Enter the other computer's ID and request access. Its host explicitly
approves viewing or keyboard/mouse control for every session.

[Website and preview downloads](https://desklink.161.97.168.233.nip.io/)

## Release status

Public downloads currently remain version 0.3.1 preview. The 0.3.2 source prepares
signed distribution: the Mac package is Developer ID signed/notarized locally,
while Windows signing through SignPath Foundation is being prepared. The project
has not yet been accepted by the Foundation. Existing Windows downloads are
unsigned; do not assume a local build or CI artifact is publisher signed.

Mac and Windows screen sharing and input were exercised in both directions
through the public relay using a Mac and Windows 11 ARM VM running Windows x64
under emulation. This does not establish physical Windows x64, fresh-install
permission onboarding, production-scale service capacity, or every network/DPI
configuration. Linux currently targets ARM64/X11; Wayland control is unsupported.

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

Extract a preview ZIP. On Mac, move DeskLink.app into Applications and open it
normally. On Windows, extract the full package and open desklink.exe. On Linux,
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
