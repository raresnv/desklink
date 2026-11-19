# Sharper desktop streaming — approved approach, 2026-11-19

The owner selected a sharper default (up to 2560 pixels on the longest edge), an
optional original-resolution mode, and 100% zoom. Current 0.3.2 is limited to
1600 pixels and JPEG quality 75, and its receiver rejects larger frames.

## Behavior

Updated pairs default to Sharp: up to 2560 pixels, higher JPEG quality with
bounded congestion adaptation. Balanced preserves the existing 1600/75 limits.
Original uses the host's actual Retina pixels without upscaling, bounded at 4096
pixels on either axis for memory/message safety. The viewer offers Fit and 100%
zoom; 100% means one transmitted pixel per local display pixel, with scrolling.
Show the actual transmitted dimensions. Higher quality consumes more bandwidth;
Original may lower frame rate. Never promise lossless or unlimited native pixels.

## Compatibility and consent

Exchange a small versioned capability marker in the existing authenticated Noise
handshake payload. Old peers currently ignore payloads, and emit empty payloads.
Empty/unknown markers mean Legacy: retain old frame dimensions/quality and never
send the new video-settings message. New peers advertise the fixed v1 video
limits and support an appended VideoSettings enum variant for live profile
selection. Retain every existing message discriminant and handshake key/context.
Quality messages are allowed in view-only sessions; remote input still requires
host-approved control. No clipboard, identity, relay routing or consent changes.

## Capture and resource limits

Use actual macOS display-mode pixel dimensions (compatible with macOS 12.3), keep
input coordinates in logical points, and preserve normalized pointer mapping.
Keep OS scaling before CPU copies. Windows retains its native DXGI/WIC path;
Linux retains its X11 fallback, both with matching bounded profile dimensions.
Frame decoder: max axis 4096, max pixels 4096 squared, max JPEG message unchanged
at 8 MiB, explicit decoder allocation limit. All capture and encode stages enforce
these limits. High-entropy frames must lower JPEG quality within a bounded retry
loop rather than disconnect solely because a frame exceeds the message budget.
Queues remain bounded/latest-frame and input releases remain fail-closed.

## Release and verification

TDD: profile scaling/limits, malformed images, authenticated capability exchange,
legacy/no-payload fallback in both directions, live settings in view-only mode,
quality bounds and pointer mapping under zoom. Verify Mac/Windows compilation and
existing consent/input tests. Measure real Sharp/Original frame dimensions and
throughput before asserting performance. Build Mac 0.3.3 with the existing company
Developer ID and Keychain notary profile, verify ticket/trust and exact HTTPS
package before changing the public Mac link. Older Windows/Linux downloads retain
their actual versions/signing labels. Test the real updated pair when available;
record any physical peer checks requiring owner action.
