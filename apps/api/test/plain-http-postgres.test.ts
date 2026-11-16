import { afterAll, describe, expect, test } from "bun:test"
import { createHash, randomBytes } from "node:crypto"
import { makeAuthenticationService } from "../src/auth"
import { makePrismaRepository } from "../src/prisma-repository"
import { makePrismaCalendarStore } from "../src/calendar-store"
import { makeAdministrationService } from "../src/administration"
import { secretBoxFromKey } from "../src/admin-secrets"
import { makeApiHandler } from "../src/api"

const enabled = process.env.RUN_AUTH_PG_TESTS === "1" && !!process.env.DATABASE_URL

/** Real Better Auth sessions on a server whose public address is plain HTTP: a private-range address, as on an
 * office network, and a public-looking host name. Runs only in a dedicated throwaway database. */
describe.skipIf(!enabled)("accounts over plain HTTP on PostgreSQL", () => {
  const resources = enabled ? makePrismaRepository(process.env.DATABASE_URL!) : null, db = resources?.database
  const ownerToken = "isolated-plain-http-bootstrap-token-at-least-32-characters", secret = randomBytes(32).toString("base64url")
  const password = "test-only-password-" + randomBytes(12).toString("hex"), run = randomBytes(4).toString("hex")
  afterAll(async () => { await resources?.close() })

  test("the account service accepts http for any host, and still refuses what is not a plain origin", () => {
    const make = (baseURL: string, trustedOrigins?: string[]) => makeAuthenticationService(db!, ownerToken, { secret, baseURL, ...(trustedOrigins ? { trustedOrigins } : {}) })
    for (const baseURL of ["http://10.30.1.77:3000", "http://notes.example.test:3000", "http://cereal.local", "http://[fd12::1]:3000", "https://notes.example.test", "http://localhost:3000"]) expect(() => make(baseURL)).not.toThrow()
    for (const baseURL of ["ftp://10.30.1.77", "http://10.30.1.77:3000/app", "http://user:pw@10.30.1.77:3000", "http://10.30.1.77:3000?x=1", "http://10.30.1.77:3000/#x"]) expect(() => make(baseURL)).toThrow("PUBLIC_BASE_URL must be an http or https address")
    // A development front end on another machine of the same network may be trusted explicitly, over http too.
    expect(() => make("http://10.30.1.77:3000", ["http://10.30.1.78:3001", "https://notes.example.test"])).not.toThrow()
    for (const origin of ["http://10.30.1.78:3001/", "10.30.1.78:3001", "ftp://10.30.1.78", "http://user@10.30.1.78"]) expect(() => make("http://10.30.1.77:3000", [origin])).toThrow("AUTH_TRUSTED_ORIGINS")
    // "localhost" and "127.0.0.1" are the same server only on this machine: no other address gets an alias.
    expect(make("http://localhost:3000").isTrustedOrigin("http://127.0.0.1:3000")).toBe(true)
    expect(make("http://10.30.1.77:3000").isTrustedOrigin("http://localhost:3000")).toBe(false)
    expect(make("http://10.30.1.77:3000").isTrustedOrigin("http://127.0.0.1:3000")).toBe(false)
    expect(make("http://10.30.1.77:3000").isTrustedOrigin("https://10.30.1.77:3000")).toBe(false)
  })

  for (const [label, baseURL] of [["a private-range address", "http://10.30.1.77:3000"], ["a public-looking host name", "http://notes.example.test:3000"]] as const) {
    describe(label, () => {
      const settingsId = "plain-http-" + createHash("sha256").update(baseURL).digest("hex").slice(0, 8)
      const administration = db ? makeAdministrationService(makePrismaCalendarStore(db, settingsId), secretBoxFromKey(randomBytes(32)), { baseURL, environment: {} }) : null
      const authentication = db ? makeAuthenticationService(db, ownerToken, { secret, baseURL, signIn: () => administration!.signInPolicy() }) : null
      const api = resources ? makeApiHandler(resources.repository, ownerToken, { authentication: authentication!, administration: administration! }) : null
      const email = `plain-http-${run}-${settingsId}@example.test`
      const request = (path: string, method = "GET", body?: unknown, headers: Record<string, string> = {}) => api!.handler(new Request(baseURL + path, { method, redirect: "manual", headers: { ...(body === undefined ? {} : { "Content-Type": "application/json" }), ...headers }, ...(body === undefined ? {} : { body: JSON.stringify(body) }) }))
      let cookie = "", userId = ""
      afterAll(async () => { await api?.dispose(); await db?.instanceSettings.deleteMany({ where: { id: settingsId } }) })

      test("signing in sets a session cookie a browser will send back over HTTP", async () => {
        const created = await request("/api/v1/admin/users", "POST", { name: "Plain Http", email, password }, { Authorization: "Bearer " + ownerToken })
        expect(created.status).toBe(200)
        userId = (await created.json()).id
        const signedIn = await request("/api/auth/sign-in/email", "POST", { email, password, rememberMe: false }, { Origin: baseURL })
        expect(signedIn.status).toBe(200)
        const cookies = signedIn.headers.getSetCookie(), session = cookies.find(value => value.includes("session_token="))!
        expect(session).toBeDefined()
        // No `Secure` (a browser would drop the cookie on http) and no name prefix that requires it.
        for (const value of cookies) { expect(value).not.toMatch(/;\s*Secure/i); expect(value).not.toMatch(/^__(Secure|Host)-/) }
        expect(session).toMatch(/^better-auth\.session_token=/)
        expect(session).toMatch(/;\s*HttpOnly/i)
        expect(session).toMatch(/;\s*SameSite=Lax/i)
        expect(session).toMatch(/;\s*Path=\//i)
        cookie = cookies.map(value => value.split(";")[0]).join("; ")
        const who = await request("/api/v1/session", "GET", undefined, { Cookie: cookie })
        expect(who.status).toBe(200)
        expect((await who.json()).user.id).toBe(userId)
      })

      test("requests that change something need this server's own http origin: no alias, no https twin, no other site", async () => {
        const create = (origin?: string) => request("/api/v1/notes", "POST", { title: "Over plain HTTP" }, { Cookie: cookie, ...(origin ? { Origin: origin } : {}) })
        expect((await create(baseURL)).status).toBe(200)
        for (const origin of [baseURL.replace("http:", "https:"), "http://localhost:3000", "http://127.0.0.1:3000", "http://evil.example", baseURL.replace(":3000", ":3001")]) expect([origin, (await create(origin)).status]).toEqual([origin, 403])
        expect((await create()).status).toBe(403)
        // Better Auth's own routes hold the same line.
        expect((await request("/api/auth/sign-out", "POST", {}, { Cookie: cookie, Origin: "http://localhost:3000" })).status).toBe(403)
      })

      test("the browser can hand its account to an app, and the code works over HTTP", async () => {
        const verifier = randomBytes(48).toString("base64url"), challenge = createHash("sha256").update(verifier).digest("base64url")
        const handoff = await request("/api/v1/auth/app-handoff", "POST", { challenge }, { Cookie: cookie, Origin: baseURL })
        expect(handoff.status).toBe(200)
        const exchanged = await request("/api/v1/auth/app-handoff/exchange", "POST", { code: (await handoff.json()).code, verifier })
        expect(exchanged.status).toBe(200)
        const bearer = (await exchanged.json()).token
        const who = await request("/api/v1/session", "GET", undefined, { Authorization: "Bearer " + bearer })
        expect((await who.json()).user.id).toBe(userId)
      })

      test("the public sign-in methods name the transport, and Google is neither offered nor startable", async () => {
        const methods = await (await request("/api/v1/auth/methods")).json()
        expect(methods).toMatchObject({ password: true, google: false, transport: baseURL.includes("10.30.1.77") ? "private" : "public-http" })
        const refused = await request("/api/v1/admin/settings", "PATCH", { signIn: { google: true, googleClientId: "1234567890-fixture.apps.googleusercontent.com", googleClientSecret: "GOCSPX-fixture-client-secret-value" } }, { Authorization: "Bearer " + ownerToken })
        expect(refused.status).toBe(400)
        expect((await refused.json()).message).toBe("Google sign-in needs an HTTPS address. This server is reached over plain HTTP; put it behind HTTPS to use Google.")
        const start = await request("/api/v1/auth/google/start")
        expect(start.status).toBe(303)
        expect(start.headers.get("location")).toBe("/login?error=google_unavailable")
        for (const value of start.headers.getSetCookie()) { expect(value).not.toMatch(/^__Host-/); expect(value).not.toMatch(/;\s*Secure/i) }
      })

      test("AI app connections say they need HTTPS: no metadata, no OAuth routes, and nothing that names an https twin", async () => {
        const needs = "Connecting AI apps needs an HTTPS address. This server is reached over plain HTTP."
        expect(authentication!.mcp.available).toBe(false)
        for (const path of ["/.well-known/oauth-protected-resource/api/mcp", "/.well-known/oauth-protected-resource", "/.well-known/oauth-authorization-server/api/auth"]) {
          const metadata = await authentication!.mcp.metadata(new Request(baseURL + path))
          expect([path, metadata.status]).toEqual([path, 404])
          expect(await metadata.json()).toEqual({ error: "not_found", message: needs })
        }
        const endpoint = await authentication!.mcp.protect(new Request(baseURL + "/api/mcp", { method: "POST" }), async () => Response.json({ reached: true }))
        expect(endpoint.status).toBe(503)
        expect(await endpoint.json()).toEqual({ error: "unavailable", message: needs })
        expect(endpoint.headers.get("www-authenticate")).toBeNull()
        for (const [path, method] of [["/api/auth/oauth2/register", "POST"], ["/api/auth/oauth2/token", "POST"], ["/api/auth/oauth2/authorize", "GET"], ["/api/auth/jwks", "GET"]] as const) {
          const response = await request(path, method, method === "POST" ? {} : undefined)
          expect([path, response.status]).toEqual([path, 404])
          const body = await response.text()
          expect(body).toContain(needs)
          expect(body).not.toContain("https://")
        }
        // The account pages behind it still answer for someone signed in.
        const connections = await request("/api/auth/mcp/connections", "GET", undefined, { Cookie: cookie })
        expect(connections.status).toBe(200)
        expect(await connections.json()).toEqual({ connections: [] })
      })

      test("signing out clears the cookie the same way it was set", async () => {
        const out = await request("/api/auth/sign-out", "POST", {}, { Cookie: cookie, Origin: baseURL })
        expect(out.status).toBe(200)
        for (const value of out.headers.getSetCookie()) expect(value).not.toMatch(/;\s*Secure/i)
        expect((await request("/api/v1/session", "GET", undefined, { Cookie: cookie })).status).toBe(401)
      })
    })
  }
})
