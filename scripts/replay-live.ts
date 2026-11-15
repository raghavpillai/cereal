/**
 * Replays a synthetic meeting (see scripts/live/make-fixture.ts) through the live transcription path as
 * a client would, and measures it: how long after each sentence its text arrives as interim text, as
 * final text, and in the context a chat request is answered from. Then checks the stored transcript.
 *
 *   bun scripts/live/make-fixture.ts /tmp/meeting
 *   bun scripts/replay-live.ts --fixture /tmp/meeting                          fake recogniser: mechanics only
 *   RUN_LIVE_PROVIDER_TESTS=1 bun --env-file=.env scripts/replay-live.ts --fixture /tmp/meeting --provider elevenlabs
 *
 * Options: --model <id>  --speed <n>  --no-chat  --parts <seconds> | --no-parts  --drop <at seconds>:<for seconds>
 *          --vad <seconds>  --out <file.json>
 *
 * The server runs in this process on the in-memory repository (scripts/live/stack.ts) with the real
 * route, session, worker and chat code. Chat requests are answered by a stub that returns the context,
 * so no language model is called and "answerable" is measured at the point the context is built.
 * The real provider costs money ($0.22 per stream-hour at list price) and is opt-in.
 */
import { readFile, writeFile } from "node:fs/promises"
import { join } from "node:path"
import { Effect } from "effect"
import { encodeLiveAudioFrame, type LiveServerMessage } from "@meeting-notes/contracts/live"
import { checkChatCitations } from "../apps/api/src/ai/citations"
import type { FixtureLine, FixtureManifest } from "./live/make-fixture"
import { wavFile } from "./live/make-fixture"
import { startLiveStack } from "./live/stack"

const args = process.argv.slice(2)
const flag = (name: string) => args.includes(`--${name}`)
const value = (name: string) => { const index = args.indexOf(`--${name}`); return index >= 0 ? args[index + 1] : undefined }
const directory = value("fixture")
if (!directory) { console.error("usage: bun scripts/replay-live.ts --fixture <directory> [--provider fake|elevenlabs] [--speed n] [--no-chat] [--parts seconds|--no-parts] [--drop at:for] [--out file]"); process.exit(2) }
const provider = value("provider") === "elevenlabs" ? "elevenlabs" : "fake"
if (provider === "elevenlabs" && process.env.RUN_LIVE_PROVIDER_TESTS !== "1") { console.error("The real provider costs money. Set RUN_LIVE_PROVIDER_TESTS=1 to run it."); process.exit(2) }
const speed = Number(value("speed") ?? 1), partSeconds = flag("no-parts") ? 0 : Number(value("parts") ?? 30), withChat = !flag("no-chat")
const drop = value("drop")?.split(":").map(Number) as [number, number] | undefined

const manifest: FixtureManifest = JSON.parse(await readFile(join(directory, "manifest.json"), "utf8"))
const rate = manifest.sampleRate, channels = Object.keys(manifest.channels) as Array<"mic" | "system">
const pcm = Object.fromEntries(await Promise.all(channels.map(async channel => [channel, new Uint8Array(await readFile(join(directory, manifest.channels[channel]!.file))).subarray(44)] as const))) as Record<"mic" | "system", Uint8Array>
const totalSamples = Math.max(...channels.map(channel => manifest.channels[channel]!.samples))

const stack = await startLiveStack({ provider, ...(value("model") ? { model: value("model") } : {}), chat: "echo", ...(value("vad") ? { vadSilenceSeconds: Number(value("vad")) } : {}) })
const headers = { Authorization: `Bearer ${stack.token}`, "Content-Type": "application/json" }
const api = async (path: string, method = "GET", body?: unknown) => {
  const response = await fetch(`${stack.base}/api/v1${path}`, { method, headers, ...(body === undefined ? {} : { body: JSON.stringify(body) }) })
  if (!response.ok) throw new Error(`${method} ${path} failed with ${response.status}`)
  return response
}
const note = await (await api("/notes", "POST", { title: "Replay of a synthetic meeting" })).json() as { id: string }
await api(`/notes/${note.id}/recording`, "POST", { command: "start" })

// ---------------------------------------------------------------- the client
const started = performance.now(), now = () => performance.now() - started
const received: Array<{ at: number; message: LiveServerMessage }> = []
const frameSamples = Math.round(rate / 10)
let socket: WebSocket, cursor = 0, connections = 0, ready: Extract<LiveServerMessage, { type: "ready" }> | undefined
const acked: Record<string, number> = {}
function connect() {
  ready = undefined; connections++
  socket = new WebSocket(`${stack.base.replace("http", "ws")}/api/v1/notes/${note.id}/live`, { headers: { Authorization: `Bearer ${stack.token}` } } as never)
  socket.onmessage = event => {
    const message = JSON.parse(String(event.data)) as LiveServerMessage
    received.push({ at: now(), message })
    if (message.type === "ready") ready = message
    if (message.type === "ack") acked[message.channel] = message.throughSample
    if (message.type === "error") console.log(`  server error: ${message.code} (${message.message})`)
  }
  socket.onopen = () => socket.send(JSON.stringify({ type: "hello", protocol: 1, captureId: "replay", language: "en", streams: channels.map(channel => ({ channel, sampleRate: rate, encoding: "pcm_s16le", baseMs: 0, resumeFromSample: acked[channel] ?? 0 })) }))
  return new Promise<void>((resolve, reject) => {
    const timer = setInterval(() => { if (ready) { clearInterval(timer); resolve() } else if (socket.readyState === WebSocket.CLOSED) { clearInterval(timer); reject(new Error("The live socket was refused")) } }, 10)
  })
}
await connect()
console.log(`Streaming ${(totalSamples / rate).toFixed(1)} s on ${channels.join(" + ")} to ${stack.provider} ${stack.model} at ${speed}x${drop ? `, dropping the connection at ${drop[0]} s for ${drop[1]} s` : ""}`)

// ---------------------------------------------------------------- chat polling
type Probe = { line: FixtureLine; index: number; expected: number; pattern: RegExp; endAt?: number; interimAt?: number; finalAt?: number; answerableAt?: number; citableAt?: number }
const count = (text: string, pattern: RegExp) => text.match(new RegExp(pattern.source, "gi"))?.length ?? 0
const probes: Probe[] = manifest.lines.map((line, index) => {
  const pattern = new RegExp(line.probe, "i")
  return { line, index, pattern, expected: manifest.lines.slice(0, index + 1).filter(other => pattern.test(other.text)).length }
})
const section = (context: string, tag: string) => new RegExp(`<${tag}>\\n([\\s\\S]*?)\\n</${tag}>`).exec(context)?.[1] ?? ""
let polling = true, chatRequests = 0
const chatLoop = withChat ? (async () => {
  while (polling) {
    const waiting = probes.filter(probe => probe.endAt !== undefined && probe.citableAt === undefined)
    if (waiting.length) {
      const response = await api(`/notes/${note.id}/chat/stream`, "POST", { message: "What was just said?", history: [], includeSources: true })
      await response.text()
      chatRequests++
      const recorded = stack.chats.at(-1), at = now()
      if (recorded) {
        const stored = section(recorded.context, "transcript"), unfinished = section(recorded.context, "unfinished")
        for (const probe of waiting) {
          if (probe.answerableAt === undefined && count(stored + "\n" + unfinished, probe.pattern) >= probe.expected) probe.answerableAt = at
          if (count(stored, probe.pattern) >= probe.expected) probe.citableAt = at
        }
      }
    }
    await Bun.sleep(250)
  }
})() : Promise.resolve()

// ---------------------------------------------------------------- durable parts, uploaded as the capture passes them
const partSamples = partSeconds * rate, uploads: Promise<unknown>[] = []
let nextPart = 0
function uploadDue(captured: number) {
  while (partSamples && (nextPart + 1) * partSamples <= captured || partSamples && captured >= totalSamples && nextPart * partSamples < totalSamples) {
    const from = nextPart * partSamples, to = Math.min(totalSamples, from + partSamples), sequence = nextPart++
    for (const channel of channels) uploads.push(api(`/notes/${note.id}/audio/chunks/${crypto.randomUUID()}`, "PUT", { channel, sequence, startMs: Math.round(from * 1000 / rate), audioBase64: Buffer.from(wavFile(pcm[channel].slice(from * 2, to * 2), rate)).toString("base64") }))
    if (to >= totalSamples) break
  }
}

// ---------------------------------------------------------------- streaming
const origin = performance.now()
let dropped = false, captured = 0
while (cursor < totalSamples) {
  // The capture keeps growing whether or not the connection is up; the stream reads from the file.
  captured = Math.min(totalSamples, Math.floor((performance.now() - origin) * speed * rate / 1000))
  uploadDue(captured)
  if (drop && !dropped && captured >= drop[0] * rate) {
    dropped = true
    console.log(`  ${now().toFixed(0)} ms: dropping the connection`)
    ;(socket as unknown as { terminate(): void }).terminate()
    await Bun.sleep(drop[1] * 1000 / speed)
    await connect()
    captured = Math.min(totalSamples, Math.floor((performance.now() - origin) * speed * rate / 1000))
    const accept = Math.min(...ready!.streams.map(stream => stream.acceptFromSample)), backlogMs = (captured - accept) * 1000 / rate
    // On reconnection: replay a backlog within the resume window, otherwise jump to five seconds before the end.
    cursor = backlogMs <= ready!.resumeWindowMs ? accept : Math.max(accept, captured - 5 * rate)
    console.log(`  ${now().toFixed(0)} ms: reconnected; server accepts from ${(accept / rate).toFixed(1)} s, capture is at ${(captured / rate).toFixed(1)} s, ${backlogMs <= ready!.resumeWindowMs ? "replaying the backlog" : "skipping to " + (cursor / rate).toFixed(1) + " s"}`)
  }
  const end = Math.min(captured, cursor + frameSamples)
  // Wait until the capture holds a whole frame (the last frame may be short).
  if (end - cursor < frameSamples && end < totalSamples) { await Bun.sleep(2); continue }
  for (const channel of channels) socket.send(encodeLiveAudioFrame({ channel, firstSample: cursor, pcm: pcm[channel].subarray(cursor * 2, end * 2) }) as Uint8Array<ArrayBuffer>)
  for (const probe of probes) if (probe.endAt === undefined && probe.line.lastSample < end) probe.endAt = now()
  cursor = end
  // Behind the capture (after a reconnect): catch up at no more than twice real time.
  if (captured - cursor >= frameSamples) await Bun.sleep(100 / speed / 2)
}
console.log(`  ${now().toFixed(0)} ms: all audio sent; waiting for the last text`)
// Trailing wait so the recogniser can finish the last sentence before the end mark flushes it.
await Bun.sleep(3000)
polling = false
await chatLoop
socket.send(JSON.stringify({ type: "mark", kind: "end", atSample: Object.fromEntries(channels.map(channel => [channel, totalSamples])) }))
await new Promise<void>(resolve => { const timer = setInterval(() => { if (socket.readyState === WebSocket.CLOSED) { clearInterval(timer); resolve() } }, 20); setTimeout(() => { clearInterval(timer); resolve() }, 8000) })
uploadDue(totalSamples)
await Promise.all(uploads)
await api(`/notes/${note.id}/recording`, "POST", { command: "stop" })
let chunks = await Effect.runPromise(stack.repository.listAudioChunks(note.id))
for (const deadline = performance.now() + 180_000; chunks.some(chunk => chunk.state !== "ready" && chunk.state !== "failed") && performance.now() < deadline;) { await Bun.sleep(500); chunks = await Effect.runPromise(stack.repository.listAudioChunks(note.id)) }

// ---------------------------------------------------------------- results
for (const { at, message } of received) {
  if (message.type !== "interim" && message.type !== "final") continue
  const text = message.type === "interim" ? message.text : message.segment.text, channel = message.type === "interim" ? message.channel : message.segment.channel
  for (const probe of probes) {
    if (probe.line.channel !== channel || probe.endAt === undefined || !probe.pattern.test(text)) continue
    // The text must belong to this line: on its channel and not before the line began.
    const lineStartMs = probe.line.firstSample * 1000 / rate, textStartMs = message.type === "interim" ? message.endMs : message.segment.endMs ?? message.segment.startMs
    if (textStartMs < lineStartMs) continue
    if (message.type === "interim") probe.interimAt ??= at; else probe.finalAt ??= at
  }
}
const stats = (values: number[]) => {
  const sorted = [...values].sort((a, b) => a - b), at = (p: number) => sorted.length ? Math.round(sorted[Math.min(sorted.length - 1, Math.ceil(p * sorted.length) - 1)]!) : null
  return { n: sorted.length, min: at(0.0001), median: at(0.5), p90: at(0.9), p95: at(0.95), max: at(1) }
}
const delays = (pick: (probe: Probe) => number | undefined) => probes.flatMap(probe => { const at = pick(probe); return at === undefined || probe.endAt === undefined ? [] : [Math.max(0, at - probe.endAt)] })
const stored = await (await api(`/notes/${note.id}`)).json() as { transcriptSegments: Array<{ id: string; channel: "mic" | "system"; speakerId: string; speakerLabel: string | null; text: string; startMs: number; endMs: number | null }>; transcriptRevision: number }
const words = (text: string) => text.toLowerCase().replace(/[^a-z0-9' ]+/g, " ").split(/\s+/).filter(Boolean)
function wordErrorRate(reference: string[], hypothesis: string[]) {
  let previous = Array.from({ length: hypothesis.length + 1 }, (_, index) => index)
  for (let i = 1; i <= reference.length; i++) {
    const row = [i]
    for (let j = 1; j <= hypothesis.length; j++) row[j] = Math.min(previous[j]! + 1, row[j - 1]! + 1, previous[j - 1]! + (reference[i - 1] === hypothesis[j - 1] ? 0 : 1))
    previous = row
  }
  return reference.length ? previous[hypothesis.length]! / reference.length : 0
}
const transcript = (channel: "mic" | "system") => stored.transcriptSegments.filter(segment => segment.channel === channel).sort((a, b) => a.startMs - b.startMs).map(segment => segment.text).join(" ")
const accuracy = Object.fromEntries(channels.map(channel => [channel, wordErrorRate(words(manifest.lines.filter(line => line.channel === channel).map(line => line.text).join(" ")), words(transcript(channel)))]))
const everything = [...stored.transcriptSegments].sort((a, b) => a.startMs - b.startMs).map(segment => segment.text).join(" ")
const distinct = [...new Map(probes.map(probe => [probe.pattern.source, probe])).values()]
const probeCounts = distinct.map(probe => ({ probe: probe.pattern.source, expected: manifest.lines.filter(line => probe.pattern.test(line.text)).length, found: count(everything, probe.pattern) }))
const names = manifest.lines.map(line => ({ token: line.token, heard: new RegExp(`\\b${line.token}\\b`, "i").test(everything) }))
// Who the recogniser thinks is speaking, against the voice that rendered each line.
const diarisation: Record<string, Record<string, number>> = {}
for (const segment of stored.transcriptSegments) {
  const middle = (segment.startMs + (segment.endMs ?? segment.startMs)) / 2 * rate / 1000
  const line = manifest.lines.find(candidate => candidate.channel === segment.channel && middle >= candidate.firstSample - rate / 4 && middle <= candidate.lastSample + rate / 4)
  const row = diarisation[segment.speakerId] ??= {}
  row[line?.voice ?? "(between lines)"] = (row[line?.voice ?? "(between lines)"] ?? 0) + words(segment.text).length
}
const issued = new Map(stack.chats.flatMap(chat => chat.citations.filter(citation => citation.source === "transcript")).map(citation => [`${citation.segmentId}:${citation.quote}`, citation]))
const citations = await checkChatCitations([...issued.values()], stack.repository, new AbortController().signal)
const ledger = await Effect.runPromise(stack.repository.live!.ledger(note.id))
const gaps = received.flatMap(({ message }) => message.type === "gap" ? [message] : [])
const upstreamSeconds = stack.upstream.seconds, cost = provider === "elevenlabs" ? upstreamSeconds / 3600 * 0.22 + stack.batch.seconds / 3600 * 0.20 : 0
const report = {
  fixture: { seconds: totalSamples / rate, channels, lines: manifest.lines.length }, provider: stack.provider, model: stack.model, speed, chatPolling: withChat, chatRequests, drop: drop ?? null, connections,
  delayMs: { speechEndToInterim: stats(delays(probe => probe.interimAt)), speechEndToFinal: stats(delays(probe => probe.finalAt)), speechEndToAnswerable: stats(delays(probe => probe.answerableAt)), speechEndToCitable: stats(delays(probe => probe.citableAt)) },
  messages: Object.fromEntries([...new Set(received.map(item => item.message.type))].map(type => [type, received.filter(item => item.message.type === type).length])),
  gaps, transcript: { segments: stored.transcriptSegments.length, revision: stored.transcriptRevision, wordErrorRate: accuracy, probeCounts: probeCounts.filter(item => item.expected !== item.found), probesChecked: probeCounts.length, inventedNamesHeard: `${names.filter(name => name.heard).length}/${names.length}`, inventedNamesMissed: names.filter(name => !name.heard).map(name => name.token), diarisation },
  parts: { uploaded: chunks.length, states: Object.fromEntries([...new Set(chunks.map(chunk => chunk.state))].map(state => [state, chunks.filter(chunk => chunk.state === state).length])), resolutions: Object.fromEntries([...new Set(chunks.map(chunk => String(chunk.resolution)))].map(resolution => [resolution, chunks.filter(chunk => String(chunk.resolution) === resolution).length])), batchCalls: stack.batch.calls, batchSeconds: Math.round(stack.batch.seconds),
    notLive: chunks.filter(chunk => chunk.resolution !== "live").map(chunk => ({ channel: chunk.channel, startMs: chunk.startMs, durationMs: chunk.durationMs, state: chunk.state, resolution: chunk.resolution ?? null })),
    ledger: ledger.map(row => `${row.channel} ${row.source} ${row.fromMs}-${row.toMs}`) },
  citations: { issued: issued.size, stillValid: citations.valid.length },
  cost: { upstreamSessions: stack.upstream.sessions, upstreamAudioSeconds: Math.round(upstreamSeconds), usd: Number(cost.toFixed(4)), basis: "list price: $0.22 per hour of audio per stream, $0.20 per hour of batch audio" },
  perLine: probes.map(probe => ({ channel: probe.line.channel, voice: probe.line.voice, probe: probe.line.probe, interimMs: probe.interimAt === undefined ? null : Math.round(probe.interimAt - probe.endAt!), finalMs: probe.finalAt === undefined ? null : Math.round(probe.finalAt - probe.endAt!), answerableMs: probe.answerableAt === undefined ? null : Math.round(probe.answerableAt - probe.endAt!), citableMs: probe.citableAt === undefined ? null : Math.round(probe.citableAt - probe.endAt!) })),
  storedTranscript: stored.transcriptSegments.map(segment => `${segment.channel} ${segment.speakerLabel} ${Math.round(segment.startMs / 100) / 10}s: ${segment.text}`),
}
if (value("out")) await writeFile(value("out")!, JSON.stringify(report, null, 2) + "\n")
const { perLine, storedTranscript, ...summary } = report
console.log(JSON.stringify(summary, null, 2))
if (provider === "fake") console.log("The fake recogniser does not understand speech: delay and transcript figures above only show that the path works.")
await stack.stop()
process.exit(0)
