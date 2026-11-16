#!/bin/sh
# Prepares a Cereal server: makes the private .env and says what to open.
#
#   ./scripts/setup.sh                              this machine only, http://localhost:3000
#   ./scripts/setup.sh http://10.30.1.77:3000       a server other machines reach, over plain HTTP
#   ./scripts/setup.sh https://notes.example.com    a server behind a reverse proxy that provides HTTPS
#   ./scripts/setup.sh --dev                        also writes DATABASE_URL, for running the server outside Docker
#
# A new .env gets two random secrets (the database password and the setup code) and, when an address is given,
# the three lines that follow from it: PUBLIC_BASE_URL, BIND_ADDRESS and PORT. On an existing .env the secrets
# are never touched; an address updates only those three lines.
set -eu
cd "$(dirname "$0")/.."

fail() { printf 'setup.sh: %s\n' "$1" >&2; exit 2; }
address='' dev=''
for argument in "$@"; do
  case "$argument" in
    --dev) dev=1 ;;
    -h|--help) sed -n '2,11p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    -*) fail "unknown option $argument" ;;
    *) [ -z "$address" ] || fail 'give one address'; address=$argument ;;
  esac
done

# The address people will open: scheme, host and an optional port, nothing else.
scheme='' host='' port='' public=''
if [ -n "$address" ]; then
  case "$address" in
    http://*) scheme=http ;;
    https://*) scheme=https ;;
    *) fail "the address must start with http:// or https://, such as http://10.30.1.77:3000 (got: $address)" ;;
  esac
  rest=${address#*://}
  rest=${rest%/}
  case "$rest" in
    ''|*/*|*\?*|*\#*|*@*|*' '*|*'	'*) fail "the address is only a host and an optional port, such as http://10.30.1.77:3000 (got: $address)" ;;
  esac
  case "$rest" in
    \[*\]:*) host=${rest%:*}; port=${rest##*:} ;;
    \[*\]) host=$rest ;;
    \[*|*\]*) fail "the address is not a valid host (got: $address)" ;;
    *:*) host=${rest%:*}; port=${rest##*:} ;;
    *) host=$rest ;;
  esac
  case "$host" in
    \[*\]) inner=${host#\[}; inner=${inner%\]}
      case "$inner" in ''|*[!0-9A-Fa-f:]*) fail "the address is not a valid host (got: $address)" ;; esac ;;
    ''|.*|*.|-*|*..*|*[!A-Za-z0-9.-]*) fail "the address is not a valid host (got: $address)" ;;
  esac
  if [ -n "$port" ]; then
    case "$port" in *[!0-9]*|0*|??????*) fail "the port must be a number from 1 to 65535 (got: $port)" ;; esac
    [ "$port" -le 65535 ] || fail "the port must be a number from 1 to 65535 (got: $port)"
  else
    case "$rest" in *:) fail "the address ends with a colon but no port (got: $address)" ;; esac
  fi
  public="$scheme://$host${port:+:$port}"
fi

# Where Mac builds are published from (Compose mounts it into the server read-only).
mkdir -p dist/release
umask 077

# Replaces the line that sets $1 in .env, or adds it. Every other line stays exactly as it is.
set_line() {
  tmp=$(mktemp .env.XXXXXX)
  awk -v key="$1" -v value="$2" 'index($0, key "=") == 1 { if (!done) print key "=" value; done = 1; next } { print } END { if (!done) print key "=" value }' .env > "$tmp"
  cat "$tmp" > .env
  rm -f "$tmp"
}
value_of() { sed -n "s/^$1=//p" .env | head -n 1; }

created=''
if [ ! -e .env ]; then
  cat > .env <<EOF
POSTGRES_PASSWORD=$(od -An -N24 -tx1 /dev/urandom | tr -d ' \n')
ADMIN_API_TOKEN=$(od -An -N32 -tx1 /dev/urandom | tr -d ' \n')
EOF
  created=1
elif [ -z "$address" ] && [ -z "$dev" ]; then
  printf '.env already exists; leaving it unchanged. The setup code can be shown with: grep ADMIN_API_TOKEN .env\n'
  exit 0
fi
[ -n "$(value_of POSTGRES_PASSWORD)" ] && [ -n "$(value_of ADMIN_API_TOKEN)" ] || fail '.env has no POSTGRES_PASSWORD or ADMIN_API_TOKEN; move it aside and run this again to make a new one'

proxy=''
if [ -n "$public" ]; then
  if [ "$scheme" = https ]; then
    # HTTPS comes from a reverse proxy on this machine; Cereal itself listens only for it.
    bind=127.0.0.1
    listen=$(value_of PORT); listen=${listen:-3000}
    proxy=1
  else
    listen=${port:-80}
    case "$host" in
      localhost|127.*|\[::1\]) bind=127.0.0.1 ;;
      *) bind=0.0.0.0 ;;
    esac
  fi
  set_line PUBLIC_BASE_URL "$public"
  set_line BIND_ADDRESS "$bind"
  set_line PORT "$listen"
fi
if [ -n "$dev" ] && [ -z "$(value_of DATABASE_URL)" ]; then
  # Only for running the server outside Docker (bun run dev): Compose gives its own containers their database address.
  set_line DATABASE_URL "postgresql://meeting_notes:$(value_of POSTGRES_PASSWORD)@127.0.0.1:54329/meeting_notes?schema=public"
fi

# What to do next, with the code the first page asks for. The code is ADMIN_API_TOKEN: whoever has it can
# claim this server, so it is shown here to the person who ran this script.
open=${public:-$(value_of PUBLIC_BASE_URL)}
open=${open:-http://localhost:3000}
line() { printf '| %-70s |\n' "$1"; }
rule='+------------------------------------------------------------------------+'
printf '\n%s\n' "$rule"
if [ -n "$created" ]; then line 'Cereal is ready to start.'; else line 'Cereal will use this address after a restart.'; fi
line ''
line '1. Start it:    docker compose up --build -d'
line "2. Open:        $open"
line '3. Setup code:'
line "   $(value_of ADMIN_API_TOKEN)"
line ''
if [ -n "$proxy" ]; then
  line 'HTTPS comes from your reverse proxy (Caddy, nginx, a tunnel). Point it'
  line "at http://127.0.0.1:$listen; Cereal listens only on this machine."
  line ''
fi
line 'The first page asks for the setup code and how people will sign in'
line '(email and password, or Google), then walks you through the rest.'
line 'Show the code again with:  grep ADMIN_API_TOKEN .env'
printf '%s\n\n' "$rule"
