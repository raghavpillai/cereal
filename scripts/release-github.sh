#!/bin/bash
# Publishes a version as a GitHub release: the tag v<version> on a commit, with the Mac build attached.
#
#   ./scripts/release-github.sh [commit] [--dir <folder with the Mac build>] [--no-mac] [--notes <text>]
#
# A release is what servers update to. The updater (scripts/updater.sh) follows the repository's releases: it
# installs the one with the highest number and downloads the Mac build attached to it, so the updated server
# offers desktop apps the same version. Publish in this order:
#
#   1. set the version everywhere (bun scripts/check-version.ts says where), commit, push
#   2. build the Mac app from that commit: ./scripts/release-macos.sh   (writes dist/release/Cereal-<version>-mac-arm64.dmg and .zip)
#   3. ./scripts/release-github.sh                                      (this script)
#   4. on each server: admin panel → Updates → Update server
#
# The commit defaults to HEAD and must be on the remote. The version is the one in that commit's package.json;
# nothing is published if a release or tag of that version exists. --no-mac publishes a release with no Mac
# build (servers update; desktop apps are offered nothing new). Needs the GitHub CLI, signed in (`gh auth status`).
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
fail() { printf 'release: %s\n' "$1" >&2; exit 1; }
commit='' dir=dist/release mac=1 notes=''
while [ $# -gt 0 ]; do
  case "$1" in
    --dir) dir="${2:?--dir needs a folder}"; shift ;;
    --notes) notes="${2:?--notes needs text}"; shift ;;
    --no-mac) mac=0 ;;
    -h|--help) sed -n '2,17p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    -*) fail "unknown option $1" ;;
    *) [ -z "$commit" ] || fail "one commit at most"; commit="$1" ;;
  esac
  shift
done
command -v gh >/dev/null || fail "the GitHub CLI (gh) is not installed"
commit="$(git rev-parse --verify --quiet "${commit:-HEAD}^{commit}")" || fail "no such commit"
version="$(git show "$commit:package.json" | sed -n 's/^ *"version": *"\([^"]*\)".*/\1/p' | head -1)"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "the version in package.json of ${commit:0:7} is not three numbers: '$version'"
tag="v$version"
git fetch --quiet --tags origin
[ -n "$(git branch -r --contains "$commit" 2>/dev/null)" ] || fail "${commit:0:7} is not on the remote yet. Push it first."
if git rev-parse --verify --quiet "refs/tags/$tag" >/dev/null || gh release view "$tag" >/dev/null 2>&1; then fail "$tag exists already. A released version is never replaced: raise the version and release that."; fi
files=()
if [ "$mac" = 1 ]; then
  for name in "Cereal-$version-mac-arm64.dmg" "Cereal-$version-mac-arm64.zip"; do
    [ -s "$dir/$name" ] || fail "$dir/$name is missing. Build it with ./scripts/release-macos.sh from ${commit:0:7}, or pass --no-mac."
    files+=("$dir/$name")
  done
  # A build that Gatekeeper would refuse must never be handed to people.
  if command -v spctl >/dev/null; then
    spctl --assess --type open --context context:primary-signature "$dir/Cereal-$version-mac-arm64.dmg" >/dev/null 2>&1 || fail "$dir/Cereal-$version-mac-arm64.dmg is not accepted by Gatekeeper (not signed and notarised?)"
  fi
fi
echo "Publishing $tag at ${commit:0:7}${files[0]:+ with the Mac build}…"
if [ -n "$notes" ]; then gh release create "$tag" --target "$commit" --title "Cereal $version" --notes "$notes" ${files[@]+"${files[@]}"}
else gh release create "$tag" --target "$commit" --title "Cereal $version" --generate-notes ${files[@]+"${files[@]}"}; fi
git fetch --quiet --tags origin
echo "Published. Servers that follow releases now offer $version under admin panel → Updates."
