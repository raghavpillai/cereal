import { importScenarios } from "./import-scenarios"
import { expect, test } from "bun:test"
import type { PrincipalData } from "@meeting-notes/contracts/api"
import { makeApiHandler } from "../src/api"
import { makeMemoryRepository } from "../src/repository"
import { importBody } from "./import-support"

// The server tests of the note import (design 5.9; X1, X2, X3 of 8.7), through HTTP, on the memory adapters.
// import-postgres.test.ts runs the same scenarios on PostgreSQL.
importScenarios("memory")


test("the import and the conditional delete keep the origin rule of every other write", async () => {
  // A browser session (a cookie, no Authorization header) is honoured only from this server's own pages.
  const user: PrincipalData = { id: "20000000-0000-4000-8000-000000000009", name: "Cookie", email: "cookie@example.test", role: "user", source: "session", expiresAt: "2099-01-01T00:00:00Z", createdAt: "2026-01-01T00:00:00Z" }
  const api = makeApiHandler(makeMemoryRepository(), "origin-rule-test-token-at-least-32-characters", { principalResolver: async headers => headers.get("cookie") === "session=valid" || headers.get("authorization") === "Bearer app" ? user : null })
  try {
    const body = importBody()
    const send = (method: string, path: string, headers: Record<string, string>, payload?: unknown) => api.handler(new Request(`http://localhost/api/v1${path}`, { method, headers: { ...headers, ...(payload ? { "Content-Type": "application/json" } : {}) }, ...(payload ? { body: JSON.stringify(payload) } : {}) }))
    const browsers: Array<Record<string, string>> = [{ Cookie: "session=valid" }, { Cookie: "session=valid", Origin: "https://evil.example" }]
    for (const headers of browsers) {
      expect((await send("POST", "/notes/import", headers, body)).status).toBe(403)
      expect((await send("POST", "/notes", headers, { title: "x" })).status).toBe(403)
    }
    expect((await send("GET", `/notes/${body.id}`, { Authorization: "Bearer app" })).status).toBe(404)
    // An app's token needs no Origin.
    expect((await send("POST", "/notes/import", { Authorization: "Bearer app" }, body)).status).toBe(200)
    expect((await send("DELETE", `/notes/${body.id}?ifChunks=0`, { Cookie: "session=valid", Origin: "https://evil.example" })).status).toBe(403)
    expect((await send("DELETE", `/notes/${body.id}?ifChunks=0`, { Authorization: "Bearer app" })).status).toBe(200)
  } finally { await api.dispose() }
})
