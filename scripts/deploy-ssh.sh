#!/bin/bash
# Updates a Cereal server that runs with Docker Compose on another machine, over SSH.
#
#   ./scripts/deploy-ssh.sh <user@host> [git-ref] [--release <dir>] [--dir <remote dir>] [--no-backup] [--check]
#
#   git-ref         the commit to deploy (default: HEAD). Only committed files are sent, through `git archive`,
#                   so edits in this working tree never reach the server.
#   --release DIR   also publish the Mac build in DIR (Cereal-<version>-mac-*.dmg and .zip, as written by
#                   scripts/release-macos.sh). Without it the server keeps offering the build it has.
#   --dir PATH      where the deployment lives on the server (default: ~/cereal). It must already hold the
#                   .env that scripts/setup.sh made there; that file is carried over and never printed.
#   --no-backup     skip the database dump taken before the update.
#   --check         only report what the server runs and whether it is ready for an update; change nothing.
#
# What it does, in order:
#   1. dumps the database to ~/cereal-backups/<time>-<previous commit>.dump (the 10 newest are kept);
#   2. unpacks the commit into <dir>.next, next to the running copy, with the existing .env and downloads;
#   3. builds the new image while the old server keeps answering;
#   4. swaps the folders (the previous copy becomes <dir>.previous) and starts the new containers, which apply
#      any database migrations first;
#   5. waits for the server to report healthy. If it does not, the previous copy is put back and started again.
#      Migrations only go forward: when a failed update had already migrated the database, restore the dump
#      from step 1 as well (see the commands it prints).
#
# The Compose project is named in compose.yaml, so the database, audio and settings volumes stay the same
# across the folder swap.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

usage() { sed -n '4,13p' "$0" | sed 's/^# \{0,1\}//' >&2; exit 2; }
target='' ref='HEAD' release='' remote_dir='~/cereal' backup=1 check=0
while [ $# -gt 0 ]; do
  case "$1" in
    --release) release="${2:?--release needs a folder}"; shift ;;
    --dir) remote_dir="${2:?--dir needs a path}"; shift ;;
    --no-backup) backup=0 ;;
    --check) check=1 ;;
    -h|--help) usage ;;
    -*) usage ;;
    *) if [ -z "$target" ]; then target="$1"; else ref="$1"; fi ;;
  esac
  shift
done
[ -n "$target" ] || usage
fail() { printf 'deploy-ssh: %s\n' "$1" >&2; exit 1; }

commit="$(git rev-parse --verify --quiet "$ref^{commit}")" || fail "no commit called $ref"
short="${commit:0:7}"
version="$(git show "$commit:package.json" | sed -n 's/^ *"version": *"\([^"]*\)".*/\1/p' | head -1)"
[ -n "$version" ] || fail "could not read the version from package.json at $short"
release_files=()
if [ -n "$release" ]; then
  for file in "$release/Cereal-$version-mac-"*.dmg "$release/Cereal-$version-mac-"*.zip; do
    [ -f "$file" ] && release_files+=("$file")
  done
  [ ${#release_files[@]} -eq 2 ] || fail "expected one Cereal-$version-mac-*.dmg and one .zip in $release"
fi

ssh_opts=(-o BatchMode=yes -o ConnectTimeout=15 -o ServerAliveInterval=30)
remote() { ssh "${ssh_opts[@]}" "$target" "$@"; }
echo "Deploying $short (version $version) to $target:$remote_dir"

# Preflight: the deployment exists, has its .env, and Docker answers.
remote "set -e; d=$remote_dir; test -f \$d/.env || { echo 'no .env in '\$d' (run scripts/setup.sh there first)' >&2; exit 1; }
  docker compose version >/dev/null; cd \$d; docker compose ps --status running --services | grep -qx db || { echo 'the database container is not running' >&2; exit 1; }
  printf 'running: %s\n' \"\$(cat \$d/DEPLOYED_COMMIT 2>/dev/null || echo unknown)\"; docker compose ps --format '{{.Service}}: {{.Status}}'; df -h \$d | tail -1 | awk '{print \"free on server: \"\$4}'"

[ "$check" = 0 ] || { echo "Check only: nothing was changed."; exit 0; }

if [ "$backup" = 1 ]; then
  remote "set -euo pipefail; d=$remote_dir; mkdir -p ~/cereal-backups; chmod 700 ~/cereal-backups
    prev=\$(cut -c1-7 \$d/DEPLOYED_COMMIT 2>/dev/null || echo unknown); out=~/cereal-backups/\$(date -u +%Y%m%dT%H%M%SZ)-\$prev.dump
    cd \$d; docker compose exec -T db pg_dump -U meeting_notes -d meeting_notes -Fc > \$out.part; mv \$out.part \$out; chmod 600 \$out
    ls -1t ~/cereal-backups/*.dump | tail -n +11 | xargs -r rm -f; echo \"database backup: \$out (\$(du -h \$out | cut -f1))\""
fi

# Unpack the commit next to the running copy, carrying over the .env and the published downloads.
remote "set -e; d=$remote_dir; rm -rf \$d.next; mkdir \$d.next"
git archive --format=tar "$commit" | remote "tar -x -C $remote_dir.next"
remote "set -e; d=$remote_dir; cp -p \$d/.env \$d.next/.env; mkdir -p \$d.next/dist/release
  [ -d \$d/dist/release ] && cp -a \$d/dist/release/. \$d.next/dist/release/; echo $commit > \$d.next/DEPLOYED_COMMIT"
if [ ${#release_files[@]} -gt 0 ]; then
  remote "rm -f $remote_dir.next/dist/release/Cereal-*"
  scp -q "${ssh_opts[@]}" "${release_files[@]}" "$target:$remote_dir.next/dist/release/"
  echo "release files: ${release_files[*]##*/}"
fi

echo "Building the image (the current server keeps running)…"
remote "set -e; cd $remote_dir.next && docker compose build --quiet" || { remote "rm -rf $remote_dir.next"; fail "the image did not build; nothing changed on the server"; }

echo "Starting the new version…"
remote "set -e; d=$remote_dir; rm -rf \$d.previous; mv \$d \$d.previous; mv \$d.next \$d; cd \$d; docker compose up -d --remove-orphans"

healthy() {
  remote "cd $remote_dir; for i in \$(seq 1 36); do s=\$(docker inspect -f '{{.State.Health.Status}}' meeting-notes-app-1 2>/dev/null || echo missing); [ \"\$s\" = healthy ] && exit 0; sleep 5; done; exit 1"
}
if ! healthy; then
  echo "The new version did not become healthy. Last log lines:" >&2
  remote "cd $remote_dir; docker compose logs --tail=60 migrate app" >&2 || true
  echo "Putting the previous version back…" >&2
  remote "set -e; d=$remote_dir; rm -rf \$d.failed; mv \$d \$d.failed; mv \$d.previous \$d; cd \$d; docker compose up -d --build --remove-orphans"
  healthy && echo "The previous version is running again. The failed copy is in $remote_dir.failed." >&2
  echo "If the failed version had migrated the database, restore the backup too:" >&2
  echo "  cd $remote_dir && docker compose stop app && docker compose exec -T db pg_restore -U meeting_notes -d meeting_notes --clean --if-exists < ~/cereal-backups/<file>.dump && docker compose start app" >&2
  exit 1
fi

base="$(remote "sed -n 's/^PUBLIC_BASE_URL=//p' $remote_dir/.env" | tr -d '\r"' | sed 's:/*$::')"
if [ -n "$base" ]; then
  ready="$(curl -fsS --max-time 15 "$base/api/v1/ready" || true)"
  offered="$(curl -fsS --max-time 15 "$base/api/v1/downloads" | sed -n 's/.*"version":"\([^"]*\)".*/\1/p' || true)"
  echo "$base/api/v1/ready: ${ready:-no answer}"
  echo "Mac build offered: ${offered:-none}"
  [ "$ready" = '{"status":"ready"}' ] || fail "the server is healthy inside Docker but $base does not answer; check the reverse proxy"
fi
echo "Deployed $short. The previous copy is in $remote_dir.previous."
