#!/bin/bash
# Copies the release files in dist/release into the downloads folder the app container reads.
# Only needed when the Docker host cannot see this folder directly (a remote daemon, or a VM
# that does not share the host's files); with an ordinary bind mount the files are already there.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source_dir="${DOWNLOADS_HOST_DIR:-$root/dist/release}"
container="${CEREAL_APP_CONTAINER:-meeting-notes-app-1}"
image="$(docker inspect "$container" --format '{{.Config.Image}}')"
mount="$(docker inspect "$container" --format '{{range .Mounts}}{{if eq .Destination "/data/downloads"}}{{.Source}}{{end}}{{end}}')"
[ -n "$mount" ] || { echo "The app container has no /data/downloads mount" >&2; exit 1; }
shopt -s nullglob
files=("$source_dir"/Cereal-*-mac-*.dmg "$source_dir"/Cereal-*-mac-*.zip)
[ ${#files[@]} -gt 0 ] || { echo "No release files in $source_dir" >&2; exit 1; }
# Remove builds that are no longer in the source folder, then stream each file in.
keep="$(printf '%s\n' "${files[@]##*/}")"
docker run --rm -i --user 0 --entrypoint sh -v "$mount:/downloads" -e KEEP="$keep" "$image" -c \
  'for f in /downloads/Cereal-*; do [ -e "$f" ] || continue; printf "%s\n" "$KEEP" | grep -qxF "${f##*/}" || rm -f "$f"; done'
for file in "${files[@]}"; do
  name="${file##*/}"
  docker run --rm -i --user 0 --entrypoint sh -v "$mount:/downloads" -e NAME="$name" "$image" -c \
    'cat > "/downloads/.$NAME.part" && mv "/downloads/.$NAME.part" "/downloads/$NAME" && chmod 644 "/downloads/$NAME"' < "$file"
  echo "Published $name"
done
