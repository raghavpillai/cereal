# Cereal

Self-hosted meeting notes. Cereal records a meeting, shows the transcript while people are still talking, lets you ask questions about it as it happens, and writes the notes afterwards. Everything is stored on a server you run.

- **Desktop app** (macOS): records microphone and system audio, live transcript, chat, notes.
- **iPhone app**: records, live transcript, chat, notes.
- **Web app**: read your notes and shared notes, manage your account, and administer the instance.
- **Server**: API, transcription and note generation, with PostgreSQL for storage.

## Run the server

Install Docker with Compose, then:

```sh
./scripts/setup.sh
docker compose up --build -d
```

`setup.sh` creates a private `.env` with random database and owner credentials, never overwrites an existing one, and prints the address to open and a **setup code**. Show the code again with `grep ADMIN_API_TOKEN .env`.

Open `http://localhost:3000`. A new server has no accounts yet, so every page leads to setup, which walks through these steps one at a time:

1. **Claim this server**: paste the setup code and create the owner account (your name, email, and a password of at least 12 characters).
2. **Name your space**: the team name and, if you like, a logo.
3. **Notes and chat**: pick OpenRouter, OpenAI or Anthropic and paste its API key. The key is checked as you paste it.
4. **Speech to text**: paste an ElevenLabs API key.
5. **Models**: the models for chat, meeting notes, speech-to-text and live transcription, already chosen for the keys you added.
6. **Who can sign in** (optional): email and password, Google, or both, and which addresses or domains may create an account by signing in with Google. To use Google, create an OAuth web client in Google Cloud, enter its client ID and secret, and add the two redirect URIs the step shows. The same client is used for the Google Calendar connection.
7. **Where files live** (optional): recorded audio and the app builds this server offers are kept on the server's disk, or in an S3-compatible bucket (Amazon S3, Cloudflare R2, MinIO, Backblaze B2). **Test connection** tries the bucket before anything is saved. New files go to the storage you choose; files already stored stay where they are and keep working, and nothing is moved.
8. **Get the apps**: the Mac download and the address to type into the apps.

Setup can be left and picked up again: it resumes at the first unfinished step. Until it is finished, `/admin` shows only setup; afterwards it is the admin panel, where every one of these settings can be changed, along with live transcription limits and the iPhone app's identifiers. The owner can go through setup again from the link at the bottom of the panel; nothing that was configured is lost. Keys and secrets are encrypted and stored on your server.

All of this is kept in the database, and a change applies without a restart. `.env` holds only what the server needs before it can read its settings: the database password, the owner token, the port and the public address (`PUBLIC_BASE_URL`). If sign-in is ever misconfigured, `ADMIN_API_TOKEN` from `.env` still works as a bearer token for `PATCH /api/v1/admin/settings`.

Upgrading from a version that kept provider keys or model names in `.env`: start the new version once. It takes those values into the database, logs each one by name, and never reads them again; the admin page then lists the variables you can delete. A server that already had a provider key, stored or in `.env`, counts as set up and opens on the admin panel as before.

```sh
docker compose logs --tail=100 app
docker compose down
```

`down` keeps your notes and audio. Back up the PostgreSQL volume and the `audio-data` volume together.

AI clients can connect through the OAuth-protected MCP endpoint at `/api/mcp`; the page at `/mcp/connect` has the setup steps. Clients set up with the earlier address, `/mcp`, keep working.

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
./scripts/setup.sh
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
