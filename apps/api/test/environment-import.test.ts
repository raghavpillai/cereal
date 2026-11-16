/** Settings that moved from the environment to the admin panel are taken into the database once (`importEnvironment`). */
import { afterEach, expect, test } from "bun:test"
import { randomBytes } from "node:crypto"
import { Effect } from "effect"
import { makeApiHandler } from "../src/api"
import { DEFAULT_IOS_APP_ID, makeAdministrationService, MOVED_VARIABLES } from "../src/administration"
import { secretBoxFromKey } from "../src/admin-secrets"
import { makeMemoryCalendarStore } from "../src/calendar-store"
import { makeDownloads } from "../src/downloads"
import { makeLiveHub } from "../src/live/hub"
import { makeMemoryRepository } from "../src/repository"

const token = "environment-import-owner-token-with-32-characters"
const ENV = { OPENROUTER_API_KEY: "sk-or-environment-key-0001", OPENAI_API_KEY: "sk-openai-environment-key-0002", ELEVENLABS_API_KEY: "sk_eleven_environment_key_0003", OPENROUTER_MODEL: "anthropic/claude-sonnet-5.5", OPENROUTER_TOOL_MODEL: "openai/gpt-6-luna",
  ELEVENLABS_STT_MODEL: "scribe_v2", LIVE_MAX_SESSIONS: "7", LIVE_HOLD_SECONDS: "120", IOS_APP_IDS: "ABCDE12345.com.example.notes, not-an-id", IOS_APP_STORE_ID: "6471234567", DATABASE_URL: "postgresql://not-moved", PUBLIC_BASE_URL: "https://notes.example.test" }
const cleanup: Array<() => Promise<unknown>> = []
afterEach(async () => { for (const dispose of cleanup.splice(0)) await dispose() })
function fixture(environment: Record<string, string> = ENV) {
  const repository = makeMemoryRepository(), store = makeMemoryCalendarStore(repository), box = secretBoxFromKey(randomBytes(32))
  /** A server start: a new service on the same database and settings key. */
  const start = (env: Record<string, string> = environment) => { const admin = makeAdministrationService(store, box, { environment: env }); cleanup.push(admin.dispose); return admin }
  return { repository, store, box, start }
}
const settings = async (admin: ReturnType<ReturnType<typeof fixture>["start"]>) => Effect.runPromise(admin.getSettings())

test("each moved setting is imported once, secrets sealed, and the environment is not read for it again", async () => {
  const f = fixture(), admin = f.start()
  // Before the first start-up import the environment is still what applies (an upgrade in progress).
  expect((await settings(admin)).providers.find(provider => provider.id === "openrouter")).toEqual({ id: "openrouter", configured: true, source: "environment", hint: expect.stringMatching(/^(?:sk-|sk-or-v1-)?…/) })
  expect((await settings(admin)).unusedEnvironment).toEqual([])
  const imported = await admin.importEnvironment()
  expect(imported.sort()).toEqual(["ELEVENLABS_API_KEY", "IOS_APP_IDS", "IOS_APP_STORE_ID", "LIVE_HOLD_SECONDS", "LIVE_MAX_SESSIONS", "OPENAI_API_KEY", "OPENROUTER_API_KEY", "OPENROUTER_MODEL"])
  const stored = (await f.store.getSettings())!
  // Sealed at rest; no value of the environment is stored in the clear or reported.
  for (const secret of [stored.openRouterSecret, stored.openAiSecret, stored.elevenLabsSecret]) expect(secret).toMatch(/^v1\./)
  expect(f.box.open(stored.openRouterSecret!)).toBe(ENV.OPENROUTER_API_KEY)
  expect(stored.anthropicSecret).toBeNull()
  for (const value of [ENV.OPENROUTER_API_KEY, ENV.OPENAI_API_KEY, ENV.ELEVENLABS_API_KEY]) expect(JSON.stringify(stored) + JSON.stringify(imported) + JSON.stringify(await settings(admin))).not.toContain(value)
  const now = await settings(admin)
  expect(now.providers).toEqual([{ id: "openrouter", configured: true, source: "stored", hint: "sk-…0001" }, { id: "openai", configured: true, source: "stored", hint: "sk-…0002" }, { id: "anthropic", configured: false, source: "none", hint: null }, { id: "elevenlabs", configured: true, source: "stored", hint: "sk_…0003" }, { id: "openai_compatible", configured: false, source: "none", hint: null }])
  expect(now).toMatchObject({ languageModel: "anthropic/claude-sonnet-5.5", generationModel: { provider: "openrouter", model: "anthropic/claude-sonnet-5.5" }, liveTranscriptionModel: { provider: "elevenlabs", model: "scribe_v3_live" }, liveMaxSessions: 7, liveHoldSeconds: 120, apps: { iosAppIds: ["ABCDE12345.com.example.notes"], iosAppStoreId: "6471234567" } })
  // Every moved variable that is set is now reported as unused; variables that stay in the environment are not.
  expect([...now.unusedEnvironment!].sort()).toEqual(Object.keys(ENV).filter(name => (MOVED_VARIABLES as ReadonlyArray<string>).includes(name)).sort())
  expect(await admin.configuration("generation")).toMatchObject({ openRouterKey: ENV.OPENROUTER_API_KEY, openAiKey: ENV.OPENAI_API_KEY, anthropicKey: "", elevenLabsKey: ENV.ELEVENLABS_API_KEY, languageModel: "anthropic/claude-sonnet-5.5" })
  // A second start imports nothing and writes nothing.
  const again = f.start()
  expect(await again.importEnvironment()).toEqual([])
  expect((await f.store.getSettings())!.updatedAt).toBe(stored.updatedAt)
  // The environment changing later changes nothing: the stored value decides.
  const changed = f.start({ ...ENV, OPENROUTER_API_KEY: "sk-or-a-different-key-9999", OPENROUTER_MODEL: "openai/gpt-4o-mini", LIVE_MAX_SESSIONS: "99" })
  expect(await changed.importEnvironment()).toEqual([])
  expect(await changed.configuration("generation")).toMatchObject({ openRouterKey: ENV.OPENROUTER_API_KEY, languageModel: "anthropic/claude-sonnet-5.5" })
  expect((await settings(changed)).liveMaxSessions).toBe(7)
})

test("a stored value is never overwritten by the environment, and a cleared one is not brought back by the next start", async () => {
  const f = fixture(), first = f.start({})
  // Saved on the panel before the variables were ever seen.
  await Effect.runPromise(first.updateSettings({ openRouterApiKey: "sk-or-saved-on-the-panel-0001", liveMaxSessions: 3, apps: { iosAppStoreId: "1111111111" } }))
  const admin = f.start()
  const imported = await admin.importEnvironment()
  for (const name of ["OPENROUTER_API_KEY", "LIVE_MAX_SESSIONS", "IOS_APP_STORE_ID"]) expect(imported).not.toContain(name)
  expect(imported).toContain("OPENAI_API_KEY")
  expect(await admin.configuration()).toMatchObject({ openRouterKey: "sk-or-saved-on-the-panel-0001", openAiKey: ENV.OPENAI_API_KEY })
  expect(await settings(admin)).toMatchObject({ liveMaxSessions: 3, apps: { iosAppStoreId: "1111111111" } })
  // Passed over, but remembered: it is reported as unused and never looked at again.
  expect((await settings(admin)).unusedEnvironment).toContain("OPENROUTER_API_KEY")
  // Cleared on the panel: gone, not silently restored from the environment, now or after a restart.
  await Effect.runPromise(admin.updateSettings({ openRouterApiKey: null, openAiApiKey: null, apps: { iosAppStoreId: null } }))
  for (const service of [admin, f.start()]) {
    expect(await service.importEnvironment()).toEqual([])
    const now = await settings(service)
    expect(now.providers.filter(provider => provider.id === "openrouter" || provider.id === "openai")).toEqual([{ id: "openrouter", configured: false, source: "none", hint: null }, { id: "openai", configured: false, source: "none", hint: null }])
    expect(now.apps!.iosAppStoreId).toBeNull()
    expect(await service.configuration()).toMatchObject({ openRouterKey: "", openAiKey: "" })
  }
})

test("values that are not usable are not imported, and nothing set means nothing happens", async () => {
  const f = fixture({ OPENROUTER_API_KEY: "short", LIVE_MAX_SESSIONS: "many", LIVE_HOLD_SECONDS: "-5", IOS_APP_IDS: "nonsense", IOS_APP_STORE_ID: "abc", OPENROUTER_MODEL: "has spaces in it", ELEVENLABS_STT_MODEL: "scribe_v2" })
  const admin = f.start()
  expect(await admin.importEnvironment()).toEqual([])
  // The batch speech model variable is never imported: speech models must stream.
  expect(await settings(admin)).toMatchObject({ languageModel: "openai/gpt-6-sol", liveTranscriptionModel: { provider: "elevenlabs", model: "scribe_v3_live" }, liveMaxSessions: 20, liveHoldSeconds: 300, apps: { iosAppIds: [DEFAULT_IOS_APP_ID], iosAppStoreId: null } })
  expect((await settings(admin)).providers.every(provider => !provider.configured)).toBe(true)
  const empty = fixture({}), fresh = empty.start()
  expect(await fresh.importEnvironment()).toEqual([])
  expect(await empty.store.getSettings()).toBeNull()
  expect(await settings(fresh)).toMatchObject({ instanceName: "Cereal", unusedEnvironment: [], liveMaxSessions: 20, liveHoldSeconds: 300 })
})

test("live limits and the iPhone app's identifiers are set on the panel, checked, and in force at once", async () => {
  const f = fixture({}), admin = f.start()
  const api = makeApiHandler(f.repository, token, { administration: admin, downloads: makeDownloads({ directory: "/nonexistent-downloads", version: "1.0.0", iosAppStoreId: async () => (await admin.apps()).iosAppStoreId }) })
  cleanup.push(api.dispose)
  const patch = async (body: unknown) => { const response = await api.handler(new Request("http://localhost/api/v1/admin/settings", { method: "PATCH", headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" }, body: JSON.stringify(body) })); return { status: response.status, body: await response.json().catch(() => null) as any } }
  for (const bad of [{ liveMaxSessions: 0 }, { liveMaxSessions: 201 }, { liveMaxSessions: 1.5 }, { liveHoldSeconds: -1 }, { liveHoldSeconds: 3601 }, { apps: { iosAppIds: ["not an app id"] } }, { apps: { iosAppStoreId: "id123" } }, { apps: { iosAppIds: Array.from({ length: 11 }, (_, i) => `ABCDE1234${i % 10}.com.example.app${i}`) } }]) expect((await patch(bad)).status, JSON.stringify(bad)).toBe(400)
  expect((await api.handler(new Request("http://localhost/api/v1/downloads"))).status).toBe(200)
  const saved = await patch({ liveMaxSessions: 5, liveHoldSeconds: 0, apps: { iosAppIds: [" ABCDE12345.com.example.notes ", "ABCDE12345.com.example.notes", "FGHIJ67890.org.example.app"], iosAppStoreId: " 6471234567 " } })
  expect(saved.status).toBe(200)
  expect(saved.body).toMatchObject({ liveMaxSessions: 5, liveHoldSeconds: 0, apps: { iosAppIds: ["ABCDE12345.com.example.notes", "FGHIJ67890.org.example.app"], iosAppStoreId: "6471234567" } })
  expect(await admin.liveLimits()).toEqual({ maxSessions: 5, holdSeconds: 0 })
  expect(await (await api.handler(new Request("http://localhost/api/v1/downloads"))).json()).toMatchObject({ iosAppStoreUrl: "https://apps.apple.com/app/id6471234567" })
  // An empty list goes back to the Cereal app; clearing the App Store ID removes the link.
  expect((await patch({ apps: { iosAppIds: [], iosAppStoreId: null } })).body.apps).toEqual({ iosAppIds: [DEFAULT_IOS_APP_ID], iosAppStoreId: null })
  expect((await (await api.handler(new Request("http://localhost/api/v1/downloads"))).json()).iosAppStoreUrl).toBeNull()
  // The hub applies whatever limit is in force when it is asked.
  let limits = { maxSessions: 1, holdMs: 0 }
  const hub = makeLiveHub(f.repository, { limits: () => limits })
  cleanup.push(() => hub.dispose())
  expect(hub.full("3f0a1b7e-6f0e-4a55-9f7c-0d2c7f1f6a10")).toBe(false)
  limits = { maxSessions: 0, holdMs: 0 }
  expect(hub.full("3f0a1b7e-6f0e-4a55-9f7c-0d2c7f1f6a10")).toBe(true)
})
