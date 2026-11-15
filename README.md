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

Open `http://localhost:3000`. `setup.sh` creates a private `.env` with random database and owner credentials and never overwrites an existing one.

Sign in as the owner and open `/admin` to finish setup:

1. **AI providers**: add an API key for ElevenLabs (speech-to-text) and for OpenRouter, OpenAI or Anthropic (chat and notes). Keys are encrypted and stored on your server.
2. **Models**: choose the model for chat, for meeting notes, for speech-to-text and for live transcription. These choices are stored in the database.

Provider keys can also be supplied through `ELEVENLABS_API_KEY` and `OPENROUTER_API_KEY` in `.env`; a key saved on the admin page takes precedence.

```sh
docker compose logs --tail=100 app
docker compose down
```

`down` keeps your notes and audio. Back up the PostgreSQL volume and the `audio-data` volume together.

AI clients can connect through the OAuth-protected `/mcp` endpoint; `/mcp/connect` has the setup steps.

## Apps

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
