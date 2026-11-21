# DeskLink app icon

Two overlapping screen frames express a connection between computers. The mark
extends the existing website's linked-screen symbol instead of introducing a new
brand. A deep green tile (#145f4d), mint rear screen (#c5e3a1) and warm cream front
screen (#f7f8f2) retain the website palette and remain distinct on light/dark UI.
There are no letters or fine details. A heavier stroke at 16–32 px improves
legibility; transparent corners keep the tile clean in native desktop surfaces.

The shared geometry in `scripts/generate-app-icons.py` generates the SVG master,
PNG sizes 16–1024, Windows multi-resolution ICO and Mac ICNS. Regeneration needs
Python with Pillow and, for ICNS, Apple's iconutil on macOS. Generated assets are
checked in, so normal Rust builds do not require those artwork tools.

The runtime window icon uses the 256 px PNG on Windows/Linux. The Mac app bundle
uses DeskLink.icns through CFBundleIconFile. Windows executable resources and
NSIS installer/uninstaller use the ICO. Icons are original project artwork,
provided under the repository's MIT license.
