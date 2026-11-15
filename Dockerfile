FROM node:24.22.3-bookworm-slim AS node-toolchain
FROM oven/bun:1.3.8 AS build
COPY --from=node-toolchain /usr/local/bin/node /usr/local/bin/node
WORKDIR /app
COPY package.json bun.lock tsconfig.base.json ./
COPY apps/api/package.json ./apps/api/package.json
COPY apps/web/package.json ./apps/web/package.json
COPY packages/contracts/package.json ./packages/contracts/package.json
COPY packages/database/package.json ./packages/database/package.json
RUN bun install --frozen-lockfile
COPY packages ./packages
COPY apps/api ./apps/api
COPY apps/web ./apps/web
# React SSR used during Start's SPA shell build requires Node on Linux with
# this Bun version. The deployed server remains Bun; no Node runtime is shipped.
RUN DATABASE_URL=postgresql://build:build@127.0.0.1:5432/build bun run generate && bun run --cwd apps/web build

FROM oven/bun:1.3.8-slim AS runtime
WORKDIR /app
COPY --from=build /app /app
RUN mkdir -p /data/audio /data/state && chown bun:bun /data/audio /data/state && chmod 700 /data/audio /data/state
USER bun
EXPOSE 3000
CMD ["bun", "apps/api/src/server.ts"]
