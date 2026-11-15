// Opt-in real-provider check. Upload only explicitly supplied synthetic fixtures.
// Never print credentials, audio, capability URLs, or provider responses.
import { mkdir, writeFile } from "node:fs/promises"
import { resolve } from "node:path"
import { Schema } from "effect"
import { AudioChunkDTO, ChatAnswerDTO, NoteDTO } from "@meeting-notes/contracts"

const base = new URL(process.env.DEPLOYMENT_URL ?? "http://127.0.0.1:3000")
const token = process.env.ADMIN_API_TOKEN
if (!token) throw new Error("ADMIN_API_TOKEN is required")
if (base.protocol !== "https:" && !(base.protocol === "http:" && ["127.0.0.1", "localhost", "[::1]"].includes(base.hostname))) throw new Error("Remote verification requires HTTPS")
const firstFile = process.env.PIPELINE_AUDIO
const resumedFile = process.env.PIPELINE_RESUME_AUDIO
if (!firstFile || !resumedFile || process.env.RUN_LIVE_PROVIDER_TESTS !== "1") throw new Error("Set RUN_LIVE_PROVIDER_TESTS=1 and synthetic PIPELINE_AUDIO / PIPELINE_RESUME_AUDIO paths")
const first = Buffer.from(await Bun.file(firstFile).arrayBuffer()).toString("base64")
const resumed = Buffer.from(await Bun.file(resumedFile).arrayBuffer()).toString("base64")
const decodeNote = Schema.decodeUnknownSync(NoteDTO)
const decodeChunk = Schema.decodeUnknownSync(AudioChunkDTO)
const decodeChat = Schema.decodeUnknownSync(ChatAnswerDTO)
function assert(value: unknown, message: string): asserts value { if (!value) throw new Error(message) }
async function call(path: string, body?: unknown, method = "GET", owner = true, expected = 200) {
  const headers: Record<string, string> = { Accept: "application/json" }
  if (owner) headers.Authorization = `Bearer ${token}`
  if (body !== undefined) headers["Content-Type"] = "application/json"
  const response = await fetch(new URL(path, base), { method, headers, body: body === undefined ? undefined : JSON.stringify(body), redirect: "error", signal: AbortSignal.timeout(180_000) })
  assert(response.status === expected, `${method} endpoint returned ${response.status}, expected ${expected}`)
  return response
}
const evidence: Record<string, unknown> = { syntheticAudioOnly: true, deployedServer: true, checks: [] }
const checks = evidence.checks as string[]
// Compose can return just before Bun starts accepting connections.
const readyBy = Date.now() + 30_000
while (true) {
  try {
    const ready = await fetch(new URL("/api/v1/ready", base), { redirect: "error", signal: AbortSignal.timeout(2000) })
    if (ready.ok) break
  } catch { /* Retry startup only; never retry provider-generating requests. */ }
  if (Date.now() >= readyBy) throw new Error("Deployment did not become ready")
  await Bun.sleep(250)
}
let id: string | undefined
try {
  let note = decodeNote(await (await call("/api/v1/notes", { title: "Synthetic Cedar pipeline verification" }, "POST")).json())
  id = note.id
  const path = `/api/v1/notes/${id}`
  const chunkID = crypto.randomUUID()
  const payload = { channel: "system", sequence: 0, startMs: 0, audioBase64: first }
  await call(`${path}/audio/chunks`, undefined, "GET", false, 401)
  await call(`${path}/audio/chunks/${chunkID}`, payload, "PUT", false, 401)
  checks.push("audio owner authentication")
  note = decodeNote(await (await call(`${path}/recording`, { command: "start" }, "POST")).json())
  assert(note.recordingState === "recording", "Start did not enter recording")
  await call(`${path}/generate`, { expectedTranscriptRevision: note.transcriptRevision }, "POST", true, 400)
  note = decodeNote(await (await call(`${path}/recording`, { command: "stop" }, "POST")).json())
  assert(note.recordingState === "stopped", "Stop did not preserve a resumable note")
  const chunk = decodeChunk(await (await call(`${path}/audio/chunks/${chunkID}`, payload, "PUT")).json())
  const duplicate = decodeChunk(await (await call(`${path}/audio/chunks/${chunkID}`, payload, "PUT")).json())
  assert(chunk.id === duplicate.id, "Upload identity changed on retry")
  await call(`${path}/audio/chunks/${chunkID}`, { ...payload, startMs: 1 }, "PUT", true, 400)
  checks.push("Start/Stop, explicit generation, immutable idempotent upload")
  async function waitReady(chunkId: string) {
    const until = Date.now() + 180_000
    while (Date.now() < until) {
      const rows = await (await call(`${path}/audio/chunks`)).json() as unknown[]
      const current = rows.map(decodeChunk).find((row) => row.id === chunkId)
      assert(current, "Durable chunk disappeared")
      if (current.state === "ready") return
      if (current.state === "failed") throw new Error(`Transcription failed: ${current.error ?? "unspecified"}`)
      await Bun.sleep(1000)
    }
    throw new Error("Transcription exceeded verification deadline")
  }
  await waitReady(chunkID)
  note = decodeNote(await (await call(path)).json())
  assert(note.transcriptRevision > 0 && note.transcriptSegments.length > 0, "No real transcript persisted")
  let text = note.transcriptSegments.map((segment) => segment.text).join(" ")
  assert(/friday/i.test(text) && /thousand|3,?000/i.test(text), "Transcription missed fixture facts")
  const speakers = new Set(note.transcriptSegments.map((segment) => segment.speakerId))
  assert(speakers.size >= 2, "Two-voice fixture did not produce two speaker labels")
  evidence.firstTranscriptRevision = note.transcriptRevision
  evidence.firstSegmentCount = note.transcriptSegments.length
  evidence.firstSpeakerCount = speakers.size
  checks.push("ElevenLabs real transcription, timestamps, and two-speaker diarization")
  const generated = decodeNote(await (await call(`${path}/generate`, { expectedTranscriptRevision: note.transcriptRevision }, "POST")).json())
  assert(generated.generatedRevision === note.transcriptRevision && generated.enhanced.length > 20, "Generation did not consume the transcript revision")
  assert(/friday/i.test(generated.enhanced), "Generated notes missed the launch date")
  checks.push("OpenRouter real note generation")
  const oldSegmentIds = new Set(note.transcriptSegments.map((segment) => segment.id))
  note = decodeNote(await (await call(`${path}/recording`, { command: "resume" }, "POST")).json())
  assert(note.id === id && note.recordingState === "recording", "Resume changed note identity")
  await call(`${path}/recording`, { command: "stop" }, "POST")
  const nextID = crypto.randomUUID()
  const offset = Math.max(...note.transcriptSegments.map((segment) => segment.endMs ?? segment.startMs)) + 1000
  await call(`${path}/audio/chunks/${nextID}`, { channel: "system", sequence: 1, startMs: offset, audioBase64: resumed }, "PUT")
  await waitReady(nextID)
  note = decodeNote(await (await call(path)).json())
  assert(note.transcriptRevision > generated.transcriptRevision, "Resume transcript did not advance revision")
  assert([...oldSegmentIds].every((segmentID) => note.transcriptSegments.some((segment) => segment.id === segmentID)), "Resume lost previous transcript")
  text = note.transcriptSegments.map((segment) => segment.text).join(" ")
  assert(/monday/i.test(text), "Resumed update missing from transcript")
  await call(`${path}/generate`, { expectedTranscriptRevision: generated.transcriptRevision }, "POST", true, 400)
  const regenerated = decodeNote(await (await call(`${path}/generate`, { expectedTranscriptRevision: note.transcriptRevision }, "POST")).json())
  assert(regenerated.generatedRevision === note.transcriptRevision && /monday/i.test(regenerated.enhanced), "Regeneration did not reflect resumed update")
  evidence.finalTranscriptRevision = note.transcriptRevision
  evidence.finalSegmentCount = note.transcriptSegments.length
  checks.push("Resume appends same note, stale revision rejection, real regeneration")
  const answer = decodeChat(await (await call(`${path}/chat`, { message: "What is the latest agreed launch day after the resumed update? Answer with the day and the reason.", history: [] }, "POST")).json())
  assert(/monday/i.test(answer.answer), "Chat did not use latest server transcript")
  checks.push("OpenRouter chat grounded in updated note")
  const share = await (await call(`${path}/shares`, { includeBody: false, includeEnhanced: true }, "POST")).json()
  const shared = await (await call(`/api/v1/shared/${share.token}`, undefined, "GET", false)).json()
  assert(shared.enhanced === regenerated.enhanced && !("transcriptSegments" in shared), "Shared projection exposed private transcript or stale notes")
  const page = await fetch(new URL(share.path, base), { redirect: "error" })
  assert(page.ok && (await page.text()).includes("/assets/"), "Shared reader route failed")
  await call(`/api/v1/shares/${share.share.id}`, undefined, "DELETE")
  await call(`/api/v1/shared/${share.token}`, undefined, "GET", false, 404)
  checks.push("generated-note web reader projection and revocation")
  evidence.passed = true
  if (process.env.PIPELINE_KEEP_FIXTURE === "1") evidence.fixtureNoteId = id
  const out = resolve(process.env.PIPELINE_EVIDENCE_DIR ?? "/tmp/meeting-pipeline-fixture")
  await mkdir(out, { recursive: true })
  await writeFile(resolve(out, "verification.json"), JSON.stringify(evidence, null, 2) + "\n", { mode: 0o600 })
  console.log(JSON.stringify(evidence, null, 2))
} finally {
  if (id && !(evidence.passed && process.env.PIPELINE_KEEP_FIXTURE === "1")) await call(`/api/v1/notes/${id}`, undefined, "DELETE")
}
