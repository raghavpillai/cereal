import { createHash, randomBytes, timingSafeEqual } from "node:crypto"
import type { Database } from "@meeting-notes/database"
import type { SecretBox } from "./admin-secrets"
import { googleNeedsHttps, GOOGLE_NEEDS_HTTPS } from "@meeting-notes/contracts/transport"

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
/** Whether a verified address may create an account by signing in with Google. A full address on the list
 * is enough. A domain entry ("@example.com") also needs Google's `hd` claim to name that domain: only a Google
 * Workspace account of the organisation has it. A consumer Google account can be opened with any address and
 * stays "verified" after its owner loses that mailbox (someone who has left the company), so it has no `hd`
 * and gets in only by being listed by address. */
export function emailAllowed(email: string, allowed: ReadonlyArray<string>, hostedDomain?: string | null): boolean {
  const address = email.trim().toLowerCase(), at = address.lastIndexOf("@")
  if (at < 1) return false
  const domain = address.slice(at), workspace = hostedDomain ? "@" + hostedDomain.trim().toLowerCase() : null
  return allowed.some(entry => entry.startsWith("@") ? entry === domain && entry === workspace : entry === address)
}
/** Whether the allow list names this address or its domain. A domain entry still needs Google's `hd` claim
 * when the person signs in (`emailAllowed`), which cannot be known before they do. */
export function emailListed(email: string, allowed: ReadonlyArray<string>): boolean {
  const address = email.trim().toLowerCase(), at = address.lastIndexOf("@")
  return at >= 1 && allowed.some(entry => entry.startsWith("@") ? entry === address.slice(at) : entry === address)
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
/** The address of a person's picture as Google's userinfo gives it (`picture`), or as it was saved: kept only
 * when it is a plain https address of ordinary length. Anything else is no picture. */
export function pictureAddress(value: unknown): string | null {
  if (typeof value !== "string" || value.length > 2048 || !value.startsWith("https://") || /[\s"'<>\\]/.test(value)) return null
  try { const url = new URL(value); return url.protocol === "https:" && url.hostname.includes(".") && !url.username && !url.password ? value : null } catch { return null }
}
/** Keeps the saved picture of the account that signs in with this Google identity the same as Google's. Only that
 * account's: a second Google account connected for its calendar is somebody else's picture. Never fails its caller. */
export async function refreshGooglePicture(db: Database, userId: string, subject: string, picture: unknown): Promise<void> {
  try {
    if (!await db.account.findFirst({ where: { providerId: "google", accountId: subject, userId } })) return
    const image = pictureAddress(picture), user = await db.user.findUnique({ where: { id: userId }, select: { image: true } })
    if (user && user.image !== image) await db.user.update({ where: { id: userId }, data: { image } })
  } catch { /* A picture is never worth failing a sign-in or a calendar connection for. */ }
}
const duplicate = (error: unknown) => (error as { code?: string })?.code === "P2002"
/** Deleting a row that is already gone is an answer (null); anything else is a database failure. */
const alreadyGone = (error: unknown) => { if ((error as { code?: string })?.code === "P2025") return null; throw error }

const STATE_MINUTES = 10, HANDOFF_SECONDS = 120, DEFAULT_RETURN = "/notes"

/** A fixed window per key. Old windows are dropped once the table grows, so it stays bounded. */
export function rateLimiter(limit: number, windowMs: number, clock: () => Date) {
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
  /** The account a new server is claimed for. */
  ownerId?: string
  /** Makes Google, with this client, the server's one way to sign in. Called only after Google has accepted the
   * client in a real sign-in. Returns a function that puts the settings back, for when what follows fails. */
  useGoogle?: (client: { clientId: string; clientSecret: string }) => Promise<() => Promise<void>>
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
  /** Workspace invitations to this address that still stand, oldest first. */
  const invitationsFor = (client: Pick<Database, "invitation">, email: string) => client.invitation.findMany({
    where: { email: { equals: email, mode: "insensitive" }, status: "pending", expiresAt: { gt: now() } },
    orderBy: { createdAt: "asc" }, include: { inviter: { select: { role: true } } } })
  /** The account for a verified Google identity: one already linked to it, else the account with the same
   * address, else a new one when the address is on the allow list or someone who administers this server has
   * invited it to a workspace (their invitation admits the address, as their adding it to the list would).
   * A new account joins the workspaces it was invited to: Google has just proved the address is theirs.
   * Null means not allowed. */
  async function accountFor(identity: { subject: string; email: string; name: string | null; hostedDomain: string | null; image: string | null }, allowed: ReadonlyArray<string>): Promise<string | null> {
    const link = (userId: string) => db.account.create({ data: { id: crypto.randomUUID(), accountId: identity.subject, providerId: "google", userId } })
    for (let attempt = 0; attempt < 2; attempt++) {
      const linked = await db.account.findFirst({ where: { providerId: "google", accountId: identity.subject } })
      if (linked) return linked.userId
      const existing = await db.user.findUnique({ where: { email: identity.email } })
      try {
        if (existing) { await link(existing.id); return existing.id }
        if (!emailAllowed(identity.email, allowed, identity.hostedDomain)
          && !(await invitationsFor(db, identity.email)).some(invitation => invitation.inviter.role === "owner" || invitation.inviter.role === "admin")) return null
        const id = crypto.randomUUID()
        await db.$transaction(async tx => {
          await tx.user.create({ data: { id, name: identity.name ?? identity.email.split("@")[0]!, email: identity.email, emailVerified: true, image: identity.image, role: "user" } })
          await tx.account.create({ data: { id: crypto.randomUUID(), accountId: identity.subject, providerId: "google", userId: id } })
          await tx.member.create({ data: { id: crypto.randomUUID(), organizationId: deps.defaultOrganizationId, userId: id, role: "member" } })
          for (const invitation of await invitationsFor(tx, identity.email)) {
            const role = invitation.role === "admin" ? "admin" : "member", place = { organizationId: invitation.organizationId, userId: id }
            await tx.member.upsert({ where: { organizationId_userId: place }, create: { id: crypto.randomUUID(), ...place, role }, update: role === "admin" ? { role } : {} })
            await tx.invitation.update({ where: { id: invitation.id }, data: { status: "accepted" } })
          }
        })
        return id
      } catch (error) { if (!duplicate(error) || attempt) throw error /* someone else created it a moment ago: look again */ }
    }
    return null
  }

  /* ── Proving a Google client ─────────────────────────────────────────────
     Claiming a server with Google, and switching a server to Google, both start from a client ID and secret that
     nobody has tried yet. They are tried with one real sign-in: Google only hands back an identity if the ID, the
     secret and this server's redirect address are all right. Until that has happened nothing is saved. */
  const verificationCookie = (secure ? "__Host-" : "") + "cereal_google_verification"
  const verificationCookieLine = (value: string, maxAge: number) => `${verificationCookie}=${value}; Path=/; Max-Age=${maxAge}; HttpOnly; SameSite=Lax${secure ? "; Secure" : ""}`
  const browserOf = (headers: Headers) => (headers.get("cookie") ?? "").split(";").map(part => part.trim()).find(part => part.startsWith(verificationCookie + "="))?.slice(verificationCookie.length + 1)
  const googleUsable = !googleNeedsHttps(origin)
  /** A small page for the window that went to Google: it closes itself; opened as a whole page, it goes back to the admin panel. */
  const verificationPage = (ok: boolean) => new Response(`<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta name="robots" content="noindex"><title>Cereal</title><style>html{color-scheme:light dark}body{margin:0;min-height:100vh;display:grid;place-items:center;font:16px/1.5 system-ui,sans-serif;background:Canvas;color:CanvasText}p{max-width:22rem;padding:1.5rem;text-align:center}</style></head><body><p>${ok ? "Google confirmed it. You can close this window." : "Google sign-in did not finish. You can close this window and look at the setup page."}</p><script>try{window.close()}catch(e){}setTimeout(function(){location.replace("/admin")},400)</script></body></html>`, { status: 200, headers: { "Content-Type": "text/html; charset=utf-8", "Cache-Control": "no-store", "Referrer-Policy": "no-referrer" } })

  type VerificationRow = NonNullable<Awaited<ReturnType<Database["googleVerification"]["findUnique"]>>>
  async function finishVerification(request: Request, url: URL, pending: VerificationRow): Promise<Response> {
    const browser = browserOf(request.headers)
    // Only the browser that started it may finish it; anyone else learns nothing and uses nothing up.
    if (!browser || !token43.test(browser) || !sameText(pending.browserHash, sha(browser))) return verificationPage(false)
    // Single use: the row is taken here, before anything is decided. A replayed callback finds it taken.
    const taken = await db.googleVerification.updateMany({ where: { stateHash: pending.stateHash, outcome: null }, data: { outcome: "working" } })
    if (taken.count !== 1) return verificationPage(pending.outcome === "done")
    const finish = async (outcome: string, cookies: ReadonlyArray<string> = []) => {
      await db.googleVerification.update({ where: { stateHash: pending.stateHash }, data: { outcome, encryptedSecret: null } }).catch(() => {})
      const page = verificationPage(outcome === "done")
      for (const value of cookies) page.headers.append("Set-Cookie", value)
      return page
    }
    try {
      if (pending.expiresAt.getTime() <= now().getTime()) return await finish("expired")
      if (url.searchParams.has("error")) return await finish(url.searchParams.get("error") === "access_denied" ? "cancelled" : "failed")
      const code = url.searchParams.get("code")
      if (!code || code.length > 4096 || !pending.encryptedSecret || !googleUsable) return await finish("failed")
      const client = { clientId: pending.clientId, clientSecret: box.open(pending.encryptedSecret) }
      // The exchange is the proof. Google's own error names are read (never its text) so the person can be told what to fix.
      const exchange = await transport(endpoints.token, { method: "POST", headers: { "Content-Type": "application/x-www-form-urlencoded" }, redirect: "error", cache: "no-store", signal: AbortSignal.timeout(15_000),
        body: new URLSearchParams({ code, client_id: client.clientId, client_secret: client.clientSecret, redirect_uri: redirectUri, grant_type: "authorization_code", code_verifier: box.open(pending.encryptedVerifier) }) })
      const answer = (await exchange.text()).slice(0, 200_000)
      let tokens: Record<string, unknown> = {}
      try { const parsed: unknown = JSON.parse(answer); if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) tokens = parsed as Record<string, unknown> } catch { /* not JSON: treated as a failure below */ }
      if (!exchange.ok) return await finish(tokens.error === "invalid_client" || tokens.error === "unauthorized_client" ? "invalid_client" : tokens.error === "redirect_uri_mismatch" ? "redirect_uri_mismatch" : "failed")
      const accessToken = text(tokens.access_token, 10_000)
      if (!accessToken) return await finish("failed")
      const identity = await json(endpoints.userinfo, { headers: { Authorization: "Bearer " + accessToken } })
      const subject = text(identity.sub, 255), email = text(identity.email, 254)?.toLowerCase() ?? null
      if (!subject || !email || !/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email)) return await finish("failed")
      if (identity.email_verified !== true) return await finish("unverified_email")
      if (!deps.useGoogle) return await finish("failed")
      const name = text(identity.name, 200) ?? email.split("@")[0]!

      if (pending.purpose === "claim") {
        const ownerId = deps.ownerId
        if (!ownerId) return await finish("failed")
        // Checked before anything is written, and again under a lock when it is.
        if (await db.account.findFirst({ where: { userId: ownerId } })) return await finish("already_claimed")
        if (await db.user.findFirst({ where: { email, id: { not: ownerId } } }) || await db.account.findFirst({ where: { providerId: "google", accountId: subject } })) return await finish("email_taken")
        const undo = await deps.useGoogle(client)
        let refused: string | null = null
        try {
          await db.$transaction(async tx => {
            await tx.$queryRawUnsafe('SELECT "id" FROM "User" WHERE "id"=$1::uuid FOR UPDATE', ownerId)
            if (await tx.account.findFirst({ where: { userId: ownerId } })) { refused = "already_claimed"; return }
            if (await tx.user.findFirst({ where: { email, id: { not: ownerId } } })) { refused = "email_taken"; return }
            await tx.user.update({ where: { id: ownerId }, data: { name, email, emailVerified: true, image: pictureAddress(identity.picture) } })
            // No password is made for this account: it signs in with Google only.
            await tx.account.create({ data: { id: crypto.randomUUID(), accountId: subject, providerId: "google", userId: ownerId } })
          })
        } catch { refused = "failed" }
        if (refused) { await undo().catch(() => {}); return await finish(refused) }
        const session = await deps.issueSession(ownerId, false, request.headers)
        return await finish("done", session.cookies)
      }

      // Switching: the Google account is tied to the administrator who asked, so they can still get in afterwards.
      const userId = pending.userId
      const user = userId ? await db.user.findUnique({ where: { id: userId } }) : null
      if (!user || (user.role !== "owner" && user.role !== "admin")) return await finish("failed")
      const linked = await db.account.findFirst({ where: { providerId: "google", accountId: subject } })
      if (linked && linked.userId !== user.id) return await finish("google_account_in_use")
      const sameAddress = await db.user.findUnique({ where: { email } })
      if (sameAddress && sameAddress.id !== user.id) return await finish("google_account_in_use")
      const undo = await deps.useGoogle(client)
      try { if (!linked) await db.account.create({ data: { id: crypto.randomUUID(), accountId: subject, providerId: "google", userId: user.id } }) }
      catch { await undo().catch(() => {}); return await finish("failed") }
      await refreshGooglePicture(db, user.id, subject, identity.picture)
      return await finish("done")
    } catch { return await finish("failed") }
  }

  return {
    /** Starts the one Google sign-in that proves a client. The caller has already checked who is asking (the setup
     * code, or an administrator's session). Answers with where to send the browser and the cookie that binds it. */
    async beginVerification(input: { purpose: "claim" | "switch"; userId?: string; clientId: string; clientSecret: string }): Promise<{ authorizationUrl: string; cookie: string }> {
      const clientId = input.clientId.trim(), clientSecret = input.clientSecret.trim()
      if (!googleUsable) throw new SignInFailure("invalid_request", GOOGLE_NEEDS_HTTPS + ". This server is reached over plain HTTP.")
      if (!/^[A-Za-z0-9._-]{10,255}$/.test(clientId)) throw new SignInFailure("invalid_request", "Enter the Google client ID exactly as Google Cloud shows it")
      if (clientSecret.length < 8 || clientSecret.length > 512 || /\s/.test(clientSecret)) throw new SignInFailure("invalid_request", "Enter the Google client secret exactly as Google Cloud shows it")
      if (input.purpose === "claim" && (!deps.ownerId || await db.account.findFirst({ where: { userId: deps.ownerId } }))) throw new SignInFailure("forbidden", "This server already has an owner")
      await db.googleVerification.deleteMany({ where: { expiresAt: { lt: now() } } })
      if (await db.googleVerification.count() >= 500) throw new SignInFailure("unavailable", "Too many sign-ins are waiting. Try again in a few minutes.")
      const state = randomBytes(32).toString("base64url"), verifier = randomBytes(48).toString("base64url"), browser = randomBytes(32).toString("base64url")
      await db.googleVerification.create({ data: { stateHash: sha(state), browserHash: sha(browser), purpose: input.purpose, userId: input.userId ?? null, clientId, encryptedSecret: box.seal(clientSecret), encryptedVerifier: box.seal(verifier), expiresAt: new Date(now().getTime() + STATE_MINUTES * 60_000) } })
      const url = new URL(endpoints.authorization)
      for (const [key, value] of Object.entries({ client_id: clientId, redirect_uri: redirectUri, response_type: "code", scope: "openid email profile", state, code_challenge: challengeOf(verifier), code_challenge_method: "S256", prompt: "select_account" })) url.searchParams.set(key, value)
      return { authorizationUrl: url.href, cookie: verificationCookieLine(browser, STATE_MINUTES * 60) }
    },
    /** How this browser's last verification went. Says nothing to a browser that did not start one. */
    async verificationStatus(headers: Headers): Promise<{ state: "none" | "pending" | "done" | "failed"; reason: string | null }> {
      const browser = browserOf(headers)
      if (!browser || !token43.test(browser)) return { state: "none", reason: null }
      const row = await db.googleVerification.findFirst({ where: { browserHash: sha(browser) }, orderBy: { createdAt: "desc" } })
      if (!row) return { state: "none", reason: null }
      if (row.outcome === "done") return { state: "done", reason: null }
      if (row.outcome && row.outcome !== "working") return { state: "failed", reason: row.outcome }
      return row.expiresAt.getTime() <= now().getTime() ? { state: "failed", reason: "expired" } : { state: "pending", reason: null }
    },
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
        // A sign-in that is proving a client (claiming the server, or switching it to Google) is finished on its own terms.
        if (state && token43.test(state)) { const proving = await db.googleVerification.findUnique({ where: { stateHash: sha(state) } }); if (proving) return finishVerification(request, url, proving) }
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
        // `hd` is the Google Workspace domain of the account; a consumer account has none.
        const userId = await accountFor({ subject, email, name: text(identity.name, 200), hostedDomain: text(identity.hd, 253), image: pictureAddress(identity.picture) }, policy.allowedEmails)
        if (!userId) return refused("not_allowed", returnPath)
        // The picture is Google's as of this sign-in, for an account made just now and for one made long ago.
        await refreshGooglePicture(db, userId, subject, identity.picture)
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
