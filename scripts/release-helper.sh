#!/bin/bash
# The steps of a Cereal server update, run on the machine that runs the server with Docker Compose.
#
#   bash release-helper.sh <step> <deployment dir> [arguments]
#
# Two things drive it, in the same order and with the same rollback:
#   scripts/deploy-ssh.sh   from another machine: sends this file over SSH with every step;
#   scripts/updater.sh      on the server itself: the "Update server" action of the admin panel.
# scripts/deploy-ssh.sh describes the layout (<dir> is a link into <dir>-releases) and what each step is for.
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
  # 36 looks, 5 seconds apart: three minutes. (The two variables shorten the wait in tests.)
  for _ in $(seq 1 "${CEREAL_HEALTH_TRIES:-36}"); do
    cid="$(app_container)"
    if [ -n "$cid" ]; then
      status="$(docker inspect -f '{{.State.Health.Status}}' "$cid" 2>/dev/null || true)"
      image="$(docker inspect -f '{{.Image}}' "$cid" 2>/dev/null || true)"
      [ "$status" = healthy ] && { [ -z "$want" ] || [ "$image" = "$want" ]; } && return 0
    fi
    sleep "${CEREAL_HEALTH_SLEEP:-5}"
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
  mkdir -p "$rel/dist/release" "$rel/dist/updater"
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
