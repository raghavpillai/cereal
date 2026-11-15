setup:
    ./scripts/setup.sh

install:
    bun install --frozen-lockfile

generate:
    bun run generate

check:
    bun run check
    cargo check --workspace --locked

test:
    bun test
    cargo test --workspace --locked

desktop *args:
    cargo run -p cereal -- {{args}}

dev:
    bun run dev

web:
    bun run dev:web

up:
    docker compose up --build -d

down:
    docker compose down

logs:
    docker compose logs -f app
