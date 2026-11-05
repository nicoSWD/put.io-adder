#!/bin/sh
# Signs (Developer ID, hardened runtime) and notarizes a .app. Only runs when the secrets exist;
# release.yml calls it and falls back to the ad-hoc signed bundle otherwise.
#
#   tools/sign-macos.sh "dist/<triple>/put.io adder.app"
#
# Environment (all required together):
#   MACOS_CERTIFICATE_P12_BASE64   base64 of the Developer ID Application .p12
#   MACOS_CERTIFICATE_PASSWORD     its password
#   MACOS_SIGN_IDENTITY            e.g. "Developer ID Application: Name (TEAMID)"
#   NOTARY_APPLE_ID, NOTARY_TEAM_ID, NOTARY_PASSWORD   app-specific password for notarytool
set -eu
app="$1"

for name in MACOS_CERTIFICATE_P12_BASE64 MACOS_CERTIFICATE_PASSWORD MACOS_SIGN_IDENTITY NOTARY_APPLE_ID NOTARY_TEAM_ID NOTARY_PASSWORD; do
    eval "value=\${$name:-}"
    [ -n "$value" ] || { echo "$name is not set" >&2; exit 1; }
done

work=$(mktemp -d)
keychain="$work/signing.keychain-db"
keychain_password=$(uuidgen)
cleanup() {
    security delete-keychain "$keychain" 2>/dev/null || true
    rm -rf "$work"
}
trap cleanup EXIT

printf '%s' "$MACOS_CERTIFICATE_P12_BASE64" | base64 --decode > "$work/cert.p12"
security create-keychain -p "$keychain_password" "$keychain"
security set-keychain-settings -lut 1800 "$keychain"
security unlock-keychain -p "$keychain_password" "$keychain"
security import "$work/cert.p12" -k "$keychain" -P "$MACOS_CERTIFICATE_PASSWORD" -T /usr/bin/codesign
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$keychain_password" "$keychain" >/dev/null
security list-keychains -d user -s "$keychain" $(security list-keychains -d user | tr -d '"')

codesign --force --options runtime --timestamp --sign "$MACOS_SIGN_IDENTITY" --keychain "$keychain" "$app"
codesign --verify --strict --verbose=2 "$app"

zip="$work/notarize.zip"
ditto -c -k --keepParent "$app" "$zip"
xcrun notarytool submit "$zip" --apple-id "$NOTARY_APPLE_ID" --team-id "$NOTARY_TEAM_ID" --password "$NOTARY_PASSWORD" --wait
xcrun stapler staple "$app"
spctl --assess --type execute --verbose=2 "$app"
echo "signed and notarized: $app"
