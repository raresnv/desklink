# Sharper desktop streaming implementation plan

> Execute in this chat, with implementation and independent review separated.

**Goal:** Deliver the approved Sharp default, Original profile and 100% zoom in a
signed/notarized Mac preview while preserving legacy app interoperability.

**Architecture:** Shared bounded VideoProfile model; authenticated handshake
capability marker; negotiated live settings; OS capture scaling; viewer-only zoom.
**Tech stack:** Rust/serde/snow/egui/ScreenCaptureKit/CoreGraphics/DXGI/WIC.

## Constraints

Mac 12.3+, no new paid service or signing credential, existing Developer ID/team,
unchanged identity store and user consent, legacy 1600-pixel/JPEG-75 fallback,
4096-axis and 4096-squared pixel bounds, unchanged 8-MiB packet budget. Work in a
feature branch/isolated checkout; leave installed/running 0.3.2 intact until tested.

## Tasks

- [ ] Core negotiation: add `src/video.rs` with `VideoProfile::{Balanced,Sharp,Original}`,
  `max_edge`, `max_quality`, `min_quality`, `label`, checked aspect-preserving
  dimensions; export from lib. Append `Message::VideoSettings(VideoProfile)`.
  Retain old message discriminants. Update protocol limits. Add authenticated
  marker in Noise payloads; expose `SecureConnection::supports_video_settings()`.
  Write failing tests first, then implement and run core tests. Prove a legacy
  handshake fixture emits no marker and still interops in both directions.
- [ ] Capture: add `Capturer::open_with_max_edge(id, edge)`, retain old `open` at1600.
  Select Mac physical display-mode dimensions, maintain logical input metadata.
  Enforce bounded callback storage, JPEG retry budget and decoder allocations.
  Scale Windows via WIC and fallback Surface, and Linux via thumbnail. Test large
  frames, oversized-axis rejection, exact header match, profile resolution and
  high-entropy budgets. Cross-check Windows compilation.
- [ ] Session/UI: use negotiated Sharp vs legacy initial profile; carry selected
  profile in bounded FrameWindow with per-profile congestion quality bounds.
  Handle VideoSettings only for negotiated peers after host approval; allow
  settings forwarding in view-only sessions while rejecting input. Reopen capture
  only when edge changes. Add viewer profile selector, Fit/100% zoom with scroll,
  correct normalized input on clipped full-image rect, and actual stream dimensions.
  Preserve release-on-focus-loss/toolbar clicks, bounded queues and input priority.
- [ ] Independent review: review complete diff for protocol compatibility, consent,
  allocation bounds, Retina coordinate scaling, UI zoom/input mapping, versioning.
  Fix findings; run targeted regressions and full required checks once.
- [ ] Release: bump Mac/source to0.3.3/build9, build with same signing identity,
  notarize/staple/assess, package README/license/notices/checksums. Publish immutable
  Mac asset first, download back over HTTPS and verify exact bytes and signature.
  Test normal launch and public presence without leaving duplicate copies running.
  Promote website Mac link/manifest with per-platform versions and rollback.
  Owner's second Mac must update for high-resolution mode. Verify real pair or
  leave the precise peer action pending; do not claim unobserved performance.
