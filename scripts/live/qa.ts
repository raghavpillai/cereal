/**
 * The live transcription test campaign, against a running server with its real recogniser and language
 * model.
 *
 *   bun scripts/live/make-qa-fixtures.ts <fixtures>            once: synthetic audio (macOS `say`)
 *   bun --env-file=.env scripts/live/qa.ts <scenario> --fixtures <fixtures> [--base http://127.0.0.1:3000] [--out <dir>]
 *
 * Scenarios: refusals, long, drops, pause, concurrency, pace, admin, audio, generate, cleanup.
 * Authenticates with ADMIN_API_TOKEN (the owner's token). Costs provider credit: the long scenario
 * streams about 39 stream-minutes ($0.14 at list price), the rest together about as much again.
 * Every note it creates has "(safe to delete)" in its title and is deleted when its scenario ends,
 * except the long meeting's note, which `generate` uses and then deletes. `cleanup` deletes any that
 * an interrupted run left behind.
 */
import { mkdir, writeFile } from "node:fs/promises"
import { join } from "node:path"
import { encodeLiveAudioFrame, type LiveServerMessage } from "@meeting-notes/contracts/live"
import { numberWords } from "./make-qa-fixtures"
import { checkTranscript, load, loadFixture, partFacts, Recording, Server, silence, stats, type StoredNote } from "./qa-lib"

const args = process.argv.slice(2), scenario = args[0]
const value = (name: string) => { const index = args.indexOf(`--${name}`); return index >= 0 ? args[index + 1] : undefined }
const fixtures = value("fixtures"), out = value("out") ?? fixtures
// `--base stack` runs the scenario against an in-process server (scripts/live/stack.ts) with the real recogniser
// and the real language model, whose requests it meters: that is where prompt and cached token counts come from.
const stack = value("base") === "stack" ? await (await import("./stack")).startLiveStack({ provider: "elevenlabs", chat: "provider", meterLanguage: true }) : null
const base = stack?.base ?? value("base") ?? "http://127.0.0.1:3000", token = stack?.token ?? process.env.ADMIN_API_TOKEN
if (!scenario || !fixtures || !token) { console.error("usage: bun --env-file=.env scripts/live/qa.ts <scenario> --fixtures <dir> [--base url|stack] [--out dir] [--tag name]"); process.exit(2) }
const local = /127\.0\.0\.1:3000|localhost:3000/.test(base)
const server = new Server(base, token, local ? { context: process.env.DOCKER_CONTEXT ?? "colima-meeting-notes", app: "meeting-notes-app-1", db: "meeting-notes-db-1" } : null)
const TITLE = (name: string) => `Live QA: ${name} (safe to delete)`
const OWNER = "00000000-0000-4000-8000-000000000001"

const checks: Array<{ name: string; pass: boolean; detail: string }> = []
const check = (name: string, pass: boolean, detail: unknown = "") => { checks.push({ name, pass, detail: typeof detail === "string" ? detail : JSON.stringify(detail) }); console.log(`${pass ? "PASS" : "FAIL"}  ${name}${detail === "" ? "" : `  (${typeof detail === "string" ? detail : JSON.stringify(detail)})`}`) }
const report: Record<string, unknown> = { scenario, base, startedAt: new Date().toISOString(), loadAtStart: load() }
const startedAt = new Date()
async function finishReport() {
  // Anything in the app log that looks like a failure, since this scenario began. The server logs no request content.
  const lines = server.logs(startedAt), bad = lines.filter(line => /error|exception|unhandled|panic|stack|    at /i.test(line) && !/^Live transcription: the \w+ provider session ended/.test(line))
  report.appLog = { lines: lines.length, providerSessionEnded: lines.filter(line => /provider session ended/.test(line)), suspicious: bad.slice(0, 20) }
  check("app log has no errors or stack traces", bad.length === 0, bad.length ? bad.slice(0, 3) : `${lines.length} lines`)
  check("server still ready", await server.ready() === 200)
  if (stack) report.stack = { recogniser: `${stack.provider} ${stack.model}`, upstreamAudioSeconds: Math.round(stack.upstream.seconds), languageRequests: stack.languageUsage.length, languageCostUsd: Number(stack.languageUsage.reduce((sum, usage) => sum + (usage.costUsd ?? 0), 0).toFixed(4)) }
  report.loadAtEnd = load(); report.finishedAt = new Date().toISOString(); report.checks = checks
  await mkdir(out!, { recursive: true })
  await writeFile(join(out!, `result-${scenario}${value("tag") ? `-${value("tag")}` : ""}.json`), JSON.stringify(report, null, 1) + "\n")
  if (stack) await stack.stop()
  const failed = checks.filter(item => !item.pass)
  console.log(`\n${scenario}: ${checks.length - failed.length} of ${checks.length} checks passed; load ${report.loadAtStart} -> ${report.loadAtEnd}`)
  process.exit(failed.length ? 1 : 0)
}
const upgradeHeaders = { Connection: "Upgrade", Upgrade: "websocket", "Sec-WebSocket-Version": "13", "Sec-WebSocket-Key": "dGhlIHNhbXBsZSBub25jZQ==" }
const hello = (captureId: string, streams: unknown = [{ channel: "system", sampleRate: 16000, encoding: "pcm_s16le", baseMs: 0, resumeFromSample: 0 }], protocol = 1) => JSON.stringify({ type: "hello", protocol, captureId, streams })
/** Opens a socket, runs the steps, and reports how the server ended it. */
async function probe(noteId: string, steps: (socket: WebSocket, state: { messages: LiveServerMessage[]; wait: (type: string, ms?: number) => Promise<boolean> }) => Promise<void>, waitCloseMs = 6000, authorization = `Bearer ${token}`) {
  const messages: LiveServerMessage[] = []
  let code: number | undefined, opened = false
  const socket = new WebSocket(`${base.replace("http", "ws")}/api/v1/notes/${noteId}/live`, { headers: { Authorization: authorization } } as never)
  socket.onmessage = event => { try { messages.push(JSON.parse(String(event.data))) } catch { /* not JSON */ } }
  socket.onclose = event => { code = event.code }
  socket.onopen = () => { opened = true }
  for (const deadline = performance.now() + 8000; !opened && code === undefined && performance.now() < deadline;) await Bun.sleep(10)
  const wait = async (type: string, ms = 12_000) => { for (const deadline = performance.now() + ms; performance.now() < deadline;) { if (messages.some(message => message.type === type) || code !== undefined) return messages.some(message => message.type === type); await Bun.sleep(10) } return false }
  if (opened) await steps(socket, { messages, wait }).catch(() => undefined)
  for (const deadline = performance.now() + waitCloseMs; code === undefined && performance.now() < deadline;) await Bun.sleep(10)
  if (code === undefined) socket.close()
  return { opened, code, messages, error: messages.find(message => message.type === "error") as Extract<LiveServerMessage, { type: "error" }> | undefined }
}
const summary = (recording: Recording) => ({
  interimMs: stats(recording.delays("interim")), finalMs: stats(recording.delays("final")),
  messages: Object.fromEntries([...new Set(recording.received.map(item => item.message.type))].map(type => [type, recording.received.filter(item => item.message.type === type).length])),
  gaps: recording.of("gap").map(item => item.message), errors: recording.of("error").map(item => item.message), closes: recording.closes.map(item => item.code), connections: recording.connections,
  revisions: (() => { const list = recording.of("final").map(item => item.message.transcriptRevision); return { count: list.length, monotonic: list.every((revision, index) => index === 0 || revision >= list[index - 1]!), last: list.at(-1) ?? null } })(),
  maxLagMs: Math.max(0, ...recording.of("status").filter(item => item.message.state !== "provider_unavailable").map(item => item.message.lagMs)),
})

// ---------------------------------------------------------------------------------------------- refusals
async function refusals() {
  const live = (noteId: string, headers: Record<string, string>) => fetch(`${base}/api/v1/notes/${noteId}/live`, { headers: { ...upgradeHeaders, ...headers } })
  const recording = new Recording(server, silence(1), { title: TITLE("refusals"), quiet: true })
  const idle = (await server.ok("/notes", "POST", { title: TITLE("refusals, not recording") })).id as string
  await recording.create()
  try {
    const leak = (text: string) => /\bat .+:\d+|\.ts:\d|stack|prisma|postgres|select |ECONN|xi-api|api\.elevenlabs/i.test(text)
    for (const [name, headers, id, status, error] of [
      ["no credentials", {}, recording.noteId, 401, "unauthorized"],
      ["a wrong bearer token", { Authorization: "Bearer not-a-real-token-0123456789abcdef0123456789" }, recording.noteId, 401, "unauthorized"],
      ["a malformed Authorization header", { Authorization: "Basic dXNlcjpwYXNz" }, recording.noteId, 401, "unauthorized"],
      ["a session cookie without a bearer token", { Cookie: "better-auth.session_token=forged.value" }, recording.noteId, 401, "unauthorized"],
      ["a valid token from an untrusted web origin", { Authorization: `Bearer ${token}`, Origin: "https://evil.example" }, recording.noteId, 403, "forbidden"],
      ["a note that does not exist", { Authorization: `Bearer ${token}` }, crypto.randomUUID(), 404, "not_found"],
      ["an identifier that is not a UUID", { Authorization: `Bearer ${token}` }, "../../etc/passwd", 404, null],
      ["a note that is not recording", { Authorization: `Bearer ${token}` }, idle, 409, "conflict"],
    ] as const) {
      const response = await live(id, headers), text = await response.text()
      let body: { error?: string } = {}; try { body = JSON.parse(text) } catch { /* not JSON */ }
      check(`refused before the upgrade: ${name}`, response.status === status && (error === null || body.error === error) && !leak(text), `HTTP ${response.status} ${text.slice(0, 90)}`)
    }
    // Another account's note: indistinguishable from a note that does not exist.
    const foreign = server.sql(`select id from "Note" where "ownerId" <> '${OWNER}' limit 1`)[0]?.[0]
    if (foreign) {
      const response = await live(foreign, { Authorization: `Bearer ${token}` }), text = await response.text()
      check("refused before the upgrade: another account's note", response.status === 404 && text === JSON.stringify({ error: "not_found", message: "Note not found" }), `HTTP ${response.status} ${text}`)
      const read = await server.call(`/notes/${foreign}`)
      check("the same note is also invisible to the ordinary read", read.status === 404, `HTTP ${read.status}`)
    } else check("refused before the upgrade: another account's note", false, "no other account's note in this database to test with")

    const expect = async (name: string, wanted: number, steps: Parameters<typeof probe>[1], waitMs = 6000) => {
      const result = await probe(recording.noteId, steps, waitMs)
      check(`after the upgrade: ${name}`, result.opened && result.code === wanted && (wanted < 4000 || result.error !== undefined) && !leak(JSON.stringify(result.messages)), `close ${result.code}${result.error ? `, error ${result.error.code}: ${result.error.message}` : ""}`)
      return result
    }
    await expect("text that is not JSON", 4400, async socket => { socket.send("this is not json") })
    await expect("JSON that is not a protocol message", 4400, async socket => { socket.send(JSON.stringify({ type: "subscribe", note: "all" })) })
    await expect("hello with protocol version 2", 4400, async socket => { socket.send(hello("p2", undefined, 2)) })
    await expect("hello with no streams", 4400, async socket => { socket.send(hello("none", [])) })
    await expect("hello naming a channel twice", 4400, async socket => { const stream = { channel: "mic", sampleRate: 16000, encoding: "pcm_s16le", baseMs: 0, resumeFromSample: 0 }; socket.send(hello("twice", [stream, stream])) })
    await expect("hello with a sample rate the recogniser does not take", 4400, async socket => { socket.send(hello("rate", [{ channel: "mic", sampleRate: 12345, encoding: "pcm_s16le", baseMs: 0, resumeFromSample: 0 }])) })
    await expect("hello with an unknown encoding", 4400, async socket => { socket.send(hello("enc", [{ channel: "mic", sampleRate: 16000, encoding: "opus", baseMs: 0, resumeFromSample: 0 }])) })
    await expect("a mark before hello", 4400, async socket => { socket.send(JSON.stringify({ type: "mark", kind: "end", atSample: {} })) })
    await expect("no hello at all", 4400, async () => { await Bun.sleep(100) }, 14_000)
    await expect("an audio frame with a bad header", 4400, async (socket, state) => { socket.send(hello("bad-frame")); await state.wait("ready"); socket.send(new Uint8Array([9, 7, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4])) })
    await expect("an audio frame with an odd number of bytes", 4400, async (socket, state) => { socket.send(hello("odd")); await state.wait("ready"); const frame = encodeLiveAudioFrame({ channel: "system", firstSample: 0, pcm: new Uint8Array(320) }); socket.send(frame.subarray(0, frame.byteLength - 1) as Uint8Array<ArrayBuffer>) })
    await expect("an audio frame of 70 KiB", 4400, async (socket, state) => { socket.send(hello("big")); await state.wait("ready"); socket.send(encodeLiveAudioFrame({ channel: "system", firstSample: 0, pcm: new Uint8Array(70 * 1024) }) as Uint8Array<ArrayBuffer>) })
    const huge = await probe(recording.noteId, async (socket, state) => { socket.send(hello("huge")); await state.wait("ready"); socket.send(new Uint8Array(1024 * 1024)) })
    check("after the upgrade: a 1 MiB frame closes the connection", huge.opened && huge.code !== undefined && huge.code !== 1000, `close ${huge.code}`)
    await expect("hello sent twice", 4400, async (socket, state) => { socket.send(hello("again")); await state.wait("ready"); socket.send(hello("again")) })
    // One publisher per note: a second device is refused while the first is connected, and the first carries on.
    const first = await probe(recording.noteId, async (socket, state) => {
      socket.send(hello("device-a")); await state.wait("ready")
      const second = await probe(recording.noteId, async other => { other.send(hello("device-b")) }, 6000)
      check("after the upgrade: a second publisher is refused with 4409", second.code === 4409 && second.error?.code === "replaced", `close ${second.code}`)
      socket.send(encodeLiveAudioFrame({ channel: "system", firstSample: 0, pcm: new Uint8Array(3200) }) as Uint8Array<ArrayBuffer>)
      await Bun.sleep(1500)
      check("the first publisher is unaffected", socket.readyState === WebSocket.OPEN && state.messages.some(message => message.type === "ack"), `${state.messages.map(message => message.type).join(",")}`)
      socket.send(JSON.stringify({ type: "mark", kind: "end", atSample: { system: 1600 } }))
    }, 8000)
    check("a clean end closes with 1000", first.code === 1000, `close ${first.code}`)
    // A stopped recording's socket is refused again, and a stale client cannot attach to it.
    await server.ok(`/notes/${recording.noteId}/recording`, "POST", { command: "stop" })
    const stopped = await live(recording.noteId, { Authorization: `Bearer ${token}` })
    check("refused before the upgrade: the note after Stop", stopped.status === 409, `HTTP ${stopped.status}`)
    const note = await recording.stored()
    check("nothing was stored by any of it", note.transcriptSegments.length === 0, `${note.transcriptSegments.length} segments`)
  } finally {
    report.deleted = [await recording.remove(), (await server.call(`/notes/${idle}`, "DELETE")).status]
  }
}

/** Item numbers an answer names, whether written as digits or as words ("Item forty-seven"). */
function itemNumbers(text: string): number[] {
  const found: Array<[number, number]> = []
  for (const match of text.matchAll(/\bitem\s+(\d+)/gi)) found.push([match.index, Number(match[1])])
  for (let n = 200; n >= 1; n--) for (const match of text.matchAll(new RegExp(`\\bitem\\s+${numberWords(n).replace(/ and /g, " (?:and )?").replace(/ /g, "[ -]")}\\b`, "gi"))) if (!found.some(([index]) => index === match.index)) found.push([match.index, n])
  return found.sort((a, b) => a[0] - b[0]).map(([, n]) => n)
}
// ---------------------------------------------------------------------------------------------- long meeting
async function long() {
  const fixture = await loadFixture(join(fixtures!, "long"), value("seconds") ? { seconds: Number(value("seconds")) } : {})
  const minutes = fixture.totalSamples / fixture.rate / 60
  const recording = new Recording(server, fixture, { title: TITLE(`long meeting, ${minutes.toFixed(0)} min`), captureId: "qa-long" })
  report.memoryBefore = server.memory()
  const memory: Array<{ atSeconds: number; memory: string; load: string }> = []
  const asked: Array<Record<string, unknown>> = []
  const ask = (label: string) => async (r: Recording) => {
    const ended = [...r.lineEnd.keys()].sort((a, b) => a - b).at(-1), last = ended === undefined ? null : fixture.lines[ended]!
    const reply = await r.ask("Which item number was mentioned most recently, and what was said about it? Start your answer with the item number.")
    const numbers = itemNumbers(reply.answer)
    // The newest text: the line that had just ended, or the one being spoken (it is in the unfinished tail).
    const newest = last ? [last.item, last.item + 1] : []
    const pass = reply.status === 200 && !reply.error && numbers.some(number => newest.includes(number))
    asked.push({ label, atSeconds: Math.round(r.captured / fixture.rate), lastItemSpoken: last?.item ?? null, itemsInAnswer: numbers.slice(0, 4), firstByteMs: reply.firstByteMs, firstTextMs: reply.firstTextMs, totalMs: reply.totalMs, citations: reply.citations.length, load: load(), answer: reply.answer.slice(0, 220), error: reply.error })
    check(`chat ${label} answers from the newest text`, pass, `last spoken item ${last?.item}, answer names ${numbers.slice(0, 3).join("/") || "none"}; first text after ${reply.firstTextMs} ms`)
  }
  const total = fixture.totalSamples / fixture.rate
  await recording.create()
  check("the socket opened with the configured model", await recording.connect() === "ready", `${recording.ready?.provider} ${recording.ready?.model}`)
  await recording.run([
    { atSeconds: 30, run: ask("near the start (0:30)") }, { atSeconds: Math.round(total / 2), run: ask("in the middle") }, { atSeconds: Math.round(total - 20), run: ask("near the end") },
    ...Array.from({ length: Math.floor(total / 120) }, (_, index) => ({ atSeconds: (index + 1) * 120, run: () => { memory.push({ atSeconds: (index + 1) * 120, memory: server.memory(), load: load() }) } })),
  ])
  const end = await recording.finish({ settleMs: 3000 })
  const chunks = await recording.partsSettled()
  const note = await recording.stored(), facts = partFacts(server, recording.noteId), transcript = checkTranscript(note, fixture), s = summary(recording)
  report.memoryAfter = server.memory(); report.memory = memory; report.chat = asked; report.end = end; report.stream = s; report.transcript = transcript; report.parts = facts.parts; report.ledger = facts.ledger; report.origins = facts.origins; report.noteSpeakers = facts.speakers
  report.noteId = recording.noteId; report.minutes = minutes; report.revision = note.transcriptRevision; report.duration = note.duration
  check("every scripted line is in the transcript exactly once", transcript.missing.length === 0 && transcript.duplicated.length === 0 && transcript.wrongChannel.length === 0, { lines: transcript.linesCaptured, missing: transcript.missing, duplicated: transcript.duplicated, wrongChannel: transcript.wrongChannel })
  check("no drift between sample positions and segment times", Math.abs(transcript.drift.changeMs) <= 150 && transcript.drift.minMs >= -400 && transcript.drift.maxMs <= 600, transcript.drift)
  check("every durable part was resolved from live coverage, with no batch call", chunks.length > 0 && facts.parts.length === 1 && /ready\/live$/.test(facts.parts[0]!), facts.parts)
  check("speaker labels are stable for the whole meeting", Object.values(transcript.speakers).every(row => Object.keys(row).length === 1) && new Set(Object.values(transcript.speakers).map(row => Object.keys(row)[0])).size === Object.keys(transcript.speakers).length, transcript.speakers)
  check("no gap, error or reconnect on the client side", s.gaps.length === 0 && s.errors.length === 0 && s.connections === 1, { gaps: s.gaps.length, errors: s.errors.length, connections: s.connections })
  check("revisions on the socket only ever grow, and match the stored note", s.revisions.monotonic && s.revisions.last === note.transcriptRevision, { finals: s.revisions.count, last: s.revisions.last, stored: note.transcriptRevision })
  check("the socket closed with 1000 and Stop returned", end.closeCode === 1000 && end.stopMs !== null, end)
  check("interim text within 2.5 s of speech at the median", (s.interimMs.median ?? 1e9) <= 2500, s.interimMs)
  check("final text within 6 s of speech at the median", (s.finalMs.median ?? 1e9) <= 6000, s.finalMs)
  console.log(`NOTE_ID=${recording.noteId} (kept for the generate scenario)`)
  await writeFile(join(out!, "long-note-id.txt"), recording.noteId)
}

// ---------------------------------------------------------------------------------------------- cleanup
async function cleanup() {
  const notes = await server.ok("/notes") as Array<{ id: string; title: string; recordingState: string }>
  const mine = notes.filter(note => /\(safe to delete\)/.test(note.title))
  for (const note of mine) { if (note.recordingState === "recording") await server.call(`/notes/${note.id}/recording`, "POST", { command: "stop" }); await server.call(`/notes/${note.id}`, "DELETE") }
  const after = await server.ok("/notes") as Array<{ title: string }>
  report.deleted = mine.map(note => note.title); report.ownerNotes = after.length; report.databaseNotes = server.sql('select count(*) from "Note"')[0]?.[0]
  check("no test note is left", !after.some(note => /\(safe to delete\)/.test(note.title)), `${mine.length} deleted; owner lists ${after.length}, database holds ${report.databaseNotes}`)
}


// ---------------------------------------------------------------------------------------------- shared by the scenarios below
type Outcome = Awaited<ReturnType<typeof outcome>>
/** Ends a recording, waits for its parts, and gathers what every scenario reports about it. */
async function outcome(recording: Recording, options: { finish?: Parameters<Recording["finish"]>[0]; baseMs?: number; cuts?: number[]; keep?: boolean } = {}) {
  const end = await recording.finish({ settleMs: 3000, ...options.finish })
  const chunks = await recording.partsSettled()
  const note = await recording.stored(), facts = partFacts(server, recording.noteId)
  const cuts = [...recording.cuts, ...recording.resumes.filter(resume => !resume.replay).map(resume => resume.cursorSample), ...(options.cuts ?? [])]
  const transcript = checkTranscript(note, recording.fixture, { capturedSamples: recording.captured, ...(options.baseMs === undefined ? {} : { baseMs: options.baseMs }), cutSamples: cuts })
  const result = { end, stream: summary(recording), transcript, parts: facts.parts, ledger: facts.ledger, origins: facts.origins, noteSpeakers: facts.speakers, failedParts: chunks.filter(chunk => chunk.state !== "ready").length, revision: note.transcriptRevision, resumes: recording.resumes.map(resume => ({ ...resume, acceptSeconds: resume.acceptSample / 16000, cursorSeconds: resume.cursorSample / 16000 })), load: load(), note }
  if (!options.keep) await recording.remove()
  return result
}
const strip = (result: Outcome) => { const { note: _note, ...rest } = result; return rest }
const complete = (result: Outcome) => result.transcript.missing.length === 0 && result.transcript.duplicated.length === 0 && result.transcript.wrongChannel.length === 0
const lineFacts = (result: Outcome) => ({ lines: result.transcript.linesCaptured, missing: result.transcript.missing, duplicated: result.transcript.duplicated, acrossCut: result.transcript.acrossCut })
const onlyLive = (result: Outcome) => result.failedParts === 0 && result.parts.length === 1 && /ready\/live$/.test(result.parts[0]!)
const stableSpeakers = (result: Outcome) => Object.values(result.transcript.speakers).every(row => Object.keys(row).length === 1)
const longFixture = (fromSeconds: number, seconds: number, extra: Parameters<typeof loadFixture>[1] = {}) => loadFixture(join(fixtures!, "long"), { fromSeconds, seconds, ...extra })

// ---------------------------------------------------------------------------------------------- drops
async function drops() {
  const results: Record<string, unknown> = {}
  {
    const recording = new Recording(server, await longFixture(0, 150), { title: TITLE("drop of 8 s"), captureId: "qa-drop-8" })
    await recording.create(); await recording.connect()
    await recording.run([{ atSeconds: 40, run: r => r.drop(8) }])
    const result = await outcome(recording); results.drop8 = strip(result)
    check("8 s drop: the same session resumed and replayed the backlog", result.resumes.length === 1 && result.resumes[0]!.replay && result.stream.gaps.length === 0, { resumes: result.resumes.map(resume => `accept ${resume.acceptSeconds.toFixed(1)} s, replay ${resume.replay}`), gaps: result.stream.gaps.length })
    check("8 s drop: every line exactly once", complete(result) && result.transcript.acrossCut.every(line => line.found === 1), lineFacts(result))
    check("8 s drop: all parts covered live, speaker numbers unchanged", onlyLive(result) && stableSpeakers(result) && result.noteSpeakers.length === 3, { parts: result.parts, speakers: result.transcript.speakers, rows: result.noteSpeakers })
  }
  {
    const recording = new Recording(server, await longFixture(150, 150), { title: TITLE("drop of 45 s"), captureId: "qa-drop-45" })
    await recording.create(); await recording.connect()
    await recording.run([{ atSeconds: 40, run: r => r.drop(45) }])
    const result = await outcome(recording); results.drop45 = strip(result)
    const gaps = result.stream.gaps
    // Only the socket is down here: parts keep uploading, the batch path transcribes what the ended session had not,
    // and the client resumes from the end of what either path accounts for. No gap is needed.
    check("45 s socket-only drop: the client resumed from where the durable parts had got to", result.resumes.length === 1 && result.resumes[0]!.acceptSample > recording.cuts[0]! && result.stream.errors.length === 0, { resume: result.resumes.map(resume => `cut at ${(recording.cuts[0]! / 16000).toFixed(1)} s, accept ${resume.acceptSeconds.toFixed(1)} s, replay ${resume.replay}`), gaps: gaps.map(gap => `${gap.channel} ${gap.fromMs}-${gap.toMs}`) })
    check("45 s drop: the gap was filled from the durable parts, nothing duplicated or lost", complete(result) && result.failedParts === 0 && result.transcript.acrossCut.every(line => line.found <= 1), { ...lineFacts(result), parts: result.parts, origins: result.origins })
    check("45 s drop: batch transcription was used only for the parts that touch the gap", result.parts.some(part => /live$/.test(part)) && result.parts.some(part => /batch$|mixed$/.test(part)), result.parts)
  }
  {
    // A real network outage: uploads stop too, and the parts that fell due go up after the connection returns.
    const recording = new Recording(server, await longFixture(432, 150), { title: TITLE("network outage of 45 s"), captureId: "qa-outage-45", holdUploadsWhileDown: true })
    await recording.create(); await recording.connect()
    await recording.run([{ atSeconds: 40, run: r => r.drop(45) }])
    const result = await outcome(recording); results.outage45 = strip(result)
    const gaps = result.stream.gaps
    check("45 s outage: the backlog was skipped and a gap declared on both channels", result.resumes.length === 1 && !result.resumes[0]!.replay && gaps.length === 2 && gaps.every(gap => gap.reason === "resume_window_exceeded"), { resume: result.resumes.map(resume => `accept ${resume.acceptSeconds.toFixed(1)} s, went on at ${resume.cursorSeconds.toFixed(1)} s`), gaps: gaps.map(gap => `${gap.channel} ${gap.fromMs}-${gap.toMs}`) })
    check("45 s outage: the gap was filled from the durable parts, nothing duplicated or lost", complete(result) && result.failedParts === 0 && result.transcript.acrossCut.every(line => line.found <= 1), { ...lineFacts(result), parts: result.parts, origins: result.origins })
    check("45 s outage: voices after the reconnect get new numbers (documented behaviour)", result.noteSpeakers.length === 6, result.noteSpeakers)
  }
  {
    // The connection dies three seconds before the user presses Stop, and stays down.
    const recording = new Recording(server, await longFixture(300, 62), { title: TITLE("cut just before Stop"), captureId: "qa-drop-end" })
    await recording.create(); await recording.connect()
    await recording.run([{ atSeconds: 58.5, run: r => r.drop() }])
    const result = await outcome(recording, { finish: { mark: false, settleMs: 0 } }); results.cutBeforeStop = strip(result)
    check("cut 3 s before Stop: Stop succeeds with the socket down", result.end.stopMs !== null && result.note.recordingState === "stopped", { stopMs: result.end.stopMs })
    check("cut 3 s before Stop: the transcript is complete once the last part is in", complete(result) && result.failedParts === 0 && result.transcript.acrossCut.every(line => line.found <= 1), { ...lineFacts(result), parts: result.parts })
  }
  {
    // The connection has been down for 30 s when Stop arrives: the server already gave up the session.
    const recording = new Recording(server, await longFixture(362, 70), { title: TITLE("Stop while the socket is down"), captureId: "qa-drop-stop" })
    await recording.create(); await recording.connect()
    await recording.run([{ atSeconds: 35, run: r => r.drop() }])
    const result = await outcome(recording, { finish: { mark: false, settleMs: 0 } }); results.stopWhileDown = strip(result)
    check("Stop 35 s after the socket died: Stop succeeds", result.end.stopMs !== null && result.note.recordingState === "stopped", { stopMs: result.end.stopMs })
    check("Stop 35 s after the socket died: the rest came from the durable parts, nothing duplicated or lost", complete(result) && result.failedParts === 0 && result.origins.some(origin => /batch/.test(origin)) && result.transcript.acrossCut.every(line => line.found <= 1), { ...lineFacts(result), parts: result.parts, origins: result.origins })
  }
  report.results = results
}

// ---------------------------------------------------------------------------------------------- pause and resume
async function pause() {
  const results: Record<string, unknown> = {}
  {
    // Pause inside one session, longer than the recogniser's 15-second idle limit.
    const recording = new Recording(server, await longFixture(440, 75), { title: TITLE("pause of 25 s"), captureId: "qa-pause" })
    await recording.create(); await recording.connect()
    await recording.run([{ atSeconds: 31, blocking: true, run: r => r.pause(25) }])
    const result = await outcome(recording); results.markPause = strip(result)
    const states = recording.of("status").map(item => item.message.state)
    check("pause mark: the session reported paused, then live again", states.includes("paused") && states.lastIndexOf("live") > states.indexOf("paused"), [...new Set(states)])
    check("pause mark: the recogniser session survived 25 s without audio (same speaker numbers, no gap, no error)", result.stream.gaps.length === 0 && result.stream.errors.length === 0 && stableSpeakers(result) && result.noteSpeakers.length === 3, { speakers: result.transcript.speakers, rows: result.noteSpeakers, providerEnded: server.logs(startedAt).filter(line => /provider session ended/.test(line)).length })
    check("pause mark: every line exactly once, all parts live, revisions only grow", complete(result) && onlyLive(result) && result.stream.revisions.monotonic && result.transcript.acrossCut.every(line => line.found === 1), { ...lineFacts(result), parts: result.parts })
  }
  {
    // Stop, then Resume on the same note: a second capture, later on the note's timeline.
    const first = new Recording(server, await longFixture(520, 45), { title: TITLE("stop then resume"), captureId: "qa-resume-1" })
    await first.create(); await first.connect(); await first.run()
    const one = await outcome(first, { keep: true })
    await Bun.sleep(4000)
    const second = new Recording(server, await longFixture(565, 45), { title: TITLE("stop then resume"), captureId: "qa-resume-2", noteId: first.noteId, baseMs: 45_000, firstSequence: 2 })
    await second.create()
    const state = await second.connect()
    await second.run()
    const two = await outcome(second, { baseMs: 45_000, keep: true })
    const note = two.note, firstCheck = checkTranscript(note, first.fixture)
    await second.remove()
    results.stopResume = { first: strip(one), second: strip(two), firstAfterSecond: firstCheck }
    const revisions = [...first.of("final"), ...second.of("final")].map(item => item.message.transcriptRevision)
    check("stop then resume: live text continued on the same note", state === "ready" && second.of("final").length > 0 && second.readies[0]!.transcriptRevision === one.revision, { secondReadyRevision: second.readies[0]?.transcriptRevision, firstEndedAt: one.revision, finalsInSecond: second.of("final").length })
    check("stop then resume: revisions are monotonic across both captures", revisions.every((revision, index) => index === 0 || revision > revisions[index - 1]!) && note.transcriptRevision >= revisions.at(-1)!, { first: revisions[0], last: revisions.at(-1), stored: note.transcriptRevision })
    check("stop then resume: both captures' lines are there exactly once, the second after the first on the timeline", complete(two) && firstCheck.missing.length === 0 && firstCheck.duplicated.length === 0 && Math.abs(two.transcript.drift.firstFifthMedianMs ?? 999) < 400, { second: lineFacts(two), firstMissing: firstCheck.missing, firstDuplicated: firstCheck.duplicated, secondOffsetsMs: two.transcript.drift })
    check("stop then resume: all parts of both captures covered live", onlyLive(two), two.parts)
    report.speakerNumbering = { firstCapture: one.transcript.speakers, secondCapture: two.transcript.speakers, rows: two.noteSpeakers }
    check("stop then resume: the second capture's voices get new numbers (documented behaviour)", two.noteSpeakers.length === 6, { rows: two.noteSpeakers, second: two.transcript.speakers })
  }
  report.results = results
}

// ---------------------------------------------------------------------------------------------- concurrency
async function concurrency() {
  report.memoryBefore = server.memory()
  const recordings = await Promise.all([0, 1, 2].map(async index => new Recording(server, await longFixture(610 + index * 120, 120), { title: TITLE(`concurrent ${index + 1} of 3`), captureId: `qa-concurrent-${index}`, quiet: true })))
  await Promise.all(recordings.map(async recording => { await recording.create(); return recording.connect() }))
  check("three recordings (six recogniser streams) opened at once", recordings.every(recording => recording.ready), recordings.map(recording => recording.refusal ?? "ready"))
  const answers: unknown[] = []
  await Promise.all(recordings.map((recording, index) => recording.run([{ atSeconds: 60 + index * 5, run: async r => { const reply = await r.ask("Which item number was mentioned most recently?"); answers.push({ recording: index + 1, firstTextMs: reply.firstTextMs, answer: reply.answer.slice(0, 120), error: reply.error }) } }])))
  report.memoryDuring = server.memory()
  const results = await Promise.all(recordings.map(recording => outcome(recording)))
  report.memoryAfter = server.memory(); report.chat = answers; report.results = results.map(strip)
  results.forEach((result, index) => {
    check(`recording ${index + 1} of 3: every line exactly once, all parts live, speakers stable`, complete(result) && onlyLive(result) && stableSpeakers(result), { ...lineFacts(result), parts: result.parts })
    check(`recording ${index + 1} of 3: no gap, error or reconnect; interim median within 2.5 s`, result.stream.gaps.length === 0 && result.stream.errors.length === 0 && result.stream.connections === 1 && (result.stream.interimMs.median ?? 1e9) <= 2500, { interimMs: result.stream.interimMs, finalMs: result.stream.finalMs })
  })
  // No text from one recording in another: the three windows hold different item numbers.
  const leaked = results.flatMap((result, index) => results.flatMap((other, otherIndex) => index === otherIndex ? [] : other.note.transcriptSegments.filter(segment => recordings[index]!.fixture.lines.some(line => new RegExp(`\\bitem ${line.item}\\b`, "i").test(segment.text))).length))
  check("no recording's text appears in another", leaked.every(count => count === 0), leaked)

  // The session limit, on an in-process server with LIVE_MAX_SESSIONS = 2 and the built-in fake recogniser
  // (the deployment's limit is 20, and reaching it for real would need 40 provider streams).
  const { startLiveStack } = await import("./stack")
  const stack = await startLiveStack({ provider: "fake", maxSessions: 2 })
  try {
    const limited = new Server(stack.base, stack.token, null)
    const three = [0, 1, 2].map(index => new Recording(limited, silence(4), { title: TITLE(`limit ${index + 1}`), captureId: `limit-${index}`, upload: false, quiet: true }))
    for (const recording of three) await recording.create()
    const opened = [await three[0]!.connect(), await three[1]!.connect(), await three[2]!.connect(3000)]
    const body = await fetch(`${stack.base}/api/v1/notes/${three[2]!.noteId}/live`, { headers: { ...upgradeHeaders, Authorization: `Bearer ${stack.token}` } })
    const text = await body.text()
    check("over LIVE_MAX_SESSIONS: the next recording is refused before the upgrade with HTTP 429", opened[0] === "ready" && opened[1] === "ready" && opened[2] === 429 && body.status === 429, { opened, body: text })
    // What a refused client does: it keeps recording and uploads parts; the server takes them.
    const part = await limited.call(`/notes/${three[2]!.noteId}/audio/chunks/${crypto.randomUUID()}`, "PUT", { channel: "mic", sequence: 0, startMs: 0, audioBase64: Buffer.from((await import("./make-fixture")).wavFile(new Uint8Array(32000), 16000)).toString("base64") })
    check("over LIVE_MAX_SESSIONS: the refused recording can still upload durable parts", part.status === 200, `HTTP ${part.status}`)
    await three[0]!.finish({ settleMs: 0 })
    check("over LIVE_MAX_SESSIONS: a slot freed by an ended recording is usable again", await three[2]!.connect() === "ready", three[2]!.refusal ?? "ready")
  } finally { await stack.stop() }
}

// ---------------------------------------------------------------------------------------------- pace: a client that does not read, a fast one, a slow one
/** A WebSocket client on a raw TCP socket, so that it can stop reading: the kernel buffers fill and the server meets back-pressure. */
async function unreadingClient(noteId: string, fixture: Awaited<ReturnType<typeof loadFixture>>, seconds: number) {
  const net = await import("node:net"), url = new URL(base)
  const socket = net.connect({ host: url.hostname, port: Number(url.port || 80) })
  const frame = (opcode: number, payload: Uint8Array) => {
    const length = payload.byteLength, header = length < 126 ? Buffer.from([0x80 | opcode, 0x80 | length]) : length < 65536 ? Buffer.from([0x80 | opcode, 0x80 | 126, length >> 8, length & 255]) : Buffer.concat([Buffer.from([0x80 | opcode, 0x80 | 127]), (() => { const b = Buffer.alloc(8); b.writeBigUInt64BE(BigInt(length)); return b })()])
    const mask = crypto.getRandomValues(new Uint8Array(4)), body = Buffer.from(payload)
    for (let i = 0; i < body.length; i++) body[i] = body[i]! ^ mask[i % 4]!
    return Buffer.concat([header, Buffer.from(mask), body])
  }
  let received = "", closed = false, bytesRead = 0
  socket.on("data", chunk => { bytesRead += chunk.length; received += chunk.toString("latin1") })
  socket.on("close", () => { closed = true })
  socket.on("error", () => { closed = true })
  await new Promise<void>(resolve => socket.once("connect", () => resolve()))
  socket.write(`GET /api/v1/notes/${noteId}/live HTTP/1.1\r\nHost: ${url.host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nAuthorization: Bearer ${token}\r\n\r\n`)
  for (const deadline = performance.now() + 8000; !received.includes("\r\n\r\n") && performance.now() < deadline;) await Bun.sleep(10)
  if (!received.startsWith("HTTP/1.1 101")) throw new Error(`upgrade refused: ${received.slice(0, 60)}`)
  socket.write(frame(1, new TextEncoder().encode(JSON.stringify({ type: "hello", protocol: 1, captureId: "qa-unreading", language: "en", streams: fixture.channels.map(channel => ({ channel, sampleRate: fixture.rate, encoding: "pcm_s16le", baseMs: 0, resumeFromSample: 0 })) }))))
  for (const deadline = performance.now() + 15_000; !received.includes('"type":"ready"') && performance.now() < deadline;) await Bun.sleep(10)
  if (!received.includes('"type":"ready"')) throw new Error("no ready")
  // From here on the client never reads again.
  socket.pause()
  const readBeforePause = bytesRead, step = fixture.rate / 10, started = performance.now(), end = Math.min(fixture.totalSamples, seconds * fixture.rate)
  for (let cursor = 0; cursor < end && !closed; cursor += step) {
    for (const channel of fixture.channels) socket.write(frame(2, encodeLiveAudioFrame({ channel, firstSample: cursor, pcm: fixture.pcm[channel].subarray(cursor * 2, (cursor + step) * 2) })))
    const wait = started + (cursor + step) * 1000 / fixture.rate - performance.now()
    if (wait > 0) await Bun.sleep(wait)
  }
  return { socket, frame, readBeforePause, closedEarly: closed, sentSamples: end, unread: () => { socket.resume(); return new Promise<{ bytes: number; closeCode: number | null; text: string }>(resolve => setTimeout(() => { const at = received.lastIndexOf("\x88"); resolve({ bytes: bytesRead - readBeforePause, closeCode: at >= 0 && received.length >= at + 4 ? (received.charCodeAt(at + 2) << 8) | received.charCodeAt(at + 3) : null, text: received }) }, 2500)) } }
}
async function pace() {
  const results: Record<string, unknown> = {}
  {
    const fixture = await longFixture(0, 120), recording = new Recording(server, fixture, { title: TITLE("client that does not read"), quiet: true })
    await recording.create()
    report.memoryBefore = server.memory()
    const client = await unreadingClient(recording.noteId, fixture, 120)
    recording.captured = client.sentSamples; recording.cursor = client.sentSamples
    client.socket.write(client.frame(1, new TextEncoder().encode(JSON.stringify({ type: "mark", kind: "end", atSample: Object.fromEntries(fixture.channels.map(channel => [channel, client.sentSamples])) }))))
    await Bun.sleep(3000)
    const stop = await server.call(`/notes/${recording.noteId}/recording`, "POST", { command: "stop" })
    const late = await client.unread()
    client.socket.destroy()
    const note = await recording.stored(), transcript = checkTranscript(note, fixture)
    report.memoryAfter = server.memory()
    results.unreading = { closedEarly: client.closedEarly, bytesHeldBackByNotReading: late.bytes, closeCodeSeenWhenReadingAgain: late.closeCode, finalsInBacklog: late.text.split('"type":"final"').length - 1, transcript, revision: note.transcriptRevision }
    check("client that never reads for 120 s: the server kept transcribing and storing, and was not harmed", stop.status === 200 && !client.closedEarly && transcript.missing.length === 0 && transcript.duplicated.length === 0, { stop: stop.status, lines: transcript.linesCaptured, missing: transcript.missing, duplicated: transcript.duplicated })
    check("client that never reads: everything it missed was still waiting for it, ending with a clean close", late.bytes > 10_000 && late.closeCode === 1000, { bytes: late.bytes, closeCode: late.closeCode, finals: (results.unreading as { finalsInBacklog: number }).finalsInBacklog })
    await recording.remove()
  }
  {
    const recording = new Recording(server, await longFixture(120, 150), { title: TITLE("three times real time"), captureId: "qa-fast", captureSpeed: 3 })
    await recording.create(); await recording.connect(); await recording.run()
    const result = await outcome(recording); results.fast = strip(result)
    const states = [...new Set(recording.of("status").map(item => item.message.state))]
    check("audio at three times real time: every line exactly once, all parts live", complete(result) && onlyLive(result) && stableSpeakers(result), { ...lineFacts(result), parts: result.parts })
    check("audio at three times real time: reported as catching_up, no gap or error", states.includes("catching_up") && result.stream.gaps.length === 0 && result.stream.errors.length === 0, { states, maxLagMs: result.stream.maxLagMs })
  }
  {
    // A stream that manages half of real time and is allowed to finish: it sends the 90 s of capture in 180 s,
    // while the durable parts arrive on time and the worker stops waiting for the stream after 40 s.
    const recording = new Recording(server, await longFixture(270, 90), { title: TITLE("half of real time, finishing late"), captureId: "qa-slow", sendSpeed: 0.5 })
    await recording.create(); await recording.connect(); await recording.run([], 90)
    const result = await outcome(recording, { finish: { settleMs: 0 } }); results.slowFinishing = strip(result)
    check("audio at half of real time, stream allowed to finish: every line exactly once although both paths transcribed", complete(result) && result.failedParts === 0 && result.origins.length === 2, { ...lineFacts(result), parts: result.parts, origins: result.origins, ledger: result.ledger })
  }
  {
    // The same, but the user stops when the capture ends: the stream has sent about half of it.
    const recording = new Recording(server, await longFixture(360, 90), { title: TITLE("half of real time, stopped at the end"), captureId: "qa-slow-stop", sendSpeed: 0.5 })
    await recording.create(); await recording.connect(); await recording.run([], 90, { drain: false })
    const sentSeconds = recording.cursor / 16000
    const result = await outcome(recording, { finish: { settleMs: 0 }, cuts: [recording.cursor] }); results.slowStopped = { ...strip(result), sentSeconds }
    check("audio at half of real time, stopped at capture end: what the stream did not reach came from the durable parts", complete(result) && result.failedParts === 0 && result.transcript.acrossCut.every(line => line.found <= 1) && sentSeconds < 60, { sentSeconds, ...lineFacts(result), parts: result.parts, origins: result.origins })
  }
  report.results = results
}

// ---------------------------------------------------------------------------------------------- admin control
async function admin() {
  const original = (await server.ok("/admin/settings")).liveTranscriptionModel
  report.settingBefore = original
  const set = async (model: unknown) => (await server.ok("/admin/settings", "PATCH", { liveTranscriptionModel: model })).liveTranscriptionModel
  const results: Record<string, unknown> = {}
  try {
    // 1. Off while a session is open.
    const open = new Recording(server, await longFixture(0, 70), { title: TITLE("admin: session open while live is turned off"), captureId: "qa-admin-open" })
    await open.create(); await open.connect()
    const other = new Recording(server, await longFixture(70, 60), { title: TITLE("admin: recording started while live is off"), captureId: "qa-admin-off", captureSpeed: 20, quiet: true })
    await other.create()
    let refused: { status: number; text: string } | undefined, reported: unknown
    await open.run([{ atSeconds: 20, blocking: true, run: async () => {
      reported = await set(null)
      refused = await fetch(`${base}/api/v1/notes/${other.noteId}/live`, { headers: { ...upgradeHeaders, Authorization: `Bearer ${token}` } }).then(async response => ({ status: response.status, text: await response.text() }))
    } }])
    const openResult = await outcome(open); results.openSession = strip(openResult)
    check("turned off: the settings API reports null", reported === null, JSON.stringify(reported))
    check("turned off: a new session is refused with 503 before the upgrade", refused?.status === 503 && refused.text === JSON.stringify({ error: "unavailable", message: "Live transcription is not configured" }), refused)
    const finalsAfter = open.of("final").filter(item => item.at > 26_000).length
    check("turned off: the session that was already open carried on to the end (documented behaviour)", finalsAfter > 0 && complete(openResult) && onlyLive(openResult) && openResult.stream.errors.length === 0, { finalsAfterTheSwitch: finalsAfter, ...lineFacts(openResult), parts: openResult.parts })
    // 2. Parts only, with live off: the batch path alone.
    check("turned off: the client's own connect is refused the same way", await other.connect(4000) === 503, other.refusal)
    other.offline(); await other.run()
    const batch = await outcome(other, { finish: { mark: false, settleMs: 0 } }); results.partsOnly = strip(batch)
    check("turned off: durable parts alone still give the full transcript through the batch path", complete(batch) && batch.failedParts === 0 && batch.parts.length === 1 && /ready\/batch$/.test(batch.parts[0]!) && batch.ledger.length === 0, { ...lineFacts(batch), parts: batch.parts, origins: batch.origins, ledgerRows: batch.ledger.length })
    // 3. The other streaming model.
    check("switched to scribe_v2_realtime: accepted by the settings API", JSON.stringify(await set({ provider: "elevenlabs", model: "scribe_v2_realtime" })) === JSON.stringify({ provider: "elevenlabs", model: "scribe_v2_realtime" }))
    const v2 = new Recording(server, await longFixture(130, 60), { title: TITLE("admin: scribe_v2_realtime"), captureId: "qa-admin-v2" })
    await v2.create(); await v2.connect(); await v2.run()
    const v2Result = await outcome(v2); results.v2 = strip(v2Result)
    check("switched to scribe_v2_realtime: the session says it uses that model", v2.readies[0]?.model === "scribe_v2_realtime", v2.readies[0]?.model)
    check("switched to scribe_v2_realtime: every line exactly once, all parts live", complete(v2Result) && onlyLive(v2Result), { ...lineFacts(v2Result), parts: v2Result.parts, interimMs: v2Result.stream.interimMs, finalMs: v2Result.stream.finalMs })
    report.v2Speakers = { transcript: v2Result.transcript.speakers, rows: v2Result.noteSpeakers }
    // A model that is not a streaming model is refused and changes nothing.
    const wrong = await server.call("/admin/settings", "PATCH", { liveTranscriptionModel: { provider: "elevenlabs", model: "scribe_v3" } })
    check("a batch model is refused as the live model", wrong.status === 400 && (await server.ok("/admin/settings")).liveTranscriptionModel.model === "scribe_v2_realtime", `HTTP ${wrong.status} ${wrong.text.slice(0, 100)}`)
  } finally {
    const restored = await set(original)
    report.settingAfter = restored
    check("the setting is back exactly as it was found", JSON.stringify(restored) === JSON.stringify(original) && JSON.stringify((await server.ok("/admin/settings")).liveTranscriptionModel) === JSON.stringify(original), JSON.stringify(restored))
  }
  const after = new Recording(server, silence(3, ["mic"]), { title: TITLE("admin: after restoring"), upload: false, quiet: true })
  await after.create()
  check("after restoring: a new session uses scribe_v3_live again", await after.connect() === "ready" && after.readies[0]?.model === "scribe_v3_live", after.readies[0]?.model ?? after.refusal)
  await after.finish({ settleMs: 0 }); await after.remove()
  report.results = results
}

// ---------------------------------------------------------------------------------------------- unusual audio
async function audio() {
  const results: Record<string, unknown> = {}
  const text = (result: Outcome) => [...result.note.transcriptSegments].sort((a, b) => a.startMs - b.startMs).map(segment => `${segment.channel} ${segment.speakerLabel} ${(segment.startMs / 1000).toFixed(1)}s: ${segment.text}`)
  {
    const recording = new Recording(server, silence(50), { title: TITLE("silence only"), captureId: "qa-silence" })
    await recording.create(); await recording.connect(); await recording.run()
    const result = await outcome(recording); results.silence = strip(result)
    check("silence only: nothing transcribed, no error, parts resolved without a batch call", result.note.transcriptSegments.length === 0 && result.stream.errors.length === 0 && onlyLive(result), { segments: result.note.transcriptSegments.length, parts: result.parts, ledger: result.ledger, interims: result.stream.messages.interim ?? 0 })
  }
  {
    const recording = new Recording(server, await longFixture(0, 75, { channels: ["mic"] }), { title: TITLE("microphone only"), captureId: "qa-mic-only" })
    await recording.create(); await recording.connect(); await recording.run()
    const result = await outcome(recording); results.microphoneOnly = { ...strip(result), text: text(result) }
    check("microphone only (as on iPhone): one stream, every line exactly once, parts live", recording.readies[0]!.streams.length === 1 && complete(result) && onlyLive(result) && result.note.transcriptSegments.every(segment => segment.channel === "mic"), { ...lineFacts(result), parts: result.parts, speakers: result.noteSpeakers })
  }
  {
    // About 30 dB down: speech whose peaks sit near the level the server treats as a pause.
    const recording = new Recording(server, await longFixture(75, 90, { gain: 0.03 }), { title: TITLE("very quiet audio"), captureId: "qa-quiet" })
    await recording.create(); await recording.connect()
    await recording.run([{ atSeconds: 40, run: async r => { const reply = await r.ask("Which item number was mentioned most recently?"); results.quietChat = { answer: reply.answer.slice(0, 160), error: reply.error } } }])
    const result = await outcome(recording); results.quiet = { ...strip(result), text: text(result) }
    const broken = result.note.transcriptSegments.filter(segment => /\w-(\s|$)/.test(segment.text)).map(segment => segment.text.slice(-40))
    check("very quiet audio (-30 dB): every line exactly once, no word cut by a flush", complete(result) && broken.length === 0 && result.failedParts === 0, { ...lineFacts(result), parts: result.parts, cutWords: broken, interimMs: result.stream.interimMs, finalMs: result.stream.finalMs })
  }
  for (const [label, language] of [["no language given", null], ["hello says en", "en"]] as const) {
    const fixture = await loadFixture(join(fixtures!, "spanish"))
    const recording = new Recording(server, fixture, { title: TITLE(`Spanish audio, ${label}`), captureId: `qa-es-${language ?? "auto"}`, language })
    await recording.create(); await recording.connect(); await recording.run()
    const result = await outcome(recording); const said = text(result).join(" ")
    results[`spanish ${label}`] = { ...strip(result), text: text(result) }
    const spanish = ["presupuesto", "viernes", "ingenieros", "Madrid", "martes"].filter(word => new RegExp(word, "i").test(said))
    check(`Spanish audio, ${label}: transcribed in Spanish`, spanish.length >= 4 && result.failedParts === 0, { wordsRecognised: spanish, segments: result.note.transcriptSegments.length, parts: result.parts, sample: said.slice(0, 160) })
  }
  report.results = results
}


// ---------------------------------------------------------------------------------------------- a muted microphone for a long time
async function muted() {
  // The common real case: the call talks for minutes while the note owner's microphone is silent.
  const minutes = Number(value("minutes") ?? 8), fixture = await longFixture(0, minutes * 60, { channels: ["system"] })
  fixture.channels = ["mic", "system"]; fixture.pcm.mic = new Uint8Array(fixture.totalSamples * 2)
  const recording = new Recording(server, fixture, { title: TITLE(`microphone silent for ${minutes} min`), captureId: "qa-muted", quiet: true })
  await recording.create(); await recording.connect(); await recording.run()
  const result = await outcome(recording)
  const ended = server.logs(startedAt).filter(line => /provider session ended/.test(line))
  report.results = { muted: strip(result) }; report.providerSessionsEnded = ended
  check(`microphone silent for ${minutes} min: the call's lines are all there exactly once, nothing on the microphone`, complete(result) && result.note.transcriptSegments.every(segment => segment.channel === "system"), { ...lineFacts(result), micSegments: result.note.transcriptSegments.filter(segment => segment.channel === "mic").length })
  check(`microphone silent for ${minutes} min: its recogniser session was kept or reopened without a gap, and its parts cost no batch call`, result.stream.gaps.length === 0 && result.stream.errors.length === 0 && onlyLive(result), { parts: result.parts, ledger: result.ledger, gaps: result.stream.gaps, errors: result.stream.errors.length, providerSessionsEnded: ended })
}

// ---------------------------------------------------------------------------------------------- chat question battery
interface Question { kind: string; question: string; history?: Array<{ role: "user" | "assistant"; content: string }>
  /** Every pattern must match the answer. */ must?: RegExp[]
  /** At least `atLeast` (default 1) of these must match. */ any?: RegExp[]; atLeast?: number
  /** None may match: a fact not said yet, or the wrong one. */ forbid?: RegExp[]
  /** At least one cited passage should match this (the fact the answer rests on). */ support?: RegExp
  /** The newest items at the moment of asking; set for "what was just said". */ newest?: number[] }
const ABSENT = /\b(?:not|hasn.t|haven.t|has not|have not|wasn.t|isn.t|no (?:mention|budget|figure|decision|information|record|discussion)|nothing|yet|doesn.t|didn.t|don.t|unclear|unknown)\b/i
/** A phrase as a pattern, allowing the spellings the recogniser was seen to use for the same word. */
const words = (text: string) => new RegExp(text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&").replace(/\s+/g, "[\\s-]+").replace(/Noemi/g, "N[ao]e?o?mi").replace(/Farid/g, "Far(?:i|ee)d").replace(/localisation/g, "locali[sz]ation").replace(/Callum/g, "Call?um"), "i")
const amount = (k: number) => new RegExp(`\\b${k}(?:,000|\\s*(?:k\\b|thousand|grand))|\\$\\s*${k}\\b|\\b${numberWords(k).replace(/ /g, "[ -]")} thousand`, "i")
const count = (k: number) => new RegExp(`\\b(?:${k}|${numberWords(k).replace(/ /g, "[ -]")})\\b`, "i")
async function chat() {
  const { chatFacts } = await import("./make-qa-fixtures")
  const fixture = await loadFixture(join(fixtures!, "chat")), facts = chatFacts(), fact = (item: number) => facts[item - 1]!
  const recording = new Recording(server, fixture, { title: TITLE("chat question battery"), captureId: "qa-chat", quiet: true })
  const rows: Array<Record<string, unknown>> = []
  // Items whose line has been fully spoken, and how long ago each ended.
  const ended = () => [...recording.lineEnd.keys()].map(index => fixture.lines[index]!.item).sort((a, b) => a - b)
  const label = (item: number) => fact(item).channel === "mic" ? "Me" : fact(item).voice === "Daniel" ? "Them 1" : "Them 2"
  const pick = (shape: ChatShape, back = 2) => { const done = ended(); return [...done].reverse().find(item => fact(item).shape === shape && item <= (done.at(-1) ?? 0) - back) }
  type ChatShape = ReturnType<typeof chatFacts>[number]["shape"]
  const builders: Record<string, () => Question | null> = {
    "just said": () => { const last = ended().at(-1) ?? 0, items = [last, last + 1].filter(item => item >= 1 && item <= facts.length); return { kind: "what was just said", question: "What was just said? One sentence.", any: items.map(item => words(fact(item).topic)), newest: items, support: undefined } },
    "early day and owner": () => ({ kind: "early fact asked late", question: `Which day did the ${fact(1).topic} move to, and who owns the follow up?`, must: [words(fact(1).day), words(fact(1).person)], support: words(fact(1).topic) }),
    "early finisher": () => ({ kind: "early fact asked late", question: `Who will finish the ${fact(3).topic}?`, must: [words(fact(3).person)], support: words(fact(3).topic) }),
    "budget": () => { const item = pick(3); return item ? { kind: "number or date", question: `What is the budget for the ${fact(item).topic}?`, must: [amount(fact(item).k)], support: words(fact(item).topic) } : null },
    "hours": () => { const item = pick(1); return item ? { kind: "number or date", question: `How many more hours does the ${fact(item).topic} need, and when is it revisited?`, must: [count(fact(item).k), words(fact(item).day)], support: words(fact(item).topic) } : null },
    "open questions": () => { const item = pick(4); return item ? { kind: "number or date", question: `How many open questions are there about the ${fact(item).topic}?`, must: [count(fact(item).k)], support: words(fact(item).topic) } : null },
    "who said, voice 1": () => { const item = [...ended()].reverse().find(candidate => fact(candidate).voice === "Daniel" && typeof fact(candidate).shape === "number" && candidate < (ended().at(-1) ?? 0)); return item ? { kind: "who said it", question: `Who talked about the ${fact(item).topic}? Answer with the speaker label exactly as the transcript shows it.`, must: [/\bThem 1\b/], forbid: [/\bThem 2\b/, /\bMe\b/], support: words(fact(item).topic) } : null },
    "who said, voice 2": () => { const item = [...ended()].reverse().find(candidate => fact(candidate).voice === "Samantha" && typeof fact(candidate).shape === "number" && candidate < (ended().at(-1) ?? 0)); return item ? { kind: "who said it", question: `Who talked about the ${fact(item).topic}? Answer with the speaker label exactly as the transcript shows it.`, must: [/\bThem 2\b/], forbid: [/\bThem 1\b/, /\bMe\b/], support: words(fact(item).topic) } : null },
    "summary early": () => { const done = ended(), last = done.at(-1) ?? 0; return { kind: "summarise so far", question: "Summarise the meeting so far.", any: done.map(item => words(fact(item).topic)), atLeast: Math.min(3, done.length), forbid: facts.filter(candidate => candidate.item > last + 1).map(candidate => words(candidate.topic)) } },
    "summary late": () => { const done = ended(), last = done.at(-1) ?? 0; return { kind: "summarise so far", question: "Summarise the meeting so far in at most eight bullets.", any: done.map(item => words(fact(item).topic)), atLeast: 5, forbid: facts.filter(candidate => candidate.item > last + 1).map(candidate => words(candidate.topic)) } },
    "missed": () => { const now = recording.now(), recent = [...recording.lineEnd.entries()].filter(([, at]) => now - at <= 75_000).map(([index]) => fixture.lines[index]!.item), last = ended().at(-1) ?? 0; return { kind: "what did I miss", question: "What did I miss in the last minute?", any: recent.map(item => words(fact(item).topic)), atLeast: Math.min(2, recent.length), forbid: facts.filter(candidate => candidate.item > last + 1).map(candidate => words(candidate.topic)) } },
    "action items": () => { const done = ended().filter(item => fact(item).shape === 0 || fact(item).shape === 2 || fact(item).shape === "report"); return { kind: "action items so far", question: "List the action items so far, with owners.", any: done.map(item => words(fact(item).shape === "report" ? "Wexcombe|Wex|report" : fact(item).person.split(" ")[0]!)).map((pattern, index) => fact(done[index]!).shape === "report" ? /report/i : pattern), atLeast: Math.min(3, done.length) } },
    "not said yet": () => { const last = ended().at(-1) ?? 0, later = facts.find(candidate => candidate.shape === 3 && candidate.item > last + 3); return later ? { kind: "not said yet", question: `What is the budget for the ${later.topic}?`, must: [ABSENT], forbid: [amount(later.k)] } : null },
    "never said": () => ({ kind: "not said yet", question: "What did they decide about the Lisbon office lease?", must: [ABSENT], forbid: [/\bdecided to\b/i] }),
    "confirmer": () => { const item = pick(3, 1); return item ? { kind: "follow-up, first turn", question: `Who will confirm the budget for the ${fact(item).topic}?`, must: [words(fact(item).person)], support: words(fact(item).topic) } : null },
    "invoice name": () => ({ kind: "misspelled name", question: "How much was the Skerryvore invoice that Marguerite Oyelaran approved?", must: [amount(11)], support: /invoice/i }),
    "report name": () => ({ kind: "misspelled name", question: "Who is the Wexcombe report going to, and by when?", must: [/Thursday/i, /Quil+\s?-?feather|Quill|Partners/i], support: /report/i }),
  }
  const ask = async (name: string, spec: Question | null, extra: Record<string, unknown> = {}, onFirstText?: () => void) => {
    if (!spec) { rows.push({ name, skipped: "no suitable line had been spoken yet" }); return null }
    const capturedMs = recording.captured * 1000 / fixture.rate
    const newestTextMs = Math.max(0, ...recording.received.map(({ message }) => message.type === "interim" ? message.endMs : message.type === "final" ? message.segment.endMs ?? message.segment.startMs : 0))
    const atLoad = load(), reply = await recording.ask(spec.question, spec.history ?? [], onFirstText)
    // Every citation is opened through the API: the segment must exist, start where cited, and contain the quote.
    const note = await recording.stored(), cited = reply.citations.filter(citation => citation.source === "transcript")
    const opened = cited.map(citation => { const segment = note.transcriptSegments.find(candidate => candidate.id === citation.segmentId); return { real: !!segment && segment.isFinal && segment.startMs === citation.startMs && segment.text.includes(citation.quote), text: segment?.text ?? "" } })
    const markers = [...new Set([...reply.answer.matchAll(/\[\[(S\d+)\]\]/g)].map(match => match[1]))]
    const plain = reply.answer.replace(/\[\[S\d+\]\]/g, "").replace(/\*\*/g, "")
    const anyHits = (spec.any ?? []).filter(pattern => pattern.test(plain)).length
    const failures = [
      ...(reply.status !== 200 || reply.error ? [`request failed: ${reply.error ?? reply.status}`] : []),
      ...(spec.must ?? []).filter(pattern => !pattern.test(plain)).map(pattern => `missing ${pattern.source.slice(0, 50)}`),
      ...(spec.any && anyHits < (spec.atLeast ?? 1) ? [`only ${anyHits} of the ${spec.atLeast ?? 1} expected topics`] : []),
      ...(spec.forbid ?? []).filter(pattern => pattern.test(plain)).map(pattern => `states ${pattern.source.slice(0, 50)}, which had not been said`),
    ]
    const unsupported = [
      ...(opened.some(item => !item.real) ? [`${opened.filter(item => !item.real).length} citation(s) do not resolve to a stored passage`] : []),
      ...(markers.length !== reply.citations.length ? [`${markers.length} markers in the text, ${reply.citations.length} citations delivered`] : []),
      ...(spec.support && cited.length > 0 && !opened.some(item => spec.support!.test(item.text)) ? ["no cited passage states the fact"] : []),
    ]
    const usedNewest = spec.newest ? spec.newest.filter(item => words(fact(item).topic).test(plain)) : undefined
    const row = { name, kind: spec.kind, phase: "during", atSeconds: Math.round(capturedMs / 1000), question: spec.question, load: atLoad, firstByteMs: reply.firstByteMs, firstTextMs: reply.firstTextMs, totalMs: reply.totalMs,
      transcriptAgeMs: Math.round(capturedMs - newestTextMs), correct: failures.length === 0, failures, citations: reply.citations.length, citationsReal: opened.filter(item => item.real).length, unsupported, ...(usedNewest ? { newestItems: spec.newest, usedItems: usedNewest } : {}), answer: reply.answer.slice(0, 400), ...extra }
    rows.push(row)
    console.log(`  ${String(row.atSeconds).padStart(4)} s  ${failures.length ? "WRONG" : "ok   "} ${String(reply.firstTextMs).padStart(5)} ms first text  ${name}${failures.length ? `: ${failures.join("; ")}` : ""}${unsupported.length ? `  [${unsupported.join("; ")}]` : ""}`)
    return reply
  }
  const one = (atSeconds: number, name: string) => ({ atSeconds, run: async () => { await ask(name, builders[name]!()) } })
  const followUp = (atSeconds: number) => ({ atSeconds, run: async () => {
    const first = builders.confirmer!(), reply = await ask("confirmer", first)
    if (!first || !reply) return
    const item = pick(3, 1)!
    await ask("follow-up: and on which day", { kind: "follow-up that needs the previous answer", question: "And on which day?", history: [{ role: "user", content: first.question }, { role: "assistant", content: reply.answer || "(no answer)" }], must: [words(fact(item).day)], support: words(fact(item).topic) })
  } })
  await recording.create()
  check("the socket opened", await recording.connect() === "ready", recording.readies[0]?.model ?? recording.refusal)
  await recording.run([
    one(20, "just said"), one(35, "never said"), one(50, "summary early"), one(62, "budget"), one(75, "who said, voice 1"), followUp(90), one(105, "just said"), one(115, "missed"),
    one(126, "hours"), one(138, "action items"),
    // Two questions at the same moment on the same note.
    { atSeconds: 150, run: async () => { await Promise.all([ask("invoice name", builders["invoice name"]!(), { concurrency: "sent together with the next" }), ask("report name", builders["report name"]!(), { concurrency: "sent together with the previous" })]) } },
    one(165, "not said yet"),
    // A question sent while the previous answer is still streaming.
    { atSeconds: 180, run: async () => { let second: Promise<unknown> | undefined; await ask("just said", builders["just said"]!(), { concurrency: "another question was sent while this streamed" }, () => { second = ask("open questions", builders["open questions"]!(), { concurrency: "sent while the previous answer was streaming" }) }); await second } },
    followUp(196), one(212, "budget"), one(224, "who said, voice 2"), one(236, "early day and owner"), one(248, "summary late"), one(260, "early finisher"), one(272, "missed"), one(284, "just said"), one(292, "action items"),
  ])
  const end = await recording.finish({ settleMs: 3000 })
  await recording.partsSettled()
  // The same questions on the finished note, right after Stop.
  for (const name of ["early day and owner", "early finisher", "budget", "hours", "open questions", "who said, voice 1", "who said, voice 2", "summary late", "action items", "invoice name", "report name", "never said"]) {
    const before = rows.length
    await ask(name, builders[name]!())
    if (rows.length > before) rows.at(-1)!.phase = "after Stop"
  }
  const note = await recording.stored(), facts2 = partFacts(server, recording.noteId), transcript = checkTranscript(note, fixture)
  if (stack) {
    // What the language provider reported for each request, matched to its question in the order asked.
    const pool = [...stack.languageUsage]
    for (const row of rows) { const index = pool.findIndex(usage => usage.question === String(row.question).slice(0, 300)); if (index < 0) continue; const [usage] = pool.splice(index, 1); Object.assign(row, { model: usage!.model, promptTokens: usage!.promptTokens, cachedTokens: usage!.cachedTokens, completionTokens: usage!.completionTokens, costUsd: usage!.costUsd, providerFirstTokenMs: usage!.firstTokenMs, providerTotalMs: usage!.totalMs, promptCharacters: usage!.requestCharacters }) }
  }
  const during = rows.filter(row => row.phase === "during"), after = rows.filter(row => row.phase === "after Stop")
  const timing = (list: typeof rows, key: string) => { const values = list.map(row => row[key]).filter((value): value is number => typeof value === "number").sort((a, b) => a - b), at = (p: number) => values.length ? values[Math.min(values.length - 1, Math.max(0, Math.ceil(p * values.length) - 1))]! : null; return { n: values.length, median: at(0.5), p90: at(0.9), worst: at(1) } }
  report.questions = rows; report.end = end; report.transcript = transcript; report.parts = facts2.parts
  report.timing = Object.fromEntries([["during", during], ["after Stop", after]].map(([name, list]) => [name as string, { firstByteMs: timing(list as typeof rows, "firstByteMs"), firstTextMs: timing(list as typeof rows, "firstTextMs"), totalMs: timing(list as typeof rows, "totalMs"), transcriptAgeMs: timing(list as typeof rows, "transcriptAgeMs"),
    ...(stack ? { promptTokens: timing(list as typeof rows, "promptTokens"), cachedTokens: timing(list as typeof rows, "cachedTokens"), completionTokens: timing(list as typeof rows, "completionTokens"), providerFirstTokenMs: timing(list as typeof rows, "providerFirstTokenMs") } : {}) }]))
  report.loads = during.map(row => row.load)
  const asked = during.filter(row => !row.skipped), wrong = asked.filter(row => row.correct === false), slow = asked.filter(row => typeof row.firstTextMs === "number" && row.firstTextMs > 2000), unsupported = asked.filter(row => (row.unsupported as string[] | undefined)?.length)
  const justSaid = asked.filter(row => row.newestItems)
  check("at least 20 questions were asked during the meeting", asked.length >= 20, `${asked.length} asked, ${during.length - asked.length} skipped`)
  check("every answer during the meeting is correct against the script", wrong.length === 0, wrong.map(row => `${row.name}@${row.atSeconds}s: ${(row.failures as string[]).join("; ")}`))
  check("every citation resolves to a stored passage that contains its quote", asked.every(row => row.citations === row.citationsReal), asked.filter(row => row.citations !== row.citationsReal).map(row => `${row.name}@${row.atSeconds}s`))
  check("cited passages state the fact the answer gives", unsupported.length === 0, unsupported.map(row => `${row.name}@${row.atSeconds}s: ${(row.unsupported as string[]).join("; ")}`))
  check("first visible text within 2 s for every question", slow.length === 0, slow.map(row => `${row.name}@${row.atSeconds}s: ${row.firstTextMs} ms at load ${row.load}`))
  check("\"what was just said\" answers from the last one or two lines", justSaid.every(row => (row.usedItems as number[]).length > 0), justSaid.map(row => `asked at ${row.atSeconds}s: newest ${(row.newestItems as number[]).join("/")}, used ${(row.usedItems as number[]).join("/") || "neither"}`))
  check("the transcript text the server had was under 3 s old at every question", (timing(asked, "transcriptAgeMs").worst ?? 1e9) <= 3000, timing(asked, "transcriptAgeMs"))
  check("the same questions after Stop are answered correctly", after.filter(row => row.correct === false).length === 0, after.filter(row => row.correct === false).map(row => `${row.name}: ${(row.failures as string[]).join("; ")}`))
  check("the transcript is complete and all parts were covered live", transcript.missing.length === 0 && transcript.duplicated.length === 0 && (!server.docker || facts2.parts.length === 1 && /ready\/live$/.test(facts2.parts[0]!)), { missing: transcript.missing, duplicated: transcript.duplicated, parts: facts2.parts })
  console.log(`timing during the meeting: ${JSON.stringify((report.timing as Record<string, unknown>).during)}`)
  console.log(`timing after Stop:         ${JSON.stringify((report.timing as Record<string, unknown>)["after Stop"])}`)
  report.deleted = await recording.remove()
}

// ---------------------------------------------------------------------------------------------- generation on the long meeting's note
async function generate() {
  const { readFile } = await import("node:fs/promises")
  const noteId = value("note") ?? (await readFile(join(out!, "long-note-id.txt"), "utf8")).trim()
  try {
    const before = await server.ok(`/notes/${noteId}`) as StoredNote
    const origins = server.sql(`select origin, count(*) from "TranscriptSegment" where "noteId"='${noteId}' group by 1`).map(row => row.join(" "))
    const from = performance.now()
    const generated = await server.call(`/notes/${noteId}/generate`, "POST", { expectedTranscriptRevision: before.transcriptRevision })
    const note = generated.json as StoredNote | undefined
    report.generation = { status: generated.status, ms: Math.round(performance.now() - from), characters: note?.enhanced?.length ?? 0, segments: before.transcriptSegments.length, origins, head: note?.enhanced?.slice(0, 500) }
    check("notes are generated from a transcript made entirely of live segments", generated.status === 200 && (note?.enhanced?.length ?? 0) > 300 && note?.generationState === "ready" && origins.length === 1 && /^live /.test(origins[0]!), { status: generated.status, characters: note?.enhanced?.length ?? 0, origins, ms: Math.round(performance.now() - from) })
    const items = [...(note?.enhanced ?? "").matchAll(/\bitem\s+(\d+)/gi)].length
    report.generation = { ...(report.generation as object), itemMentions: items }
    // "Explain this paragraph": the supporting quotes must be real stored segments.
    const paragraph = (note?.enhanced ?? "").split("\n").map(line => line.trim()).find(line => /^[-*]\s+\S/.test(line) && line.length > 40) ?? ""
    const evidence = await server.call(`/notes/${noteId}/evidence`, "POST", { paragraph, expectedTranscriptRevision: before.transcriptRevision })
    const quotes = (evidence.json?.quotes ?? []) as Array<{ segmentId: string; text: string; startMs: number }>
    const liveIds = new Set(server.sql(`select id from "TranscriptSegment" where "noteId"='${noteId}' and origin='live'`).map(row => row[0]))
    const byId = new Map(before.transcriptSegments.map(segment => [segment.id, segment]))
    check("evidence for a generated paragraph quotes stored live segments exactly", evidence.status === 200 && quotes.length > 0 && quotes.every(quote => liveIds.has(quote.segmentId) && byId.get(quote.segmentId)?.text === quote.text && byId.get(quote.segmentId)?.startMs === quote.startMs), { status: evidence.status, paragraph: paragraph.slice(0, 120), quotes: quotes.map(quote => quote.text.slice(0, 70)) })
    // Chat on the finished note: citations point at live segments and quote them.
    const helper = new Recording(server, silence(1), { title: "unused", noteId })
    helper.noteId = noteId
    const reply = await helper.ask("What was decided about the vendor contract? Cite the transcript.")
    const cited = reply.citations.filter(citation => citation.source === "transcript")
    check("chat after the meeting cites live segments, and each quote is in its segment", !reply.error && cited.length > 0 && cited.every(citation => citation.segmentId !== null && liveIds.has(citation.segmentId) && byId.get(citation.segmentId)!.text.includes(citation.quote)), { citations: reply.citations.length, transcript: cited.length, answer: reply.answer.slice(0, 200), error: reply.error })
  } finally {
    const removed = await server.call(`/notes/${noteId}`, "DELETE")
    check("the long meeting's note was deleted", removed.status === 200, `HTTP ${removed.status}`)
    check("its rows are gone from the database", server.sql(`select count(*) from "TranscriptSegment" where "noteId"='${noteId}'`)[0]?.[0] === "0" && server.sql(`select count(*) from "LiveCoverage" where "noteId"='${noteId}'`)[0]?.[0] === "0" && server.sql(`select count(*) from "AudioChunk" where "noteId"='${noteId}'`)[0]?.[0] === "0")
  }
}


const scenarios: Record<string, () => Promise<void>> = { refusals, long, drops, pause, concurrency, pace, admin, audio, muted, chat, generate, cleanup }
const run = scenarios[scenario]
if (!run) { console.error(`unknown scenario ${scenario}; one of ${Object.keys(scenarios).join(", ")}`); process.exit(2) }
try { await run() } catch (error) { check("the scenario ran to its end", false, String(error)) }
await finishReport()
