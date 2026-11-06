#!/bin/sh
# Signs (Developer ID, hardened runtime), notarizes and staples a .app or a .dmg. release.yml calls
# it for the app, then again for the disk image made from the stapled app; it falls back to the
# ad-hoc signed bundle when the secrets do not exist.
#
#   tools/sign-macos.sh "dist/<triple>/put.io adder.app"
#   tools/sign-macos.sh dist/putio-adder-vX.Y.Z-macos-arm64.dmg
#
# Environment (all required together):
#   MACOS_CERTIFICATE_P12_BASE64   base64 of the Developer ID Application .p12
#   MACOS_CERTIFICATE_PASSWORD     its password
#   MACOS_SIGN_IDENTITY            e.g. "Developer ID Application: Name (TEAMID)"
#   ASC_KEY_P8_BASE64              base64 of the App Store Connect API key (AuthKey_XXXX.p8)
#   ASC_KEY_ID, ASC_ISSUER_ID      that key's id and the issuer id of the team
set -eu
target="$1"
[ -e "$target" ] || { echo "no such file: $target" >&2; exit 1; }

for name in MACOS_CERTIFICATE_P12_BASE64 MACOS_CERTIFICATE_PASSWORD MACOS_SIGN_IDENTITY ASC_KEY_P8_BASE64 ASC_KEY_ID ASC_ISSUER_ID; do
    eval "value=\${$name:-}"
    [ -n "$value" ] || { echo "$name is not set" >&2; exit 1; }
done

work=$(mktemp -d)
keychain="$work/signing.keychain-db"
keychain_password=$(uuidgen)
original_keychains=$(security list-keychains -d user | tr -d '"')
cleanup() {
    # shellcheck disable=SC2086
    security list-keychains -d user -s $original_keychains 2>/dev/null || true
    security delete-keychain "$keychain" 2>/dev/null || true
    rm -rf "$work"
}
trap cleanup EXIT

printf '%s' "$MACOS_CERTIFICATE_P12_BASE64" | base64 --decode > "$work/cert.p12"
printf '%s' "$ASC_KEY_P8_BASE64" | base64 --decode > "$work/AuthKey.p8"
security create-keychain -p "$keychain_password" "$keychain"
security set-keychain-settings -lut 1800 "$keychain"
security unlock-keychain -p "$keychain_password" "$keychain"
security import "$work/cert.p12" -k "$keychain" -P "$MACOS_CERTIFICATE_PASSWORD" -T /usr/bin/codesign
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$keychain_password" "$keychain" >/dev/null
# shellcheck disable=SC2086
security list-keychains -d user -s "$keychain" $original_keychains

notarize() {
    # $1: the file to upload (a zip for an app, the image itself for a dmg)
    if ! xcrun notarytool submit "$1" --key "$work/AuthKey.p8" --key-id "$ASC_KEY_ID" --issuer "$ASC_ISSUER_ID" \
            --wait --output-format json > "$work/notarize.json"; then
        cat "$work/notarize.json" >&2
        echo "notarytool submit failed" >&2
        exit 1
    fi
    cat "$work/notarize.json"
    status=$(plutil -extract status raw -o - "$work/notarize.json")
    if [ "$status" != "Accepted" ]; then
        id=$(plutil -extract id raw -o - "$work/notarize.json")
        xcrun notarytool log "$id" --key "$work/AuthKey.p8" --key-id "$ASC_KEY_ID" --issuer "$ASC_ISSUER_ID" || true
        echo "notarization finished with status $status" >&2
        exit 1
    fi
}

# Stapling fetches the ticket from Apple's CloudKit, which now and then drops the connection even
# though the submission was Accepted: try again a few times before giving up.
staple() {
    attempt=1
    while ! xcrun stapler staple "$1"; do
        [ "$attempt" -lt 5 ] || { echo "stapler staple failed" >&2; exit 1; }
        echo "stapler staple failed (attempt $attempt); retrying" >&2
        sleep $((attempt * 20))
        attempt=$((attempt + 1))
    done
}

case "$target" in
    *.app)
        codesign --force --options runtime --timestamp --sign "$MACOS_SIGN_IDENTITY" --keychain "$keychain" "$target"
        codesign --verify --deep --strict --verbose=2 "$target"
        ditto -c -k --keepParent "$target" "$work/notarize.zip"
        notarize "$work/notarize.zip"
        staple "$target"
        codesign --verify --deep --strict --verbose=2 "$target"
        spctl --assess --type execute --verbose=2 "$target"
        ;;
    *.dmg)
        codesign --force --timestamp --sign "$MACOS_SIGN_IDENTITY" --keychain "$keychain" "$target"
        codesign --verify --verbose=2 "$target"
        notarize "$target"
        staple "$target"
        spctl --assess --type open --context context:primary-signature --verbose=2 "$target"
        ;;
    *)
        echo "expected a .app or a .dmg: $target" >&2
        exit 1
        ;;
esac
echo "signed and notarized: $target"
