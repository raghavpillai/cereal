#!/bin/bash
# Cereal's updater: the part of "Update server" (admin panel → Updates) that runs on the machine itself.
#
#   ./scripts/updater.sh install <git remote> [ref] [--dir <deployment>] [--ssh-key <file>]
#   ./scripts/updater.sh run     [--dir <deployment>]     the service: waits for requests from the admin panel
#   ./scripts/updater.sh check   [--dir <deployment>]     look for a newer version once, from the shell
#   ./scripts/updater.sh update  [--dir <deployment>]     update once, from the shell
#   ./scripts/updater.sh status  [--dir <deployment>]     what it knows: version, what is available, last update
#
# Why it exists. Updating means building an image and restarting containers, which takes the Docker socket.
# The server's own container never gets that socket. Instead this script runs on the host, as the user who
# runs `docker compose` there, and the server can only ask it for two things: "look for a newer version" and
# "update to it". Where the version comes from (the git remote and ref) is written here, on the host, by
# `install`; a request cannot name another source, a commit or a command.
#
# Where a newer version comes from. The source is a private repository, so it is a git remote this machine
# can read: either the repository itself with a read-only deploy key (git@github.com:you/cereal.git and
# --ssh-key), or a bare repository on this machine that you push to (`git init --bare ~/cereal.git`, then
# `git push server:cereal.git main` from your own machine). "Newer" means: the commit <ref> names there is
# not the commit this server runs. The version number shown beside it is `version` in that commit's
# package.json.
#
# How the server asks. <deployment>-releases/.updater/spool is mounted into the server's container
# (compose.yaml, UPDATER_DIR). The server drops a small request file in spool/requests; this script answers
# by writing spool/state, spool/job and spool/log, which the server only reads. A request carries a
# SHA-256 over the shared secret (UPDATER_TOKEN in .env, TOKEN in .updater/config), a rising timestamp and
# the action, so a file from anyone else on the machine, or a copy of an old one, is thrown away.
#
# What an update does: the steps of scripts/release-helper.sh, the same ones scripts/deploy-ssh.sh runs over
# SSH: database dump, new release folder, image build while the old server answers, switch, migrations,
# wait for healthy, and back to the previous release if any of that fails.
set -u
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
helper_file="$here/release-helper.sh"

usage() { sed -n '4,8p' "$0" | sed 's/^# \{0,1\}//' >&2; exit 2; }
fail() { printf 'updater: %s\n' "$1" >&2; exit 1; }
command="${1:-}"; [ $# -gt 0 ] && shift
D='' remote_arg='' ref_arg='' key_arg=''
while [ $# -gt 0 ]; do
  case "$1" in
    --dir) D="${2:?--dir needs a path}"; shift ;;
    --ssh-key) key_arg="${2:?--ssh-key needs a file}"; shift ;;
    -h|--help) usage ;;
    -*) usage ;;
    *) if [ -z "$remote_arg" ]; then remote_arg="$1"; elif [ -z "$ref_arg" ]; then ref_arg="$1"; else usage; fi ;;
  esac
  shift
done
# The deployment is the folder this script is in, by the name it was reached through (so a link stays a link).
[ -n "$D" ] || D="$(cd "$here/.." && pwd -L)"
case "$D" in '~') D="$HOME" ;; '~/'*) D="$HOME/${D#\~/}" ;; esac
D="${D%/}"
R="$D-releases"; H="$R/.updater"; S="$H/spool"; M="$H/source.git"
POLL="${CEREAL_UPDATER_POLL:-2}"
CHECK_EVERY="${CEREAL_UPDATER_CHECK_EVERY:-21600}"

now() { date -u +%s; }
one_line() { local v="${1//$'\n'/ }"; v="${v//$'\r'/ }"; printf '%s' "${v:0:400}"; }
digest() { if command -v sha256sum >/dev/null 2>&1; then printf '%s' "$1" | sha256sum | cut -c1-64; else printf '%s' "$1" | shasum -a 256 | cut -c1-64; fi; }
config() { sed -n "s/^$1=//p" "$H/config" 2>/dev/null | head -1; }
version_in() { sed -n 's/^ *"version": *"\([^"]*\)".*/\1/p' | head -1; }
log() { printf '%s %s\n' "$(date -u +%H:%M:%S)" "$*" >> "$S/log"; }
log_lines() { local line; while IFS= read -r line; do [ -z "$line" ] || log "$line"; done; }
helper() { bash "$helper_file" "$@"; }
git_source() { GIT_TERMINAL_PROMPT=0 git --git-dir="$M" "$@"; }

load_config() {
  [ -f "$H/config" ] || fail "not installed for $D. Run: $0 install <git remote> [ref] --dir $D"
  REMOTE="$(config REMOTE)"; REF="$(config REF)"; TOKEN="$(config TOKEN)"; SSH_KEY="$(config SSH_KEY)"
  [ -n "$REMOTE" ] && [ -n "$REF" ] && [ "${#TOKEN}" -ge 32 ] || fail "$H/config is incomplete; run install again"
  if [ -n "$SSH_KEY" ]; then export GIT_SSH_COMMAND="ssh -i $SSH_KEY -o IdentitiesOnly=yes -o BatchMode=yes -o StrictHostKeyChecking=accept-new"; fi
  # What the admin panel shows as the source: never a password or token that is part of the address.
  REMOTE_SHOWN="$(printf '%s' "$REMOTE" | sed 's#://[^/@]*@#://#')"
}

# ── What the server reads ────────────────────────────────────────────────────────────────────────────────
check_at='' check_commit='' check_version='' check_relation='' check_error=''
job_id='' job_status='' job_step='' job_started='' job_finished='' job_from_commit='' job_from_version='' job_to_commit='' job_to_version='' job_error='' job_helper_lines=0
put() { printf '%s=%s\n' "$1" "$(one_line "$2")"; }
current() {
  current_commit="$(head -c 64 "$D/DEPLOYED_COMMIT" 2>/dev/null | tr -cd '0-9a-f')"
  current_version="$(version_in < "$D/package.json" 2>/dev/null)"
}
write_state() {
  current
  { put protocol 1; put remote "$REMOTE_SHOWN"; put ref "$REF"; put current_commit "$current_commit"; put current_version "$current_version"
    put check_at "$check_at"; put check_commit "$check_commit"; put check_version "$check_version"; put check_relation "$check_relation"; put check_error "$check_error"
  } > "$S/state.tmp" && chmod 644 "$S/state.tmp" && mv -f "$S/state.tmp" "$S/state"
}
write_job() {
  { put id "$job_id"; put status "$job_status"; put step "$job_step"; put started "$job_started"; put finished "$job_finished"
    put from_commit "$job_from_commit"; put from_version "$job_from_version"; put to_commit "$job_to_commit"; put to_version "$job_to_version"
    put error "$job_error"; put helper_lines "$job_helper_lines"
  } > "$S/job.tmp" && chmod 644 "$S/job.tmp" && mv -f "$S/job.tmp" "$S/job"
}
read_saved() {
  local key
  for key in check_at check_commit check_version check_relation check_error; do eval "$key=\"\$(sed -n 's/^$key=//p' \"\$S/state\" 2>/dev/null | head -1)\""; done
  for key in id status step started finished from_commit from_version to_commit to_version error helper_lines; do eval "job_$key=\"\$(sed -n 's/^$key=//p' \"\$S/job\" 2>/dev/null | head -1)\""; done
  case "$job_helper_lines" in ''|*[!0-9]*) job_helper_lines=0 ;; esac
}
step() { job_step="$1"; write_job; }
# How a job ended. Requests left from while it ran are dropped first (the server refuses to make any during an
# update, so these are strays); one made after this is written is a new request and is kept.
finish() { rm -f "$S/requests"/* 2>/dev/null; job_status="$1"; job_error="${2:-}"; job_finished="$(now)"; write_job; }

# ── Looking for a newer version ──────────────────────────────────────────────────────────────────────────
# Fetches <ref> into the local copy and compares it with what runs. Sets check_*; returns 1 when it could not look.
look() {
  local out commit
  check_at="$(now)"; check_error=''
  if ! out="$(git_source fetch --quiet --force --no-tags "$REMOTE" "$REF" 2>&1)"; then
    check_error="Could not fetch $REF from $REMOTE_SHOWN: $(one_line "$out")"; write_state; return 1
  fi
  if ! commit="$(git_source rev-parse --verify --quiet 'FETCH_HEAD^{commit}')"; then
    check_error="$REF in $REMOTE_SHOWN is not a commit"; write_state; return 1
  fi
  git_source update-ref refs/updates/target "$commit"
  check_commit="$commit"
  check_version="$(git_source show "$commit:package.json" 2>/dev/null | version_in)"
  current
  if [ -z "$current_commit" ]; then check_relation=unknown
  elif [ "$commit" = "$current_commit" ]; then check_relation=same
  elif ! git_source cat-file -e "$current_commit^{commit}" 2>/dev/null; then check_relation=different
  elif git_source merge-base --is-ancestor "$current_commit" "$commit"; then check_relation=newer
  elif git_source merge-base --is-ancestor "$commit" "$current_commit"; then check_relation=older
  else check_relation=different
  fi
  [ -n "$check_version" ] || { check_error="$REF has no version in its package.json"; write_state; return 1; }
  write_state
}

# ── Updating ─────────────────────────────────────────────────────────────────────────────────────────────
# Follows the detached job that switches releases (release-helper.sh `activate`) until it ends, copying its
# log. It carries on by itself, so this can be picked up again after the updater was restarted.
follow() {
  local id="$1" answer status='' lines began; began="$(now)"
  while :; do
    if answer="$(helper poll "$D" "$id" "$job_helper_lines" 2>/dev/null)"; then
      status="$(printf '%s\n' "$answer" | sed -n '1s/^status=//p')"
      lines="$(printf '%s\n' "$answer" | sed -n '2s/^lines=//p')"
      printf '%s\n' "$answer" | tail -n +3 | sed '/^$/d' >> "$S/log"
      case "$answer" in *'Putting the previous release back'*) job_step=rollback ;; esac
      job_helper_lines="${lines:-$job_helper_lines}"; write_job
      case "$status" in starting|running) ;; *) break ;; esac
    fi
    [ $(( $(now) - began )) -lt 3600 ] || { status=interrupted; break; }
    sleep "$POLL"
  done
  case "$status" in
    deployed) log "Updated to ${job_to_version:-?} (${job_to_commit:0:7})."; finish deployed ;;
    rolled-back) finish rolled-back "The new version did not start. The previous version is running again." ;;
    rollback-failed) finish rollback-failed "The new version did not start and the previous version did not come back: the server is down. See $R/.jobs/$id/log on the server." ;;
    failed-no-previous) finish failed-no-previous "The new version did not start and there was no previous release to go back to." ;;
    none) finish failed "The new version was not started. Nothing changed." ;;
    *) finish interrupted "The update stopped before it finished (was the machine restarted?). See $R/.jobs/$id/log on the server." ;;
  esac
}

update() {
  local out rel id short dump=''
  current
  job_id="$(date -u +%Y%m%dT%H%M%SZ)"; job_status=running; job_started="$(now)"; job_finished=''; job_error=''; job_helper_lines=0
  job_from_commit="$current_commit"; job_from_version="$current_version"; job_to_commit=''; job_to_version=''
  : > "$S/log"; step fetch
  log "Looking for $REF in $REMOTE_SHOWN…"
  if ! look; then log "$check_error"; finish failed "$check_error Nothing changed."; return 1; fi
  job_to_commit="$check_commit"; job_to_version="$check_version"; short="${check_commit:0:7}"
  if [ "$check_relation" = same ]; then log "This server already runs $short (version $check_version)."; finish up-to-date; return 0; fi
  if [ "$check_relation" = older ]; then
    log "$REF ($short) is older than what this server runs. Database migrations only go forward, so it is not installed."
    finish failed "$REF is older than what this server runs. Nothing changed."; return 1
  fi
  log "Found version $check_version ($short). This server runs ${current_version:-an unknown version} (${current_commit:0:7})."
  id="$job_id-$short"; job_id="$id"

  step preflight
  log "Checking the server…"
  out="$(helper preflight "$D" 2>&1)" || { printf '%s\n' "$out" | log_lines; finish failed "The server is not ready for an update. Nothing changed."; return 1; }
  printf '%s\n' "$out" | log_lines

  step backup
  log "Backing up the database…"
  out="$(helper backup "$D" 2>&1)" || { printf '%s\n' "$out" | log_lines; finish failed "The database backup failed. Nothing changed."; return 1; }
  dump="$(printf '%s\n' "$out" | sed -n 's/^backup=//p')"
  printf '%s\n' "$out" | grep -v '^backup=' | log_lines

  step build
  log "Unpacking $short as a new release…"
  out="$(helper prepare "$D" "$id" 2>&1)" || { printf '%s\n' "$out" | log_lines; finish failed "Could not make the release folder. Nothing changed."; return 1; }
  rel="$(printf '%s\n' "$out" | tail -1)"
  printf '%s\n' "$out" | sed '$d' | log_lines
  discard() { helper discard "$D" "$id" >/dev/null 2>&1 || true; }
  if ! out="$( { set -o pipefail; git_source archive --format=tar "$check_commit" | tar -x -C "$rel"; } 2>&1 && helper stage "$D" "$id" "$check_commit" "${check_commit:0:12}" 0 2>&1)"; then
    printf '%s\n' "$out" | log_lines; discard; finish failed "Could not unpack $short. Nothing changed."; return 1
  fi
  log "Building the image meeting-notes:${check_commit:0:12} (the current server keeps running)…"
  if ! out="$(helper build "$D" "$id" 2>&1)"; then
    printf '%s\n' "$out" | tail -40 | log_lines; discard; finish failed "The image did not build. Nothing changed."; return 1
  fi

  step start
  log "Starting the new version (the server restarts now)…"
  if ! cp "$helper_file" "$R/.jobs/$id/deploy.sh" || ! out="$(helper start "$D" "$id" 2>&1)"; then
    printf '%s\n' "${out:-}" | log_lines; discard; finish failed "Could not start the new version. Nothing changed."; return 1
  fi
  follow "$id"
  [ -z "$dump" ] || [ "$job_status" = deployed ] || log "The database backup taken before this update: $dump. scripts/deploy-ssh.sh prints how to restore it."
  [ "$job_status" = deployed ]
}

# ── Requests from the server ─────────────────────────────────────────────────────────────────────────────
# One request file: three lines, a timestamp in milliseconds, the action, and SHA-256("<token>:<timestamp>:<action>").
# Prints the action when the file is genuine and newer than the last one accepted.
accept() {
  local file="$1" stamp action mac last age
  [ -f "$file" ] && [ ! -L "$file" ] || return 1
  { IFS= read -r stamp; IFS= read -r action; IFS= read -r mac; } < <(head -c 256 "$file" 2>/dev/null)
  case "$stamp" in ''|*[!0-9]*) return 1 ;; esac
  [ "${#stamp}" -eq 13 ] || return 1
  case "$action" in check|update) ;; *) return 1 ;; esac
  [ "$mac" = "$(digest "$TOKEN:$stamp:$action")" ] || return 1
  last="$(cat "$H/last-request" 2>/dev/null || echo 0)"
  case "$last" in ''|*[!0-9]*) last=0 ;; esac
  [ "$stamp" -gt "$last" ] || return 1
  age=$(( $(now) - stamp / 1000 )); [ "$age" -lt 600 ] && [ "$age" -gt -600 ] || return 1
  printf '%s\n' "$stamp" > "$H/last-request"
  printf '%s' "$action"
}
requests() {
  local file action wanted=''
  for file in "$S/requests"/*.req; do
    [ -e "$file" ] || [ -L "$file" ] || continue
    action="$(accept "$file")" || action=''
    rm -f "$file"
    case "$action" in update) wanted=update ;; check) [ -n "$wanted" ] || wanted=check ;; esac
  done
  printf '%s' "$wanted"
}

prepare_spool() {
  mkdir -p "$S/requests" "$R/.jobs"
  chmod 700 "$H"; chmod 755 "$S"
  # The server's container runs as its own user, which is not this one: anyone may drop a file here, and only
  # a signed one counts. The sticky bit keeps them from removing each other's.
  chmod 1733 "$S/requests"
  # The server's container reads these as another user. They hold progress and versions, never a secret.
  [ -f "$S/log" ] || : > "$S/log"
  chmod 644 "$S/log"; [ ! -f "$S/state" ] || chmod 644 "$S/state"; [ ! -f "$S/job" ] || chmod 644 "$S/job"
  [ -d "$M" ] || git init --quiet --bare "$M"
}
running_pid() { local pid; pid="$(cat "$H/pid" 2>/dev/null)"; [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null && printf '%s' "$pid"; }

case "$command" in
  install)
    [ -n "$remote_arg" ] || usage
    [ -f "$D/.env" ] && [ -f "$D/compose.yaml" ] || fail "$D is not a Cereal deployment (no .env and compose.yaml there). Give it with --dir."
    [ -f "$helper_file" ] || fail "scripts/release-helper.sh is missing beside this script"
    command -v git >/dev/null || fail "git is not installed on this machine"
    command -v docker >/dev/null || echo "Note: docker is not on this user's PATH; the updater needs it to run." >&2
    [ -z "$key_arg" ] || [ -f "$key_arg" ] || fail "no such key file: $key_arg"
    case "$remote_arg$ref_arg$key_arg" in *$'\n'*) fail "the remote, ref and key must each be one line" ;; esac
    umask 077
    mkdir -p "$H"
    token="$(config TOKEN)"; [ "${#token}" -ge 32 ] || token="$(od -An -N32 -tx1 /dev/urandom | tr -d ' \n')"
    printf 'REMOTE=%s\nREF=%s\nTOKEN=%s\nSSH_KEY=%s\n' "$remote_arg" "${ref_arg:-main}" "$token" "$key_arg" > "$H/config"
    load_config; prepare_spool
    # The two lines the server's container needs, in the deployment's private .env. Every other line stays.
    set_line() {
      awk -v key="$1" -v value="$2" 'index($0, key "=") == 1 { if (!done) print key "=" value; done = 1; next } { print } END { if (!done) print key "=" value }' "$D/.env" > "$H/env.tmp" \
        && cat "$H/env.tmp" > "$D/.env"; rm -f "$H/env.tmp"
    }
    set_line UPDATER_DIR "$S"; set_line UPDATER_TOKEN "$token"
    read_saved
    if look; then echo "Source: $REF in $REMOTE_SHOWN is version $check_version (${check_commit:0:7})."
    else echo "Warning: $check_error" >&2; echo "The updater is installed, but it cannot read the source yet. Fix access and run: $0 check --dir $D" >&2; fi
    unit="$H/cereal-updater.service"
    printf '%s\n' '[Unit]' 'Description=Cereal updater (the admin panel'"'"'s "Update server")' 'After=docker.service' '' '[Service]' \
      "User=$(id -un)" "ExecStart=/bin/bash $D/scripts/updater.sh run --dir $D" 'Restart=always' 'RestartSec=5' \
      '# A running update carries on if this service is restarted.' 'KillMode=process' '' '[Install]' 'WantedBy=multi-user.target' > "$unit"
    cat <<EOF

The updater is set up for $D. Two things are left:

1. Keep it running. With systemd:
     sudo cp $unit /etc/systemd/system/cereal-updater.service
     sudo systemctl daemon-reload && sudo systemctl enable --now cereal-updater
   Without systemd, run this under whatever keeps a program alive:
     $D/scripts/updater.sh run --dir $D

2. Restart the server once, so that it sees the updater:
     cd $D && docker compose up -d

Then open the admin panel → Updates.
EOF
    ;;
  status)
    load_config; current
    echo "deployment: $D"
    echo "runs: version ${current_version:-unknown} (${current_commit:0:7})"
    echo "source: $REF in $REMOTE_SHOWN"
    if pid="$(running_pid)"; then echo "service: running (process $pid)"; else echo "service: not running"; fi
    [ ! -f "$S/state" ] || sed -n 's/^check_/last check: /p' "$S/state"
    [ ! -f "$S/job" ] || grep -E '^(id|status|error)=' "$S/job" | sed 's/^/last update: /'
    ;;
  check|update)
    load_config; prepare_spool
    if pid="$(running_pid)"; then fail "the updater service is running (process $pid); use the admin panel, or stop the service first"; fi
    printf '%s\n' "$$" > "$H/pid"; trap 'rm -f "$H/pid"' EXIT
    read_saved
    if [ "$command" = check ]; then
      look || fail "$check_error"
      echo "$REF is version $check_version (${check_commit:0:7}): $check_relation. This server runs ${current_version:-unknown} (${current_commit:0:7})."
    else
      update; result=$?
      cat "$S/log"; echo "Result: $job_status${job_error:+ ($job_error)}"
      exit "$result"
    fi
    ;;
  run)
    load_config; prepare_spool
    if pid="$(running_pid)" && [ "$pid" != "$$" ]; then fail "already running (process $pid)"; fi
    printf '%s\n' "$$" > "$H/pid"
    read_saved; write_state
    # The server tells a running updater from a stopped one by how recently this file was touched.
    ( trap 'exit 0' TERM; while :; do touch "$S/heartbeat"; sleep "$POLL"; done ) &
    beat=$!
    trap 'kill "$beat" 2>/dev/null; rm -f "$H/pid"; exit 0' INT TERM
    trap 'kill "$beat" 2>/dev/null' EXIT
    # An update that was under way when this service last stopped: its switch runs detached, so see how it ended.
    if [ "$job_status" = running ]; then
      if [ "$job_step" = start ] || [ "$job_step" = rollback ]; then follow "$job_id"
      else finish interrupted "The updater was stopped before the new version was started. Nothing changed."; fi
      write_state
    fi
    last_look=0
    while :; do
      wanted="$(requests)"
      if [ -z "$wanted" ] && [ $(( $(now) - last_look )) -ge "$CHECK_EVERY" ]; then wanted=check; fi
      case "$wanted" in
        check) look || true; last_look="$(now)" ;;
        update)
          update || true
          last_look="$(now)"; check_relation=''; look || true
          # The release that now runs may carry a newer updater: carry on as that one.
          if [ "$job_status" = deployed ] && [ -f "$D/scripts/updater.sh" ]; then
            kill "$beat" 2>/dev/null; trap - EXIT
            exec /bin/bash "$D/scripts/updater.sh" run --dir "$D"
          fi ;;
      esac
      sleep "$POLL"
    done
    ;;
  *) usage ;;
esac
