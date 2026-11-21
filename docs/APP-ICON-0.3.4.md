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

## Verified release

The native Mac ARM64 build succeeded. The library suite passed 49 tests with
2 intentionally ignored synthetic stress tests. The first restricted run could
not bind its loopback test listener; the same suite passed with local connection
permission. No application code was changed to accommodate that test environment.
All PNG alpha/sizes, seven ICO frames and 1024 px ICNS decoding passed.

Both Windows app binaries and both NSIS setup EXEs contain the correct seven
icon frames (16/24/32/48/64/128/256); resource images match the master. Windows'
own associated-icon extraction produces an exact 32 px RGBA pixel match for
all four files. Their version is 0.3.4 and Authenticode status is NotSigned.
These were checked as files without launching/installing the new app. A running
user installation (0.3.3) and running VM were left in place. The earlier 0.3.3
installer's 42 install/reinstall/removal/retention checks retain that scope; this
release's installer logic changes only icon/version resources. Windows SDK/MSVC
resource compilation and a physical x64 PC were not exercised. GNU x64 and
LLVM-MinGW ARM64 release builds were verified and import only system DLLs.

Apple accepted the Mac app under submission
`ce46bfa5-ed77-4ee5-8b39-29ddd3941867` and the DMG under
`f60242c2-06a6-45a1-917e-aa1a27aca766`. Both are signed by the existing
Overtex Core Solutions SRL Developer ID (WVNWL3N9G8), stapled and Gatekeeper
accepted. The downloaded DMG retained its quarantine marker; its contained app
was copied to an isolated verification directory, then its executable mode,
version/build, icon hash, signature, ticket and Gatekeeper trust were verified.
Finder opened the downloaded image normally and visibly displayed the custom
DeskLink icon. Installed Mac copies were not replaced in this task.

No remote screen-sharing/control session was granted, no remote input was sent and no
new capture/control permissions were enabled. Full two-way input, fresh
permission onboarding and a second physical Mac check were not repeated on
0.3.4. Future Linux source builds get the runtime window icon; the published
Linux ARM64/X11 download remains 0.3.1.

## Immutable downloads

App/asset source commit: `d12f0b5dbf217f831a072c6d155d2c6ef524ab53`.
No third-party package versions changed; only DeskLink's own version changed in
Cargo.lock. The existing notices were retained and its inventory version updated.
Windows installers include the NSIS 3.13 notice.

- [DeskLink-0.3.4-macos-arm64.dmg](https://desklink.161.97.168.233.nip.io/downloads/0.3.4/DeskLink-0.3.4-macos-arm64.dmg): 7,435,957 bytes; SHA-256 `5daf95ecbe4c19d73b69d7d2762a303045aa61f264c4ae6f49f76116442c15b4`.
- [DeskLink-0.3.4-windows-x64-setup.exe](https://desklink.161.97.168.233.nip.io/downloads/0.3.4/DeskLink-0.3.4-windows-x64-setup.exe): 5,599,646 bytes; SHA-256 `6f048c2094adf7d141b3317ca0f7dd5f9e0dc126c4ad512548de4278b8e319bd`.
- [DeskLink-0.3.4-windows-arm64-setup.exe](https://desklink.161.97.168.233.nip.io/downloads/0.3.4/DeskLink-0.3.4-windows-arm64-setup.exe): 4,934,008 bytes; SHA-256 `6e1cb9b2df132e1ebdda9baeb70a98ac2f896034fd9aaac314ff7f1604541e00`.
- [DeskLink-0.3.4-windows-x64.zip](https://desklink.161.97.168.233.nip.io/downloads/0.3.4/DeskLink-0.3.4-windows-x64.zip): 7,197,678 bytes; SHA-256 `5e32c94df37ef5f3aef1557792f186f22182e8fc469d1d0332da74c1a59555dc`.
- [DeskLink-0.3.4-windows-arm64.zip](https://desklink.161.97.168.233.nip.io/downloads/0.3.4/DeskLink-0.3.4-windows-arm64.zip): 6,531,469 bytes; SHA-256 `20c393c050004b85d83004519fc4b4bd7da1ba1badd4df189afe9208a4453c40`.

All five files were downloaded back over verified HTTPS. Windows app payload
hashes, embedded version and unsigned status matched the candidates checked in
the VM. Mac icon SHA-256 is
`844df0edeaf62e8c0036c9d26296f2eb3317c7b3e1abb5f1a3c3676bbc9d38fc`.

The website now selects 0.3.4 Mac/Windows packages, retaining the earlier preview
limits and unsigned Windows labels. Previous immutable downloads remain on the
server; the relay was not changed or restarted. Website files were guarded by
expected hashes and backed up before replacement. The previous four website files are backed up at
`/opt/desklink/site-before-icon034-20261121T112723Z`; restoring those files
rolls back default links without modifying immutable assets.
