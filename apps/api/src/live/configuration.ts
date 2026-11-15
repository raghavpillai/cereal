/**
 * Which streaming recogniser this instance uses.
 *
 * The model is an instance setting (`InstanceSettings.liveTranscriptionModel`), chosen on the admin panel:
 * Scribe v3 Live until an administrator picks another or turns it off, and only once its provider has a
 * key. Nothing about it is read from the environment. `LIVE_TRANSCRIPTION_PROVIDER=fake` serves the
 * built-in fake instead, for client development without a provider key or cost.
 */
import type { LiveTranscriber } from "@meeting-notes/contracts/live"
import { makeElevenLabsRealtime } from "./providers/elevenlabs-realtime"
import { makeFakeTranscriber } from "./providers/fake"

export interface LiveConfigurationSource {
  liveConfiguration(): Promise<{ provider: "elevenlabs"; model: string; apiKey: () => Promise<string> } | null>
}
export function makeLiveTranscriberResolver(source: LiveConfigurationSource, environment: NodeJS.ProcessEnv = process.env): () => Promise<LiveTranscriber | null> {
  const number = (name: string) => { const value = Number(environment[name]); return environment[name] !== undefined && Number.isFinite(value) && value >= 0 ? value : undefined }
  if (environment.LIVE_TRANSCRIPTION_PROVIDER === "fake") {
    const fake = makeFakeTranscriber({ interimDelayMs: number("LIVE_FAKE_INTERIM_DELAY_MS"), finalDelayMs: number("LIVE_FAKE_FINAL_DELAY_MS") })
    return async () => fake
  }
  const vadSilenceSeconds = number("LIVE_VAD_SILENCE_SECONDS")
  const made = new Map<string, LiveTranscriber>()
  return async () => {
    const selected = await source.liveConfiguration()
    if (!selected) return null
    let transcriber = made.get(selected.model)
    // The key is read again on every upstream connection, so a rotated key takes effect at once.
    if (!transcriber) made.set(selected.model, transcriber = makeElevenLabsRealtime({ apiKey: selected.apiKey, model: selected.model, ...(vadSilenceSeconds === undefined ? {} : { vadSilenceSeconds }) }))
    return transcriber
  }
}
