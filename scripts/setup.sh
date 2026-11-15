#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
# Where Mac builds are published from (Compose mounts it into the server read-only).
mkdir -p dist/release
if [ -e .env ]; then
  printf '.env already exists; leaving it unchanged. The setup code can be shown with: grep ADMIN_API_TOKEN .env\n'
  exit 0
fi
umask 077
meeting_db_secret=$(od -An -N24 -tx1 /dev/urandom | tr -d ' \n')
meeting_api_secret=$(od -An -N32 -tx1 /dev/urandom | tr -d ' \n')
cat > .env <<EOF
POSTGRES_PASSWORD=$meeting_db_secret
ADMIN_API_TOKEN=$meeting_api_secret
DATABASE_URL=postgresql://meeting_notes:$meeting_db_secret@127.0.0.1:54329/meeting_notes?schema=public
PORT=3000
BIND_ADDRESS=127.0.0.1
EOF
# What to do next, with the code the first page asks for. The code is ADMIN_API_TOKEN: whoever has it can
# claim this server, so it is shown here, once, to the person who ran this script.
line() { printf '| %-70s |\n' "$1"; }
rule='+------------------------------------------------------------------------+'
printf '\n%s\n' "$rule"
line 'Cereal is ready to start.'
line ''
line '1. Start it:    docker compose up --build -d'
line '2. Open:        http://localhost:3000'
line '3. Setup code:'
line "   $meeting_api_secret"
line ''
line 'The page asks for the setup code, then walks you through the rest.'
line 'Show the code again with:  grep ADMIN_API_TOKEN .env'
printf '%s\n\n' "$rule"
