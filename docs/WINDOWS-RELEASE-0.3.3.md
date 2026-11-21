# Windows 0.3.3 preview release — 19 November 2026

The public Windows x64 and experimental ARM64 downloads now contain the existing
0.3.3 source's Sharp, Balanced and Original / Retina quality choices, Fit / 100%
zoom and panning. Both peers need 0.3.3 for the new profiles. Greater resolution
can reduce frame rate and increase bandwidth. These are unsigned previews.

## Build provenance

x64: GitHub Actions run [44503866752](https://github.com/raresnv/desklink/actions/runs/44503866752),
public commit `73cfac4c9c8e153800aa62d973ee302143798511`, downloaded artifact hash
verified against its unsigned-build.json. Native ARM64: Rust 1.100.0,
LLVM-MinGW 20261104, locked/offline dependency graph and the same application
source. The private workspace adds only a diagnostic example declaration in
Cargo.toml; all application source, Windows packaging resources and Cargo.lock
match the public commit. No new application behavior was added for this release.

## Public archives

- [DeskLink-0.3.3-windows-x64.zip](https://desklink.161.97.168.233.nip.io/downloads/0.3.3/DeskLink-0.3.3-windows-x64.zip): 7,103,514 bytes.
  Archive SHA-256: `43789092ab43082127a3303669b10d3acc371017f06ec215bf15c8cc5949292a`.
  Executable SHA-256: `a51d09f71a2fcbf7ca6928a391747d80f7dcd62d2af80958e2877d0dc1846e76`.
- [DeskLink-0.3.3-windows-arm64.zip](https://desklink.161.97.168.233.nip.io/downloads/0.3.3/DeskLink-0.3.3-windows-arm64.zip): 6,490,586 bytes.
  Archive SHA-256: `b1b4e3d0318f96c2ef436858fbd501211fd66c8685e2f6ef1eb440b32927c541`.
  Executable SHA-256: `e2457e271ec67cb1a4be3776c818dcb1d55457d545245bdc3fac0519bf3999e0`.

Each archive includes the executable, installation/quality notes, MIT license and
the reviewed dependency notices. No diagnostics, private identity or credentials
are included. PE architecture, embedded 0.3.3 metadata, absence of a signing
certificate and system DLL imports were checked. Existing Mac 0.3.3-r1 and Linux
0.3.1 package objects and immutable older downloads were preserved.

## Verification and limits

- Native Windows 11 ARM VM: 44 unit tests passed, 0 failed, 3 intentionally ignored.
  These include WIC JPEG decoding, quality validation, DPI/coordinate mapping,
  zoom/panning and synthetic egui input routing. They are not OS input tests.
- Native ARM64 DXGI capture/encode/decode succeeded with a 3600x2068 display:
  Balanced 1600x919, Sharp 2560x1471, Original 3600x2068. The static-desktop
  diagnostic produced fresh-frame delivery of 5.49, 1.90 and 3.15 fps respectively;
  processing-only rates were 21.95, 11.31 and 11.75 fps. This is not a moving-screen
  or end-to-end benchmark and establishes no 30 fps claim for these settings.
- Both exact public ZIPs were fetched over verified HTTPS inside Windows,
  extracted and their archive/executable SHA-256 values checked. Windows reports
  ProductVersion 0.3.3 and Authenticode NotSigned for both.
- Each downloaded executable started separately as a responsive process and
  opened an established TCP connection to the public relay on port 9443.
  Computer-use actions did not bring the app to the foreground reliably;
  UI rendering, Online status/ID and new quality controls were not visually
  verified in this release check. No real screen-sharing session was approved,
  no remote input was sent, and no screen images were saved.
- Full 0.3.3 Mac/Windows screen sharing and keyboard/mouse checks in both
  directions, physical Windows x64 and fresh onboarding remain outstanding.
  Earlier 0.3.1 two-way test results do not prove 0.3.3 acceptance.

## Publication and signing

The website and manifest now identify Windows 0.3.3, experimental ARM64,
unsigned status and actual verification scope. The SignPath Foundation
application was not approved; no certificate, active signing relationship or
endorsement exists. No paid subscription was started. Mac signing/notarization
is separate and remains in place.

The four website files were backed up under
`/opt/desklink/site-before-windows033-20261119T112637Z` and replaced with guarded
atomic file operations only after the archives existed and matched hashes.
The relay was not restarted. Rollback can restore those four files without
changing the immutable archives. Temporary capture/launch tasks and test app
processes were removed; the Windows VM was returned to its original suspended
state. The private transfer server was stopped.

## Installer distribution update — 21 November 2026

The default x64 and experimental ARM64 links now select per-user unsigned EXE
installers. The app executables are unchanged and these ZIPs remain portable
alternatives. See [the installer record](WINDOWS-INSTALLER-0.3.3.md) for hashes,
installation/removal checks, the shortcut fix and remaining verification limits.
