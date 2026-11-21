# DeskLink 0.3.3 — sharper stream preview

The approved default is Sharp, up to 2560 pixels on the longest edge. Balanced
retains the 1600-pixel stream. Original / Retina preserves native capture pixels
up to 4096 without upscaling. The viewer offers Fit and 100% physical-pixel zoom,
with scroll bars for panning and the actual received dimensions in its status.
Both apps need the update; old peers safely retain legacy quality.

## Source and verification

Public source: `96dedf2c20b76de7e6c38c1cfa566d3ac7314b7d` in raresnv/desklink.
Core, capture, session/UI and whole-branch independent reviews passed after fixes.
The source preserves authenticated handshakes, existing message tags, identity
storage, per-session consent and the 8 MiB wire limit. View-only sessions can
change quality while keyboard and mouse input still requires control approval.

Review caught and fixed decoder working-memory accounting, X11 source bounds,
scroll smoothing after focus release, approved monitor geometry on capture
reopens, and X11 DPI cropping. The X11 backend now uses the selected monitor's
native RandR rectangle for both capture and pointer coordinates and rejects
unsupported panning/transforms. Windows retains DXGI/WIC and bounded GDI fallback.
WIC's temporary encoded stream can exceed the wire budget, but its input pixels
are bounded and no oversized JPEG is queued or transmitted.

Final Mac all-targets: 102 tests passed, seven opt-in tests skipped, plus the
main-thread keyboard-layout check. Windows x64 all-targets compilation and hosted
source build passed: GitHub Actions run 44496359105. Linux ARM64 VM: 99 tests passed,
seven skipped. Parallel linking exceeded VM memory; a serial rerun passed. Its
native RandR probe captured and decoded all 90 frames at 1440 × 900. The Linux VM
was returned to its prior powered-off state. These capture and protocol checks injected no remote keyboard or mouse input.

Generated-pixel tests passed all three modes through encrypted loopback and the
public numeric-ID relay. The exact downloaded Mac viewer then passed live profile
switching at 1600 × 1039, 2560 × 1663 and 3600 × 2338, Fit, 100% physical-pixel
sizing and scroll panning through the public relay in view-only mode. The generated
host sends frames only on selection, so its UI's idle 0 fps is not a benchmark.

A signed local capture diagnostic measured real Retina dimensions on this Mac:
Sharp 2560 × 1663 at 32.75 local capture/decode frames per second; Original
3600 × 2338 at 18.99. Those measurements predate the final geometry guard and
exclude internet transport. They are not final downloaded-host performance claims.
The final exact downloaded host then passed an approved view-only session through the public relay to a client on this Mac: Balanced 1600 × 1039 at 46.80 fps; Sharp 2560 × 1663 at 34.19 fps; Original 3600 × 2338 at 18.60 fps. Each measurement window was five seconds after warmup, with changed-frame counts confirming fresh content. The client saved no screen images and sent no input; it disconnected and the host returned to Online. Full two-way input,
fresh permission onboarding and an updated second physical Mac remain unverified.

## Signed package

Mac source/app version 0.3.3, build 9, Apple Silicon, macOS 12.3+. Publisher:
Overtex Core Solutions SRL, team WVNWL3N9G8. Hardened runtime and secure timestamp.
Apple accepted notarization `e13248d6-49b9-48bf-9977-bb561309aa2b`; ticket stapled.

The signing attempt waited long enough for its pre-approval signing timestamp to
expire. A fresh timestamp after owner Keychain approval succeeded. No signing key
or password was exported. Screen Recording and Accessibility grants were retained.

The first package lost its executable permission while copying the binary. The
real public-download launch test caught that packaging error. It was not promoted
on the website. The corrected revision preserves mode 0755 and the same signed
code and Apple ticket. The initial uploaded files remain immutable for traceability.

Corrected archive: `DeskLink-0.3.3-macos-arm64-r1.zip` (6,736,445 bytes).
SHA256: `bcf9ef4685ed659db21f88f3e738d6c2433c89265f9d92fd12424ff37ea92039`.
Executable SHA256: `ecca4202abc9323bff8fce5bcfecde19e290b7b2364476bdc24805f850295ba8`.
URL: https://desklink.161.97.168.233.nip.io/downloads/0.3.3/DeskLink-0.3.3-macos-arm64-r1.zip

The corrected archive was downloaded over HTTPS and matched exactly. ZIP and
extracted executable permissions, strict code signature, stapled ticket and
Gatekeeper acceptance were checked. Its downloaded-file quarantine was retained.
The installed exact public copy opened normally under AppTranslocation, showed
version 0.3.3, retained the existing computer ID and became Online. Only one
production app instance remains running. The prior working app is saved for rollback.

The website now links the corrected 0.3.3 package; its live HTML, script, manifest and checksums were byte-verified. Rollback: `/opt/desklink/site-before-sharp-mac-20261119T100601Z`. At that Mac publication, Windows/Linux remained 0.3.1 previews. Windows was
subsequently updated to an unsigned 0.3.3 preview; the Foundation application was
not approved. See [the Windows release record](WINDOWS-RELEASE-0.3.3.md).
All releases remain previews, without a general-availability or audit claim.

Private evidence: `.tools/sharp-video/` reports and logs;
`dist/production/0.3.3/macos-release-r1.json` and
`publication-verification-r1.json`. No actual screen images are saved by the
real-screen test client; viewer screenshots contain only generated fixture pixels.

## DMG distribution update — 21 November 2026

The default public Mac link now selects a separately signed, notarized and
stapled DMG containing the unchanged app. The previous ZIP is retained at its
immutable URL. See [the DMG release record](MAC-DMG-0.3.3.md) for the new
container hash, Apple receipt, installation checks and rollback details.
