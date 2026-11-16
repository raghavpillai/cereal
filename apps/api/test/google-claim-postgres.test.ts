import { afterAll, beforeAll, describe, expect, test } from "bun:test"
import { createHash, randomBytes } from "node:crypto"
import { makeAuthenticationService, BOOTSTRAP_OWNER_ID } from "../src/auth"
import { makePrismaRepository } from "../src/prisma-repository"
import { makePrismaCalendarStore } from "../src/calendar-store"
import { makeAdministrationService } from "../src/administration"
import { secretBoxFromKey } from "../src/admin-secrets"
import { makeApiHandler } from "../src/api"

const enabled = process.env.RUN_AUTH_PG_TESTS === "1" && !!process.env.DATABASE_URL
const challengeOf = (verifier: string) => createHash("sha256").update(verifier).digest("base64url")

/** Setting up sign-in in one step: claiming with Google (proved by one real sign-in against a local stand-in for
 * Google), claiming with email and password, and switching the one method afterwards. Runs only in a dedicated
 * throwaway database; it returns the bootstrap owner to "never claimed" before and after. */
describe.skipIf(!enabled)("one sign-in method: claiming with Google or a password, and switching, on PostgreSQL", () => {
  const resources = enabled ? makePrismaRepository(process.env.DATABASE_URL!) : null, db = resources?.database
  const setupCode = "isolated-google-claim-setup-code-at-least-32-characters", baseURL = "http://localhost:3000", settingsId = "google-claim-test"
  const clientId = "1234567890-harbour.apps.googleusercontent.com", clientSecret = "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"
  const password = "test-only-password-" + randomBytes(12).toString("hex"), run = randomBytes(4).toString("hex")
  const ownerEmail = `ada-${run}@harbour.example`, memberEmail = `grace-${run}@harbour.example`

  // The stand-in for Google. It knows one client and its secret, and can be told its redirect address is not registered.
  interface Identity { sub: string; email: string; email_verified: boolean; name?: string }
  const issued = new Map<string, { identity: Identity; challenge: string }>(), tokens = new Map<string, Identity>()
  let redirectRegistered = true
  const fakeGoogle = Object.assign(async (...[input, init]: Parameters<typeof fetch>) => {
    const request = new Request(input, init), url = new URL(request.url)
    if (url.href === "https://google.test/token") {
      const form = new URLSearchParams(await request.text())
      if (form.get("client_id") !== clientId || form.get("client_secret") !== clientSecret) return Response.json({ error: "invalid_client", error_description: "PRIVATE_PROVIDER_TEXT Unauthorized" }, { status: 401 })
      if (!redirectRegistered) return Response.json({ error: "redirect_uri_mismatch", error_description: "PRIVATE_PROVIDER_TEXT Bad Request" }, { status: 400 })
      const grant = issued.get(form.get("code") ?? ""); issued.delete(form.get("code") ?? "")
      if (!grant || form.get("redirect_uri") !== baseURL + "/api/v1/auth/google/callback" || challengeOf(form.get("code_verifier") ?? "") !== grant.challenge) return Response.json({ error: "invalid_grant" }, { status: 400 })
      const access = randomBytes(16).toString("hex"); tokens.set(access, grant.identity)
      return Response.json({ access_token: access, token_type: "Bearer", expires_in: 3600 })
    }
    if (url.href === "https://google.test/userinfo") {
      const identity = tokens.get((request.headers.get("authorization") ?? "").replace(/^Bearer /, ""))
      return identity ? Response.json(identity) : Response.json({ error: "invalid_token" }, { status: 401 })
    }
    return new Response("unexpected", { status: 500 })
  }, { preconnect: () => {} }) as typeof fetch

  let skew = 0
  const clock = () => new Date(Date.now() + skew)
  const administration = db ? makeAdministrationService(makePrismaCalendarStore(db, settingsId), secretBoxFromKey(randomBytes(32)), { baseURL, environment: {} }) : null
  const authentication = db ? makeAuthenticationService(db, setupCode, { secret: randomBytes(32).toString("base64url"), baseURL, clock, signIn: () => administration!.signInPolicy(), useGoogle: client => administration!.useGoogle(client),
    google: { endpoints: { authorization: "https://google.test/authorize", token: "https://google.test/token", userinfo: "https://google.test/userinfo" }, fetch: fakeGoogle } }) : null
  const api = resources ? makeApiHandler(resources.repository, setupCode, { authentication: authentication!, administration: administration!, clock }) : null

  let caller = 0
  const request = (path: string, method = "GET", body?: unknown, headers: Record<string, string> = {}) => api!.handler(new Request(baseURL + path, { method, redirect: "manual", headers: { ...(body === undefined ? {} : { "Content-Type": "application/json" }), ...headers }, ...(body === undefined ? {} : { body: JSON.stringify(body) }) }))
  const asOwner = { Authorization: "Bearer " + setupCode }
  const cookiesOf = (response: Response) => response.headers.getSetCookie().filter(value => !/Max-Age=0/i.test(value)).map(value => value.split(";")[0]).join("; ")
  /** A browser starting to claim with Google: each from its own address, so the attempt limit is not what is tested. */
  async function beginClaim(secret = clientSecret, id = clientId) {
    const response = await request("/api/v1/admin/owner/setup/google", "POST", { clientId: id, clientSecret: secret }, { ...asOwner, "X-Forwarded-For": `203.0.113.${++caller}` })
    if (response.status !== 200) return { response, cookie: "", state: "", challenge: "" }
    const location = new URL((await response.clone().json()).authorizationUrl)
    return { response, cookie: cookiesOf(response), state: location.searchParams.get("state")!, challenge: location.searchParams.get("code_challenge")!, location }
  }
  type Started = Awaited<ReturnType<typeof beginClaim>>
  /** The person approves at the stand-in, which sends the browser back with a code. */
  function approve(started: Started, identity: Identity, cookie = started.cookie) {
    const code = randomBytes(16).toString("hex"); issued.set(code, { identity, challenge: started.challenge })
    const url = `/api/v1/auth/google/callback?state=${encodeURIComponent(started.state)}&code=${code}`
    return { url, send: () => request(url, "GET", undefined, cookie ? { Cookie: cookie } : {}) }
  }
  const status = async (cookie: string) => await (await request("/api/v1/auth/google/verification", "GET", undefined, cookie ? { Cookie: cookie } : {})).json() as { state: string; reason: string | null }
  const methods = async () => await (await request("/api/v1/auth/methods")).json() as { password: boolean; google: boolean; setupRequired: boolean; instanceName: string; transport: string }
  const signInSettings = async () => (await (await request("/api/v1/admin/settings", "GET", undefined, asOwner)).json()).signIn as { password: boolean; google: boolean; googleClientId: string | null; googleClientSecretSet: boolean }
  const ada: Identity = { sub: "google-sub-ada-" + run, email: ownerEmail, email_verified: true, name: "Ada Lovelace" }
  /** Nothing of a failed claim is kept: no owner, no sign-in setting, no client, no secret left in the pending row. */
  async function nothingCommitted() {
    expect(await methods()).toMatchObject({ password: true, google: false, setupRequired: true })
    expect(await signInSettings()).toMatchObject({ password: true, google: false, googleClientId: null, googleClientSecretSet: false })
    expect(await db!.account.count({ where: { userId: BOOTSTRAP_OWNER_ID } })).toBe(0)
    expect((await db!.user.findUniqueOrThrow({ where: { id: BOOTSTRAP_OWNER_ID } })).email).toBe("owner@localhost.invalid")
    expect(await db!.googleVerification.count({ where: { outcome: { notIn: ["working"] }, NOT: { outcome: null }, encryptedSecret: { not: null } } })).toBe(0)
  }
  async function unclaim() {
    await db!.session.deleteMany({ where: { userId: BOOTSTRAP_OWNER_ID } })
    await db!.account.deleteMany({ where: { userId: BOOTSTRAP_OWNER_ID } })
    await db!.user.deleteMany({ where: { email: { in: [ownerEmail, memberEmail] }, id: { not: BOOTSTRAP_OWNER_ID } } })
    await db!.user.update({ where: { id: BOOTSTRAP_OWNER_ID }, data: { name: "Owner", email: "owner@localhost.invalid" } })
    await db!.instanceSettings.deleteMany({ where: { id: settingsId } })
    await db!.googleVerification.deleteMany({})
  }
  beforeAll(unclaim)
  afterAll(async () => { await unclaim().catch(() => {}); await api?.dispose(); await resources?.close() })

  test("starting needs the setup code, a plausible client, and an unclaimed server; someone with only the address gets nowhere", async () => {
    const body = { clientId, clientSecret }
    expect((await request("/api/v1/admin/owner/setup/google", "POST", body, { "X-Forwarded-For": "198.51.100.1" })).status).toBe(401)
    expect((await request("/api/v1/admin/owner/setup/google", "POST", body, { Authorization: "Bearer not-the-setup-code-not-the-setup-code", "X-Forwarded-For": "198.51.100.1" })).status).toBe(401)
    // A session cookie is not the setup code either.
    expect((await request("/api/v1/admin/owner/setup/google", "POST", body, { Cookie: "better-auth.session_token=forged", Origin: baseURL, "X-Forwarded-For": "198.51.100.1" })).status).toBe(401)
    for (const bad of [{ clientId: "short", clientSecret }, { clientId, clientSecret: "has space in it" }, { clientId }, {}]) expect((await request("/api/v1/admin/owner/setup/google", "POST", bad, { ...asOwner, "X-Forwarded-For": "198.51.100.2" })).status).toBe(400)
    expect(await db!.googleVerification.count()).toBe(0)
    // The callback and the status say nothing to a stranger.
    const stranger = await request(`/api/v1/auth/google/callback?state=${randomBytes(32).toString("base64url")}&code=anything`)
    expect(stranger.status).toBe(303)
    expect(stranger.headers.get("location")).toBe("/login?error=failed")
    expect(await status("")).toEqual({ state: "none", reason: null })
    expect(await status("cereal_google_verification=" + randomBytes(32).toString("base64url"))).toEqual({ state: "none", reason: null })
    // The attempt limit covers this address like the password one: the eleventh try from one client waits.
    for (let i = 0; i < 9; i++) await request("/api/v1/admin/owner/setup/google", "POST", body, { Authorization: "Bearer wrong-wrong-wrong-wrong-wrong-wrong", "X-Forwarded-For": "198.51.100.3" })
    expect((await request("/api/v1/admin/owner/setup/google", "POST", body, { Authorization: "Bearer wrong-wrong-wrong-wrong-wrong-wrong", "X-Forwarded-For": "198.51.100.3" })).status).toBe(401)
    expect((await request("/api/v1/admin/owner/setup/google", "POST", body, { ...asOwner, "X-Forwarded-For": "198.51.100.3" })).status).toBe(429)
    await nothingCommitted()
  })

  test("the browser is sent to Google with nothing secret in the address, and holds only an HttpOnly cookie", async () => {
    const started = await beginClaim()
    expect(started.response.status).toBe(200)
    const address = started.location!.href
    expect(started.location!.origin + started.location!.pathname).toBe("https://google.test/authorize")
    expect(Object.fromEntries(started.location!.searchParams)).toMatchObject({ client_id: clientId, redirect_uri: baseURL + "/api/v1/auth/google/callback", response_type: "code", scope: "openid email profile", code_challenge_method: "S256" })
    for (const secret of [setupCode, clientSecret]) { expect(address).not.toContain(secret); expect(started.response.headers.getSetCookie().join("\n")).not.toContain(secret) }
    const cookie = started.response.headers.getSetCookie()[0]!
    expect(cookie).toMatch(/^cereal_google_verification=[A-Za-z0-9_-]{43}; Path=\/; Max-Age=600; HttpOnly; SameSite=Lax$/)
    // Only hashes and sealed values are stored.
    const row = await db!.googleVerification.findFirstOrThrow({ orderBy: { createdAt: "desc" } })
    expect(JSON.stringify(row)).not.toContain(clientSecret); expect(JSON.stringify(row)).not.toContain(started.state); expect(JSON.stringify(row)).not.toContain(started.cookie.split("=")[1]!)
    expect(await status(started.cookie)).toEqual({ state: "pending", reason: null })
    await nothingCommitted()
  })

  test("each way it can fail is named, and commits nothing", async () => {
    // A wrong client secret: Google refuses the exchange.
    const wrongSecret = await beginClaim("GOCSPX-not-the-secret-at-all")
    const refused = await approve(wrongSecret, ada).send()
    expect(refused.status).toBe(200)
    expect(refused.headers.getSetCookie().some(value => value.includes("session_token"))).toBe(false)
    expect(await refused.text()).not.toContain("PRIVATE_PROVIDER_TEXT")
    expect(await status(wrongSecret.cookie)).toEqual({ state: "failed", reason: "invalid_client" })
    await nothingCommitted()

    // The redirect address is not on the client.
    redirectRegistered = false
    const mismatch = await beginClaim()
    await approve(mismatch, ada).send()
    redirectRegistered = true
    expect(await status(mismatch.cookie)).toEqual({ state: "failed", reason: "redirect_uri_mismatch" })
    await nothingCommitted()

    // The person pressed Cancel at Google.
    const cancelled = await beginClaim()
    expect((await request(`/api/v1/auth/google/callback?state=${cancelled.state}&error=access_denied`, "GET", undefined, { Cookie: cancelled.cookie })).status).toBe(200)
    expect(await status(cancelled.cookie)).toEqual({ state: "failed", reason: "cancelled" })
    await nothingCommitted()

    // An address Google has not verified.
    const unverified = await beginClaim()
    await approve(unverified, { ...ada, email_verified: false }).send()
    expect(await status(unverified.cookie)).toEqual({ state: "failed", reason: "unverified_email" })
    await nothingCommitted()

    // Someone on this server already has that address: the owner cannot take over their account by claiming.
    expect((await request("/api/v1/admin/users", "POST", { name: "Grace Hopper", email: memberEmail, password }, asOwner)).status).toBe(200)
    const taken = await beginClaim()
    await approve(taken, { sub: "google-sub-grace-" + run, email: memberEmail, email_verified: true, name: "Grace Hopper" }).send()
    expect(await status(taken.cookie)).toEqual({ state: "failed", reason: "email_taken" })
    await nothingCommitted()
    expect((await db!.user.findUniqueOrThrow({ where: { email: memberEmail } })).role).toBe("user")

    // Left too long.
    const late = await beginClaim()
    skew = 11 * 60_000
    await approve(late, ada).send()
    expect(await status(late.cookie)).toEqual({ state: "failed", reason: "expired" })
    skew = 0
    await nothingCommitted()
  })

  test("only the browser that started it can finish it; then Google's verified account is the owner, with no password", async () => {
    // Two browsers hold the setup code and both start. The first to finish wins.
    const first = await beginClaim(), second = await beginClaim()
    const approved = approve(first, ada)
    // The callback address alone, in a browser without the cookie or with another browser's, does nothing and uses nothing up.
    for (const cookie of ["", second.cookie, "cereal_google_verification=" + randomBytes(32).toString("base64url")]) {
      const foreign = await approve(first, ada, cookie).send()
      expect(foreign.headers.getSetCookie().some(value => value.includes("session_token"))).toBe(false)
    }
    expect(await status(first.cookie)).toEqual({ state: "pending", reason: null })
    await nothingCommitted()

    const finished = await approved.send()
    expect(finished.status).toBe(200)
    expect(finished.headers.get("content-type")).toContain("text/html")
    const session = cookiesOf(finished)
    expect(session).toContain("session_token=")
    expect(await status(first.cookie)).toEqual({ state: "done", reason: null })
    expect(await methods()).toEqual({ password: false, google: true, instanceName: "Cereal", setupRequired: false, transport: "secure" })
    expect(await signInSettings()).toMatchObject({ password: false, google: true, googleClientId: clientId, googleClientSecretSet: true })
    const who = await (await request("/api/v1/session", "GET", undefined, { Cookie: session })).json()
    expect(who.user).toMatchObject({ id: BOOTSTRAP_OWNER_ID, email: ownerEmail, name: "Ada Lovelace", role: "owner" })
    const accounts = await db!.account.findMany({ where: { userId: BOOTSTRAP_OWNER_ID } })
    expect(accounts.map(account => [account.providerId, account.password])).toEqual([["google", null]])
    expect((await db!.googleVerification.findUniqueOrThrow({ where: { stateHash: createHash("sha256").update(first.state).digest("hex") } })).encryptedSecret).toBeNull()

    // Replaying the callback gives nothing: no second session, no change.
    const replay = await approved.send()
    expect(replay.status).toBe(200)
    expect(replay.headers.getSetCookie().some(value => value.includes("session_token"))).toBe(false)
    expect(await db!.session.count({ where: { userId: BOOTSTRAP_OWNER_ID } })).toBe(1)

    // The second browser's claim arrives after the first: it is told, and changes nothing.
    await approve(second, { sub: "google-sub-other-" + run, email: `mallory-${run}@elsewhere.example`, email_verified: true }).send()
    expect(await status(second.cookie)).toEqual({ state: "failed", reason: "already_claimed" })
    expect((await db!.user.findUniqueOrThrow({ where: { id: BOOTSTRAP_OWNER_ID } })).email).toBe(ownerEmail)
    expect(await signInSettings()).toMatchObject({ google: true, googleClientId: clientId })

    // A claimed server cannot be claimed again, either way.
    expect((await beginClaim()).response.status).toBe(409)
    expect((await request("/api/v1/admin/owner/setup", "POST", { name: "Again", email: `again-${run}@example.test`, password }, { ...asOwner, "X-Forwarded-For": "203.0.113.250" })).status).toBe(409)
    // It is Google or nothing now: a password is refused even for the member who has one, and the setup code still works for the API.
    expect((await request("/api/auth/sign-in/email", "POST", { email: memberEmail, password }, { Origin: baseURL })).status).toBe(403)
    expect((await request("/api/v1/admin/settings", "GET", undefined, asOwner)).status).toBe(200)
  })

  test("the owner signs in with Google afterwards like anyone else, and their own address needs no entry on the list", async () => {
    const start = await request("/api/v1/auth/google/start")
    expect(start.status).toBe(303)
    const location = new URL(start.headers.get("location")!)
    const code = randomBytes(16).toString("hex"); issued.set(code, { identity: ada, challenge: location.searchParams.get("code_challenge")! })
    const back = await request(`/api/v1/auth/google/callback?state=${location.searchParams.get("state")}&code=${code}`, "GET", undefined, { Cookie: cookiesOf(start) })
    expect(back.status).toBe(303)
    expect(back.headers.get("location")).toBe("/notes")
    expect((await (await request("/api/v1/session", "GET", undefined, { Cookie: cookiesOf(back) })).json()).user.id).toBe(BOOTSTRAP_OWNER_ID)
    expect((await signInSettings() as unknown as { allowedEmails: string[] }).allowedEmails).toEqual([])
  })

  test("switching to email and password needs the administrator's own session and a password for them; then Google is off", async () => {
    const start = await request("/api/v1/auth/google/start"), location = new URL(start.headers.get("location")!)
    const code = randomBytes(16).toString("hex"); issued.set(code, { identity: ada, challenge: location.searchParams.get("code_challenge")! })
    const session = cookiesOf(await request(`/api/v1/auth/google/callback?state=${location.searchParams.get("state")}&code=${code}`, "GET", undefined, { Cookie: cookiesOf(start) }))
    const browser = { Cookie: session, Origin: baseURL }
    // Not a token, not another site, not a short password.
    expect((await request("/api/v1/admin/sign-in/password", "POST", { password }, asOwner)).status).toBe(403)
    expect((await request("/api/v1/admin/sign-in/password", "POST", { password }, { Cookie: session, Origin: "https://evil.example" })).status).toBe(403)
    expect((await request("/api/v1/admin/sign-in/password", "POST", { password: "short" }, browser)).status).toBe(400)
    expect(await methods()).toMatchObject({ password: false, google: true })
    const switched = await request("/api/v1/admin/sign-in/password", "POST", { password }, browser)
    expect(switched.status).toBe(200)
    expect((await switched.json()).signIn).toMatchObject({ password: true, google: false, googleClientSecretSet: true })
    expect(await methods()).toMatchObject({ password: true, google: false })
    // The administrator can get in the new way at once, and Google is no longer offered.
    const signedIn = await request("/api/auth/sign-in/email", "POST", { email: ownerEmail, password, rememberMe: false }, { Origin: baseURL })
    expect(signedIn.status).toBe(200)
    expect((await request("/api/v1/auth/google/start")).headers.get("location")).toBe("/login?error=google_unavailable")
    // The settings route cannot turn both on.
    expect((await request("/api/v1/admin/settings", "PATCH", { signIn: { password: true, google: true } }, asOwner)).status).toBe(400)
  })

  test("switching to Google is proved by a sign-in first, tied to the administrator who asked, and a failure leaves password in place", async () => {
    const signedIn = await request("/api/auth/sign-in/email", "POST", { email: ownerEmail, password, rememberMe: false }, { Origin: baseURL })
    const browser = { Cookie: cookiesOf(signedIn), Origin: baseURL }
    const member = cookiesOf(await request("/api/auth/sign-in/email", "POST", { email: memberEmail, password, rememberMe: false }, { Origin: baseURL }))
    const body = { clientId, clientSecret }
    expect((await request("/api/v1/admin/sign-in/google", "POST", body, asOwner)).status).toBe(403)
    expect((await request("/api/v1/admin/sign-in/google", "POST", body, { Cookie: browser.Cookie })).status).toBe(403)
    expect((await request("/api/v1/admin/sign-in/google", "POST", body, { Cookie: member, Origin: baseURL })).status).toBe(403)
    expect((await request("/api/v1/admin/sign-in/google", "POST", body, { Origin: baseURL })).status).toBe(401)
    async function beginSwitch(secret = clientSecret) {
      const response = await request("/api/v1/admin/sign-in/google", "POST", { clientId, clientSecret: secret }, browser)
      expect(response.status).toBe(200)
      const location = new URL((await response.clone().json()).authorizationUrl)
      return { response, cookie: cookiesOf(response), state: location.searchParams.get("state")!, challenge: location.searchParams.get("code_challenge")!, location }
    }
    // A wrong secret: still email and password.
    const wrong = await beginSwitch("GOCSPX-not-the-secret-at-all")
    await approve(wrong, ada).send()
    expect(await status(wrong.cookie)).toEqual({ state: "failed", reason: "invalid_client" })
    expect(await methods()).toMatchObject({ password: true, google: false })
    // A Google account that belongs to someone else here cannot be used to prove it.
    const others = await beginSwitch()
    await approve(others, { sub: "google-sub-grace-" + run, email: memberEmail, email_verified: true }).send()
    expect(await status(others.cookie)).toEqual({ state: "failed", reason: "google_account_in_use" })
    expect(await methods()).toMatchObject({ password: true, google: false })
    // Proved: Google is the method, password sign-in stops, and this administrator's Google account is theirs.
    const good = await beginSwitch()
    const finished = await approve(good, ada).send()
    expect(finished.status).toBe(200)
    expect(await status(good.cookie)).toEqual({ state: "done", reason: null })
    expect(await methods()).toMatchObject({ password: false, google: true })
    expect((await request("/api/auth/sign-in/email", "POST", { email: ownerEmail, password, rememberMe: false }, { Origin: baseURL })).status).toBe(403)
    expect(await db!.account.count({ where: { userId: BOOTSTRAP_OWNER_ID, providerId: "google" } })).toBe(1)
    // The session that asked is still good.
    expect((await request("/api/v1/session", "GET", undefined, { Cookie: browser.Cookie })).status).toBe(200)
  })

  test("claiming with email and password makes that the one method", async () => {
    await unclaim()
    expect(await methods()).toMatchObject({ password: true, google: false, setupRequired: true })
    const claimed = await request("/api/v1/admin/owner/setup", "POST", { name: "Ada Lovelace", email: ownerEmail, password }, { ...asOwner, "X-Forwarded-For": "203.0.113.251" })
    expect(claimed.status).toBe(200)
    expect(await methods()).toMatchObject({ password: true, google: false, setupRequired: false })
    expect(await db!.instanceSettings.findUniqueOrThrow({ where: { id: settingsId } })).toMatchObject({ signInPassword: true, signInGoogle: false, googleClientId: null })
    const accounts = await db!.account.findMany({ where: { userId: BOOTSTRAP_OWNER_ID } })
    expect(accounts.map(account => account.providerId)).toEqual(["credential"])
    // A Google claim is no longer possible.
    expect((await beginClaim()).response.status).toBe(409)
    // "Who may join" is not a step on a password server; it is one on a Google server.
    expect((await (await request("/api/v1/admin/settings", "GET", undefined, asOwner)).json()).onboarding.steps.signIn).toBe(true)
  })

  test("the migration keeps password where both were on, and leaves a Google-only server alone", async () => {
    const { readFile } = await import("node:fs/promises"), { resolve } = await import("node:path")
    const sql = await readFile(resolve(import.meta.dir, "../../../packages/database/prisma/migrations/20261129000000_one_sign_in_method/migration.sql"), "utf8")
    const rule = /UPDATE "InstanceSettings"[\s\S]*?;/.exec(sql)?.[0]
    expect(rule).toBeDefined()
    const seen = new Map<string, [boolean, boolean]>()
    class Undo extends Error {}
    await db!.$transaction(async tx => {
      await tx.instanceSettings.createMany({ data: [
        { id: "method-both", signInPassword: true, signInGoogle: true }, { id: "method-google", signInPassword: false, signInGoogle: true },
        { id: "method-password", signInPassword: true, signInGoogle: false },
      ] })
      await tx.$executeRawUnsafe(rule!)
      for (const row of await tx.instanceSettings.findMany({ where: { id: { startsWith: "method-" } } })) seen.set(row.id, [row.signInPassword, row.signInGoogle])
      throw new Undo()
    }).catch(error => { if (!(error instanceof Undo)) throw error })
    expect(Object.fromEntries(seen)).toEqual({ "method-both": [true, false], "method-google": [false, true], "method-password": [true, false] })
  })
})
