import { expect, test } from "bun:test"
import { randomBytes } from "node:crypto"
import { Effect } from "effect"
import { makeMemoryRepository } from "../src/repository"
import { makeMemoryCalendarStore } from "../src/calendar-store"
import { makeAdministrationService } from "../src/administration"
import { makeGoogleService } from "../src/google"
import { makeMemoryGoogleStore } from "../src/google-store"
import { secretBoxFromKey } from "../src/admin-secrets"
import { makeApiHandler } from "../src/api"
import { emailAllowed, normalizeAllowedEmail, pictureAddress, safeReturnPath } from "../src/sign-in"

const run = Effect.runPromise
const failure = <A>(effect: Effect.Effect<A, unknown>) => run(Effect.flip(effect)) as Promise<{ error: string; message: string }>
const clientId = "1234567890-fixture.apps.googleusercontent.com", clientSecret = "GOCSPX-fixture-client-secret-value"
function fixture(environment: NodeJS.ProcessEnv = {}) {
  const repository = makeMemoryRepository(), store = makeMemoryCalendarStore(repository), box = secretBoxFromKey(randomBytes(32))
  return { repository, store, box, admin: makeAdministrationService(store, box, { environment, baseURL: "https://notes.example.test" }) }
}

test("a return path stays on this origin: other sites, protocol-relative forms and the API are refused", () => {
  const origin = "https://notes.example.test"
  for (const path of ["/notes", "/notes/abc?tab=notes#top", "/login?app=1&challenge=x&state=y", "/s/" + "A".repeat(43), "/%2f%2fevil.example", "/%5Cevil.example", "/a%2F..%2F..%2Fevil.example"]) expect(safeReturnPath(path, origin)).toBe(path)
  for (const path of [null, undefined, "", "notes", "https://evil.example/", "//evil.example", "///evil.example", "/\\evil.example", "/\\/evil.example", "/.//evil.example", "/..//evil.example", "/notes/..//evil.example",
    // Encoded dot segments are resolved by browsers like plain ones: "/%2e%2e//host" is "//host".
    "/%2e%2e//evil.example", "/%2E%2E//evil.example", "/notes/%2e%2e/%2e%2e//evil.example", "/.%2e//evil.example", "/%2e//evil.example", "https:evil.example", "https:/evil.example", "http://notes.example.test@evil.example/", " /notes", "/\\evil.example",
    "javascript:alert(1)", "/a\nb", "/a\tb", "/\u0000", "/api", "/api/auth/sign-out", "/api/v1/auth/google/start", "/" + "a".repeat(16_384)]) expect(safeReturnPath(path, origin)).toBeNull()
  // Long enough for an AI client's signed connection query, which passes through sign-in.
  expect(safeReturnPath("/mcp/consent?state=" + "a".repeat(6000) + "&name=Caf%C3%A9", origin)).toBe("/mcp/consent?state=" + "a".repeat(6000) + "&name=Caf%C3%A9")
  // Whatever is accepted resolves to this origin and nowhere else.
  for (const path of ["/notes", "/%2f%2fevil.example", "/%5Cevil.example", "/@evil.example", "/:evil.example", "/a%2F..%2F..%2Fevil.example", "/s/" + "A".repeat(43)]) expect(new URL(safeReturnPath(path, origin)!, origin).origin).toBe(origin)
})

test("allow-list entries are a full address or a whole domain, compared without regard to case", () => {
  expect(normalizeAllowedEmail("  Someone@Example.COM ")).toBe("someone@example.com")
  expect(normalizeAllowedEmail("@Example.org")).toBe("@example.org")
  for (const bad of ["", "example.org", "someone@", "someone@example", "@", "@example", "a@b@c.test", "some one@example.test", "@.test", "someone@example..test", "x".repeat(250) + "@example.test"]) expect(normalizeAllowedEmail(bad)).toBeNull()
  const allowed = ["someone@example.com", "@example.org"]
  expect(emailAllowed("Someone@Example.com", allowed)).toBe(true)
  expect(emailAllowed("anyone@EXAMPLE.org", allowed, "Example.org")).toBe(true)
  for (const address of ["other@example.com", "someone@example.com.evil.test", "anyone@sub.example.org", "anyone@notexample.org", "anyone@example.org.evil.test", "@example.org", "example.org", ""]) expect(emailAllowed(address, allowed, "example.org")).toBe(false)
  expect(emailAllowed("anyone@example.org", [], "example.org")).toBe(false)
})

test("a domain entry admits only the organisation's Google Workspace accounts (hd); a full address admits any verified account", () => {
  // A consumer Google account opened with a company address has no hd, and stays "verified" after its owner
  // has left the company and lost the mailbox.
  expect(emailAllowed("leaver@example.org", ["@example.org"])).toBe(false)
  expect(emailAllowed("leaver@example.org", ["@example.org"], null)).toBe(false)
  expect(emailAllowed("leaver@example.org", ["@example.org"], "")).toBe(false)
  expect(emailAllowed("leaver@example.org", ["@example.org"], "other.example")).toBe(false)
  expect(emailAllowed("leaver@example.org", ["@example.org"], "sub.example.org")).toBe(false)
  // Listed by address, the same account gets in, hd or not.
  expect(emailAllowed("leaver@example.org", ["@example.org", "leaver@example.org"])).toBe(true)
  expect(emailAllowed("Leaver@Example.org", ["leaver@example.org"], "other.example")).toBe(true)
  // The organisation's own Workspace account gets in through the domain entry.
  expect(emailAllowed("staff@example.org", ["@example.org"], "example.org")).toBe(true)
})

test("an allow-list domain entry for a mail service anyone can sign up with is refused, with the reason", async () => {
  const f = fixture()
  for (const entry of ["@gmail.com", "@GMAIL.com", "@yahoo.co.uk", "@outlook.de", "@icloud.com", "@privaterelay.appleid.com", "@mail.ru", "@comcast.net"]) {
    const refused = await failure(f.admin.updateSettings({ signIn: { allowedEmails: ["someone@example.test", entry] } }))
    expect(refused.error, entry).toBe("invalid_request")
    expect(refused.message, entry).toBe(`Anyone can sign up for an address at ${entry.slice(1).toLowerCase()}, so the whole domain cannot be allowed. Add people’s full addresses instead`)
  }
  // A full address at such a provider is fine, and so is an organisation's (or a school's) domain.
  const saved = await run(f.admin.updateSettings({ signIn: { allowedEmails: ["someone@gmail.com", "@example.org", "@stanford.edu"] } }))
  expect(saved.signIn?.allowedEmails).toEqual(["someone@gmail.com", "@example.org", "@stanford.edu"])
})

test("sign-in settings: defaults, at least one method, Google only with a stored client, and a sealed secret that is never returned", async () => {
  const f = fixture()
  expect((await run(f.admin.getSettings())).signIn).toEqual({ password: true, google: false, allowedEmails: [], googleClientId: null, googleClientSecretSet: false, googleClientSecretHint: null,
    googleRedirectUris: ["https://notes.example.test/api/v1/auth/google/callback", "https://notes.example.test/api/integrations/google/callback"], googleNeedsHttps: false })
  expect(await f.admin.signInPolicy()).toEqual({ password: true, google: false, allowedEmails: [], googleCredentials: null, instanceName: "Cereal" })

  // One method, never both and never neither. Turning password off is choosing Google, which needs its client.
  expect((await failure(f.admin.updateSettings({ signIn: { password: false } }))).message).toBe("Add a Google client ID and client secret before turning on Google sign-in")
  expect((await failure(f.admin.updateSettings({ signIn: { password: false, google: false } }))).message).toBe("Keep one sign-in method turned on")
  expect((await failure(f.admin.updateSettings({ signIn: { password: true, google: true, googleClientId: clientId, googleClientSecret: clientSecret } }))).message).toBe("A server has one way to sign in: choose email and password, or Google")
  expect((await failure(f.admin.updateSettings({ signIn: { google: true } }))).message).toBe("Add a Google client ID and client secret before turning on Google sign-in")
  expect((await failure(f.admin.updateSettings({ signIn: { google: true, googleClientId: clientId } }))).error).toBe("invalid_request")
  expect((await failure(f.admin.updateSettings({ signIn: { google: true, googleClientSecret: clientSecret } }))).error).toBe("invalid_request")
  expect((await failure(f.admin.updateSettings({ signIn: { allowedEmails: ["someone@example.test", "nonsense"] } }))).error).toBe("invalid_request")
  expect((await failure(f.admin.updateSettings({ signIn: { allowedEmails: Array.from({ length: 501 }, (_, index) => `p${index}@example.test`) } }))).error).toBe("invalid_request")
  // A refused change saves nothing, including the other fields sent with it.
  expect((await failure(f.admin.updateSettings({ instanceName: "Renamed", signIn: { password: false } }))).error).toBe("invalid_request")
  expect(await f.store.getSettings()).toBeNull()

  const saved = await run(f.admin.updateSettings({ signIn: { google: true, googleClientId: ` ${clientId} `, googleClientSecret: clientSecret, allowedEmails: ["B@Example.test", "@Team.example", "b@example.test"] } }))
  expect(saved.signIn).toMatchObject({ password: false, google: true, allowedEmails: ["b@example.test", "@team.example"], googleClientId: clientId, googleClientSecretSet: true })
  expect(JSON.stringify(saved)).not.toContain(clientSecret)
  const stored = await f.store.getSettings()
  expect(JSON.stringify(stored)).not.toContain(clientSecret)
  expect(f.box.open(stored!.googleClientSecret!)).toBe(clientSecret)
  // Choosing Google turned password off in the same save.
  expect(await f.admin.signInPolicy()).toMatchObject({ password: false, google: true, googleCredentials: { clientId, clientSecret } })
  expect((await f.store.getSettings())).toMatchObject({ signInGoogle: true, signInPassword: false })

  // Other edits leave sign-in alone; while Google is the method its client cannot be removed.
  expect((await run(f.admin.updateSettings({ instanceName: "Product studio" }))).signIn).toEqual(saved.signIn)
  expect((await run(f.admin.updateSettings({ signIn: { password: false } }))).signIn).toMatchObject({ password: false, google: true })
  expect((await failure(f.admin.updateSettings({ signIn: { googleClientSecret: null } }))).message).toBe("Turn off Google sign-in before removing its client ID or secret")
  expect((await failure(f.admin.updateSettings({ signIn: { googleClientId: null } }))).error).toBe("invalid_request")
  // Turning Google off is choosing email and password; saying so either way gives the same answer.
  expect((await run(f.admin.updateSettings({ signIn: { google: false } }))).signIn).toMatchObject({ password: true, google: false, googleClientSecretSet: true })
  expect(await f.admin.signInPolicy()).toMatchObject({ password: true, google: false })
  expect((await run(f.admin.updateSettings({ signIn: { google: true } }))).signIn).toMatchObject({ password: false, google: true })
  expect((await run(f.admin.updateSettings({ signIn: { password: true, googleClientSecret: null } }))).signIn).toMatchObject({ password: true, google: false, googleClientSecretSet: false })
  // A server from before this rule that had both on reads as email and password.
  await f.store.saveSettings({ signInPassword: true, signInGoogle: true, googleClientSecret: f.box.seal(clientSecret) })
  expect((await run(f.admin.getSettings())).signIn).toMatchObject({ password: true, google: false })
  expect(await f.admin.signInPolicy()).toMatchObject({ password: true, google: false })
  // Swapping methods in one save is fine, and the client can then be removed.
  const swapped = await run(f.admin.updateSettings({ signIn: { password: true, google: false, googleClientId: null, googleClientSecret: null } }))
  expect(swapped.signIn).toMatchObject({ password: true, google: false, googleClientId: null, googleClientSecretSet: false })
  expect(await f.admin.googleCredentials()).toBeNull()
})

test("a stored secret that can no longer be read turns Google off rather than failing open or leaking", async () => {
  const f = fixture()
  await run(f.admin.updateSettings({ signIn: { google: true, password: false, googleClientId: clientId, googleClientSecret: clientSecret } }))
  // The settings key was replaced: the same rows, read with another key.
  const other = makeAdministrationService(f.store, secretBoxFromKey(randomBytes(32)), { environment: {}, baseURL: "https://notes.example.test" })
  expect(await other.signInPolicy()).toMatchObject({ password: false, google: false, googleCredentials: null })
  expect(await other.googleCredentials()).toBeNull()
  // Both switches off can only be left by two saves racing; password then counts as on.
  await f.store.saveSettings({ signInPassword: false, signInGoogle: false })
  expect(await f.admin.signInPolicy()).toMatchObject({ password: true, google: false })
})

test("the Google Calendar connection uses the stored client, falls back to the environment, and follows a change without a restart", async () => {
  const f = fixture({ GOOGLE_CLIENT_ID: "environment-client", GOOGLE_CLIENT_SECRET: "environment-secret" })
  const requests: URLSearchParams[] = []
  const service = makeGoogleService(makeMemoryGoogleStore(f.repository), f.box, { baseURL: "http://localhost:3000", credentials: () => f.admin.googleCredentials(),
    fetch: Object.assign(async (...[input, init]: Parameters<typeof fetch>) => { requests.push(new URLSearchParams(await new Request(input, init).text())); return Response.json({ error: "invalid_grant" }, { status: 400 }) }, { preconnect: () => {} }) as typeof fetch })
  const user = "31bb7a9f-fbd5-411d-8fa2-17390ad35a76", session = "a".repeat(64)
  const clientOf = async () => new URL((await run(service.beginConnect(user, session))).authorizationUrl).searchParams.get("client_id")
  try {
    expect(await f.admin.googleCredentials()).toEqual({ clientId: "environment-client", clientSecret: "environment-secret" })
    expect(await clientOf()).toBe("environment-client")
    // Saved on the admin panel: the very next request uses it.
    await run(f.admin.updateSettings({ signIn: { googleClientId: clientId, googleClientSecret: clientSecret } }))
    expect(await clientOf()).toBe(clientId)
    expect((await run(service.getStatus(user))).configured).toBe(true)
    // Half a stored client is not a client: the environment applies again.
    await run(f.admin.updateSettings({ signIn: { googleClientSecret: null } }))
    expect(await clientOf()).toBe("environment-client")
  } finally { await service.dispose() }
  // With neither, the integration reports itself unconfigured instead of failing.
  const bare = fixture(), unconfigured = makeGoogleService(makeMemoryGoogleStore(bare.repository), bare.box, { baseURL: "http://localhost:3000", credentials: () => bare.admin.googleCredentials() })
  try {
    expect(await run(unconfigured.getStatus(user))).toEqual({ configured: false, redirectUri: null, connections: [], needsHttps: false })
    expect((await failure(unconfigured.beginConnect(user, session))).error).toBe("unavailable")
    await run(bare.admin.updateSettings({ signIn: { googleClientId: clientId, googleClientSecret: clientSecret } }))
    expect((await run(unconfigured.getStatus(user))).configured).toBe(true)
  } finally { await unconfigured.dispose() }
  expect(requests).toEqual([])
})

test("the public sign-in methods need no credentials and say nothing about the Google client", async () => {
  const f = fixture(), token = "sign-in-methods-operator-token-at-least-32-characters"
  const api = makeApiHandler(f.repository, token, { administration: f.admin })
  try {
    const methods = await api.handler(new Request("http://localhost/api/v1/auth/methods"))
    expect(methods.status).toBe(200)
    expect(await methods.json()).toEqual({ password: true, google: false, instanceName: "Cereal", setupRequired: false, transport: "secure" })
    await run(f.admin.updateSettings({ instanceName: "Product studio", signIn: { google: true, googleClientId: clientId, googleClientSecret: clientSecret } }))
    const text = await (await api.handler(new Request("http://localhost/api/v1/auth/methods"))).text()
    expect(text).not.toContain(clientId); expect(text).not.toContain(clientSecret)
    // Without an account service nobody can finish a Google sign-in, so it is not offered.
    expect(JSON.parse(text)).toEqual({ password: false, google: false, instanceName: "Product studio", setupRequired: false, transport: "secure" })
    expect((await api.handler(new Request("http://localhost/api/v1/auth/google/start"))).headers.get("location")).toBe("/login?error=google_unavailable")
    expect((await api.handler(new Request("http://localhost/api/v1/auth/app-handoff/exchange", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ code: "a".repeat(43), verifier: "b".repeat(43) }) }))).status).toBe(503)
  } finally { await api.dispose() }
})

test("a picture is kept only as a plain https address of ordinary length", () => {
  for (const address of ["https://lh3.googleusercontent.com/a/ACg8ocFixture=s96-c", "https://pictures.example.test/people/1.png?size=96"]) expect(pictureAddress(address)).toBe(address)
  for (const value of [null, undefined, 7, "", "lh3.googleusercontent.com/a/x", "http://lh3.googleusercontent.com/a/x", "//lh3.googleusercontent.com/a/x", "javascript:alert(1)", "data:image/png;base64,AAAA",
    "https://user:secret@pictures.example.test/a.png", "https://localhost/a.png", "https://pictures.example.test/a b.png", "https://pictures.example.test/a.png\nSet-Cookie: x=1", "https://pictures.example.test/\"onerror=\"x", " https://pictures.example.test/a.png",
    "https://pictures.example.test/" + "a".repeat(2048)]) expect(pictureAddress(value)).toBeNull()
})
