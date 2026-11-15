#!/bin/bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
profile="debug"
build_app=true

for argument in "$@"; do
  case "$argument" in
    --release) profile="release" ;;
    --no-build) build_app=false ;;
    *) printf 'Usage: %s [--release] [--no-build]\n' "$0" >&2; exit 2 ;;
  esac
done

cd "$project_root"
if "$build_app"; then
  if [[ "$profile" == "release" ]]; then
    cargo build --locked --release
  else
    cargo build --locked
  fi
fi

target_dir="${CARGO_TARGET_DIR:-$project_root/target}"
if [[ "$target_dir" != /* ]]; then
  target_dir="$project_root/$target_dir"
fi
binary="$target_dir/$profile/cereal"
if [[ ! -x "$binary" ]]; then
  printf 'Missing executable: %s\nBuild the app first or omit --no-build.\n' "$binary" >&2
  exit 1
fi

# CEREAL_BUNDLE_OUT chooses where the bundle is written (default: dist).
bundle="${CEREAL_BUNDLE_OUT:-$project_root/dist}/Cereal.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
install -m 755 "$binary" "$bundle/Contents/MacOS/cereal"
install -m 644 "$project_root/apps/desktop/assets/Info.plist" "$bundle/Contents/Info.plist"
# CEREAL_BUNDLE_VERSION packages the same build under another version number (default: the plist's own).
# The app reads its version from its bundle, so this is how an older or newer build is made for testing updates.
if [[ -n "${CEREAL_BUNDLE_VERSION:-}" ]]; then
  if [[ ! "$CEREAL_BUNDLE_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$ ]]; then
    printf 'CEREAL_BUNDLE_VERSION must look like 1.2.3\n' >&2
    exit 2
  fi
  /usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $CEREAL_BUNDLE_VERSION" "$bundle/Contents/Info.plist"
fi
if [[ -f "$project_root/apps/desktop/native/OpenOats-LICENSE" ]]; then
  install -m 644 "$project_root/apps/desktop/native/OpenOats-LICENSE" "$bundle/Contents/Resources/OpenOats-LICENSE"
fi
printf 'APPL????' > "$bundle/Contents/PkgInfo"

# Cereal's own application icon.
install -m 644 "$project_root/apps/desktop/assets/app-icons/app.icns" "$bundle/Contents/Resources/app.icns"

# rust-embed embeds fonts/SVGs in release builds, or debug builds with debug-embed.
# GPUI also embeds its compiled Metal shaders in the executable.
# Ad hoc signed for local use. A release sets CEREAL_CODESIGN_IDENTITY (and usually
# CEREAL_CODESIGN_KEYCHAIN) to sign with a Developer ID and the hardened runtime; see release-macos.sh.
if [[ -n "${CEREAL_CODESIGN_IDENTITY:-}" ]]; then
  codesign --force --options runtime --timestamp --sign "$CEREAL_CODESIGN_IDENTITY" \
    ${CEREAL_CODESIGN_KEYCHAIN:+--keychain "$CEREAL_CODESIGN_KEYCHAIN"} \
    --entitlements "$project_root/apps/desktop/assets/Cereal.entitlements" "$bundle"
elif command -v codesign >/dev/null 2>&1; then
  codesign --force --sign - --entitlements "$project_root/apps/desktop/assets/Cereal.entitlements" "$bundle"
fi
plutil -lint "$bundle/Contents/Info.plist"
printf 'Packaged %s\nOpen with: open "%s"\n' "$profile" "$bundle"
