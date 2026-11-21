# DeskLink 0.3.4 — linked-screen app icon

The icon extends the existing website mark: two overlapping screens, in mint and
cream, on a deep green tile. It uses no lettering, arrows or fine details. Small
sizes use a heavier outline. SVG/PNG/ICO/ICNS share one geometry generator; see
assets/icons/README.md. The artwork is original MIT-licensed project content.

Integration covers the native window icon on Windows/Linux, Finder/Dock app
bundle icon on macOS, Windows executable resources, and Windows installer and
uninstaller icons. Both GNU and Windows SDK resource paths include the icon.
The Mac bundle keeps dev.desklink.desktop and the same publisher requirement;
user identity storage and the remote-session protocol are unchanged.

This ships as 0.3.4 / Mac build 10 to avoid overwriting 0.3.3 downloads. Mac app
and DMG must be Developer ID signed, Apple accepted and stapled. Windows setup,
app and uninstaller remain unsigned previews. Linux's existing download remains
0.3.1; future Linux source builds use the runtime window icon.

Release gates: decode all image sizes/alpha and ICO/ICNS containers; format and
native Rust checks; build current Mac ARM64 and Windows x64/ARM64 targets; verify
native packaged resources/icons and version metadata; verify signatures/tickets
for the downloaded Mac image; install/remove the Windows candidates and preserve
existing ID/settings; verify public bytes before changing website links. No
screen sharing or remote input session is needed for this artwork change.
