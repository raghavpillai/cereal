#!/bin/bash
# A stand-in for `docker` in updates.test.ts: it answers the calls scripts/release-helper.sh makes and keeps
# what "runs" in a folder ($FAKE_DOCKER). Nothing here reaches a Docker daemon.
#
# What a release does is decided by marker files in it (the directory Compose is run in):
#   BUILD_FAILS   `docker compose build` fails
#   START_FAILS   `docker compose up` fails (a migration that does not apply)
#   UNHEALTHY     the server starts but never reports healthy
S="${FAKE_DOCKER:?}"
printf '%s | %s\n' "$PWD" "$*" >> "$S/calls"
tag_here() { local t; t="$(sed -n 's/^ *image: *meeting-notes:\(.*\)$/\1/p' compose.override.yaml 2>/dev/null | head -1)"; printf '%s' "${t:-local}"; }
case "$1 ${2:-}" in
  "compose version") echo "Docker Compose version v0.0.0-fake" ;;
  "compose ps")
    case "$*" in
      *--services*) echo db; [ ! -f "$S/running" ] || echo app ;;
      *"-q app"*) [ ! -f "$S/running" ] || echo fake-app-container ;;
      *) echo "db: Up 3 days (healthy)"; echo "app: Up 3 days ($(cat "$S/health"))" ;;
    esac ;;
  "compose exec") echo "FAKE DATABASE DUMP while running $(cat "$S/running")" ;;
  "compose build")
    sleep "${FAKE_DOCKER_BUILD_SECONDS:-0}"
    [ ! -f BUILD_FAILS ] || { echo "ERROR: failed to solve: this release does not build" >&2; exit 1; }
    touch "$S/images/$(tag_here)" ;;
  "compose up")
    [ ! -f START_FAILS ] || { echo "service \"migrate\" didn't complete successfully: exit 1" >&2; exit 1; }
    tag_here > "$S/running"
    if [ -f UNHEALTHY ]; then echo unhealthy > "$S/health"; else echo healthy > "$S/health"; fi
    # A harness that runs a real server process in place of the container restarts it here.
    [ ! -x "$S/on-up" ] || "$S/on-up" ;;
  "compose logs") echo "app-1  | a line from the server's log" ;;
  "inspect -f") case "$3" in *Health*) cat "$S/health" ;; *Image*) echo "sha256:$(cat "$S/running")" ;; *) exit 1 ;; esac ;;
  "image inspect") name="${*: -1}"; [ -f "$S/images/${name#meeting-notes:}" ] || exit 1; echo "sha256:${name#meeting-notes:}" ;;
  "image rm") rm -f "$S/images/${3#meeting-notes:}" ;;
  "tag "*) touch "$S/images/${3#meeting-notes:}" ;;
  *) echo "fake docker: unexpected call: $*" >&2; exit 97 ;;
esac
