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
  const store = makeMemoryCalendarStore(makeMemoryRepository())
  const admin = makeAdministrationService(store, undefined, { fetch: transport, environment })
  return { admin, requests, store }
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
  // Only streaming speech models are offered, and only they are accepted.
  expect(catalogue.models.every(model => model.capabilities.includes("live_transcription"))).toBe(true)
  for (const model of ["scribe_v3", "scribe_v2", "scribe_v1", "gpt-transcribe", "whisper-1"])
      await expect(run(f.admin.updateSettings({ elevenLabsApiKey: "stored-eleven-key", liveTranscriptionModel: { provider: "elevenlabs", model } }))).rejects.toMatchObject({ error: "invalid_request", message: expect.stringContaining("Only live transcription models") })
  // Without a key the provider must be connected first.
  await expect(run(f.admin.updateSettings({ liveTranscriptionModel: { provider: "elevenlabs", model: "scribe_v2_realtime" } }))).rejects.toMatchObject({ error: "invalid_request" })
  const on = await run(f.admin.updateSettings({ elevenLabsApiKey: "stored-eleven-key", liveTranscriptionModel: { provider: "elevenlabs", model: "scribe_v3_live" } }))
  expect(on.liveTranscriptionModel).toEqual({ provider: "elevenlabs", model: "scribe_v3_live" })
  const configured = await f.admin.liveConfiguration()
  expect(configured).toMatchObject({ provider: "elevenlabs", model: "scribe_v3_live" })
  expect(await configured!.apiKey()).toBe("stored-eleven-key")
  // Unrelated edits keep it. It cannot be turned off: the server refuses null whoever sends it, and keeps the model.
  expect((await run(f.admin.updateSettings({ instanceName: "Studio" }))).liveTranscriptionModel).toEqual({ provider: "elevenlabs", model: "scribe_v3_live" })
  await expect(run(f.admin.updateSettings({ liveTranscriptionModel: null } as never))).rejects.toMatchObject({ error: "invalid_request", message: expect.stringContaining("always on") })
  expect((await run(f.admin.getSettings())).liveTranscriptionModel).toEqual({ provider: "elevenlabs", model: "scribe_v3_live" })
  expect(await f.admin.liveConfiguration()).toMatchObject({ provider: "elevenlabs", model: "scribe_v3_live" })
  const other = await run(f.admin.updateSettings({ liveTranscriptionModel: { provider: "elevenlabs", model: "scribe_v2_realtime" } }))
  expect(other.liveTranscriptionModel).toEqual({ provider: "elevenlabs", model: "scribe_v2_realtime" })
  // The removed batch setting is not part of the settings any more.
  expect("transcriptionModel" in other).toBe(false)
})

test("LIVE_TRANSCRIPTION_PROVIDER=fake serves the built-in recogniser without any key", async () => {
  const transcriber = await makeLiveTranscriberResolver(fixture().admin, { LIVE_TRANSCRIPTION_PROVIDER: "fake" })()
  expect(transcriber).toMatchObject({ provider: "fake" })
})
