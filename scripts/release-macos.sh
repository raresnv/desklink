#!/usr/bin/env bash
# Requires configured signing accounts. Does not store or print credentials.
set -euo pipefail
desklink_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$desklink_root"
printenv DESKLINK_DISTRIBUTION_IDENTITY >/dev/null || { echo 'Set an installed Developer ID Application identity.' >&2; exit 1; }
printenv DESKLINK_NOTARY_PROFILE >/dev/null || { echo 'Set a notarytool keychain profile.' >&2; exit 1; }
case "$DESKLINK_DISTRIBUTION_IDENTITY" in
  "Developer ID Application:"*) ;;
  *) echo 'Production requires Developer ID Application signing.' >&2; exit 1;;
esac
[ "$(uname -m)" = arm64 ] || { echo 'This release target is Apple Silicon.' >&2; exit 1; }
desklink_version=$(/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' packaging/macos/Info.plist)
desklink_output="$desklink_root/dist/production/$desklink_version"
if [ -e "$desklink_output" ]; then echo 'Use a new version; release output already exists.' >&2; exit 1; fi
export MACOSX_DEPLOYMENT_TARGET=12.3
DESKLINK_SIGNING_IDENTITY="$DESKLINK_DISTRIBUTION_IDENTITY" bash scripts/bundle-macos.sh
desklink_bundle="$desklink_root/dist/DeskLink.app"
codesign --force --options runtime --timestamp --sign "$DESKLINK_DISTRIBUTION_IDENTITY" "$desklink_bundle"
codesign --verify --deep --strict "$desklink_bundle"
mkdir -p "$desklink_output"
ditto -c -k --sequesterRsrc --keepParent "$desklink_bundle" "$desklink_output/notary-submission.zip"
xcrun notarytool submit "$desklink_output/notary-submission.zip" --keychain-profile "$DESKLINK_NOTARY_PROFILE" --wait --output-format json > "$desklink_output/notary-result.json"
xcrun stapler staple "$desklink_bundle"
xcrun stapler validate "$desklink_bundle"
spctl --assess --type execute --verbose "$desklink_bundle"
ditto -c -k --sequesterRsrc --keepParent "$desklink_bundle" "$desklink_output/DeskLink-$desklink_version-macos-arm64.zip"
shasum -a 256 "$desklink_output/DeskLink-$desklink_version-macos-arm64.zip" > "$desklink_output/SHA256SUMS.txt"
