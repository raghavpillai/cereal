# Cereal

Self-hosted meeting notes. Cereal records a meeting, shows the transcript while people are still talking, lets you ask questions about it as it happens, and writes the notes afterwards. Everything is stored on a server you run.

- **Desktop app** (macOS): records microphone and system audio, live transcript, chat, notes.
- **iPhone app**: records, live transcript, chat, notes.
- **Web app**: read your notes and shared notes, manage your account, and administer the instance.
- **Server**: API, transcription and note generation, with PostgreSQL for storage.

## Run the server

Install Docker with Compose, then:

```sh
./scripts/setup.sh                              # on this machine: http://localhost:3000
docker compose up --build -d
```

For a server other machines reach, give `setup.sh` the address people will open:

```sh
./scripts/setup.sh http://10.30.1.77:3000       # plain HTTP on a private network
./scripts/setup.sh https://notes.example.com    # behind a reverse proxy that provides HTTPS
```

`setup.sh` creates a private `.env`, never replaces its secrets, and prints the address to open and a **setup code**. Show the code again with `grep ADMIN_API_TOKEN .env`. Run it again with a different address to move the server; only the address lines change. This is everything a deployed `.env` contains:

```sh
POSTGRES_PASSWORD=…                       # random, made by setup.sh
ADMIN_API_TOKEN=…                         # random, made by setup.sh: the setup code, and the owner's bearer token
PUBLIC_BASE_URL=http://10.30.1.77:3000    # the three address lines are written only when an address is given
BIND_ADDRESS=0.0.0.0
PORT=3000
```

Plain HTTP works on any address. On a private network (`10.x`, `192.168.x`, `.local` and the like) the admin panel notes it in one line; on a public address the sign-in page and the admin panel say that passwords and notes travel unencrypted and that the server belongs behind HTTPS. Three things need an HTTPS address (or `localhost`) because the other side requires it, and each says so where you would turn it on: Google sign-in, the Google Calendar connection, and AI app connections (MCP). For HTTPS, run a reverse proxy (Caddy, nginx, a tunnel) on the same machine and point it at `http://127.0.0.1:3000`.

Open the address. A new server has no accounts yet, so every page leads to setup, which walks through these steps one at a time:

1. **Set up sign-in**: paste the setup code and choose the one way people will sign in to this server. It is one or the other, never both.
   - **Email and password**: enter your name, email and a password of at least 12 characters. You become the owner, and you make an account for each person afterwards.
   - **Google**: create an OAuth client of type "Web application" in Google Cloud, add the two redirect addresses the step shows, and enter its client ID and secret. Continuing signs you in with Google once: that proves the client works, the Google account you use becomes the owner, and nobody gets a password. If Google refuses (a wrong secret, a redirect address that is not registered, a cancelled sign-in) the step says which, and nothing is saved. Google needs an HTTPS address or `localhost`.
2. **Name your space**: the team name and, if you like, a logo.
3. **Notes and chat**: pick OpenRouter, OpenAI or Anthropic and paste its API key. The key is checked as you paste it.
4. **Speech to text**: paste an ElevenLabs API key.
5. **Models**: the models for chat, meeting notes, speech-to-text and live transcription, already chosen for the keys you added.
6. **Who may join** (Google servers only, optional): the addresses and domains whose Google accounts get an account the first time they sign in. Your own address is always allowed. The same client is used for the Google Calendar connection.
7. **Where files live** (optional): recorded audio and the app builds this server offers are kept on the server's disk, or in an S3-compatible bucket (Amazon S3, Cloudflare R2, MinIO, Backblaze B2). **Test connection** tries the bucket before anything is saved. New files go to the storage you choose; files already stored stay where they are and keep working, and nothing is moved.
8. **Get the apps**: the Mac download and the address to type into the apps.

Setup can be left and picked up again: it resumes at the first unfinished step. Until it is finished, `/admin` shows only setup; afterwards it is the admin panel, where every one of these settings can be changed. The sign-in method is changed there by proving the new one first (signing in with Google once, or setting your own password), so a switch cannot lock you out, along with live transcription limits and the iPhone app's identifiers. The owner can go through setup again from the link at the bottom of the panel; nothing that was configured is lost. Keys and secrets are encrypted and stored on your server.

All of this is kept in the database, and a change applies without a restart. `.env` holds only what the server needs before it can read its settings: the database password, the owner token and, off `localhost`, the address. If sign-in is ever misconfigured, `ADMIN_API_TOKEN` from `.env` still works as a bearer token for `PATCH /api/v1/admin/settings`.

Upgrading from a version that kept provider keys or model names in `.env`: start the new version once. It takes those values into the database, logs each one by name, and never reads them again; the admin page then lists the variables you can delete. A server that already had a provider key, stored or in `.env`, counts as set up and opens on the admin panel as before.

```sh
docker compose logs --tail=100 app
docker compose down
```

`down` keeps your notes and audio. Back up the PostgreSQL volume and the `audio-data` volume together.

AI clients can connect through the OAuth-protected MCP endpoint at `/api/mcp`; the page at `/mcp/connect` has the setup steps. Clients set up with the earlier address, `/mcp`, keep working.

## Update a server

On the server itself, from a git checkout:

```sh
git pull
docker compose up --build -d                    # applies database migrations before the server starts
docker compose ps                               # app should read "healthy" within a minute
```

For a server you reach over SSH, `scripts/deploy-ssh.sh` sends a commit from your machine instead, so the server needs no git checkout and nothing uncommitted is sent:

```sh
./scripts/deploy-ssh.sh ubuntu@notes.example.com --check       # what it runs now; changes nothing
./scripts/deploy-ssh.sh ubuntu@notes.example.com main          # deploy main
./scripts/deploy-ssh.sh ubuntu@notes.example.com main --release dist/release   # and publish a Mac build
```

It dumps the database to `~/cereal-backups` first, builds the new image while the old server keeps answering, swaps in the new copy, and waits for it to report healthy. If it does not, it puts the previous copy back and prints the command to restore the dump. Migrations only go forward, so after a failed update that had already migrated, restore the dump as well. The `.env` and the published downloads are carried over; the commit that is running is in `DEPLOYED_COMMIT`.

## Apps

People get the Mac app from your server: `/download` offers the installer for the version the server is running, and the desktop app's "Check for updates" reads the same place (`GET /api/v1/downloads`). To publish a release:

```sh
./scripts/release-macos.sh                           # signed, notarised build in dist/release
bun --env-file=.env scripts/publish-release.ts       # puts it where the server reads builds, and confirms
```

With local storage the server reads `dist/release` directly (Compose mounts it read-only; set `DOWNLOADS_HOST_DIR` to use another directory), so the second command only confirms that the server offers the build. With S3-compatible storage it uploads the files to `<prefix>downloads/` in the bucket; set `S3_SECRET_ACCESS_KEY` for that command, because the server never returns the secret. The version is the one in the root `package.json`; `bun run check` fails if the desktop and iPhone apps carry a different one.

To build and run the desktop app locally:

```sh
./scripts/package.sh --release
open "dist/Cereal.app"
```

The desktop app needs macOS 14.2 or later. The iPhone project is generated from `apps/ios/project.yml` and builds with Xcode. Both apps sign in to your server; connections to a remote server require HTTPS.

## Repository

| Path | Contents |
| --- | --- |
| `apps/desktop` | Desktop app (Rust, GPUI) |
| `apps/ios` | iPhone app (SwiftUI) and its Swift packages |
| `apps/web` | Web app (TanStack Start) |
| `apps/api` | HTTP API, live transcription, AI requests |
| `packages/contracts` | API schemas and generated OpenAPI, JSON Schema, Swift and Rust types |
| `packages/database` | Prisma schema and SQL migrations |
| `crates/api-types` | Generated Rust wire types |
| `vendor/gpui` | GPUI with a local macOS fix |

## Develop

Bun 1.3.8, Node 24 (web build), a Rust toolchain with edition 2024, and Xcode for the iPhone app.

```sh
./scripts/setup.sh --dev                  # also writes DATABASE_URL, which a server outside Docker needs
bun install --frozen-lockfile
docker compose -f compose.yaml -f compose.dev.yaml up db -d
bun run generate
bun run db:migrate
bun run dev
```

`bun run dev:web` serves the web app on port 3001 and proxies `/api` to port 3000.

## Checks

```sh
bun run generate
bun run check
bun test
bun scripts/test-postgres-isolated.ts
cargo test --workspace
swift test --package-path apps/ios/Packages/APIClient
./scripts/smoke.sh
```

Third-party code and assets are listed in [LICENSES.md](LICENSES.md).
