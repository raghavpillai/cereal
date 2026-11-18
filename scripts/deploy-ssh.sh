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
# Layout on the server: <dir> is a symbolic link to one release, <dir>-releases/<time>-<commit>, and every
# release holds its own copy of the .env and the published downloads. A deployment that is still a plain
# folder (made by setup.sh, or by an earlier version of this script) is moved into <dir>-releases at the
# next deploy, and <dir> becomes the link. The link is replaced in one rename, so <dir> always names a
# complete release, even when the connection drops halfway.
#
# What it does, in order:
#   1. dumps the database to ~/cereal-backups/<time>-<previous commit>.dump (the 10 newest are kept);
#   2. unpacks the commit into a new release, with the existing .env and downloads;
#   3. builds its image, meeting-notes:<commit>, while the old server keeps answering;
#   4. on the server, as a job that carries on if the SSH connection drops: points <dir> at the new release
#      and starts it (database migrations run first), then waits for it to report healthy. If anything in
#      this step fails, it points <dir> back at the previous release and starts that again from its own
#      image, without rebuilding. The job's log is <dir>-releases/.jobs/<release>/log.
#   5. Migrations only go forward: when a failed update had already migrated the database, restore the dump
#      from step 1 as well (it prints the commands).
#
# The Compose project is named in compose.yaml, so the database, audio and settings volumes stay the same
# whichever release is running.
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
tag="${commit:0:12}"
version="$(git show "$commit:package.json" | sed -n 's/^ *"version": *"\([^"]*\)".*/\1/p' | head -1)"
[ -n "$version" ] || fail "could not read the version from package.json at $short"
release_files=()
if [ -n "$release" ]; then
  for file in "$release/Cereal-$version-mac-"*.dmg "$release/Cereal-$version-mac-"*.zip; do
    [ -f "$file" ] && release_files+=("$file")
  done
  [ ${#release_files[@]} -eq 2 ] || fail "expected one Cereal-$version-mac-*.dmg and one .zip in $release"
fi

# Everything that runs on the server: scripts/release-helper.sh, as it is in this working tree. It is sent
# with every call (`bash -s`), and saved next to the job for step 4, which runs detached from the SSH session.
[ -f "$root/scripts/release-helper.sh" ] || fail "scripts/release-helper.sh is missing"
helper_program="$(cat "$root/scripts/release-helper.sh")"

ssh_opts=(-o BatchMode=yes -o ConnectTimeout=15 -o ServerAliveInterval=15 -o ServerAliveCountMax=4)
quoted() { local out='' word; for word in "$@"; do out+=" $(printf '%q' "$word")"; done; printf '%s' "$out"; }
remote() { ssh "${ssh_opts[@]}" "$target" "$@"; }
step() { local name="$1"; shift; remote "bash -s --$(quoted "$name" "$remote_dir" "$@")" <<<"$helper_program"; }

if [ "$check" = 1 ]; then
  echo "Checking $target:$remote_dir (HEAD here is $short, version $version)"
  step check || fail "the server is not ready for an update (see above)"
  echo "Check only: nothing was changed."
  exit 0
fi

echo "Deploying $short (version $version) to $target:$remote_dir"
step preflight || fail "the server is not ready for an update (see above); nothing was changed"

dump=''
if [ "$backup" = 1 ]; then
  out="$(step backup)"
  printf '%s\n' "$out" | grep -v '^backup='
  dump="$(printf '%s\n' "$out" | sed -n 's/^backup=//p')"
fi

# Unpack the commit as a new release, carrying over the .env and the published downloads.
id="$(date -u +%Y%m%dT%H%M%SZ)-$short"
out="$(step prepare "$id")" || fail "could not make the release folder on the server"
printf '%s\n' "$out" | sed '$d'
rel="$(printf '%s\n' "$out" | tail -1)"
discard() { step discard "$id" || true; }
git archive --format=tar "$commit" | remote "tar -x -C$(quoted "$rel")" || { discard; fail "could not send $short; nothing changed on the server"; }
step stage "$id" "$commit" "$tag" "$([ ${#release_files[@]} -gt 0 ] && echo 1 || echo 0)"
for file in ${release_files[@]+"${release_files[@]}"}; do
  remote "cat >$(quoted "$rel/dist/release/${file##*/}")" < "$file" || { discard; fail "could not send ${file##*/}; nothing changed on the server"; }
done
[ ${#release_files[@]} -eq 0 ] || echo "release files: ${release_files[*]##*/}"

echo "Building the image meeting-notes:$tag (the current server keeps running)…"
step build "$id" || { discard; fail "the image did not build; nothing changed on the server"; }

echo "Starting the new version…"
job="${rel%/*}/.jobs/$id"
remote "cat >$(quoted "$job/deploy.sh")" <<<"$helper_program" || { discard; fail "could not start the new version; nothing changed on the server"; }
step start "$id" || echo "(the connection dropped while starting; checking what the server did…)" >&2

# Follow the job. It carries on by itself on the server, so a dropped connection only means asking again.
shown=0 status='' last_answer=$SECONDS warned=0
while :; do
  if answer="$(step poll "$id" "$shown" 2>/dev/null)"; then
    last_answer=$SECONDS warned=0
    status="$(printf '%s\n' "$answer" | sed -n '1s/^status=//p')"
    lines="$(printf '%s\n' "$answer" | sed -n '2s/^lines=//p')"
    printf '%s\n' "$answer" | tail -n +3 | sed '/^$/d'
    shown="${lines:-$shown}"
    case "$status" in
      none) discard; fail "the new version was not started; nothing changed on the server" ;;
      starting|running) ;;
      *) break ;;
    esac
  else
    [ "$warned" = 1 ] || echo "(no answer from the server; the update carries on there, asking again…)" >&2
    warned=1
    [ $((SECONDS - last_answer)) -lt 900 ] || fail "lost the connection for 15 minutes. The update may still be running on the server; its log is $job/log, and --check shows what runs"
  fi
  [ "$SECONDS" -lt 3600 ] || fail "the update is still running on the server after an hour; its log is $job/log"
  sleep 3
done

restore_help() {
  echo "If the failed version had applied a database migration, restore the backup taken before it:" >&2
  echo "  ssh $target" >&2
  echo "  cd $remote_dir && docker compose stop app \\" >&2
  echo "    && docker compose exec -T db pg_restore -U meeting_notes -d meeting_notes --clean --if-exists < ${dump:-~/cereal-backups/<file>.dump} \\" >&2
  echo "    && docker compose up -d" >&2
  echo "The last command runs the migrate step of the version in $remote_dir again and then starts its server." >&2
}
case "$status" in
  deployed) ;;
  rolled-back)
    echo "The update failed and the previous version is running again. The failed release is kept in $rel." >&2
    restore_help; exit 1 ;;
  rollback-failed)
    echo "The update failed and the previous version did not come back either: the server is down." >&2
    echo "$remote_dir points at the previous release. The job log is $job/log." >&2
    restore_help; exit 1 ;;
  failed-no-previous)
    echo "The update failed and there was no previous release to go back to. The job log is $job/log." >&2
    restore_help; exit 1 ;;
  interrupted)
    fail "the update stopped on the server before it finished (was the server restarted?). Its log is $job/log; --check shows what runs" ;;
  *) fail "the update ended as '$status'. Its log is $job/log" ;;
esac

base="$(remote "sed -n 's/^PUBLIC_BASE_URL=//p' $(quoted "$rel/.env")" | tr -d '\r"' | sed 's:/*$::')"
if [ -n "$base" ]; then
  ready="$(curl -fsS --max-time 15 "$base/api/v1/ready" || true)"
  offered="$(curl -fsS --max-time 15 "$base/api/v1/downloads" | sed -n 's/.*"version":"\([^"]*\)".*/\1/p' || true)"
  echo "$base/api/v1/ready: ${ready:-no answer}"
  echo "Mac build offered: ${offered:-none}"
  [ "$ready" = '{"status":"ready"}' ] || fail "the server is healthy inside Docker but $base does not answer; check the reverse proxy"
fi
echo "Deployed $short as release $id. The previous release stays in ${rel%/*} until it is pruned."
