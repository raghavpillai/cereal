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

# Everything that runs on the server. It is sent with every call (`bash -s`), and saved next to the job
# for step 4, which runs detached from the SSH session.
# (Read with `read -d ''`, not `$(cat <<…)`: macOS's bash 3.2 misreads `case` patterns inside `$(…)`.)
IFS= read -r -d '' helper_program <<'REMOTE' || true
set -euo pipefail
expand() { case "$1" in '~') printf '%s' "$HOME" ;; '~/'*) printf '%s/%s' "$HOME" "${1#\~/}" ;; *) printf '%s' "$1" ;; esac; }
D="$(expand "$2")"; D="${D%/}"
R="$D-releases"; J="$R/.jobs"
fail() { printf '%s\n' "$1" >&2; exit 1; }
say() { printf '%s %s\n' "$(date -u +%H:%M:%S)" "$*"; }

# Points $D at a release in a single rename, so it is never missing (GNU mv -T on Linux, rename(2) elsewhere).
swap_link() {
  ln -sfn "${R##*/}/$1" "$D.swap"
  mv -Tf "$D.swap" "$D" 2>/dev/null || perl -e 'rename($ARGV[0], $ARGV[1]) or die "rename: $!\n"' "$D.swap" "$D"
}
current_id() { [ -L "$D" ] && basename "$(readlink "$D")" || true; }
image_of() { sed -n 's/^ *image: *meeting-notes:\(.*\)$/\1/p' "$1/compose.override.yaml" 2>/dev/null | head -1; }
write_override() {
  printf '%s\n' '# Written by scripts/deploy-ssh.sh: this release runs the image built from its own commit,' \
    '# so going back to it needs no rebuild.' 'services:' '  migrate:' "    image: meeting-notes:$2" \
    '  app:' "    image: meeting-notes:$2" > "$1/compose.override.yaml"
}
app_container() { docker compose ps -q app 2>/dev/null | head -1; }
# healthy <tag>: the app container that Compose runs here uses the image built for <tag>, and reports healthy.
healthy() {
  local cid status image want=''
  [ -z "$1" ] || want="$(docker image inspect -f '{{.Id}}' "meeting-notes:$1" 2>/dev/null || echo missing)"
  for _ in $(seq 1 36); do
    cid="$(app_container)"
    if [ -n "$cid" ]; then
      status="$(docker inspect -f '{{.State.Health.Status}}' "$cid" 2>/dev/null || true)"
      image="$(docker inspect -f '{{.Image}}' "$cid" 2>/dev/null || true)"
      [ "$status" = healthy ] && { [ -z "$want" ] || [ "$image" = "$want" ]; } && return 0
    fi
    sleep 5
  done
  return 1
}
# A job is alive while its process runs. Before it has written its process id, give it a minute.
job_alive() {
  if [ -f "$1/pid" ]; then kill -0 "$(cat "$1/pid")" 2>/dev/null
  else [ -z "$(find "$1/status" -mmin +1 2>/dev/null)" ]
  fi
}
job_status() {
  local s; s="$(cat "$1/status" 2>/dev/null || echo none)"
  case "$s" in
    starting|running)
      # Read it again after the check: the job may have written its last status and exited in between.
      job_alive "$1" || { s="$(cat "$1/status" 2>/dev/null || echo none)"; case "$s" in starting|running) s=interrupted ;; esac; } ;;
  esac
  printf '%s' "$s"
}

# Repairs what an interrupted run can leave behind. Only used when deploying; --check reports instead.
repair() {
  rm -f "$D.swap"
  if [ ! -e "$D" ] && [ ! -L "$D" ]; then
    if [ -f "$R/.moving" ] && [ -d "$R/$(cat "$R/.moving")" ]; then
      swap_link "$(cat "$R/.moving")"; echo "repaired: $D points at $(current_id) again (a move into $R had stopped halfway)"
    elif [ -d "$D.previous" ] && [ -f "$D.previous/.env" ]; then
      mv "$D.previous" "$D"; echo "repaired: put $D.previous back as $D (an earlier deploy had stopped between its two moves)"
    fi
  fi
  rm -f "$R/.moving"
}

describe() {
  if [ -L "$D" ]; then printf 'release: %s\n' "$(current_id)"
  elif [ -d "$D" ]; then printf 'layout: a plain folder; the next deploy moves it into %s and makes %s a link\n' "$R" "$D"
  elif [ -d "$D.previous" ]; then fail "no $D, but $D.previous exists: an earlier deploy stopped halfway. The next deploy puts it back"
  else fail "no deployment at $D"
  fi
  test -f "$D/.env" || fail "no .env in $D (run scripts/setup.sh there first)"
  docker compose version >/dev/null
  cd "$D"
  docker compose ps --status running --services | grep -x db >/dev/null || fail 'the database container is not running'
  printf 'running: %s\n' "$(cat DEPLOYED_COMMIT 2>/dev/null || echo unknown)"
  docker compose ps --format '{{.Service}}: {{.Status}}'
  df -h . | tail -1 | awk '{print "free on server: "$4}'
  local last; last="$(ls -1 "$J" 2>/dev/null | tail -1 || true)"
  [ -z "$last" ] || printf 'last deploy: %s (%s)\n' "$last" "$(job_status "$J/$last")"
}

busy() {
  local job
  for job in "$J"/*/; do
    [ -d "$job" ] || continue
    case "$(cat "$job/status" 2>/dev/null)" in
      starting|running) job_alive "$job" && fail "another deploy is still running on the server: ${job%/}/log" ;;
    esac
  done
  return 0
}

backup() {
  mkdir -p ~/cereal-backups; chmod 700 ~/cereal-backups
  local prev out
  prev="$(cut -c1-7 "$D/DEPLOYED_COMMIT" 2>/dev/null || echo unknown)"
  out=~/cereal-backups/"$(date -u +%Y%m%dT%H%M%SZ)-$prev.dump"
  cd "$D"
  docker compose exec -T db pg_dump -U meeting_notes -d meeting_notes -Fc > "$out.part"
  mv "$out.part" "$out"; chmod 600 "$out"
  ls -1t ~/cereal-backups/*.dump | tail -n +11 | xargs -r rm -f
  echo "database backup: $out ($(du -h "$out" | cut -f1))"
  echo "backup=$out"
}

# Moves a plain-folder deployment into $R and makes $D a link to it. The running containers are not touched.
adopt() {
  [ -d "$D" ] && [ ! -L "$D" ] || return 0
  mkdir -p "$J"
  local old id cid
  old="$(cut -c1-12 "$D/DEPLOYED_COMMIT" 2>/dev/null || true)"
  id="$(date -u +%Y%m%dT%H%M%SZ)-${old:0:7}"; [ -n "$old" ] || id="${id}unknown"
  id="$id-adopted"
  [ ! -e "$R/$id" ] || fail "$R/$id already exists"
  # Tag the image the server is running now, so that going back to this release needs no rebuild.
  cid="$(cd "$D" && app_container)"
  if [ -n "$old" ] && [ -n "$cid" ] && docker tag "$(docker inspect -f '{{.Image}}' "$cid")" "meeting-notes:$old"; then
    write_override "$D" "$old"
  fi
  printf '%s\n' "$id" > "$R/.moving"
  trap '' HUP INT TERM
  mv "$D" "$R/$id" && ln -s "${R##*/}/$id" "$D"
  trap - HUP INT TERM
  rm -f "$R/.moving"
  # The earlier version of this script kept these and deleted them at the next deploy.
  rm -rf "$D.previous" "$D.next"
  echo "moved the deployment into $R/$id; $D is now a link to it"
}

prepare() {  # prepare <id>: makes the empty release folder and prints its absolute path
  adopt
  [ ! -e "$R/$1" ] || fail "$R/$1 already exists"
  mkdir -p "$R/$1" "$J/$1"
  printf '%s\n' "$R/$1"
}

stage() {  # stage <id> <commit> <tag> <replace downloads 0|1>
  local rel="$R/$1"
  cp -p "$D/.env" "$rel/.env"
  mkdir -p "$rel/dist/release"
  if [ "$4" = 1 ]; then :; elif [ -d "$D/dist/release" ]; then cp -a "$D/dist/release/." "$rel/dist/release/"; fi
  printf '%s\n' "$2" > "$rel/DEPLOYED_COMMIT"
  write_override "$rel" "$3"
}

build() { cd "$R/$1"; docker compose build --quiet; }
discard() { rm -rf "${R:?}/$1" "${J:?}/$1"; }

start() {  # start <id>: launches activate, detached from this SSH session
  local job="$J/$1" prev
  prev="$(current_id)"
  printf '%s\n' "$prev" > "$job/previous"
  printf 'starting\n' > "$job/status"
  if command -v setsid >/dev/null; then
    nohup setsid bash "$job/deploy.sh" activate "$D" "$1" </dev/null >>"$job/log" 2>&1 &
  else
    nohup bash "$job/deploy.sh" activate "$D" "$1" </dev/null >>"$job/log" 2>&1 &
  fi
  # The job writes its own process id (setsid can fork, so $! is not always it).
}

prune() {  # keeps the running and previous releases and the three newest, and their images
  local keep=("$@") id old t used rel
  while IFS= read -r id; do keep+=("$id"); done < <(ls -1 "$R" | tail -3)
  for rel in "$R"/*/; do
    old="$(basename "$rel")"
    [[ " ${keep[*]} " == *" $old "* ]] && continue
    t="$(image_of "$rel")"; rm -rf "$rel" "$J/$old"
    [ -n "$t" ] || continue
    used=0; for id in "${keep[@]}"; do [ "$(image_of "$R/$id")" = "$t" ] && used=1; done
    [ "$used" = 1 ] || docker image rm "meeting-notes:$t" >/dev/null 2>&1 || true
  done
  local jobs=(); while IFS= read -r id; do jobs+=("$id"); done < <(ls -1 "$J")
  local i; for ((i = 0; i < ${#jobs[@]} - 10; i++)); do [ -d "$R/${jobs[i]}" ] || rm -rf "${J:?}/${jobs[i]}"; done
}

# Runs detached. Every failure after the switch ends in the rollback, and the status file says how it ended.
activate() {
  local id="$1" job="$J/$1" prev ptag tag
  trap '' HUP PIPE
  set +e
  printf '%s\n' "$$" > "$job/pid"
  prev="$(cat "$job/previous")"
  tag="$(image_of "$R/$id")"
  state() { printf '%s\n' "$1" > "$job/status.tmp" && mv -f "$job/status.tmp" "$job/status"; }
  logs() { say "Last log lines:"; docker compose logs --no-color --tail=60 migrate app 2>&1; }
  state running
  say "Pointing $D at $id and starting it (migrations run first)…"
  swap_link "$id" || { say "Could not switch $D."; state failed-no-switch; return 1; }
  cd "$D" || return 1
  if docker compose up -d --remove-orphans; then
    if healthy "$tag"; then
      say "The new version is healthy."
      prune "$id" "$prev"
      state deployed
      return 0
    fi
    say "The new version did not become healthy."
  else
    say "The new version could not be started (see above)."
  fi
  logs
  if [ -z "$prev" ] || [ ! -d "$R/$prev" ]; then
    say "There is no previous release to go back to."
    state failed-no-previous
    return 1
  fi
  say "Putting the previous release back ($prev)…"
  swap_link "$prev" || { say "Could not switch $D back."; state rollback-failed; return 1; }
  cd "$D" || return 1
  ptag="$(image_of "$R/$prev")"
  if ! docker image inspect "meeting-notes:${ptag:-local}" >/dev/null 2>&1; then
    say "Its image is gone; building it again…"
    docker compose build --quiet
  fi
  if ! docker compose up -d --remove-orphans; then
    say "The previous release could not be started normally (often its migrate step refuses a database that the"
    say "failed version migrated). Starting its server without the migrate step…"
    docker compose up -d --no-deps app
  fi
  if healthy "$ptag"; then
    say "The previous release is running again."
    state rolled-back
  else
    logs
    say "The previous release did not become healthy either."
    state rollback-failed
  fi
  return 1
}

poll() {  # poll <id> <lines already shown>: status, the log's line count, then the new log lines
  local job="$J/$1" n
  printf 'status=%s\n' "$(job_status "$job")"
  n="$(wc -l < "$job/log" 2>/dev/null || echo 0)"; n="${n// /}"
  printf 'lines=%s\n' "$n"
  [ "$n" -le "$2" ] || tail -n "+$(($2 + 1))" "$job/log" | head -n "$((n - $2))"
}

main() {
  exec </dev/null
  local command="$1"; shift 2
  case "$command" in
    check) describe ;;
    preflight) repair; busy; describe ;;
    backup|prepare|stage|build|discard|start|poll|activate) "$command" "$@" ;;
    *) fail "unknown step $command" ;;
  esac
}
main "$@"; exit $?
REMOTE

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
