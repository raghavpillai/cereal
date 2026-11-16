import { afterAll, describe, expect, test } from "bun:test"
import { createHash, randomBytes } from "node:crypto"
import { makeAuthenticationService, BOOTSTRAP_OWNER_ID } from "../src/auth"
import { makePrismaRepository } from "../src/prisma-repository"
import { makePrismaCalendarStore } from "../src/calendar-store"
import { makeAdministrationService } from "../src/administration"
import { secretBoxFromKey } from "../src/admin-secrets"
import { makeGoogleService } from "../src/google"
import { makePrismaGoogleStore } from "../src/google-store"
import { DEFAULT_ORGANIZATION_ID } from "../src/organizations"
import { makeApiHandler } from "../src/api"

const enabled = process.env.RUN_AUTH_PG_TESTS === "1" && !!process.env.DATABASE_URL
const challengeOf = (verifier: string) => createHash("sha256").update(verifier).digest("base64url")

/** Runs only in a dedicated throwaway database. Google is a local stand-in: an authorize URL the test reads,
 * and token and userinfo endpoints served by the injected fetch. Nothing here contacts Google. */
describe.skipIf(!enabled)("sign-in settings, Google sign-in and the app handoff on PostgreSQL", () => {
  const resources = enabled ? makePrismaRepository(process.env.DATABASE_URL!) : null, db = resources?.database
  const ownerToken = "isolated-sign-in-bootstrap-token-at-least-32-characters", baseURL = "http://localhost:3000"
  const clientId = "1234567890-fixture.apps.googleusercontent.com", clientSecret = "GOCSPX-fixture-client-secret-value"
  const password = "test-only-password-" + randomBytes(12).toString("hex")
  let skew = 0
  const clock = () => new Date(Date.now() + skew)

  // The stand-in for Google: codes it has issued, and what its token endpoint was sent.
  interface Identity { sub: string; email: string; email_verified: boolean; name?: string }
  const issued = new Map<string, { identity: Identity; challenge: string }>(), tokens = new Map<string, Identity>()
  const tokenRequests: URLSearchParams[] = []
  const fakeGoogle = Object.assign(async (...[input, init]: Parameters<typeof fetch>) => {
    const request = new Request(input, init), url = new URL(request.url)
    if (url.href === "https://google.test/token") {
      const form = new URLSearchParams(await request.text()); tokenRequests.push(form)
      const grant = issued.get(form.get("code") ?? ""); issued.delete(form.get("code") ?? "")
      if (!grant || form.get("client_id") !== clientId || form.get("client_secret") !== clientSecret || form.get("redirect_uri") !== baseURL + "/api/v1/auth/google/callback" || challengeOf(form.get("code_verifier") ?? "") !== grant.challenge) return Response.json({ error: "invalid_grant", error_description: "PRIVATE_PROVIDER_TEXT" }, { status: 400 })
      const access = randomBytes(16).toString("hex"); tokens.set(access, grant.identity)
      return Response.json({ access_token: access, token_type: "Bearer", expires_in: 3600 })
    }
    if (url.href === "https://google.test/userinfo") {
      const identity = tokens.get((request.headers.get("authorization") ?? "").replace(/^Bearer /, ""))
      return identity ? Response.json(identity) : Response.json({ error: "invalid_token" }, { status: 401 })
    }
    return new Response("unexpected", { status: 500 })
  }, { preconnect: () => {} }) as typeof fetch

  const box = secretBoxFromKey(randomBytes(32)), store = db ? makePrismaCalendarStore(db, "sign-in-test") : null
  const administration = store ? makeAdministrationService(store, box, { baseURL, environment: {} }) : null
  const authentication = db ? makeAuthenticationService(db, ownerToken, { secret: randomBytes(32).toString("base64url"), baseURL, signIn: () => administration!.signInPolicy(), clock,
    google: { endpoints: { authorization: "https://google.test/authorize", token: "https://google.test/token", userinfo: "https://google.test/userinfo" }, fetch: fakeGoogle } }) : null
  const google = db ? makeGoogleService(makePrismaGoogleStore(db), box, { baseURL, credentials: () => administration!.googleCredentials() }) : null
  const api = resources ? makeApiHandler(resources.repository, ownerToken, { authentication: authentication!, administration: administration!, google: google! }) : null
  afterAll(async () => { await api?.dispose(); await resources?.close() })

  const request = (path: string, method = "GET", body?: unknown, headers: Record<string, string> = {}) => api!.handler(new Request(baseURL + path, { method, redirect: "manual", headers: { ...(body === undefined ? {} : { "Content-Type": "application/json" }), ...headers }, ...(body === undefined ? {} : { body: JSON.stringify(body) }) }))
  const asOwner = { Authorization: "Bearer " + ownerToken }
  const settings = (signIn: unknown, headers: Record<string, string> = asOwner) => request("/api/v1/admin/settings", "PATCH", { signIn }, headers)
  const cookiesOf = (response: Response) => response.headers.getSetCookie().filter(value => !/Max-Age=0/i.test(value)).map(value => value.split(";")[0]).join("; ")
  async function passwordSignIn(email: string) { return request("/api/auth/sign-in/email", "POST", { email, password, rememberMe: true }, { Origin: baseURL }) }
  async function createUser(name: string, email: string) { const response = await request("/api/v1/admin/users", "POST", { name, email, password }, asOwner); expect(response.status).toBe(200); return (await response.json()).id as string }
  const whoAmI = async (headers: Record<string, string>) => { const response = await request("/api/v1/session", "GET", undefined, headers); return response.status === 200 ? (await response.json()).user as { id: string; email: string; role: string } : null }

  /** Starts a Google sign-in as a browser would and returns what the browser now holds. */
  async function startGoogle(returnPath?: string) {
    const response = await request("/api/v1/auth/google/start" + (returnPath === undefined ? "" : "?return=" + encodeURIComponent(returnPath)))
    expect(response.status).toBe(303)
    const location = new URL(response.headers.get("location")!, baseURL)
    return { response, location, state: location.searchParams.get("state") ?? "", challenge: location.searchParams.get("code_challenge") ?? "", cookie: cookiesOf(response) }
  }
  /** The person approves at the stand-in, which sends the browser back with a code. */
  async function finishGoogle(started: Awaited<ReturnType<typeof startGoogle>>, identity: Identity, options: { cookie?: string; state?: string } = {}) {
    const code = randomBytes(16).toString("hex"); issued.set(code, { identity, challenge: started.challenge })
    const cookie = options.cookie ?? started.cookie
    return request(`/api/v1/auth/google/callback?state=${encodeURIComponent(options.state ?? started.state)}&code=${code}`, "GET", undefined, cookie ? { Cookie: cookie } : {})
  }
  const googleSignIn = async (identity: Identity, returnPath?: string) => finishGoogle(await startGoogle(returnPath), identity)

  let aliceId = "", aliceCookie = "", aliceToken = "", bobId = "", bobCookie = ""

  test("sign-in settings: defaults, validation, and a client secret that is sealed and never returned", async () => {
    const initial = await (await request("/api/v1/admin/settings", "GET", undefined, asOwner)).json()
    expect(initial.signIn).toEqual({ password: true, google: false, allowedEmails: [], googleClientId: null, googleClientSecretSet: false, googleRedirectUris: [baseURL + "/api/v1/auth/google/callback", baseURL + "/api/integrations/google/callback"], googleNeedsHttps: false })
    expect(await (await request("/api/v1/auth/methods")).json()).toEqual({ password: true, google: false, instanceName: initial.instanceName, setupRequired: expect.any(Boolean), transport: "secure" })

    expect((await settings({ password: false })).status).toBe(400) // turning password off is choosing Google, which has no client yet
    expect((await settings({ password: true, google: true, googleClientId: clientId, googleClientSecret: clientSecret })).status).toBe(400) // one method, never both
    expect((await settings({ google: true })).status).toBe(400) // no client stored yet
    expect((await settings({ google: true, googleClientId: clientId })).status).toBe(400) // a client id alone is not enough
    expect((await settings({ allowedEmails: ["not-an-address"] })).status).toBe(400)
    expect((await settings({ allowedEmails: ["someone@example"] })).status).toBe(400)
    expect((await settings({ allowedEmails: Array.from({ length: 501 }, (_, index) => `person${index}@example.test`) })).status).toBe(400)
    expect((await settings({ googleClientId: "has spaces in it" })).status).toBe(400)
    expect((await settings({ googleClientSecret: "short" })).status).toBe(400)
    expect((await (await request("/api/v1/admin/settings", "GET", undefined, asOwner)).json()).signIn).toEqual(initial.signIn) // nothing was half-saved

    const listed = await settings({ allowedEmails: ["Carol@SignIn.Test", "@Team.Example", "carol@signin.test"] })
    expect(listed.status).toBe(200)
    expect((await listed.json()).signIn.allowedEmails).toEqual(["carol@signin.test", "@team.example"])

    const saved = await settings({ googleClientId: clientId, googleClientSecret: clientSecret, google: true })
    const savedText = await saved.text()
    expect(saved.status).toBe(200)
    expect(savedText).not.toContain(clientSecret)
    expect(JSON.parse(savedText).signIn).toMatchObject({ google: true, googleClientId: clientId, googleClientSecretSet: true })
    expect(await (await request("/api/v1/admin/settings", "GET", undefined, asOwner)).text()).not.toContain(clientSecret)
    const row = await db!.instanceSettings.findUniqueOrThrow({ where: { id: "sign-in-test" } })
    expect(row.googleClientSecret).toStartWith("v1.")
    expect(JSON.stringify(row)).not.toContain(clientSecret)
    // Choosing Google turned password off: a server has exactly one way to sign in.
    expect(await (await request("/api/v1/auth/methods")).json()).toMatchObject({ password: false, google: true })

    expect((await settings({ googleClientSecret: null })).status).toBe(400) // not while Google sign-in depends on it
    // The same stored client is what the Google Calendar connection uses, read at the time of use.
    expect(await administration!.googleCredentials()).toEqual({ clientId, clientSecret })
  })

  test("only administrators change sign-in settings, and a cookie needs a trusted origin to do it", async () => {
    // Back to email and password for the accounts this file signs in with.
    expect((await settings({ password: true })).status).toBe(200)
    expect(await (await request("/api/v1/auth/methods")).json()).toMatchObject({ password: true, google: false })
    aliceId = await createUser("Alice", "alice@signin.test"); bobId = await createUser("Bob", "bob@signin.test")
    const alice = await passwordSignIn("alice@signin.test"); expect(alice.status).toBe(200)
    aliceCookie = cookiesOf(alice); aliceToken = alice.headers.get("set-auth-token")!
    const bob = await passwordSignIn("bob@signin.test"); expect(bob.status).toBe(200); bobCookie = cookiesOf(bob)
    expect((await settings({ password: true }, { Authorization: "Bearer " + aliceToken })).status).toBe(403)
    expect((await settings({ password: true }, { Cookie: aliceCookie, Origin: baseURL })).status).toBe(403)
    expect((await settings({ password: true }, {})).status).toBe(401)
    expect((await request("/api/v1/admin/settings", "GET", undefined, { Authorization: "Bearer " + aliceToken })).status).toBe(403)
  })

  test("with Google as the method, password sign-in refuses everyone, keeps sessions and the bootstrap owner token, and comes back when chosen again", async () => {
    expect((await settings({ password: false })).status).toBe(200)
    expect(await (await request("/api/v1/auth/methods")).json()).toMatchObject({ password: false, google: true })
    const refused = await passwordSignIn("alice@signin.test")
    expect(refused.status).toBe(403)
    expect(refused.headers.getSetCookie()).toEqual([])
    expect(refused.headers.get("set-auth-token")).toBeNull()
    // The same answer for a wrong password and for an address with no account: nothing is looked up.
    expect((await request("/api/auth/sign-in/email", "POST", { email: "alice@signin.test", password: "definitely-the-wrong-password" }, { Origin: baseURL })).status).toBe(403)
    expect((await request("/api/auth/sign-in/email", "POST", { email: "nobody@signin.test", password }, { Origin: baseURL })).status).toBe(403)
    // People who are already signed in stay signed in, and the operator's token is untouched.
    expect((await whoAmI({ Authorization: "Bearer " + aliceToken }))?.id).toBe(aliceId)
    expect((await whoAmI({ Cookie: aliceCookie }))?.id).toBe(aliceId)
    expect((await whoAmI(asOwner))?.id).toBe(BOOTSTRAP_OWNER_ID)
    expect((await settings({ google: false, password: false })).status).toBe(400) // that would leave no way in
    expect((await settings({ google: false })).status).toBe(200) // turning Google off is choosing email and password
    expect(await (await request("/api/v1/auth/methods")).json()).toMatchObject({ password: true, google: false })
    expect((await passwordSignIn("alice@signin.test")).status).toBe(200)
    // The tests that follow are about Google sign-in.
    expect((await settings({ google: true })).status).toBe(200)
  })

  test("Google sign-in sends the browser to Google with PKCE and a browser-bound state, storing neither in the clear", async () => {
    const started = await startGoogle("/notes/abc?tab=notes")
    expect(started.location.origin + started.location.pathname).toBe("https://google.test/authorize")
    expect(Object.fromEntries(started.location.searchParams)).toMatchObject({ client_id: clientId, redirect_uri: baseURL + "/api/v1/auth/google/callback", response_type: "code", scope: "openid email profile", code_challenge_method: "S256" })
    expect(started.state).toMatch(/^[A-Za-z0-9_-]{43}$/); expect(started.challenge).toMatch(/^[A-Za-z0-9_-]{43}$/)
    expect(started.location.href).not.toContain(clientSecret)
    const set = started.response.headers.getSetCookie().join("\n")
    expect(set).toContain("cereal_google_sign_in="); expect(set).toContain("HttpOnly"); expect(set).toContain("SameSite=Lax")
    const rows = await db!.googleSignInState.findMany()
    expect(rows).toHaveLength(1)
    expect(JSON.stringify(rows)).not.toContain(started.state)
    expect(JSON.stringify(rows)).not.toContain(started.cookie.split("=")[1]!)
    expect(rows[0]!.returnPath).toBe("/notes/abc?tab=notes")
    await db!.googleSignInState.deleteMany()
  })

  test("the allow list decides who may create an account: a listed address and a listed domain are accepted, anyone else is refused", async () => {
    const stranger = await googleSignIn({ sub: "g-stranger", email: "stranger@elsewhere.test", email_verified: true, name: "Stranger" })
    expect(stranger.status).toBe(303); expect(stranger.headers.get("location")).toBe("/login?error=not_allowed")
    expect(cookiesOf(stranger)).toBe("")
    expect(await db!.user.findUnique({ where: { email: "stranger@elsewhere.test" } })).toBeNull()

    const carol = await googleSignIn({ sub: "g-carol", email: "Carol@SignIn.Test", email_verified: true, name: "Carol Example" }, "/notes/abc?tab=notes")
    expect(carol.status).toBe(303); expect(carol.headers.get("location")).toBe("/notes/abc?tab=notes")
    const carolUser = await db!.user.findUniqueOrThrow({ where: { email: "carol@signin.test" }, include: { accounts: true, memberships: true } })
    expect(carolUser).toMatchObject({ name: "Carol Example", role: "user", emailVerified: true })
    expect(carolUser.accounts.map(account => [account.providerId, account.accountId, account.password])).toEqual([["google", "g-carol", null]])
    expect(carolUser.memberships.map(member => [member.organizationId, member.role])).toEqual([[DEFAULT_ORGANIZATION_ID, "member"]])
    // The cookie is a normal session: HttpOnly, SameSite=Lax, and accepted by the API.
    const set = carol.headers.getSetCookie().join("\n")
    expect(set).toContain("session_token="); expect(set).toContain("HttpOnly"); expect(set).toContain("SameSite=Lax")
    expect(await whoAmI({ Cookie: cookiesOf(carol) })).toMatchObject({ id: carolUser.id, role: "user" })

    const dana = await googleSignIn({ sub: "g-dana", email: "dana@team.example", email_verified: true })
    expect(dana.headers.get("location")).toBe("/notes")
    expect(await whoAmI({ Cookie: cookiesOf(dana) })).toMatchObject({ email: "dana@team.example", role: "user" })
    // A domain entry is the whole domain, not a suffix of another one.
    expect((await googleSignIn({ sub: "g-eve", email: "eve@notteam.example", email_verified: true })).headers.get("location")).toBe("/login?error=not_allowed")
    expect((await googleSignIn({ sub: "g-eve2", email: "eve@team.example.evil.test", email_verified: true })).headers.get("location")).toBe("/login?error=not_allowed")

    // Taking an address off the list stops new accounts; the account that exists keeps signing in.
    expect((await settings({ allowedEmails: [] })).status).toBe(200)
    expect((await googleSignIn({ sub: "g-frank", email: "frank@team.example", email_verified: true })).headers.get("location")).toBe("/login?error=not_allowed")
    const again = await googleSignIn({ sub: "g-carol", email: "carol@signin.test", email_verified: true })
    expect(await whoAmI({ Cookie: cookiesOf(again) })).toMatchObject({ id: carolUser.id })
    expect(await db!.user.count({ where: { email: "carol@signin.test" } })).toBe(1)
  })

  test("an existing account signs in by its verified address without being on the list, and stays linked to that Google identity", async () => {
    const first = await googleSignIn({ sub: "g-alice", email: "ALICE@signin.test", email_verified: true, name: "Someone Else" })
    expect(first.headers.get("location")).toBe("/notes")
    expect(await whoAmI({ Cookie: cookiesOf(first) })).toMatchObject({ id: aliceId, email: "alice@signin.test" })
    const alice = await db!.user.findUniqueOrThrow({ where: { id: aliceId }, include: { accounts: true } })
    expect(alice.name).toBe("Alice") // the account is not rewritten from Google's profile
    expect(alice.accounts.map(account => account.providerId).sort()).toEqual(["credential", "google"])
    // The link is the Google subject: a later sign-in reaches the same account even if Google reports a new address.
    const moved = await googleSignIn({ sub: "g-alice", email: "alice.renamed@elsewhere.test", email_verified: true })
    expect(await whoAmI({ Cookie: cookiesOf(moved) })).toMatchObject({ id: aliceId })
    expect(await db!.account.count({ where: { userId: aliceId, providerId: "google" } })).toBe(1)

    // An address Google has not verified proves nothing, whether or not an account uses it.
    const unverified = await googleSignIn({ sub: "g-impostor", email: "bob@signin.test", email_verified: false })
    expect(unverified.headers.get("location")).toBe("/login?error=failed"); expect(cookiesOf(unverified)).toBe("")
    expect(await db!.account.count({ where: { userId: bobId, providerId: "google" } })).toBe(0)
  })

  test("a Google sign-in cannot be replayed, moved to another browser, redirected off-site, or finished after Google is turned off", async () => {
    const identity = { sub: "g-alice", email: "alice@signin.test", email_verified: true }
    const once = await startGoogle()
    expect((await finishGoogle(once, identity)).headers.get("location")).toBe("/notes")
    const replay = await finishGoogle(once, identity)
    expect(replay.headers.get("location")).toBe("/login?error=failed"); expect(cookiesOf(replay)).toBe("")

    // The state belongs to the browser that started it: no cookie, or another browser's cookie, is refused.
    const mine = await startGoogle(), theirs = await startGoogle()
    expect((await finishGoogle(mine, identity, { cookie: "" })).headers.get("location")).toBe("/login?error=failed")
    expect((await finishGoogle(theirs, identity, { cookie: mine.cookie })).headers.get("location")).toBe("/login?error=failed")
    expect((await request("/api/v1/auth/google/callback?state=" + "A".repeat(43) + "&code=anything", "GET", undefined, { Cookie: mine.cookie })).headers.get("location")).toBe("/login?error=failed")
    // A request without the browser's cookie does not spend the state: the browser that started it can still finish.
    expect((await finishGoogle(mine, identity)).headers.get("location")).toBe("/notes")

    // Google's own token endpoint is the only thing that turns a code into an identity, and it checks the verifier.
    const forged = await startGoogle()
    const wrongChallenge = await finishGoogle({ ...forged, challenge: challengeOf("some other verifier") }, identity)
    expect(wrongChallenge.headers.get("location")).toBe("/login?error=failed")
    expect(await wrongChallenge.text()).not.toContain("PRIVATE_PROVIDER_TEXT")
    expect(wrongChallenge.headers.get("location")).not.toContain("PRIVATE_PROVIDER_TEXT")

    const cancelled = await startGoogle()
    expect((await request(`/api/v1/auth/google/callback?state=${cancelled.state}&error=access_denied`, "GET", undefined, { Cookie: cancelled.cookie })).headers.get("location")).toBe("/login?error=cancelled")

    for (const target of ["https://evil.example/steal", "//evil.example/steal", "/\\evil.example", "/.//evil.example/steal", "/%2e%2e//evil.example/steal", "/notes/%2E%2E/%2e%2e//evil.example", "https:evil.example", "/api/auth/sign-out", "javascript:alert(1)", "/notes\r\nSet-Cookie: x=1", "/\t/evil.example"]) {
      const response = await googleSignIn(identity, target)
      expect(response.headers.get("location")).toBe("/notes")
      expect(response.headers.getSetCookie().join("\n")).not.toContain("x=1")
    }
    // A shared link someone followed while signed out: after Google they are back on it. Encoded forms stay on this origin.
    const sharedLink = "/s/" + randomBytes(32).toString("base64url")
    expect((await googleSignIn(identity, sharedLink)).headers.get("location")).toBe(sharedLink)
    for (const target of ["/%2f%2fevil.example", "/%5Cevil.example"]) expect(new URL((await googleSignIn(identity, target)).headers.get("location")!, baseURL).origin).toBe(baseURL)
    // A refusal in the middle of an app handoff goes back to the same handoff, with the reason.
    const appReturn = "/login?app=1&challenge=" + "c".repeat(43) + "&state=app-state"
    expect((await googleSignIn({ sub: "g-zed", email: "zed@elsewhere.test", email_verified: true }, appReturn)).headers.get("location")).toBe(appReturn + "&error=not_allowed")
    expect((await googleSignIn({ sub: "g-zed", email: "zed@elsewhere.test", email_verified: true }, "/notes/abc")).headers.get("location")).toBe("/login?return=%2Fnotes%2Fabc&error=not_allowed")

    const late = await startGoogle()
    expect((await settings({ google: false })).status).toBe(200)
    expect((await finishGoogle(late, identity)).headers.get("location")).toBe("/login?error=google_unavailable")
    expect((await request("/api/v1/auth/google/start")).headers.get("location")).toBe("/login?error=google_unavailable")
    expect((await request("/api/v1/auth/google/start", "POST", {}, { Origin: baseURL })).status).toBe(405)
    expect((await settings({ google: true })).status).toBe(200)
    // None of the single-use rows outlive their sign-in.
    expect(await db!.googleSignInState.count()).toBe(0)
    expect(tokenRequests.every(form => form.get("grant_type") === "authorization_code" && (form.get("code_verifier") ?? "").length >= 43)).toBe(true)
  })

  test("the app handoff needs a browser session and a trusted origin, and its code is stored only as a hash", async () => {
    const verifier = randomBytes(32).toString("base64url"), challenge = challengeOf(verifier), path = "/api/v1/auth/app-handoff"
    expect((await request(path, "POST", { challenge })).status).toBe(401)
    expect((await request(path, "POST", { challenge }, { Cookie: aliceCookie })).status).toBe(403) // no Origin
    expect((await request(path, "POST", { challenge }, { Cookie: aliceCookie, Origin: "https://evil.example" })).status).toBe(403)
    expect((await request(path, "POST", { challenge }, { Authorization: "Bearer " + aliceToken })).status).toBe(403) // an app cannot mint for itself
    expect((await request(path, "POST", { challenge }, asOwner)).status).toBe(403) // nor can the bootstrap token
    for (const bad of ["", "short", challenge + "A", challenge.slice(0, 42) + "!"]) {
      const response = await request(path, "POST", { challenge: bad }, { Cookie: aliceCookie, Origin: baseURL })
      expect(response.status).toBe(400); expect((await response.json()).error).toBe("invalid_request")
    }
    expect(await db!.appSignInHandoff.count()).toBe(0)
    const created = await request(path, "POST", { challenge }, { Cookie: aliceCookie, Origin: baseURL })
    expect(created.status).toBe(200)
    const { code } = await created.json() as { code: string }
    expect(code).toMatch(/^[A-Za-z0-9_-]{43}$/)
    const rows = await db!.appSignInHandoff.findMany()
    expect(rows).toHaveLength(1)
    expect(rows[0]).toMatchObject({ userId: aliceId, challenge, codeHash: createHash("sha256").update(code).digest("hex") })
    expect(JSON.stringify(rows)).not.toContain(code)
    expect(Math.round((rows[0]!.expiresAt.getTime() - Date.now()) / 1000)).toBeGreaterThan(110)
    expect(Math.round((rows[0]!.expiresAt.getTime() - Date.now()) / 1000)).toBeLessThanOrEqual(120)

    // The app redeems it and holds a bearer session of its own for the same person.
    const exchanged = await request(path + "/exchange", "POST", { code, verifier }, { "X-Forwarded-For": "198.51.100.1" })
    expect(exchanged.status).toBe(200)
    const { token } = await exchanged.json() as { token: string }
    expect(exchanged.headers.getSetCookie()).toEqual([]) // the app gets a token, not this browser's cookie
    expect(await whoAmI({ Authorization: "Bearer " + token })).toMatchObject({ id: aliceId })
    expect(token).not.toBe(aliceToken)
    expect(token.includes(".")).toBe(true) // signed, like the set-auth-token value after password sign-in
    // Single use.
    const second = await request(path + "/exchange", "POST", { code, verifier }, { "X-Forwarded-For": "198.51.100.1" })
    expect(second.status).toBe(401); expect((await second.json()).error).toBe("invalid_code")
    expect(await db!.appSignInHandoff.count()).toBe(0)
    // Signing the browser out does not sign the app out: they are separate sessions.
    const sessions = await db!.session.findMany({ where: { userId: aliceId } })
    expect(new Set(sessions.map(session => session.id)).size).toBe(sessions.length)
  })

  test("a handoff code fails with the wrong verifier, after 120 seconds, when malformed, and for anyone but the account it was made for", async () => {
    const path = "/api/v1/auth/app-handoff", from = (address: string) => ({ "X-Forwarded-For": address })
    async function handoff(cookie: string) {
      const verifier = randomBytes(32).toString("base64url")
      const response = await request(path, "POST", { challenge: challengeOf(verifier) }, { Cookie: cookie, Origin: baseURL }); expect(response.status).toBe(200)
      return { verifier, code: (await response.json()).code as string }
    }
    const exchange = (code: string, verifier: string, headers: Record<string, string>) => request(path + "/exchange", "POST", { code, verifier }, headers)

    // Wrong verifier: refused, and the code is spent, so the right verifier no longer works either.
    const guessed = await handoff(aliceCookie)
    const wrong = await exchange(guessed.code, randomBytes(32).toString("base64url"), from("198.51.100.2"))
    expect(wrong.status).toBe(401); expect((await wrong.json()).error).toBe("invalid_code")
    expect((await exchange(guessed.code, guessed.verifier, from("198.51.100.2"))).status).toBe(401)

    // Expired: 119 seconds is in time, 121 is not.
    const inTime = await handoff(aliceCookie), late = await handoff(aliceCookie)
    skew = 119_000; expect((await exchange(inTime.code, inTime.verifier, from("198.51.100.3"))).status).toBe(200)
    skew = 121_000; expect((await exchange(late.code, late.verifier, from("198.51.100.3"))).status).toBe(401)
    skew = 0

    // Bob cannot redeem Alice's code: his session, cookie and own verifier are worth nothing here.
    const alices = await handoff(aliceCookie), bobs = await handoff(bobCookie)
    expect((await exchange(alices.code, bobs.verifier, { ...from("198.51.100.4"), Cookie: bobCookie })).status).toBe(401)
    // And a code is for the account that asked for it, whoever sends the request.
    const forBob = await exchange(bobs.code, bobs.verifier, { ...from("198.51.100.4"), Cookie: aliceCookie })
    expect(forBob.status).toBe(200)
    expect(await whoAmI({ Authorization: "Bearer " + (await forBob.json()).token })).toMatchObject({ id: bobId })

    for (const [code, verifier] of [["", ""], ["short", guessed.verifier], [guessed.code, "short"], [guessed.code + "A", guessed.verifier], [guessed.code, "has spaces " + guessed.verifier]]) {
      const response = await exchange(code!, verifier!, from("198.51.100.5"))
      expect(response.status).toBe(400); expect((await response.json()).error).toBe("invalid_request")
    }
    // A person keeps at most five codes waiting; older ones give way.
    for (let index = 0; index < 7; index++) await handoff(aliceCookie)
    expect(await db!.appSignInHandoff.count({ where: { userId: aliceId } })).toBe(5)
    // An account that is gone takes its codes with it.
    expect((await settings({ password: true })).status).toBe(200) // this account signs in with a password
    const doomedId = await createUser("Doomed", "doomed@signin.test"), doomedSignIn = await passwordSignIn("doomed@signin.test")
    const doomed = await handoff(cookiesOf(doomedSignIn))
    await db!.user.delete({ where: { id: doomedId } })
    expect((await exchange(doomed.code, doomed.verifier, from("198.51.100.6"))).status).toBe(401)
  })

  test("redeeming codes is rate limited per client", async () => {
    const statuses: number[] = []
    for (let index = 0; index < 12; index++) statuses.push((await request("/api/v1/auth/app-handoff/exchange", "POST", { code: randomBytes(32).toString("base64url"), verifier: randomBytes(32).toString("base64url") }, { "X-Forwarded-For": "203.0.113.9" })).status)
    expect(statuses.slice(0, 10)).toEqual(Array(10).fill(401))
    expect(statuses.slice(10)).toEqual([429, 429])
    // Another client is not affected.
    expect((await request("/api/v1/auth/app-handoff/exchange", "POST", { code: randomBytes(32).toString("base64url"), verifier: randomBytes(32).toString("base64url") }, { "X-Forwarded-For": "203.0.113.10" })).status).toBe(401)
  })
})
