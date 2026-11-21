# Mac 0.3.3 signed DMG — 21 November 2026

The public Mac download now uses a signed, Apple-notarized, read-only DMG with
DeskLink.app and an Applications shortcut. It includes installation instructions,
MIT license and third-party notices. It supports Apple Silicon / macOS 12.3+.
It is still a preview. The app itself is the same 0.3.3/build 9 signed code as the
previous ZIP; it was not rebuilt or re-signed. Existing IDs and permission
identity are preserved. This packaging change adds no remote-access privileges.

## Artifact and Apple acceptance

- [DeskLink-0.3.3-macos-arm64.dmg](https://desklink.161.97.168.233.nip.io/downloads/0.3.3/DeskLink-0.3.3-macos-arm64.dmg)
- Size: 7,419,712 bytes.
- DMG SHA-256: `1c8dc82ce99e696120c11aea1554f586d5b66eef37a57e0fac636d671418dad9`.
- App executable SHA-256: `ecca4202abc9323bff8fce5bcfecde19e290b7b2364476bdc24805f850295ba8`.
- App source commit: `96dedf2c20b76de7e6c38c1cfa566d3ac7314b7d`.
- Developer ID publisher: Overtex Core Solutions SRL, team WVNWL3N9G8.
- DMG identifier: `dev.desklink.desktop.dmg`.
- DMG Apple submission: `4ce1f8ec-ef62-4600-b7ba-907aef65e7b1`, status Accepted.
- Existing app Apple submission: `e13248d6-49b9-48bf-9977-bb561309aa2b`, Accepted.
- Both the app and DMG have validated stapled tickets.

The DMG was signed with the existing Keychain identity and secure timestamp,
submitted with the existing Keychain notarization profile, then stapled. No key
or credential was exported. The reusable packaging command is:

```sh
DESKLINK_DISTRIBUTION_IDENTITY='Developer ID Application: Your Company (TEAMID)' \
DESKLINK_NOTARY_PROFILE=your-keychain-profile \
DESKLINK_TEAM_ID=TEAMID \
bash scripts/package-macos-dmg.sh /path/DeskLink.app /path/THIRD-PARTY-NOTICES /new/output-directory
```

The helper refuses an existing output path, validates the app's Developer ID team,
Apple trust and ticket before packaging, and requires Apple's Accepted result
before final ticket/checksum generation. It uses HFS+/UDZO for compatibility with
the app's supported macOS versions. It never changes the app's signed contents.

## Verification

The exact public DMG was fetched over verified HTTPS. Its bytes and SHA-256
matched the staged artifact. A standard download quarantine marker was added to
the verification copy and retained. Both the downloaded DMG and contained app
passed the expected Developer ID/team requirement, strict signature validation,
stapler validation and Gatekeeper assessment. DMG integrity checks also passed.

The image mounted read-only; its Applications shortcut resolved to /Applications.
The app was copied into an isolated installation-check directory. Its executable
permission, 0.3.3/build 9 metadata, unchanged executable hash, signature, ticket
and Gatekeeper acceptance all passed after copying. The downloaded disk image
also opened through Finder normally and visibly displayed the app, Applications
shortcut, instructions and notices. No Gatekeeper bypass was used.

This task did not replace the user's installed app or refresh sharing permissions.
It did not launch the copied app, approve a remote session or send input. The
unchanged app retains the earlier test scope in SHARP-VIDEO-0.3.3.md; full two-way
input, fresh permission onboarding and second physical Mac checks remain pending.

## Publication and rollback

The DMG was added at a new immutable URL before switching the website. Older ZIPs
remain available at their original URLs. HTML, manifest and checksum now select
the DMG; installation notes explain opening it, dragging to Applications, ejecting
and launching the installed app. Live HTML/script/manifest/checksum bytes and the
browser's Mac download entry were verified. Windows/Linux package objects and
signing status did not change. The relay was not restarted.

The previous four website files were saved at
`/opt/desklink/site-before-mac-dmg033-20261121T092858Z`. Restoring those files
rolls back the default Mac link without changing immutable assets.

Workflow references: [Apple packaging guidance](https://developer.apple.com/documentation/xcode/packaging-mac-software-for-distribution)
and [custom notarization](https://developer.apple.com/documentation/security/customizing-the-notarization-workflow).
