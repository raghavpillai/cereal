/**
 * Building blocks of the live transcription test campaign (scripts/live/qa.ts): a recording client that
 * behaves like the desktop and iPhone apps against a running server, and the checks applied to what it
 * leaves behind. Everything goes through the public API with a bearer token, except read-only SQL and
 * container statistics, which are used when the server is the local Compose deployment.
 */
import { readFile } from "node:fs/promises"
import { join } from "node:path"
import { encodeLiveAudioFrame, type LiveServerMessage } from "@meeting-notes/contracts/live"
import { wavFile } from "./make-fixture"
import { itemPattern, type QaLine, type QaManifest } from "./make-qa-fixtures"

export type Channel = "mic" | "system"
export interface Fixture { rate: number; channels: Channel[]; pcm: Record<Channel, Uint8Array>; totalSamples: number; lines: QaLine[] }
/** Loads a fixture, optionally cut to a window of it, restricted to some channels, or with its level changed. */
export async function loadFixture(directory: string, options: { fromSeconds?: number; seconds?: number; channels?: Channel[]; gain?: number } = {}): Promise<Fixture> {
  const manifest: QaManifest = JSON.parse(await readFile(join(directory, "manifest.json"), "utf8")), rate = manifest.sampleRate
  const channels = (Object.keys(manifest.channels) as Channel[]).filter(channel => !options.channels || options.channels.includes(channel))
  const from = Math.round((options.fromSeconds ?? 0) * rate), all = Math.max(...channels.map(channel => manifest.channels[channel]!.samples))
  const to = Math.min(all, options.seconds === undefined ? all : from + Math.round(options.seconds * rate))
  const pcm = {} as Record<Channel, Uint8Array>
  for (const channel of channels) {
    const bytes = new Uint8Array(new Uint8Array(await readFile(join(directory, manifest.channels[channel]!.file))).subarray(44 + from * 2, 44 + to * 2))
    if (options.gain !== undefined) { const view = new DataView(bytes.buffer); for (let i = 0; i < bytes.byteLength; i += 2) view.setInt16(i, Math.round(view.getInt16(i, true) * options.gain), true) }
    pcm[channel] = bytes
  }
  // Only lines that lie wholly inside the window, on a kept channel, re-based to the window.
  const lines = manifest.lines.filter(line => channels.includes(line.channel) && line.firstSample >= from && line.lastSample < to).map(line => ({ ...line, firstSample: line.firstSample - from, lastSample: line.lastSample - from }))
  return { rate, channels, pcm, totalSamples: to - from, lines }
}
export const silence = (seconds: number, channels: Channel[] = ["mic", "system"], rate = 16000): Fixture => ({ rate, channels, pcm: Object.fromEntries(channels.map(channel => [channel, new Uint8Array(seconds * rate * 2)])) as Record<Channel, Uint8Array>, totalSamples: seconds * rate, lines: [] })

export const load = () => { const text = Bun.spawnSync(["uptime"]).stdout.toString(); return /load averages?: ([\d.]+)/.exec(text)?.[1] ?? "?" }
export const stats = (values: number[]) => {
  const sorted = [...values].sort((a, b) => a - b), at = (p: number) => sorted.length ? Math.round(sorted[Math.min(sorted.length - 1, Math.max(0, Math.ceil(p * sorted.length) - 1))]!) : null
  return { n: sorted.length, median: at(0.5), p95: at(0.95), max: at(1) }
}

export class Server {
  constructor(readonly base: string, readonly token: string, readonly docker: { context: string; app: string; db: string } | null) {}
  async call(path: string, method = "GET", body?: unknown, headers?: Record<string, string>) {
    const response = await fetch(`${this.base}/api/v1${path}`, { method, headers: headers ?? { Authorization: `Bearer ${this.token}`, ...(body === undefined ? {} : { "Content-Type": "application/json" }) }, ...(body === undefined ? {} : { body: JSON.stringify(body) }) })
    const text = await response.text()
    let json: unknown; try { json = JSON.parse(text) } catch { json = undefined }
    return { status: response.status, text, json: json as any }
  }
  async ok(path: string, method = "GET", body?: unknown) {
    const result = await this.call(path, method, body)
    if (result.status < 200 || result.status >= 300) throw new Error(`${method} ${path} answered ${result.status}: ${result.text.slice(0, 200)}`)
    return result.json
  }
  /** Read-only SQL against the deployment's database, for facts the API does not expose (how a part was resolved). */
  sql(query: string): string[][] {
    if (!this.docker) return []
    const result = Bun.spawnSync(["docker", "--context", this.docker.context, "exec", this.docker.db, "psql", "-U", "meeting_notes", "-d", "meeting_notes", "-At", "-F", "\t", "-c", query])
    if (result.exitCode !== 0) throw new Error(`psql failed: ${result.stderr.toString().slice(0, 200)}`)
    return result.stdout.toString().split("\n").filter(Boolean).map(row => row.split("\t"))
  }
  memory(): string { return this.docker ? Bun.spawnSync(["docker", "--context", this.docker.context, "stats", "--no-stream", "--format", "{{.MemUsage}}", this.docker.app]).stdout.toString().trim().split(" / ")[0] ?? "?" : "?" }
  /** App container log lines since a moment, without request content (the server logs none). */
  logs(since: Date): string[] { return this.docker ? Bun.spawnSync(["docker", "--context", this.docker.context, "logs", "--since", since.toISOString(), this.docker.app]).stderr.toString().concat(Bun.spawnSync(["docker", "--context", this.docker.context, "logs", "--since", since.toISOString(), this.docker.app]).stdout.toString()).split("\n").filter(Boolean) : [] }
  async ready() { return (await fetch(`${this.base}/api/v1/ready`).catch(() => null))?.status ?? 0 }
}

export interface RecordingOptions {
  title: string
  captureId?: string
  /** Note-timeline position of this capture's first sample. */
  baseMs?: number
  language?: string | null
  partSeconds?: number
  /** Durable parts are uploaded as the capture passes them, as the clients do. */
  upload?: boolean
  /** A real network outage also stops uploads: parts that fall due while the connection is down wait for it. */
  holdUploadsWhileDown?: boolean
  /** First sequence number of this capture's parts (a second capture on the same note continues the count). */
  firstSequence?: number
  /** How fast the capture grows against the wall clock (3 = a file replayed at three times real time). */
  captureSpeed?: number
  /** Ceiling on what the stream sends, in multiples of real time (0.5 = a connection that cannot keep up). */
  sendSpeed?: number
  noteId?: string
  quiet?: boolean
}
export interface Event { atSeconds: number; run: (recording: Recording) => Promise<void> | void; blocking?: boolean; done?: boolean }

/** One recording client: a note, a capture that grows with the clock, a live socket that follows it, durable parts. */
export class Recording {
  noteId = ""
  readonly received: Array<{ at: number; message: LiveServerMessage }> = []
  readonly closes: Array<{ at: number; code: number }> = []
  readonly log: string[] = []
  readonly uploads: Promise<unknown>[] = []
  readonly readies: Array<Extract<LiveServerMessage, { type: "ready" }>> = []
  readonly lineEnd = new Map<number, number>()
  /** Every reconnect: where the server asked to resume, where the capture stood, and where the client went on. */
  readonly resumes: Array<{ acceptSample: number; capturedSample: number; fromSample: number; cursorSample: number; replay: boolean }> = []
  /** Capture positions at which the connection was cut. */
  readonly cuts: number[] = []
  socket: WebSocket | undefined
  ready: Extract<LiveServerMessage, { type: "ready" }> | undefined
  refusal: number | undefined
  cursor = 0
  captured = 0
  connections = 0
  private readonly acked: Partial<Record<Channel, number>> = {}
  private nextPart = 0
  private pausedMs = 0
  private origin = 0
  private started = performance.now()
  private sentBudgetAt = 0
  private reconnectAt: number | undefined
  private down = false
  readonly now = () => performance.now() - this.started

  constructor(readonly server: Server, readonly fixture: Fixture, readonly options: RecordingOptions) {}
  private say(text: string) { const line = `${(this.now() / 1000).toFixed(1).padStart(7)} s  ${text}`; this.log.push(line); if (!this.options.quiet) console.log(`  [${this.options.title.slice(0, 18)}] ${line}`) }

  async create(start = true) {
    this.noteId = this.options.noteId ?? (await this.server.ok("/notes", "POST", { title: this.options.title })).id
    if (start) await this.server.ok(`/notes/${this.noteId}/recording`, "POST", { command: this.options.noteId ? "resume" : "start" })
    return this.noteId
  }
  /** Opens the socket and says hello. Resolves "ready", or the HTTP status or close code of a refusal. */
  async connect(timeoutMs = 20_000): Promise<"ready" | number> {
    this.ready = undefined; this.refusal = undefined; this.connections++
    const { fixture } = this
    const socket = new WebSocket(`${this.server.base.replace("http", "ws")}/api/v1/notes/${this.noteId}/live`, { headers: { Authorization: `Bearer ${this.server.token}` } } as never)
    this.socket = socket
    let closed: number | undefined
    socket.onmessage = event => {
      const message = JSON.parse(String(event.data)) as LiveServerMessage
      this.received.push({ at: this.now(), message })
      if (message.type === "ready") { this.ready = message; this.readies.push(message) }
      if (message.type === "ack") this.acked[message.channel] = message.throughSample
      if (message.type === "error" || message.type === "gap") this.say(`server ${message.type}: ${JSON.stringify(message)}`)
    }
    socket.onclose = event => { closed = event.code; this.closes.push({ at: this.now(), code: event.code }); if (this.socket === socket) this.socket = undefined }
    socket.onerror = () => { closed ??= 0 }
    socket.onopen = () => socket.send(JSON.stringify({ type: "hello", protocol: 1, captureId: this.options.captureId ?? "qa-capture", ...(this.options.language === null ? {} : { language: this.options.language ?? "en" }),
      streams: fixture.channels.map(channel => ({ channel, sampleRate: fixture.rate, encoding: "pcm_s16le", baseMs: this.options.baseMs ?? 0, resumeFromSample: this.acked[channel] ?? 0 })) }))
    for (const deadline = performance.now() + timeoutMs; !this.ready;) {
      if (closed !== undefined || performance.now() > deadline) {
        // A refusal before the upgrade is an HTTP status the WebSocket API hides: ask again with plain HTTP to read it.
        const status = closed && closed >= 4000 ? closed : (await fetch(`${this.server.base}/api/v1/notes/${this.noteId}/live`, { headers: { Authorization: `Bearer ${this.server.token}`, Upgrade: "websocket", Connection: "Upgrade", "Sec-WebSocket-Version": "13", "Sec-WebSocket-Key": "dGhlIHNhbXBsZSBub25jZQ==" } }).catch(() => null))?.status ?? 0
        this.refusal = status; this.socket = undefined
        return status
      }
      await Bun.sleep(10)
    }
    // On reconnection: replay a backlog within the window, otherwise skip to five seconds before the end of the capture.
    const accept = Math.min(...this.ready!.streams.map(stream => stream.acceptFromSample)), backlogMs = (this.captured - accept) * 1000 / fixture.rate
    const before = this.cursor
    this.cursor = backlogMs <= this.ready!.resumeWindowMs ? accept : Math.max(accept, this.captured - 5 * fixture.rate)
    this.cursor -= this.cursor % Math.round(fixture.rate / 10)
    if (this.connections > 1) this.resumes.push({ acceptSample: accept, capturedSample: this.captured, fromSample: before, cursorSample: this.cursor, replay: backlogMs <= this.ready!.resumeWindowMs })
    if (this.connections > 1) this.say(`reconnected: server accepts from ${(accept / fixture.rate).toFixed(1)} s, capture at ${(this.captured / fixture.rate).toFixed(1)} s, was sending at ${(before / fixture.rate).toFixed(1)} s, ${backlogMs <= this.ready!.resumeWindowMs ? "replaying" : `skipping to ${(this.cursor / fixture.rate).toFixed(1)} s`}`)
    return "ready"
  }
  /** Cuts the connection as a lost network does, for `seconds` (or for good). */
  drop(seconds?: number) {
    this.say(`dropping the connection${seconds === undefined ? "" : ` for ${seconds} s`}`)
    this.down = true
    this.cuts.push(this.cursor)
    try { (this.socket as unknown as { terminate(): void } | undefined)?.terminate() } catch { /* already gone */ }
    this.socket = undefined
    this.reconnectAt = seconds === undefined ? undefined : performance.now() + seconds * 1000
  }
  /** A recording with no live connection at all: only durable parts. */
  offline() { this.down = true; return this }
  mark(kind: "pause" | "resume" | "end", at = this.cursor) { this.socket?.send(JSON.stringify({ type: "mark", kind, atSample: Object.fromEntries(this.fixture.channels.map(channel => [channel, at])) })) }
  /** Stops the capture clock for a while, as a paused recording does (no audio exists for that time). */
  async pause(seconds: number) {
    this.say(`pause for ${seconds} s`)
    this.mark("pause")
    const from = performance.now()
    await Bun.sleep(seconds * 1000)
    this.pausedMs += performance.now() - from
    this.mark("resume")
    this.say("resume")
  }
  private uploadDue(final = false) {
    if (this.options.upload === false || (this.options.holdUploadsWhileDown && this.down && !final)) return
    const { fixture } = this, part = (this.options.partSeconds ?? 30) * fixture.rate
    while ((this.nextPart + 1) * part <= this.captured || (final && this.nextPart * part < this.captured)) {
      const from = this.nextPart * part, to = Math.min(this.captured, from + part), sequence = (this.options.firstSequence ?? 0) + this.nextPart++
      for (const channel of fixture.channels) this.uploads.push(this.server.ok(`/notes/${this.noteId}/audio/chunks/${crypto.randomUUID()}`, "PUT", { channel, sequence, startMs: (this.options.baseMs ?? 0) + Math.round(from * 1000 / fixture.rate), audioBase64: Buffer.from(wavFile(fixture.pcm[channel].slice(from * 2, to * 2), fixture.rate)).toString("base64") }).catch(error => this.say(`part upload failed: ${error}`)))
      if (to >= this.captured && final) break
    }
  }
  /** Runs the capture to `untilSeconds` (default: the end of the fixture), firing events at their capture time. */
  async run(events: Event[] = [], untilSeconds?: number, options: { /** False: return when the capture ends even if the stream has not sent all of it. */ drain?: boolean } = {}) {
    const { fixture } = this, frame = Math.round(fixture.rate / 10), end = Math.min(fixture.totalSamples, untilSeconds === undefined ? fixture.totalSamples : Math.round(untilSeconds * fixture.rate))
    const speed = this.options.captureSpeed ?? 1
    if (!this.origin) this.origin = performance.now() - this.captured * 1000 / fixture.rate / speed
    let sentSince = performance.now(), sentSamples = 0
    while (this.captured < end || (options.drain !== false && this.cursor < end && this.socket && !this.down)) {
      this.captured = Math.min(end, Math.floor((performance.now() - this.origin - this.pausedMs) * speed * fixture.rate / 1000))
      for (const event of events) if (!event.done && this.captured >= event.atSeconds * fixture.rate) { event.done = true; const work = Promise.resolve(event.run(this)).catch(error => this.say(`event failed: ${error}`)); if (event.blocking) await work }
      this.uploadDue()
      if (this.down && this.reconnectAt !== undefined && performance.now() >= this.reconnectAt) { this.reconnectAt = undefined; const result = await this.connect(); if (result === "ready") this.down = false; else { this.say(`reconnect refused: ${result}`); this.reconnectAt = performance.now() + 2000 } continue }
      // A connection the server closed is reopened after a moment, as a client would.
      if (!this.socket && !this.down && this.refusal === undefined) { this.say(`socket closed by the server (${this.closes.at(-1)?.code}); reconnecting`); await Bun.sleep(500); const result = await this.connect(); if (result !== "ready") { this.say(`reconnect refused: ${result}`); this.down = true } continue }
      const to = Math.min(this.captured, this.cursor + frame)
      if (!this.socket || this.down || this.socket.readyState !== WebSocket.OPEN || (to - this.cursor < frame && to < end) || to <= this.cursor) { await Bun.sleep(2); continue }
      if (this.options.sendSpeed !== undefined) {
        // A stream that cannot keep up: a budget of samples per wall second.
        const allowed = (performance.now() - sentSince) * this.options.sendSpeed * fixture.rate / 1000
        if (sentSamples + frame > allowed) { await Bun.sleep(5); continue }
        sentSamples += to - this.cursor
      }
      for (const channel of fixture.channels) this.socket.send(encodeLiveAudioFrame({ channel, firstSample: this.cursor, pcm: fixture.pcm[channel].subarray(this.cursor * 2, to * 2) }) as Uint8Array<ArrayBuffer>)
      fixture.lines.forEach((line, index) => { if (!this.lineEnd.has(index) && line.lastSample < to) this.lineEnd.set(index, this.now()) })
      this.cursor = to
      // Behind the capture (after a reconnect): catch up at no more than twice real time.
      if (this.captured - this.cursor >= frame) await Bun.sleep(100 / speed / 2)
    }
    void sentSince; void this.sentBudgetAt
  }
  /** One chat question through the streaming endpoint. */
  async ask(question: string, history: Array<{ role: "user" | "assistant"; content: string }> = [], onFirstText?: () => void) {
    const asked = performance.now()
    const response = await fetch(`${this.server.base}/api/v1/notes/${this.noteId}/chat/stream`, { method: "POST", headers: { Authorization: `Bearer ${this.server.token}`, "Content-Type": "application/json" }, body: JSON.stringify({ message: question, history, includeSources: true }) })
    const reader = response.body!.getReader(), decoder = new TextDecoder()
    let body = "", firstAt: number | undefined, firstDeltaAt: number | undefined
    for (;;) { const { done, value } = await reader.read(); if (done) break; firstAt ??= performance.now(); body += decoder.decode(value, { stream: true }); if (firstDeltaAt === undefined && body.includes('"type":"delta"')) { firstDeltaAt = performance.now(); onFirstText?.() } }
    const events = [...body.matchAll(/^data: (.*)$/gm)].flatMap(match => { try { return [JSON.parse(match[1]!) as { type: string; text: string; citations?: Array<{ id: string; source: string; segmentId: string | null; startMs: number | null; quote: string }> }] } catch { return [] } })
    return { status: response.status, answer: events.filter(event => event.type === "delta").map(event => event.text).join(""), error: events.find(event => event.type === "error")?.text ?? null,
      citations: events.find(event => event.type === "citations")?.citations ?? [], firstByteMs: Math.round((firstAt ?? performance.now()) - asked), firstTextMs: firstDeltaAt === undefined ? null : Math.round(firstDeltaAt - asked), totalMs: Math.round(performance.now() - asked) }
  }
  /** The end of a recording as the clients do it: last parts, `mark: end`, wait for the close, Stop. */
  async finish(options: { mark?: boolean; waitClose?: boolean; stop?: boolean; settleMs?: number } = {}) {
    if (options.settleMs) await Bun.sleep(options.settleMs)
    const socket = this.socket
    if (options.mark !== false && socket?.readyState === WebSocket.OPEN) {
      // `atSample` is what the capture holds, which is more than was sent when the stream is behind.
      this.mark("end", this.captured)
      if (options.waitClose !== false) for (const deadline = performance.now() + 8000; socket.readyState !== WebSocket.CLOSED && performance.now() < deadline;) await Bun.sleep(20)
    }
    this.down = true
    this.uploadDue(true)
    await Promise.all(this.uploads)
    let stopMs: number | null = null
    if (options.stop !== false) { const from = performance.now(); await this.server.ok(`/notes/${this.noteId}/recording`, "POST", { command: "stop" }); stopMs = Math.round(performance.now() - from) }
    return { stopMs, closeCode: this.closes.at(-1)?.code ?? null }
  }
  /** Waits until every durable part of the note is ready or failed. */
  async partsSettled(timeoutMs = 240_000) {
    for (const deadline = performance.now() + timeoutMs; ;) {
      const chunks = await this.server.ok(`/notes/${this.noteId}/audio/chunks`) as Array<{ state: string }>
      if (chunks.every(chunk => chunk.state === "ready" || chunk.state === "failed")) return chunks
      if (performance.now() > deadline) return chunks
      await Bun.sleep(1000)
    }
  }
  async stored() { return await this.server.ok(`/notes/${this.noteId}`) as StoredNote }
  async remove() { const result = await this.server.call(`/notes/${this.noteId}`, "DELETE"); return result.status }
  of<K extends LiveServerMessage["type"]>(type: K) { return this.received.flatMap(item => item.message.type === type ? [{ at: item.at, message: item.message as Extract<LiveServerMessage, { type: K }> }] : []) }
  /** Delay from the last sample of each line being sent to its end-of-line phrase arriving as interim or final text. */
  delays(type: "interim" | "final") {
    const { fixture } = this
    return fixture.lines.flatMap((line, index) => {
      const end = this.lineEnd.get(index), pattern = new RegExp(line.probe, "i"), lineStartMs = (this.options.baseMs ?? 0) + line.firstSample * 1000 / fixture.rate
      if (end === undefined) return []
      const hit = this.received.find(({ message }) => message.type === type && (message.type === "interim" ? message.channel === line.channel && message.endMs >= lineStartMs && pattern.test(message.text)
        : message.type === "final" && message.segment.channel === line.channel && (message.segment.endMs ?? message.segment.startMs) >= lineStartMs && pattern.test(message.segment.text)))
      return hit ? [Math.max(0, hit.at - end)] : []
    })
  }
}
export interface StoredNote { id: string; title: string; recordingState: string; transcriptRevision: number; duration: string; enhanced: string; generationState: string
  transcriptSegments: Array<{ id: string; channel: Channel; speakerId: string; speakerLabel: string | null; text: string; startMs: number; endMs: number | null; isFinal: boolean }> }

/** What the stored transcript says about the scripted lines that were captured. */
export function checkTranscript(note: StoredNote, fixture: Fixture, options: { capturedSamples?: number; baseMs?: number;
  /** Capture positions where audio was cut (a dropped connection, a skip): a line spoken across one can be split or clipped, and is reported apart. */
  cutSamples?: number[] } = {}) {
  const captured = options.capturedSamples ?? fixture.totalSamples, base = options.baseMs ?? 0, rate = fixture.rate
  const lines = fixture.lines.filter(line => line.lastSample < captured)
  const text = (channel: Channel) => note.transcriptSegments.filter(segment => segment.channel === channel).sort((a, b) => a.startMs - b.startMs).map(segment => segment.text).join(" ")
  const texts = Object.fromEntries(fixture.channels.map(channel => [channel, text(channel)])) as Record<Channel, string>
  const count = (haystack: string, pattern: string) => haystack.match(new RegExp(pattern, "gi"))?.length ?? 0
  const missing: number[] = [], duplicated: number[] = [], wrongChannel: number[] = [], acrossCut: Array<{ item: number; found: number }> = []
  const margin = rate / 2
  for (const line of lines) {
    const own = count(texts[line.channel], itemPattern(line.item)), other = fixture.channels.filter(channel => channel !== line.channel).reduce((sum, channel) => sum + count(texts[channel], itemPattern(line.item)), 0)
    if ((options.cutSamples ?? []).some(cut => cut >= line.firstSample - margin && cut <= line.lastSample + margin)) { acrossCut.push({ item: line.item, found: own }); continue }
    if (own === 0) missing.push(line.item); else if (own > 1) duplicated.push(line.item)
    if (other) wrongChannel.push(line.item)
  }
  // Drift: where a segment begins with a line's marker, its start time against the sample position the line was spoken at.
  const offsets: Array<{ item: number; atSeconds: number; offsetMs: number }> = []
  const speakers: Record<string, Record<string, number>> = {}
  for (const segment of note.transcriptSegments) {
    for (const line of lines) {
      if (line.channel !== segment.channel || !new RegExp(`^\\W*${itemPattern(line.item).replace(/^\\b/, "")}`, "i").test(segment.text)) continue
      offsets.push({ item: line.item, atSeconds: line.firstSample / rate, offsetMs: Math.round(segment.startMs - base - line.firstSample * 1000 / rate) })
      const row = speakers[`${segment.channel} ${line.voice}`] ??= {}
      row[segment.speakerId] = (row[segment.speakerId] ?? 0) + 1
    }
  }
  offsets.sort((a, b) => a.atSeconds - b.atSeconds)
  const head = offsets.slice(0, Math.max(1, Math.floor(offsets.length / 5))), tail = offsets.slice(-Math.max(1, Math.floor(offsets.length / 5)))
  const median = (rows: typeof offsets) => stats(rows.map(row => row.offsetMs)).median
  const ordered = [...note.transcriptSegments].sort((a, b) => a.startMs - b.startMs)
  return {
    linesCaptured: lines.length, segments: note.transcriptSegments.length, missing, duplicated, wrongChannel, acrossCut,
    drift: { linesMeasured: offsets.length, firstFifthMedianMs: median(head), lastFifthMedianMs: median(tail), changeMs: (median(tail) ?? 0) - (median(head) ?? 0), minMs: Math.min(...offsets.map(row => row.offsetMs)), maxMs: Math.max(...offsets.map(row => row.offsetMs)) },
    speakers, allFinal: note.transcriptSegments.every(segment => segment.isFinal),
    overlapWithinChannel: ordered.filter((segment, index) => ordered.slice(index + 1).some(later => later.channel === segment.channel && later.startMs < (segment.endMs ?? segment.startMs) - 300)).length,
  }
}
/** How the note's durable parts were accounted for, and the live coverage ledger, from the database. */
export function partFacts(server: Server, noteId: string) {
  const parts = server.sql(`select coalesce(resolution,'-'), state, count(*) from "AudioChunk" where "noteId"='${noteId}' group by 1,2 order by 1,2`).map(([resolution, state, count]) => `${count} ${state}/${resolution}`)
  const origins = server.sql(`select origin, count(*) from "TranscriptSegment" where "noteId"='${noteId}' group by 1 order by 1`).map(([origin, count]) => `${count} ${origin}`)
  const ledger = server.sql(`select channel, source, "fromMs", "toMs" from "LiveCoverage" where "noteId"='${noteId}' order by channel, "fromMs"`).map(([channel, source, from, to]) => `${channel} ${source} ${from}-${to}`)
  const speakers = server.sql(`select channel, number from "NoteSpeaker" where "noteId"='${noteId}' order by channel, number`).map(([channel, number]) => `${channel}:${number}`)
  return { parts, origins, ledger, speakers }
}
