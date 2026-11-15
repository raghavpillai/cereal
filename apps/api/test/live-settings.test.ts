import { expect, test } from "bun:test"
import { Effect } from "effect"
import { makeAdministrationService } from "../src/administration"
import { makeMemoryCalendarStore } from "../src/calendar-store"
import { makeLiveTranscriberResolver } from "../src/live/configuration"
import { makeMemoryRepository } from "../src/repository"

const run = Effect.runPromise
function fixture(environment: NodeJS.ProcessEnv = {}) {
  const requests: string[] = []
  const transport = Object.assign(async (input: unknown) => {
    requests.push(String(input))
    // ElevenLabs' model list holds voice models only; Scribe is not in it.
    return Response.json(String(input).includes("elevenlabs") ? [{ model_id: "eleven_multilingual", can_do_text_to_speech: true }] : { data: [] })
  }, { preconnect: () => {} }) as typeof fetch
  const admin = makeAdministrationService(makeMemoryCalendarStore(makeMemoryRepository()), undefined, { fetch: transport, environment })
  return { admin, requests }
}

test("live transcription defaults to Scribe v3 Live and runs once its provider has a key", async () => {
  const fresh = fixture()
  expect((await run(fresh.admin.getSettings())).liveTranscriptionModel).toEqual({ provider: "elevenlabs", model: "scribe_v3_live" })
  // Without a key there is nothing to call, so sessions are refused and clients fall back to parts.
  expect(await fresh.admin.liveConfiguration()).toBeNull()
  expect(await makeLiveTranscriberResolver(fresh.admin, {})()).toBeNull()
  // The model is an instance setting only: the environment cannot choose it.
  const named = fixture({ ELEVENLABS_LIVE_STT_MODEL: "scribe_v2_realtime", ELEVENLABS_API_KEY: "environment-eleven-key" })
  expect((await run(named.admin.getSettings())).liveTranscriptionModel).toEqual({ provider: "elevenlabs", model: "scribe_v3_live" })
  const env = fixture({ ELEVENLABS_API_KEY: "environment-eleven-key" })
  const transcriber = await makeLiveTranscriberResolver(env.admin, {})()
  expect(transcriber).toMatchObject({ provider: "elevenlabs", model: "scribe_v3_live" })
  // The key never appears in settings.
  expect(JSON.stringify(await run(env.admin.getSettings()))).not.toContain("environment-eleven-key")
})

test("an administrator selects a streaming model from the catalogue and can turn it off again", async () => {
  const f = fixture()
  const catalogue = await run(f.admin.getModelCatalog({ provider: "elevenlabs", apiKey: "draft-eleven-key" }))
  expect(catalogue.models.filter(model => model.capabilities.includes("live_transcription")).map(model => model.id).sort()).toEqual(["scribe_v2_realtime", "scribe_v3_live"])
  // A batch model is not a streaming model, and the reverse.
  expect(catalogue.models.find(model => model.id === "scribe_v3")!.capabilities).toEqual(["transcription"])
  await expect(run(f.admin.updateSettings({ elevenLabsApiKey: "stored-eleven-key", liveTranscriptionModel: { provider: "elevenlabs", model: "scribe_v3" } }))).rejects.toMatchObject({ error: "invalid_request" })
  await expect(run(f.admin.updateSettings({ elevenLabsApiKey: "stored-eleven-key", transcriptionModel: { provider: "elevenlabs", model: "scribe_v3_live" } }))).rejects.toMatchObject({ error: "invalid_request" })
  // Without a key the provider must be connected first.
  await expect(run(f.admin.updateSettings({ liveTranscriptionModel: { provider: "elevenlabs", model: "scribe_v2_realtime" } }))).rejects.toMatchObject({ error: "invalid_request" })
  const on = await run(f.admin.updateSettings({ elevenLabsApiKey: "stored-eleven-key", liveTranscriptionModel: { provider: "elevenlabs", model: "scribe_v3_live" } }))
  expect(on.liveTranscriptionModel).toEqual({ provider: "elevenlabs", model: "scribe_v3_live" })
  const configured = await f.admin.liveConfiguration()
  expect(configured).toMatchObject({ provider: "elevenlabs", model: "scribe_v3_live" })
  expect(await configured!.apiKey()).toBe("stored-eleven-key")
  // Unrelated edits keep it; null turns it off.
  expect((await run(f.admin.updateSettings({ instanceName: "Studio" }))).liveTranscriptionModel).toEqual({ provider: "elevenlabs", model: "scribe_v3_live" })
  expect((await run(f.admin.updateSettings({ liveTranscriptionModel: null }))).liveTranscriptionModel).toBeNull()
  expect(await f.admin.liveConfiguration()).toBeNull()
  // Off is remembered: later edits do not bring the default back.
  expect((await run(f.admin.updateSettings({ instanceName: "Studio again" }))).liveTranscriptionModel).toBeNull()
  const other = await run(f.admin.updateSettings({ liveTranscriptionModel: { provider: "elevenlabs", model: "scribe_v2_realtime" } }))
  expect(other.liveTranscriptionModel).toEqual({ provider: "elevenlabs", model: "scribe_v2_realtime" })
})

test("LIVE_TRANSCRIPTION_PROVIDER=fake serves the built-in recogniser without any key", async () => {
  const transcriber = await makeLiveTranscriberResolver(fixture().admin, { LIVE_TRANSCRIPTION_PROVIDER: "fake" })()
  expect(transcriber).toMatchObject({ provider: "fake" })
})
