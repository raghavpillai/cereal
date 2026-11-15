#!/bin/bash
# Builds a signed, notarized Cereal.app and DMG for distribution.
#
#   CEREAL_SIGNING_DIR=<folder> ./scripts/release-macos.sh [--skip-build]
#
# The folder is private and lives outside the repository. It holds:
#   certificate.p12   Developer ID Application certificate and private key
#   key-password      the P12's password
#   asc-key.json      App Store Connect API key for notarization: {"issuerId", "keyId", "keyP8"}
# Every file must be readable by its owner only. Nothing from it is printed or copied into the output.
#
# The certificate is imported into a temporary keychain that is removed when the script ends, so the
# login keychain is never changed. Output: dist/release/Cereal-<version>-mac-<arch>.dmg
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
signing="${CEREAL_SIGNING_DIR:?Set CEREAL_SIGNING_DIR to the folder holding certificate.p12, key-password and asc-key.json}"
build=true
for argument in "$@"; do
  case "$argument" in
    --skip-build) build=false ;;
    *) printf 'Usage: CEREAL_SIGNING_DIR=<folder> %s [--skip-build]\n' "$0" >&2; exit 2 ;;
  esac
done
fail() { printf 'release-macos: %s\n' "$1" >&2; exit 1; }
for name in certificate.p12 key-password asc-key.json; do
  [[ -f "$signing/$name" ]] || fail "missing $name in the signing folder"
  [[ "$(stat -f %Lp "$signing/$name")" =~ ^[46]00$ ]] || fail "$name must be readable by its owner only (chmod 600)"
done

work="$(mktemp -d -t cereal-release)"
keychain="$work/signing.keychain-db"
keychain_password="$(openssl rand -hex 24)"
saved_keychains=()
while IFS= read -r line; do
  line="${line#"${line%%[![:space:]]*}"}"; line="${line#\"}"; line="${line%\"}"
  [[ -n "$line" ]] && saved_keychains+=("$line")
done < <(security list-keychains -d user)
cleanup() {
  security list-keychains -d user -s "${saved_keychains[@]}" >/dev/null 2>&1 || true
  security delete-keychain "$keychain" >/dev/null 2>&1 || true
  rm -rf "$work"
}
trap cleanup EXIT

security create-keychain -p "$keychain_password" "$keychain"
security set-keychain-settings -lut 7200 "$keychain"
security unlock-keychain -p "$keychain_password" "$keychain"
security import "$signing/certificate.p12" -k "$keychain" -f pkcs12 -P "$(<"$signing/key-password")" -T /usr/bin/codesign >/dev/null
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$keychain_password" "$keychain" >/dev/null
security list-keychains -d user -s "$keychain" "${saved_keychains[@]}" >/dev/null
identity="$(security find-identity -v -p codesigning "$keychain" | awk '/Developer ID Application/ {print $2; exit}')"
[[ -n "$identity" ]] || fail "the certificate is not a Developer ID Application identity"

umask 077
api_key="$work/notarization.p8"
read -r key_id issuer_id < <(python3 - "$signing/asc-key.json" "$api_key" <<'PY'
import json, sys
credential = json.load(open(sys.argv[1]))
if not all(credential.get(name) for name in ("keyP8", "keyId", "issuerId")):
    sys.exit("incomplete notarization credential")
open(sys.argv[2], "w").write(credential["keyP8"])
print(credential["keyId"], credential["issuerId"])
PY
) || fail "asc-key.json is incomplete"
umask 022

notarize() {  # notarize <file>: submits it, waits, and fails unless Apple accepts it
  local result="$work/notary-$(basename "$1").json"
  xcrun notarytool submit "$1" --key "$api_key" --key-id "$key_id" --issuer "$issuer_id" --wait --output-format json > "$result" \
    || fail "notarization of $(basename "$1") could not be submitted"
  local status id
  status="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("status",""))' "$result")"
  id="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("id",""))' "$result")"
  if [[ "$status" != "Accepted" ]]; then
    xcrun notarytool log "$id" --key "$api_key" --key-id "$key_id" --issuer "$issuer_id" >&2 || true
    fail "Apple did not accept $(basename "$1") (status: ${status:-unknown})"
  fi
  printf 'Notarized %s\n' "$(basename "$1")"
}

cd "$root"
if "$build"; then cargo build --locked --release -p cereal; fi
stage="$work/stage"
mkdir -p "$stage"
CEREAL_BUNDLE_OUT="$stage" CEREAL_CODESIGN_IDENTITY="$identity" CEREAL_CODESIGN_KEYCHAIN="$keychain" \
  "$root/scripts/package.sh" --release --no-build >/dev/null
app="$stage/Cereal.app"
version="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$app/Contents/Info.plist")"
identifier="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Contents/Info.plist")"
architecture="$(lipo -archs "$app/Contents/MacOS/cereal" | tr ' ' '-')"

ditto -c -k --keepParent "$app" "$work/Cereal.zip"
notarize "$work/Cereal.zip"
xcrun stapler staple "$app" >/dev/null

output="$root/dist/release"
mkdir -p "$output"
dmg="$output/Cereal-$version-mac-$architecture.dmg"
ln -s /Applications "$stage/Applications"
rm -f "$dmg"
hdiutil create -volname Cereal -srcfolder "$stage" -ov -format UDZO "$dmg" >/dev/null
codesign --force --timestamp --sign "$identity" --keychain "$keychain" "$dmg"
notarize "$dmg"
xcrun stapler staple "$dmg" >/dev/null

# Verify what will actually be downloaded: the DMG, and the app inside it.
hdiutil verify "$dmg" >/dev/null
xcrun stapler validate "$dmg" >/dev/null
mount="$work/mount"
mkdir -p "$mount"
hdiutil attach "$dmg" -mountpoint "$mount" -readonly -nobrowse -quiet
detach() { hdiutil detach "$mount" -quiet || true; }
trap 'detach; cleanup' EXIT
mounted="$mount/Cereal.app"
codesign --verify --deep --strict "$mounted"
details="$(codesign -dv --verbose=4 "$mounted" 2>&1)"
[[ "$details" == *"Authority=Developer ID Application:"* ]] || fail "the app is not signed with a Developer ID"
[[ "$details" == *"Identifier=$identifier"* ]] || fail "the app's identifier is not $identifier"
[[ "$details" == *"runtime"* ]] || fail "the hardened runtime is not enabled"
spctl --assess --type execute "$mounted"
xcrun stapler validate "$mounted" >/dev/null
detach
trap cleanup EXIT

printf 'Released %s (%s, version %s)\n' "$dmg" "$identifier" "$version"
