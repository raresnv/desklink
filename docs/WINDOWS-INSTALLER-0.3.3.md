# Windows 0.3.3 unsigned installers — 21 November 2026

The default Windows downloads now use per-user EXE installers for x64 and
experimental ARM64. The app executables are unchanged from the reviewed 0.3.3
ZIPs. Installer, app and uninstaller remain unsigned; no certificate or paid
signing service is configured. Windows may warn or block them. The installer
welcome screen, installed-app name and website explicitly say Unsigned Preview.

## Installation behavior

The installer runs at the normal user's privilege level and defaults to
`%LOCALAPPDATA%\Programs\DeskLink`. It creates Start menu application/removal
shortcuts and an entry in that user's Installed apps list. It adds no startup
entry, service, firewall rule or remote-access approval. It creates no desktop
shortcut. The finish page's Open DeskLink checkbox is off by default.

The x64 app supports 64-bit Windows 10 or later (tested under x64 emulation in
Windows 11 ARM). Native ARM64 requires Windows 11 ARM64 and remains experimental.
NSIS uses a standard x86 installer bootstrap, including for the ARM64 app payload.
No Intel/x86 32-bit app is provided.

An unrelated nonempty install folder is refused. An existing DeskLink-owned
installation can be reinstalled/upgraded. A locked application file stops an
upgrade or uninstall before changing files. Removal uses an explicit build-time
list of packaged files and non-recursive directory removal. Unrelated files and
shortcuts, and the existing private DeskLink ID/settings directories, are kept.
There is no silent deletion of an entire install directory or profile folder.

Uninstall from Windows Installed apps or the Start menu. Close DeskLink before
upgrading or removing it. The computer ID remains available for a later reinstall.
Portable ZIP downloads remain linked in the website's Windows installation notes.

## Build and artifacts

The public helper `scripts/package-windows-installer.py` requires Python 3.9+
and NSIS 3.13. It accepts a reviewed, complete Windows payload directory with
app/README/license/notices, validates architecture/version/unsigned status,
adds installer-specific instructions and the NSIS notice, then compiles the
installer from `packaging/windows/installer.nsi`. A new output directory is
required; existing releases are not overwritten. For example:

```sh
python3 scripts/package-windows-installer.py --payload /reviewed/package-x64 \
  --arch x64 --output /new/installer-x64
```

App x64 provenance remains GitHub Actions run 44503866752 / public commit
`73cfac4c9c8e153800aa62d973ee302143798511`; ARM64 remains the native locked
build recorded in WINDOWS-RELEASE-0.3.3.md. NSIS 3.13/zlib packaging was performed
on this Mac. The installer includes the compiler project's license notices; its
source is available from [NSIS](https://nsis.sourceforge.io/).

- [DeskLink-0.3.3-windows-x64-setup.exe](https://desklink.161.97.168.233.nip.io/downloads/0.3.3/DeskLink-0.3.3-windows-x64-setup.exe): 5,481,004 bytes.
  Installer SHA-256: `72762d82c4657ddc82ee864af35fd75199b3c01198fbac2f0a96663879b8b7b8`.
  App SHA-256: `a51d09f71a2fcbf7ca6928a391747d80f7dcd62d2af80958e2877d0dc1846e76`.
- [DeskLink-0.3.3-windows-arm64-setup.exe](https://desklink.161.97.168.233.nip.io/downloads/0.3.3/DeskLink-0.3.3-windows-arm64-setup.exe): 4,868,643 bytes.
  Installer SHA-256: `9d1075ffd63e7c8b93c9bcefb878df0aeae027a7f4bf50e72b89fd9c80be17cd`.
  App SHA-256: `e2457e271ec67cb1a4be3776c818dcb1d55457d545245bdc3fac0519bf3999e0`.

## Verification and fix

Both final installers passed 42 total checks in the Windows 11 ARM VM, running
through an interactive task with the normal user's token and no elevation:

- installer hash and NotSigned status; native 64-bit test process;
- unrelated nonempty target refused without altering its sentinel file;
- default per-user install, exact app hash/version, app/uninstaller NotSigned;
- license/notices, unsigned-preview Installed apps registration, correct
  Start menu target/working directory, no desktop shortcut by default;
- locked executable blocks upgrade; unlocked reinstall succeeds;
- existing ID/settings hashes retained across reinstall and removal;
- removal completes even if a packaged license file is already missing;
- unrelated files and a same-name desktop shortcut retained;
- installed app/uninstaller, owned shortcuts and uninstall registration removed.

An initial harness checked too early while NSIS's temporary uninstaller was
still running. Waiting for app/uninstaller/registration cleanup resolved that
harness timing issue. A subsequent native regression check found that a
same-name desktop shortcut could be deleted despite not being created by the
installer. Desktop shortcut creation/removal was removed from this release;
the final native test proves the unrelated shortcut remains. No initial
candidate was published.

The x64 installer welcome screen was visibly inspected in Parallels and clearly
shows Unsigned Preview and the unknown-publisher/blocking warning. Public
installers were downloaded back over verified HTTPS in Windows; their exact
hashes, 0.3.3 version, Unsigned Preview descriptions and NotSigned status matched
the tested candidates. No Defender or SmartScreen settings were changed or
bypassed, and no persistent PowerShell execution policy was changed. This is not proof of SmartScreen reputation or a clean
browser-download launch on a second physical PC.

No application code, screen sharing, input routing or network session approval
was changed or newly exercised. The earlier native tests/capture checks retain
their scope. Full two-way input on 0.3.3 and physical x64 testing remain pending.

## Publication

New immutable setup URLs were published before the website switch. The primary
Windows button and experimental ARM64 link now choose installers. Both portable
ZIPs remain available as alternatives. Manifest and checksums include all four
Windows choices. Mac DMG and Linux package objects, older files and relay service
were preserved. The website remains a public preview (`production_ready: false`).

The exact website files and public installers were verified after publication.
The previous four website files are backed up at
`/opt/desklink/site-before-windows-installer033-20261121T095009Z`. Restoring them
rolls back the default links without modifying immutable files. Temporary QA
and welcome tasks were removed, the private file-transfer listener stopped and
the VM returned to its original suspended state.
