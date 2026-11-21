#!/usr/bin/env bash
# Wrap an already signed/stapled app; never rebuild or re-sign the app itself.
set -euo pipefail
if [ "$#" -ne 3 ]; then
  echo 'Usage: package-macos-dmg.sh /path/DeskLink.app /path/THIRD-PARTY-NOTICES /new/output-directory' >&2
  exit 1
fi
: "${DESKLINK_DISTRIBUTION_IDENTITY:?Set an installed Developer ID Application identity}"
: "${DESKLINK_NOTARY_PROFILE:?Set a notarytool Keychain profile}"
: "${DESKLINK_TEAM_ID:?Set the expected Apple team ID}"
case "$DESKLINK_DISTRIBUTION_IDENTITY" in
  'Developer ID Application:'*) ;;
  *) echo 'A Developer ID Application identity is required.' >&2; exit 1 ;;
esac
desklink_root="$(cd "$(dirname "$0")/.." && pwd)"
desklink_app="$(cd "$1" && pwd)"
desklink_notices="$(cd "$2" && pwd)"
desklink_output="$3"
[ ! -e "$desklink_output" ] || { echo 'Output exists; do not overwrite an immutable release.' >&2; exit 1; }
[ -f "$desklink_notices/inventory.json" ]
desklink_version=$(/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' "$desklink_app/Contents/Info.plist")
desklink_bundle_id=$(/usr/libexec/PlistBuddy -c 'Print CFBundleIdentifier' "$desklink_app/Contents/Info.plist")
[ "$desklink_bundle_id" = 'dev.desklink.desktop' ]
case "$desklink_version" in ''|*[!0-9.]*) echo 'Unexpected version.' >&2; exit 1;; esac
[ -x "$desklink_app/Contents/MacOS/DeskLink" ]
lipo -verify_arch arm64 "$desklink_app/Contents/MacOS/DeskLink"
codesign --verify --deep --strict -R "=anchor apple generic and certificate leaf[field.1.2.840.113635.100.6.1.13] exists and certificate leaf[subject.OU] = \"$DESKLINK_TEAM_ID\"" "$desklink_app"
xcrun stapler validate "$desklink_app"
spctl --assess --type execute --verbose=2 "$desklink_app"
mkdir -p "$desklink_output"
desklink_output="$(cd "$desklink_output" && pwd)"
desklink_stage=$(mktemp -d "$desklink_output/.dmg-content.XXXXXX")
trap 'rm -rf "$desklink_stage"' EXIT
ditto "$desklink_app" "$desklink_stage/DeskLink.app"
ditto "$desklink_notices" "$desklink_stage/THIRD-PARTY-NOTICES"
cp "$desklink_root/LICENSE" "$desklink_stage/LICENSE.txt"
ln -s /Applications "$desklink_stage/Applications"
cat > "$desklink_stage/Install DeskLink.txt" <<EOF
DeskLink $desklink_version — signed preview for Apple Silicon Macs

1. Quit any older copy of DeskLink.
2. Drag DeskLink.app onto the Applications shortcut in this disk image.
   If upgrading, replace the old application when Finder asks.
3. Eject this disk image, then open DeskLink from Applications.

The app and disk image are Developer ID signed by Overtex Core Solutions SRL,
Apple notarized and include stapled tickets. Keep normal macOS security settings.
Apple Silicon / ARM64; macOS 12.3 or later. Intel Macs are not supported here.

DeskLink automatically connects to the public relay and shows your computer ID.
Your existing ID and remembered peers are retained when upgrading. Every remote
session requires host approval. Screen Recording is needed to share the screen;
Accessibility / Device Control is needed to allow keyboard and mouse control.

Install 0.3.3 or later on both computers for Sharp, Balanced and Original / Retina
quality, Fit / 100% zoom and panning. Higher resolution uses more bandwidth and
may reduce frame rate. Windows previews remain unsigned.

This is preview software with limited platform/network testing, no independent
security audit and no production-scale guarantee. Website and privacy details:
https://desklink.161.97.168.233.nip.io/
EOF
desklink_dmg="$desklink_output/DeskLink-$desklink_version-macos-arm64.dmg"
hdiutil create -volname "DeskLink $desklink_version" -srcfolder "$desklink_stage" -fs HFS+ -format UDZO "$desklink_dmg"
codesign --timestamp --identifier "$desklink_bundle_id.dmg" --sign "$DESKLINK_DISTRIBUTION_IDENTITY" "$desklink_dmg"
codesign --verify --strict -R "=anchor apple generic and certificate leaf[field.1.2.840.113635.100.6.1.13] exists and certificate leaf[subject.OU] = \"$DESKLINK_TEAM_ID\"" "$desklink_dmg"
xcrun notarytool submit "$desklink_dmg" --keychain-profile "$DESKLINK_NOTARY_PROFILE" --wait --output-format json > "$desklink_output/notary-result.json"
/usr/bin/plutil -extract status raw -o - "$desklink_output/notary-result.json" | /usr/bin/grep -qx Accepted
xcrun stapler staple "$desklink_dmg"
xcrun stapler validate "$desklink_dmg"
codesign --verify --strict "$desklink_dmg"
spctl --assess --type open --context context:primary-signature --verbose=2 "$desklink_dmg"
hdiutil verify "$desklink_dmg"
(cd "$desklink_output" && shasum -a 256 "$(basename "$desklink_dmg")") > "$desklink_output/SHA256SUMS-macos-dmg.txt"
echo "Signed, notarized and stapled DMG: $desklink_dmg"
