/**
 * A local Cereal API with live transcription for client development. In-memory storage, one account,
 * no PostgreSQL, no Docker. Everything is lost when it stops.
 *
 *   bun scripts/live/dev-server.ts                                  fake recogniser, no key, no cost
 *   LIVE_DEV_PROVIDER=elevenlabs bun --env-file=.env scripts/live/dev-server.ts     the real recogniser
 *
 * Environment: LIVE_DEV_PORT (3210), LIVE_DEV_TOKEN, LIVE_DEV_PROVIDER (fake | elevenlabs),
 * LIVE_DEV_CHAT (echo | provider), LIVE_FAKE_INTERIM_DELAY_MS, LIVE_FAKE_FINAL_DELAY_MS.
 */
import { DEV_TOKEN, startLiveStack } from "./stack"

const number = (name: string) => process.env[name] === undefined ? undefined : Number(process.env[name])
const provider = process.env.LIVE_DEV_PROVIDER === "elevenlabs" ? "elevenlabs" : "fake"
const stack = await startLiveStack({
  port: number("LIVE_DEV_PORT") ?? 3210, token: process.env.LIVE_DEV_TOKEN ?? DEV_TOKEN, provider,
  chat: process.env.LIVE_DEV_CHAT === "provider" ? "provider" : "echo",
  fakeInterimDelayMs: number("LIVE_FAKE_INTERIM_DELAY_MS"), fakeFinalDelayMs: number("LIVE_FAKE_FINAL_DELAY_MS"),
})
const custom = process.env.LIVE_DEV_TOKEN !== undefined
console.log(`Cereal live development server on ${stack.base} (recogniser: ${stack.provider} ${stack.model}; storage: memory)`)
console.log(`Bearer token: ${custom ? "the value of LIVE_DEV_TOKEN" : DEV_TOKEN}`)
console.log(`Live socket:  ws://127.0.0.1:${stack.server.port}/api/v1/notes/<note id>/live`)
let stopping = false
for (const signal of ["SIGINT", "SIGTERM"] as const) process.on(signal, async () => {
  if (stopping) return
  stopping = true
  await stack.stop()
  if (provider === "elevenlabs") console.log(`Streamed ${stack.upstream.seconds.toFixed(0)} s to the provider in ${stack.upstream.sessions} sessions (about $${(stack.upstream.seconds / 3600 * 0.22).toFixed(4)} at $0.22 per hour).`)
  process.exit(0)
})
