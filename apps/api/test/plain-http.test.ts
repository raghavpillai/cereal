/** A server reached over plain HTTP: what it reports about its address, and what Google is refused. */
import { afterEach, expect, test } from "bun:test"
import { randomBytes } from "node:crypto"
import { Effect } from "effect"
import { makeApiHandler } from "../src/api"
import { makeAdministrationService } from "../src/administration"
import { secretBoxFromKey } from "../src/admin-secrets"
import { makeMemoryCalendarStore } from "../src/calendar-store"
import { makeGoogleService } from "../src/google"
import { makeMemoryGoogleStore } from "../src/google-store"
import { makeLinkPreviews } from "../src/app-links"
import { makeMemoryRepository } from "../src/repository"

const token = "plain-http-bootstrap-token-with-at-least-32-characters"
const clientId = "1234567890-fixture.apps.googleusercontent.com", clientSecret = "GOCSPX-fixture-client-secret-value"
const cleanup: Array<() => Promise<unknown>> = []
afterEach(async () => { for (const dispose of cleanup.splice(0)) await dispose() })

function server(baseURL: string) {
  const repository = makeMemoryRepository(), store = makeMemoryCalendarStore(repository), box = secretBoxFromKey(randomBytes(32))
  const admin = makeAdministrationService(store, box, { baseURL, environment: {} })
  const google = makeGoogleService(makeMemoryGoogleStore(repository), box, { baseURL, credentials: () => admin.googleCredentials() })
  const api = makeApiHandler(repository, token, { administration: admin, google })
  cleanup.push(api.dispose)
  const call = (path: string, method = "GET", body?: unknown, signedIn = true) => api.handler(new Request(baseURL + path, {
    method, headers: { ...(signedIn ? { Authorization: `Bearer ${token}` } : {}), ...(body === undefined ? {} : { "Content-Type": "application/json" }) },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  }))
  return { admin, google, call }
}

test("the server says how its address is reached: secure, private, or plain HTTP in public", async () => {
  for (const [baseURL, transport] of [
    ["http://10.30.1.77:3000", "private"], ["http://192.168.1.20:3000", "private"], ["http://cereal.local:3000", "private"], ["http://cereal", "private"],
    ["http://notes.example.test:3000", "public-http"], ["http://203.0.113.9", "public-http"],
    ["https://notes.example.test", "secure"], ["http://localhost:3000", "secure"], ["http://127.0.0.1:3000", "secure"],
  ] as const) {
    const f = server(baseURL)
    expect(f.admin.transport).toBe(transport)
    const methods = await (await f.call("/api/v1/auth/methods", "GET", undefined, false)).json()
    expect([baseURL, methods]).toEqual([baseURL, { password: true, google: false, instanceName: "Cereal", setupRequired: false, transport }])
  }
})

test("over plain HTTP off this machine, Google sign-in cannot be turned on, and the error says why", async () => {
  for (const baseURL of ["http://10.30.1.77:3000", "http://notes.example.test:3000"]) {
    const f = server(baseURL)
    const before = await (await f.call("/api/v1/admin/settings")).json()
    expect(before.signIn.googleNeedsHttps).toBe(true)
    // The redirect addresses are the server's own, unchanged: they are shown, they are just no use to Google.
    expect(before.signIn.googleRedirectUris).toEqual([baseURL + "/api/v1/auth/google/callback", baseURL + "/api/integrations/google/callback"])
    // The client itself may be saved; turning Google on is what is refused.
    expect((await f.call("/api/v1/admin/settings", "PATCH", { signIn: { googleClientId: clientId, googleClientSecret: clientSecret } })).status).toBe(200)
    const refused = await f.call("/api/v1/admin/settings", "PATCH", { signIn: { google: true } })
    expect(refused.status).toBe(400)
    expect(await refused.json()).toEqual({ error: "invalid_request", message: "Google sign-in needs an HTTPS address. This server is reached over plain HTTP; put it behind HTTPS to use Google." })
    const together = await f.call("/api/v1/admin/settings", "PATCH", { signIn: { google: true, googleClientId: clientId, googleClientSecret: clientSecret } })
    expect(together.status).toBe(400)
    expect((await together.json()).message).toContain("Google sign-in needs an HTTPS address")
    expect((await (await f.call("/api/v1/admin/settings")).json()).signIn.google).toBe(false)
    expect((await f.admin.signInPolicy()).google).toBe(false)
    // With Google out of reach, password sign-in cannot be the one that is off.
    const locked = await f.call("/api/v1/admin/settings", "PATCH", { signIn: { password: false } })
    expect(locked.status).toBe(400)
    // Nothing else about the settings is harder over HTTP.
    expect((await f.call("/api/v1/admin/settings", "PATCH", { instanceName: "Harbour Studio", signIn: { allowedEmails: ["@example.test"] } })).status).toBe(200)
  }
})

test("a server that had Google on and is now on plain HTTP keeps people able to sign in, and Google can still be turned off", async () => {
  const repository = makeMemoryRepository(), store = makeMemoryCalendarStore(repository), box = secretBoxFromKey(randomBytes(32))
  // Set up while it had an HTTPS address: Google only.
  const secure = makeAdministrationService(store, box, { baseURL: "https://notes.example.test", environment: {} })
  await Effect.runPromise(secure.updateSettings({ signIn: { google: true, password: false, googleClientId: clientId, googleClientSecret: clientSecret } }))
  expect(await secure.signInPolicy()).toMatchObject({ password: false, google: true })
  // The same database behind a plain HTTP address.
  const plain = makeAdministrationService(store, box, { baseURL: "http://10.30.1.77:3000", environment: {} })
  expect(await plain.signInPolicy()).toMatchObject({ password: true, google: false })
  const settings = await Effect.runPromise(plain.getSettings())
  expect(settings.signIn).toMatchObject({ password: true, google: false, googleNeedsHttps: true, googleClientSecretSet: true })
  // Other sign-in settings can still be changed, and saying "email and password" out loud works.
  await Effect.runPromise(plain.updateSettings({ signIn: { allowedEmails: ["@example.test"] } }))
  expect((await Effect.runPromise(plain.updateSettings({ signIn: { google: false, password: true } }))).signIn).toMatchObject({ password: true, google: false })
})

test("where Google will redirect (HTTPS, or this machine) nothing is refused", async () => {
  for (const baseURL of ["https://notes.example.test", "http://localhost:3000"]) {
    const f = server(baseURL)
    const saved = await f.call("/api/v1/admin/settings", "PATCH", { signIn: { google: true, googleClientId: clientId, googleClientSecret: clientSecret } })
    expect(saved.status).toBe(200)
    expect((await saved.json()).signIn).toMatchObject({ google: true, googleNeedsHttps: false })
  }
})

test("the Google Calendar connection starts on plain HTTP, says it needs HTTPS, and refuses to send anyone to Google", async () => {
  const f = server("http://10.30.1.77:3000")
  await f.call("/api/v1/admin/settings", "PATCH", { signIn: { googleClientId: clientId, googleClientSecret: clientSecret } })
  const status = await (await f.call("/api/v1/integrations/google")).json()
  expect(status).toEqual({ configured: true, redirectUri: "http://10.30.1.77:3000/api/integrations/google/callback", connections: [], needsHttps: true })
  const refused = await Effect.runPromise(Effect.result(f.google.beginConnect("00000000-0000-4000-8000-000000000001", "a".repeat(64))))
  expect(refused._tag).toBe("Failure")
  expect(JSON.stringify(refused)).toContain("Google Calendar needs an HTTPS address. This server is reached over plain HTTP.")
  expect((await (await server("https://notes.example.test").call("/api/v1/integrations/google")).json()).needsHttps).toBe(false)
  expect((await (await server("http://127.0.0.1:3000").call("/api/v1/integrations/google")).json()).needsHttps).toBe(false)
})

test("link previews carry the plain HTTP address unchanged", async () => {
  const head = await makeLinkPreviews({ origin: "http://10.30.1.77:3000", iosAppStoreId: "6471234567" })("/notes/5b0a2c5e-8f0f-4d5b-9c4e-0a8a4a1d2f10")
  expect(head).toContain('content="http://10.30.1.77:3000/link-preview.png"')
  expect(head).toContain("app-argument=http://10.30.1.77:3000/notes/5b0a2c5e-8f0f-4d5b-9c4e-0a8a4a1d2f10")
  expect(head).not.toContain("https://10.30.1.77")
})
