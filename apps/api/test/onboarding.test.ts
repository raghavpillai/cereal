/** First-run setup: what `onboarding` reports, how steps are recorded, and who may finish or restart it. */
import { afterEach, expect, test } from "bun:test"
import { randomBytes } from "node:crypto"
import { Effect } from "effect"
import type { AdminSettingsDTO } from "@meeting-notes/contracts"
import type { PrincipalData } from "@meeting-notes/contracts/api"
import { makeApiHandler } from "../src/api"
import { makeAdministrationService } from "../src/administration"
import { secretBoxFromKey } from "../src/admin-secrets"
import { makeMemoryCalendarStore } from "../src/calendar-store"
import { makeMemoryRepository, BOOTSTRAP_OWNER_ID } from "../src/repository"

const token = "onboarding-bootstrap-token-with-at-least-32-characters"
const ids = { owner: BOOTSTRAP_OWNER_ID, admin: "30000000-0000-4000-8000-000000000002", member: "30000000-0000-4000-8000-000000000003" }
type Account = keyof typeof ids
const secrets = { openai: "sk-onboarding-openai-secret-0001", eleven: "sk_onboarding_eleven_secret_0002", router: "sk-or-onboarding-router-secret-0003" }
const catalogs: Record<string, unknown> = {
  "https://openrouter.ai/api/v1/models": { data: [{ id: "vendor/chat-a", name: "Chat A", architecture: { input_modalities: ["text"], output_modalities: ["text"] } }] },
  "https://api.openai.com/v1/models": { data: [{ id: "gpt-test" }, { id: "gpt-transcribe" }] },
  "https://api.elevenlabs.io/v1/models": [{ model_id: "scribe_v3", name: "Scribe v3" }],
}
const cleanup: Array<() => Promise<unknown>> = []
afterEach(async () => { for (const dispose of cleanup.splice(0)) await dispose() })

function fixture(environment: Record<string, string> = {}) {
  let time = Date.parse("2026-11-20T09:00:00.000Z")
  const clock = () => new Date(time)
  const transport = Object.assign(async (input: unknown) => { const url = String(input); return url in catalogs ? Response.json(catalogs[url]) : new Response(null, { status: 404 }) }, { preconnect: () => {} }) as typeof fetch
  const repository = makeMemoryRepository(), store = makeMemoryCalendarStore(repository)
  const admin = makeAdministrationService(store, secretBoxFromKey(randomBytes(32)), { environment, fetch: transport, clock })
  const api = makeApiHandler(repository, token, {
    administration: admin, clock,
    principalResolver: async headers => {
      const account = (headers.get("authorization") ?? "").replace("Bearer ", "") as Account
      if (!(account in ids)) return null
      return { id: ids[account], name: account, email: `${account}@example.test`, role: account === "member" ? "user" : account, source: "session", expiresAt: "2099-01-01T00:00:00Z", createdAt: "2026-01-01T00:00:00Z" } satisfies PrincipalData
    },
  })
  cleanup.push(api.dispose)
  const call = (account: Account | null, path: string, method = "GET", body?: unknown, extra: Record<string, string> = {}) => api.handler(new Request(`http://localhost/api/v1${path}`, {
    method, headers: { ...(account ? { Authorization: `Bearer ${account}` } : {}), ...(body === undefined ? {} : { "Content-Type": "application/json" }), ...extra },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  }))
  const settings = async (account: Account = "owner") => await (await call(account, "/admin/settings")).json() as AdminSettingsDTO
  return { admin, store, call, settings, advance: (ms: number) => { time += ms } }
}
// A new server signs in with email and password, where "who may join" is not a step: it reads as done.
const nothingDone = { owner: true, name: false, language: false, speech: false, models: false, signIn: true, storage: false }

test("a fresh server reports setup as not finished, with every step read from what is stored", async () => {
  const f = fixture()
  const fresh = await f.settings()
  expect(fresh.onboarding).toEqual({ completedAt: null, steps: nothingDone })

  // A saved name, a chat provider key, the speech key and chosen models each finish their own step and no other.
  expect((await f.call("owner", "/admin/settings", "PATCH", { instanceName: "Harbour Studio" })).status).toBe(200)
  expect((await f.settings()).onboarding!.steps).toEqual({ ...nothingDone, name: true })
  expect((await f.call("owner", "/admin/settings", "PATCH", { openAiApiKey: secrets.openai })).status).toBe(200)
  expect((await f.settings()).onboarding!.steps).toEqual({ ...nothingDone, name: true, language: true })
  const patched = await (await f.call("owner", "/admin/settings", "PATCH", { elevenLabsApiKey: secrets.eleven })).json() as AdminSettingsDTO
  expect(patched.onboarding!.steps).toEqual({ ...nothingDone, name: true, language: true, speech: true })
  const chosen = await f.call("owner", "/admin/settings", "PATCH", { chatModels: [{ provider: "openai", model: "gpt-test" }], generationModel: { provider: "openai", model: "gpt-test" } })
  expect(chosen.status).toBe(200)
  expect((await f.settings()).onboarding!.steps).toEqual({ ...nothingDone, name: true, language: true, speech: true, models: true })

  // Removing the key un-finishes the step: nothing is remembered that is no longer true.
  await f.call("owner", "/admin/settings", "PATCH", { openAiApiKey: null })
  expect((await f.settings()).onboarding!.steps.language).toBe(false)
})

test("speech to text is live, so only an ElevenLabs key finishes it; OpenAI can no longer be chosen for it", async () => {
  const f = fixture()
  await f.call("owner", "/admin/settings", "PATCH", { openAiApiKey: secrets.openai })
  expect((await f.settings()).onboarding!.steps.speech).toBe(false)
  expect((await f.call("owner", "/admin/settings", "PATCH", { liveTranscriptionModel: { provider: "openai", model: "gpt-transcribe" } })).status).toBe(400)
  // Nor a batch model of ElevenLabs: speech models must stream.
  const batch = await f.call("owner", "/admin/settings", "PATCH", { elevenLabsApiKey: secrets.eleven, liveTranscriptionModel: { provider: "elevenlabs", model: "scribe_v3" } })
  expect(batch.status).toBe(400)
  expect(((await batch.json()) as { message: string }).message).toContain("Only live transcription models")
  // Nor can live transcription be turned off through the API: the contract refuses null.
  expect((await f.call("owner", "/admin/settings", "PATCH", { liveTranscriptionModel: null })).status).toBe(400)
  expect((await f.settings()).onboarding!.steps.speech).toBe(false)
})

test("the OpenAI-compatible server finishes the language step with its address alone (no key needed)", async () => {
  const f = fixture()
  await f.call("owner", "/admin/settings", "PATCH", { openAiCompatibleBaseUrl: "http://10.0.0.5:11434/v1" })
  expect((await f.settings()).onboarding!.steps.language).toBe(true)
  await f.call("owner", "/admin/settings", "PATCH", { openAiCompatibleBaseUrl: null })
  expect((await f.settings()).onboarding!.steps.language).toBe(false)
})

test("the setup state carries nothing secret", async () => {
  const f = fixture({ OPENROUTER_API_KEY: secrets.router })
  await f.call("owner", "/admin/settings", "PATCH", { openAiApiKey: secrets.openai, elevenLabsApiKey: secrets.eleven, signIn: { googleClientId: "1234567890-fixture.apps.googleusercontent.com", googleClientSecret: "GOCSPX-onboarding-client-secret" }, storage: { bucket: "cereal-files", accessKeyId: "AKIAONBOARDINGKEY001", secretAccessKey: "onboarding-storage-secret-value-0004" } })
  await f.call("owner", "/admin/onboarding", "POST", { step: "signIn", state: "done" })
  const body = await (await f.call("owner", "/admin/onboarding", "POST", { step: "storage", state: "skipped" })).text()
  const { onboarding } = JSON.parse(body) as AdminSettingsDTO
  expect(Object.keys(onboarding!).sort()).toEqual(["completedAt", "steps"])
  expect(Object.keys(onboarding!.steps).sort()).toEqual(["language", "models", "name", "owner", "signIn", "speech", "storage"])
  for (const value of Object.values(onboarding!.steps)) expect(typeof value).toBe("boolean")
  for (const secret of [...Object.values(secrets), "GOCSPX-onboarding-client-secret", "onboarding-storage-secret-value-0004", token]) expect(body).not.toContain(secret)
})

test("steps are recorded as done or skipped; required steps cannot be skipped or claimed", async () => {
  const f = fixture()
  const record = (step: string, state: string, account: Account = "owner") => f.call(account, "/admin/onboarding", "POST", { step, state })

  const skipped = await record("signIn", "skipped")
  expect(skipped.status).toBe(200)
  expect((await skipped.json() as AdminSettingsDTO).onboarding!.steps).toEqual({ ...nothingDone, signIn: true })
  // An administrator may walk the flow too.
  expect((await record("storage", "done", "admin")).status).toBe(200)
  expect((await record("name", "done")).status).toBe(200)
  expect((await record("models", "done")).status).toBe(200)
  expect((await f.settings()).onboarding!.steps).toEqual({ ...nothingDone, name: true, models: true, signIn: true, storage: true })
  expect((await f.store.getSettings())!.onboardingSteps).toEqual({ signIn: "skipped", storage: "done", name: "done", models: "done" })

  // Required steps are finished by what is stored, never by saying so.
  for (const step of ["owner", "language", "speech"]) {
    const refused = await record(step, "skipped")
    expect(refused.status).toBe(400)
    expect((await refused.json()).error).toBe("invalid_request")
  }
  expect((await record("language", "done")).status).toBe(409)
  expect((await record("speech", "done")).status).toBe(409)
  expect((await record("owner", "done")).status).toBe(200) // already true here: there is no account service to claim
  expect((await f.settings()).onboarding!.steps.language).toBe(false)

  // Unknown steps and states are rejected by the contract; members and strangers are turned away.
  expect((await record("billing", "done")).status).toBe(400)
  expect((await record("signIn", "maybe")).status).toBe(400)
  expect((await record("signIn", "done", "member")).status).toBe(403)
  expect((await f.call(null, "/admin/onboarding", "POST", { step: "signIn", state: "done" })).status).toBe(401)
})

test("finishing is refused until the owner, language and speech steps are done", async () => {
  const f = fixture()
  const complete = (account: Account | null = "owner") => f.call(account, "/admin/onboarding/complete", "POST")

  const early = await complete()
  expect(early.status).toBe(409)
  expect((await early.json()).message).toContain("language")
  await f.call("owner", "/admin/settings", "PATCH", { openAiApiKey: secrets.openai })
  const withoutSpeech = await complete()
  expect(withoutSpeech.status).toBe(409)
  expect((await withoutSpeech.json()).message).toBe("Finish the required setup steps first: speech")
  await f.call("owner", "/admin/settings", "PATCH", { elevenLabsApiKey: secrets.eleven })

  // The owner step is read through the account service: an unclaimed server cannot finish.
  const unclaimed = await Effect.runPromise(Effect.result(f.admin.completeOnboarding(async () => false)))
  expect(unclaimed._tag).toBe("Failure")
  expect(JSON.stringify(unclaimed)).toContain("Finish the required setup steps first: owner")
  expect((await f.settings()).onboarding!.completedAt).toBeNull()

  expect((await complete("member")).status).toBe(403)
  expect((await complete(null)).status).toBe(401)
  const done = await complete("admin")
  expect(done.status).toBe(200)
  expect((await done.json() as AdminSettingsDTO).onboarding!.completedAt).toBe("2026-11-20T09:00:00.000Z")
  // Finishing again keeps the first time.
  f.advance(3_600_000)
  expect((await (await complete()).json() as AdminSettingsDTO).onboarding!.completedAt).toBe("2026-11-20T09:00:00.000Z")
})

test("only the owner can show setup again; restarting forgets confirmations, not configuration", async () => {
  const f = fixture()
  await f.call("owner", "/admin/settings", "PATCH", { instanceName: "Harbour Studio", openAiApiKey: secrets.openai, elevenLabsApiKey: secrets.eleven })
  await f.call("owner", "/admin/onboarding", "POST", { step: "signIn", state: "done" })
  await f.call("owner", "/admin/onboarding", "POST", { step: "storage", state: "skipped" })
  expect((await f.call("owner", "/admin/onboarding/complete", "POST")).status).toBe(200)

  for (const account of ["admin", "member"] as const) {
    const refused = await f.call(account, "/admin/onboarding/restart", "POST")
    expect(refused.status).toBe(403)
    expect((await refused.json()).error).toBe("forbidden")
  }
  expect((await f.call(null, "/admin/onboarding/restart", "POST")).status).toBe(401)
  expect((await f.settings()).onboarding!.completedAt).toBe("2026-11-20T09:00:00.000Z")

  const restarted = await f.call("owner", "/admin/onboarding/restart", "POST")
  expect(restarted.status).toBe(200)
  const after = await restarted.json() as AdminSettingsDTO
  expect(after.onboarding).toEqual({ completedAt: null, steps: { owner: true, name: true, language: true, speech: true, models: false, signIn: true, storage: false } })
  expect(after.instanceName).toBe("Harbour Studio")
  expect(after.providers.filter(provider => provider.configured).map(provider => provider.id).sort()).toEqual(["elevenlabs", "openai"])
  // And it can be finished again.
  f.advance(60_000)
  expect((await (await f.call("owner", "/admin/onboarding/complete", "POST")).json() as AdminSettingsDTO).onboarding!.completedAt).toBe("2026-11-20T09:01:00.000Z")
})

test("a server whose provider keys came from its environment is already set up", async () => {
  const configured = fixture({ OPENROUTER_API_KEY: secrets.router, ELEVENLABS_API_KEY: secrets.eleven })
  expect(await configured.admin.importEnvironment()).toEqual(["OPENROUTER_API_KEY", "ELEVENLABS_API_KEY"])
  expect((await configured.settings()).onboarding).toEqual({ completedAt: "2026-11-20T09:00:00.000Z", steps: { ...nothingDone, language: true, speech: true } })

  // Settings that are not provider keys say nothing about whether the server was set up.
  const limitsOnly = fixture({ LIVE_MAX_SESSIONS: "7" })
  expect(await limitsOnly.admin.importEnvironment()).toEqual(["LIVE_MAX_SESSIONS"])
  expect((await limitsOnly.settings()).onboarding).toEqual({ completedAt: null, steps: nothingDone })
})

test("the public sign-in methods say whether the server still has to be claimed", async () => {
  const f = fixture()
  const methods = await (await f.call(null, "/auth/methods")).json()
  // Without an account service there is no owner account to wait for.
  expect(methods).toEqual({ password: true, google: false, instanceName: "Cereal", setupRequired: false, transport: "secure" })
})

test("claiming is limited to a few attempts a minute per client", async () => {
  const f = fixture()
  // A wrong code from one address, as a reverse proxy reports it.
  const attempt = (client: string) => f.call(null, "/admin/owner/setup", "POST", { name: "Owner", email: "owner@example.test", password: "a-long-enough-password" }, { Authorization: "Bearer not-the-setup-code", "X-Forwarded-For": client })
  for (let i = 0; i < 10; i++) expect((await attempt("203.0.113.7")).status).toBe(401)
  const limited = await attempt("203.0.113.7")
  expect(limited.status).toBe(429)
  expect(limited.headers.get("retry-after")).toBe("60")
  expect(limited.headers.get("cache-control")).toBe("no-store")
  expect(await limited.json()).toEqual({ error: "rate_limited", message: "Too many attempts. Wait a minute and try again." })
  // Another client is not held back by the first, and the window passes.
  expect((await attempt("198.51.100.9")).status).toBe(401)
  f.advance(61_000)
  expect((await attempt("203.0.113.7")).status).toBe(401)
  // Other requests are never counted.
  for (let i = 0; i < 12; i++) expect((await f.call("owner", "/admin/settings")).status).toBe(200)
})
