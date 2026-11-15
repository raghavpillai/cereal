#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
if [ -e .env ]; then
  printf '.env already exists; leaving it unchanged.\n'
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
printf 'Created private .env. Start with docker compose up --build -d.\n'
