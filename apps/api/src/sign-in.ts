import { createHash, randomBytes, timingSafeEqual } from "node:crypto"
import type { Database } from "@meeting-notes/database"
import type { SecretBox } from "./admin-secrets"

/** What sign-in enforces right now. `google` is true only while Google is on and its OAuth client is stored. */
export interface SignInPolicy {
  password: boolean
  google: boolean
  allowedEmails: ReadonlyArray<string>
  googleCredentials: { clientId: string; clientSecret: string } | null
  instanceName: string
}
export const passwordOnly: SignInPolicy = { password: true, google: false, allowedEmails: [], googleCredentials: null, instanceName: "Cereal" }

/** One allow-list entry, lower-cased: a full address or "@domain". Null when it is neither. */
export function normalizeAllowedEmail(raw: string): string | null {
  const entry = raw.trim().toLowerCase()
  return entry.length <= 254 && /^[^\s@]*@[^\s@.]+(?:\.[^\s@.]+)+$/.test(entry) ? entry : null
}
/** Whether a verified address may create an account by signing in with Google. */
export function emailAllowed(email: string, allowed: ReadonlyArray<string>): boolean {
  const address = email.trim().toLowerCase(), at = address.lastIndexOf("@")
  if (at < 1) return false
  const domain = address.slice(at)
  return allowed.some(entry => entry.startsWith("@") ? entry === domain : entry === address)
}

/** Where a browser may be sent after signing in: a path on this origin, never another site and never the API. */
export function safeReturnPath(raw: string | null | undefined, origin: string): string | null {
  // The same bound as the web app: an AI client's signed connection query passes through sign-in.
  if (!raw || raw.length > 16_384 || !raw.startsWith("/") || raw.startsWith("//") || /[\\\u0000-\u001f\u007f]/.test(raw)) return null
  try {
    const url = new URL(raw, origin)
    if (url.origin !== origin) return null
    // Dot segments are resolved by now ("/.//host" becomes "//host"), so the result is checked, not the input.
    const path = url.pathname + url.search + url.hash
    return path.startsWith("//") || path === "/api" || path.startsWith("/api/") ? null : path
  } catch { return null }
}

export type SignInRefusal = "not_allowed" | "google_unavailable" | "cancelled" | "failed"
export class SignInFailure extends Error {
  constructor(readonly code: "invalid_request" | "invalid_code" | "rate_limited" | "forbidden" | "unavailable", message: string) { super(message) }
}

export interface GoogleEndpoints { authorization: string; token: string; userinfo: string }
export const googleEndpoints: GoogleEndpoints = {
  authorization: "https://accounts.google.com/o/oauth2/v2/auth",
  token: "https://oauth2.googleapis.com/token",
  userinfo: "https://openidconnect.googleapis.com/v1/userinfo",
}

const sha = (value: string) => createHash("sha256").update(value).digest("hex")
const challengeOf = (verifier: string) => createHash("sha256").update(verifier).digest("base64url")
const token43 = /^[A-Za-z0-9_-]{43}$/
const sameText = (a: string, b: string) => { const left = Buffer.from(a), right = Buffer.from(b); return left.length === right.length && timingSafeEqual(left, right) }
const text = (value: unknown, max: number) => typeof value === "string" && value.trim() ? value.trim().slice(0, max) : null
const duplicate = (error: unknown) => (error as { code?: string })?.code === "P2002"
/** Deleting a row that is already gone is an answer (null); anything else is a database failure. */
const alreadyGone = (error: unknown) => { if ((error as { code?: string })?.code === "P2025") return null; throw error }

const STATE_MINUTES = 10, HANDOFF_SECONDS = 120, DEFAULT_RETURN = "/notes"

/** A fixed window per key. Old windows are dropped once the table grows, so it stays bounded. */
function rateLimiter(limit: number, windowMs: number, clock: () => Date) {
  const windows = new Map<string, { count: number; reset: number }>()
  return (key: string) => {
    const now = clock().getTime()
    if (windows.size > 5000) for (const [name, window] of windows) if (window.reset <= now) windows.delete(name)
    const window = windows.get(key)
    if (!window || window.reset <= now) { if (windows.size <= 20000) windows.set(key, { count: 1, reset: now + windowMs }); return true }
    return ++window.count <= limit
  }
}

/** Google sign-in and the browser-to-app handoff. Sessions are created by Better Auth (`issueSession`), so the
 * cookies and bearer tokens issued here are the same kind password sign-in issues. Authorization codes, state
 * values, tokens and provider error text never reach logs, redirects or responses. */
export function makeSignInService(deps: {
  db: Database
  /** Seals the PKCE verifier while a Google sign-in is in progress. */
  box: SecretBox
  /** The public origin: scheme, host and port. */
  origin: string
  policy: () => Promise<SignInPolicy>
  /** Creates a session for an account and returns its Set-Cookie headers and its bearer token. */
  issueSession: (userId: string, remember: boolean, request: Headers) => Promise<{ cookies: string[]; token: string }>
  defaultOrganizationId: string
  fetch?: typeof fetch
  clock?: () => Date
  /** Google's endpoints. Only tests point these elsewhere. */
  endpoints?: GoogleEndpoints
}) {
  const { db, box, origin } = deps
  const now = deps.clock ?? (() => new Date()), transport = deps.fetch ?? globalThis.fetch, endpoints = deps.endpoints ?? googleEndpoints
  const secure = origin.startsWith("https:"), redirectUri = origin + "/api/v1/auth/google/callback"
  // "__Host-" pins the cookie to this exact host over HTTPS; a loopback development origin cannot use it.
  const stateCookie = (secure ? "__Host-" : "") + "cereal_google_sign_in"
  const cookie = (value: string, maxAge: number) => `${stateCookie}=${value}; Path=/; Max-Age=${maxAge}; HttpOnly; SameSite=Lax${secure ? "; Secure" : ""}`
  const exchangeByClient = rateLimiter(10, 60_000, now), exchangeOverall = rateLimiter(1200, 60_000, now)

  function redirect(location: string, cookies: ReadonlyArray<string> = []) {
    const headers = new Headers({ Location: location, "Cache-Control": "no-store", "Referrer-Policy": "no-referrer" })
    for (const value of cookies) headers.append("Set-Cookie", value)
    return new Response(null, { status: 303, headers })
  }
  /** The sign-in page with a reason, keeping where the person was going (and an app handoff in progress). */
  function refused(code: SignInRefusal, returnPath?: string | null) {
    const target = new URLSearchParams()
    if (returnPath) {
      const back = new URL(returnPath, origin)
      if (back.pathname === "/login") { for (const [key, value] of back.searchParams) if (key !== "error") target.append(key, value) }
      else if (returnPath !== DEFAULT_RETURN) target.set("return", returnPath)
    }
    target.set("error", code)
    return redirect("/login?" + target.toString(), [cookie("", 0)])
  }
  async function json(url: string, init: RequestInit): Promise<Record<string, unknown>> {
    const response = await transport(url, { ...init, redirect: "error", cache: "no-store", signal: AbortSignal.timeout(15_000) })
    const body = await response.text()
    if (!response.ok || body.length > 200_000) throw new Error("Google request failed")
    const data: unknown = JSON.parse(body)
    if (typeof data !== "object" || data === null || Array.isArray(data)) throw new Error("Google request failed")
    return data as Record<string, unknown>
  }
  /** The account for a verified Google identity: one already linked to it, else the account with the same
   * address, else a new one when the address is on the allow list. Null means not allowed. */
  async function accountFor(identity: { subject: string; email: string; name: string | null }, allowed: ReadonlyArray<string>): Promise<string | null> {
    const link = (userId: string) => db.account.create({ data: { id: crypto.randomUUID(), accountId: identity.subject, providerId: "google", userId } })
    for (let attempt = 0; attempt < 2; attempt++) {
      const linked = await db.account.findFirst({ where: { providerId: "google", accountId: identity.subject } })
      if (linked) return linked.userId
      const existing = await db.user.findUnique({ where: { email: identity.email } })
      try {
        if (existing) { await link(existing.id); return existing.id }
        if (!emailAllowed(identity.email, allowed)) return null
        const id = crypto.randomUUID()
        await db.$transaction(async tx => {
          await tx.user.create({ data: { id, name: identity.name ?? identity.email.split("@")[0]!, email: identity.email, emailVerified: true, role: "user" } })
          await tx.account.create({ data: { id: crypto.randomUUID(), accountId: identity.subject, providerId: "google", userId: id } })
          await tx.member.create({ data: { id: crypto.randomUUID(), organizationId: deps.defaultOrganizationId, userId: id, role: "member" } })
        })
        return id
      } catch (error) { if (!duplicate(error) || attempt) throw error /* someone else created it a moment ago: look again */ }
    }
    return null
  }

  return {
    async methods() { const policy = await deps.policy(); return { password: policy.password, google: policy.google, instanceName: policy.instanceName } },

    /** GET /api/v1/auth/google/start?return=<path>: remembers this browser and sends it to Google. */
    async googleStart(request: Request): Promise<Response> {
      const returnPath = safeReturnPath(new URL(request.url).searchParams.get("return"), origin)
      try {
        const policy = await deps.policy()
        if (!policy.google || !policy.googleCredentials) return refused("google_unavailable", returnPath)
        const state = randomBytes(32).toString("base64url"), verifier = randomBytes(48).toString("base64url"), browser = randomBytes(32).toString("base64url")
        await db.googleSignInState.deleteMany({ where: { expiresAt: { lt: now() } } })
        // Nobody is signed in yet, so anyone can start one of these. Refuse rather than let the table grow without bound.
        if (await db.googleSignInState.count() >= 5000) return refused("failed", returnPath)
        await db.googleSignInState.create({ data: { stateHash: sha(state), browserHash: sha(browser), encryptedVerifier: box.seal(verifier), returnPath: returnPath ?? DEFAULT_RETURN, expiresAt: new Date(now().getTime() + STATE_MINUTES * 60_000) } })
        const url = new URL(endpoints.authorization)
        for (const [key, value] of Object.entries({ client_id: policy.googleCredentials.clientId, redirect_uri: redirectUri, response_type: "code", scope: "openid email profile", state, code_challenge: challengeOf(verifier), code_challenge_method: "S256", prompt: "select_account" })) url.searchParams.set(key, value)
        return redirect(url.href, [cookie(browser, STATE_MINUTES * 60)])
      } catch { return refused("failed", returnPath) }
    },

    /** GET /api/v1/auth/google/callback: finishes the sign-in this browser started, or says why not. */
    async googleCallback(request: Request): Promise<Response> {
      let returnPath: string | null = null
      try {
        const url = new URL(request.url), state = url.searchParams.get("state"), code = url.searchParams.get("code")
        const browser = (request.headers.get("cookie") ?? "").split(";").map(part => part.trim()).find(part => part.startsWith(stateCookie + "="))?.slice(stateCookie.length + 1)
        if (!state || !token43.test(state) || !browser || !token43.test(browser)) return refused("failed")
        // Single use: the row is taken before anything else is decided.
        const attempt = await db.googleSignInState.delete({ where: { stateHash: sha(state) } }).catch(alreadyGone)
        if (!attempt || attempt.expiresAt.getTime() <= now().getTime() || !sameText(attempt.browserHash, sha(browser))) return refused("failed")
        returnPath = safeReturnPath(attempt.returnPath, origin)
        if (url.searchParams.has("error")) return refused(url.searchParams.get("error") === "access_denied" ? "cancelled" : "failed", returnPath)
        const policy = await deps.policy()
        if (!policy.google || !policy.googleCredentials) return refused("google_unavailable", returnPath)
        if (!code || code.length > 4096) return refused("failed", returnPath)
        const tokens = await json(endpoints.token, { method: "POST", headers: { "Content-Type": "application/x-www-form-urlencoded" }, body: new URLSearchParams({ code, client_id: policy.googleCredentials.clientId, client_secret: policy.googleCredentials.clientSecret, redirect_uri: redirectUri, grant_type: "authorization_code", code_verifier: box.open(attempt.encryptedVerifier) }) })
        const accessToken = text(tokens.access_token, 10_000)
        if (!accessToken) return refused("failed", returnPath)
        // The identity is read from Google directly over TLS with the token just issued to this client.
        const identity = await json(endpoints.userinfo, { headers: { Authorization: "Bearer " + accessToken } })
        const subject = text(identity.sub, 255), email = text(identity.email, 254)?.toLowerCase() ?? null
        if (!subject || !email || !/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email) || identity.email_verified !== true) return refused("failed", returnPath)
        const userId = await accountFor({ subject, email, name: text(identity.name, 200) }, policy.allowedEmails)
        if (!userId) return refused("not_allowed", returnPath)
        // Like the web password form: the session ends with the browser session.
        const session = await deps.issueSession(userId, false, request.headers)
        return redirect(returnPath ?? DEFAULT_RETURN, [cookie("", 0), ...session.cookies])
      } catch { return refused("failed", returnPath) }
    },

    /** A signed-in browser asks for a code its app can redeem. The code is stored only as a hash. */
    async createHandoff(userId: string, challenge: string): Promise<{ code: string }> {
      if (!token43.test(challenge)) throw new SignInFailure("invalid_request", "The app sign-in request is not valid. Start again from the app.")
      await db.appSignInHandoff.deleteMany({ where: { expiresAt: { lt: now() } } })
      // A person has no use for more than a few codes at once; the oldest give way.
      const waiting = await db.appSignInHandoff.findMany({ where: { userId }, orderBy: { createdAt: "desc" }, skip: 4, select: { codeHash: true } })
      if (waiting.length) await db.appSignInHandoff.deleteMany({ where: { codeHash: { in: waiting.map(row => row.codeHash) } } })
      const code = randomBytes(32).toString("base64url")
      await db.appSignInHandoff.create({ data: { codeHash: sha(code), userId, challenge, expiresAt: new Date(now().getTime() + HANDOFF_SECONDS * 1000) } })
      return { code }
    },

    /** The app redeems its code with the verifier only it holds and gets a bearer session for that account. */
    async exchangeHandoff(code: string, verifier: string, request: Headers): Promise<{ token: string }> {
      const client = request.get("x-forwarded-for")?.split(",")[0]?.trim().slice(0, 64) || "direct"
      if (!exchangeOverall("all") || !exchangeByClient(client)) throw new SignInFailure("rate_limited", "Too many attempts. Wait a minute and sign in again.")
      if (!token43.test(code) || !/^[A-Za-z0-9._~-]{43,128}$/.test(verifier)) throw new SignInFailure("invalid_request", "The sign-in code or verifier is not valid.")
      const refuse = () => new SignInFailure("invalid_code", "This sign-in code is no longer valid. Start again from the app.")
      // Single use: deleting is the claim, so a second redemption (or a wrong verifier) finds nothing left.
      const handoff = await db.appSignInHandoff.delete({ where: { codeHash: sha(code) } }).catch(alreadyGone)
      if (!handoff || handoff.expiresAt.getTime() <= now().getTime()) throw refuse()
      if (!sameText(challengeOf(verifier), handoff.challenge)) throw refuse()
      let session: { token: string }
      try { session = await deps.issueSession(handoff.userId, true, request) } catch { throw refuse() }
      if (!session.token) throw refuse()
      return { token: session.token }
    },
  }
}
export type SignInService = ReturnType<typeof makeSignInService>
