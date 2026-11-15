/** Shared fixture for the live transcription tests: a real `Bun.serve` with the live route, the in-memory
 * repository, the fake streaming recogniser and a batch transcriber that reads the same scripted audio. */
import { mkdtemp, rm } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { Effect } from "effect"
import type { PrincipalData } from "@meeting-notes/contracts/api"
import type { ChatInput, NoteDTO, TranscriptSegmentDTO } from "@meeting-notes/contracts"
import { encodeLiveAudioFrame, type LiveHelloMessage, type LiveServerMessage, type LiveTranscriber } from "@meeting-notes/contracts/live"
import { makeLiveHub } from "../src/live/hub"
import { makeLiveEndpoint, type LiveConnection } from "../src/live/route"
import { makeVoiceMatcher } from "../src/live/voices"
import { decodeFakeSpeech, makeFakeTranscriber, type FakeScript, type FakeTranscriberOptions } from "../src/live/providers/fake"
import type { LiveConfig } from "../src/live/session"
import { makeProcessingService } from "../src/processing"
import type { ProviderNote, ProviderServices } from "../src/providers"
import { makeMemoryRepository, type NotesRepositoryService } from "../src/repository"

const defaultUsers = { alice: "30000000-0000-4000-8000-000000000001", bob: "30000000-0000-4000-8000-000000000002" } as const
export const users = { alice: "30000000-0000-4000-8000-000000000001", bob: "30000000-0000-4000-8000-000000000002" } as const
export type User = keyof typeof users
export const RATE = 16000

export function wav(pcm: Uint8Array, sampleRate = RATE) {
  const header = new DataView(new ArrayBuffer(44)), ascii = (offset: number, text: string) => { for (let i = 0; i < text.length; i++) header.setUint8(offset + i, text.charCodeAt(i)) }
  ascii(0, "RIFF"); header.setUint32(4, 36 + pcm.byteLength, true); ascii(8, "WAVE"); ascii(12, "fmt "); header.setUint32(16, 16, true); header.setUint16(20, 1, true); header.setUint16(22, 1, true)
  header.setUint32(24, sampleRate, true); header.setUint32(28, sampleRate * 2, true); header.setUint16(32, 2, true); header.setUint16(34, 16, true); ascii(36, "data"); header.setUint32(40, pcm.byteLength, true)
  const out = new Uint8Array(44 + pcm.byteLength); out.set(new Uint8Array(header.buffer)); out.set(pcm, 44); return out
}

export interface LiveFixtureOptions {
  script?: FakeScript
  fake?: FakeTranscriberOptions
  config?: Partial<LiveConfig>
  maxSessions?: number
  /** Null: live transcription is not configured. */
  transcriber?: LiveTranscriber | null
  /** False imitates a batch provider that returns no word times. */
  batchWordTimes?: boolean
  repository?: NotesRepositoryService
  livePendingWaitMs?: number
  /** False: no comparison of voices across provider sessions (as with no batch recogniser configured). */
  voices?: boolean
  /** Account IDs to use instead of the fixture's own (PostgreSQL needs rows that exist). */
  accounts?: Partial<Record<User, string>>
}
export async function liveFixture(options: LiveFixtureOptions = {}) {
  const repository = options.repository ?? makeMemoryRepository()
  const users = { ...defaultUsers, ...options.accounts }
  const fake = makeFakeTranscriber({ ...(options.script ? { vocabulary: options.script.vocabulary } : {}), interimDelayMs: 30, finalDelayMs: 60, ...options.fake })
  const storageDir = await mkdtemp(join(tmpdir(), "live-test-"))
  // The comparison of voices across provider sessions reads the same durable parts and the same batch recogniser as the worker.
  const voiceCalls: number[] = []
  const voices = makeVoiceMatcher({ storageDir, segments: async noteId => (await Effect.runPromise(repository.getNote(noteId)))?.transcriptSegments ?? [], parts: noteId => Effect.runPromise(repository.listAudioChunks(noteId)),
    transcribe: input => { voiceCalls.push(input.durationMs); return Effect.runPromise(providers.transcribe(input)) } })
  const hub = makeLiveHub(repository, { config: { tickMs: 50, ackIntervalMs: 100, ...options.config }, ...(options.maxSessions ? { maxSessions: options.maxSessions } : {}), ...(options.voices === false ? {} : { voices }) })
  const batchCalls: Array<{ chunkId: string; startMs: number; keep?: ReadonlyArray<{ fromMs: number; toMs: number }> }> = []
  const chatContexts: Array<{ note: ProviderNote & { live?: unknown }; system: string; context: string }> = []
  const providers: ProviderServices = {
    transcribe: input => Effect.sync(() => {
      batchCalls.push({ chunkId: input.chunkId, startMs: input.startMs, ...(input.keep ? { keep: input.keep } : {}) })
      const words = decodeFakeSpeech(input.audio.subarray(44), RATE, options.script?.vocabulary ?? [])
      const honour = input.keep !== undefined && options.batchWordTimes !== false
      const segments: TranscriptSegmentDTO[] = []
      let current: { speaker: string; text: string; start: number; end: number } | undefined
      const flush = () => { if (current) segments.push({ id: crypto.randomUUID(), channel: input.channel, speakerId: `${input.channel}:${input.chunkId}:${current.speaker}`, speakerLabel: "Speaker 1", text: current.text.trim(), isFinal: true, startMs: input.startMs + Math.round(current.start * 1000), endMs: input.startMs + Math.round(current.end * 1000) }); current = undefined }
      for (const word of words) {
        const middle = input.startMs + (word.startSec + word.endSec) * 500
        if (honour && !input.keep!.some(range => middle >= range.fromMs && middle < range.toMs)) { flush(); continue }
        const speaker = word.speaker ?? "speaker_1"
        if (current && current.speaker === speaker) { current.text += word.text; current.end = word.endSec } else { flush(); current = { speaker, text: word.text, start: word.startSec, end: word.endSec } }
      }
      flush()
      return honour ? Object.assign(segments, { limitedToKeep: true }) : segments
    }),
    generate: () => Effect.succeed("generated"), chat: () => Effect.succeed("answer"),
    async *chatStream(note, input: ChatInput) {
      const { chatAnswerMessages, meetingChatDocument } = await import("../src/ai")
      const messages = chatAnswerMessages(note, input)
      // The meeting context can span several messages once the transcript is long; the helper joins it.
      chatContexts.push({ note, system: messages[0]!.content, context: meetingChatDocument(messages) })
      yield "ok"
    },
  }
  const processing = makeProcessingService(repository, { worker: false, providers, storageDir, live: hub, livePendingRetryMs: 20, ...(options.livePendingWaitMs === undefined ? {} : { livePendingWaitMs: options.livePendingWaitMs }) })
  const endpoint = makeLiveEndpoint({
    repository, hub,
    resolvePrincipal: async headers => {
      const name = /^Bearer (\w+)$/.exec(headers.get("authorization") ?? "")?.[1] as User | undefined
      if (!name || !(name in defaultUsers) || revoked.has(name)) return null
      return { id: users[name], name, email: `${name}@example.test`, role: "user", source: "session", expiresAt: "2099-01-01T00:00:00Z", createdAt: "2026-01-01T00:00:00Z" } satisfies PrincipalData
    },
    isTrustedOrigin: origin => origin === "https://cereal.example.test",
    transcriber: async () => options.transcriber === undefined ? fake : options.transcriber,
    helloTimeoutMs: 1500, reauthenticateMs: 80,
  })
  const revoked = new Set<User>()
  const server = Bun.serve<LiveConnection>({
    port: 0, hostname: "127.0.0.1", websocket: endpoint.websocket,
    fetch(request, server) {
      const noteId = endpoint.match(new URL(request.url).pathname)
      return noteId ? endpoint.upgrade(request, server, noteId) : new Response("not found", { status: 404 })
    },
  })
  const sockets: LiveClient[] = []
  const url = (noteId: string, scheme = "ws") => `${scheme}://127.0.0.1:${server.port}/api/v1/notes/${noteId}/live`
  async function recordingNote(user: User = "alice", title = "Live test") {
    const scoped = repository.forUser(users[user])
    const note = await Effect.runPromise(scoped.createNote({ title }))
    await Effect.runPromise(scoped.recording(note.id, "start"))
    return note.id
  }
  const note = async (id: string, user: User = "alice") => (await Effect.runPromise(repository.forUser(users[user]).getNote(id)))!
  function connect(noteId: string, user: User | null = "alice", headers: Record<string, string> = {}) {
    const client = new LiveClient(url(noteId), { ...(user ? { Authorization: `Bearer ${user}` } : {}), ...headers })
    sockets.push(client)
    return client
  }
  /** Uploads scripted audio as one durable part and runs the batch worker until it has nothing claimable. */
  async function uploadPart(noteId: string, pcm: Uint8Array, startMs: number, sequence: number, channel: "mic" | "system" = "system", user: User = "alice") {
    const scoped = processing.forRepository(repository.forUser(users[user]))
    return Effect.runPromise(scoped.upload(noteId, crypto.randomUUID(), { channel, sequence, startMs, audioBase64: Buffer.from(wav(pcm)).toString("base64") }))
  }
  const chunks = (noteId: string) => Effect.runPromise(repository.listAudioChunks(noteId))
  async function settleParts(noteId: string, timeoutMs = 15_000) {
    const deadline = performance.now() + timeoutMs
    for (;;) {
      await processing.runPending()
      if ((await chunks(noteId)).every(chunk => chunk.state === "ready" || chunk.state === "failed")) return
      if (performance.now() > deadline) throw new Error("Durable parts did not settle")
      await Bun.sleep(25)
    }
  }
  async function chat(noteId: string, message: string, user: User = "alice", includeSources = true) {
    const scoped = processing.forRepository(repository.forUser(users[user]))
    const stream = await Effect.runPromise(scoped.chatStream(noteId, { message, history: [], includeSources }))
    const { Stream } = await import("effect")
    const events = await Effect.runPromise(Stream.runCollect(stream))
    return { events: [...events], request: chatContexts.at(-1)! }
  }
  async function dispose() {
    for (const socket of sockets) socket.close()
    await hub.dispose()
    await processing.dispose()
    // Not awaited: with client sockets of this same process still closing, the promise does not settle.
    void server.stop(true)
    await rm(storageDir, { recursive: true, force: true })
  }
  return { repository, fake, hub, processing, server, url, recordingNote, note, connect, uploadPart, chunks, settleParts, chat, batchCalls, voiceCalls, chatContexts, revoked, dispose }
}
export type LiveFixture = Awaited<ReturnType<typeof liveFixture>>

/** A WebSocket client that records everything the server sends. */
export class LiveClient {
  readonly messages: LiveServerMessage[] = []
  readonly socket: WebSocket
  closed: { code: number; reason: string } | undefined
  failed = false
  private waiters: Array<() => void> = []
  constructor(url: string, headers: Record<string, string>) {
    this.socket = new WebSocket(url, { headers } as never)
    this.socket.binaryType = "arraybuffer"
    this.socket.onmessage = event => { this.messages.push(JSON.parse(String(event.data))); this.wake() }
    this.socket.onclose = event => { this.closed = { code: event.code, reason: event.reason }; this.wake() }
    this.socket.onerror = () => { this.failed = true; this.wake() }
  }
  private wake() { for (const waiter of this.waiters.splice(0)) waiter() }
  /** The limit is generous because the suite also runs in a slow container; a passing wait returns at once. */
  async until<T>(probe: () => T | undefined | false | null, label = "condition", timeoutMs = 15_000): Promise<T> {
    const deadline = performance.now() + timeoutMs
    for (;;) {
      const value = probe()
      if (value) return value
      const left = deadline - performance.now()
      if (left <= 0) throw new Error(`Timed out waiting for ${label}; received ${JSON.stringify(this.messages.map(message => message.type))}, closed ${JSON.stringify(this.closed)}`)
      await new Promise<void>(resolve => { const timer = setTimeout(resolve, Math.min(left, 50)); this.waiters.push(() => { clearTimeout(timer); resolve() }) })
    }
  }
  of<K extends LiveServerMessage["type"]>(type: K) { return this.messages.filter((message): message is Extract<LiveServerMessage, { type: K }> => message.type === type) }
  opened() { return this.until(() => this.socket.readyState === WebSocket.OPEN || this.closed || this.failed, "open") }
  async hello(streams: LiveHelloMessage["streams"], captureId = "capture-1") {
    await this.opened()
    this.socket.send(JSON.stringify({ type: "hello", protocol: 1, captureId, streams } satisfies LiveHelloMessage))
    return this.until(() => this.of("ready").at(-1) ?? (this.closed ? ("closed" as const) : undefined), "ready")
  }
  /** Sends `[fromSample, toSample)` of a channel's PCM in 100 ms frames, `speed` times faster than real time. */
  async audio(pcm: Uint8Array, fromSample: number, toSample: number, options: { channel?: "mic" | "system"; speed?: number; frameMs?: number; offset?: number } = {}) {
    const frame = Math.round(RATE * (options.frameMs ?? 100) / 1000), speed = options.speed ?? 20, started = performance.now(), offset = options.offset ?? 0
    for (let sample = fromSample; sample < toSample && this.socket.readyState === WebSocket.OPEN; sample += frame) {
      const end = Math.min(toSample, sample + frame)
      this.socket.send(encodeLiveAudioFrame({ channel: options.channel ?? "system", firstSample: sample + offset, pcm: pcm.subarray(sample * 2, end * 2) }) as Uint8Array<ArrayBuffer>)
      const wait = started + (end - fromSample) * 1000 / RATE / speed - performance.now()
      if (wait > 0) await Bun.sleep(wait)
    }
  }
  mark(kind: "pause" | "resume" | "end", atSample: { mic?: number; system?: number } = {}) { this.socket.send(JSON.stringify({ type: "mark", kind, atSample })) }
  lastAck(channel: "mic" | "system" = "system") { return this.of("ack").filter(ack => ack.channel === channel).at(-1)?.throughSample ?? 0 }
  finals() { return this.of("final") }
  close() { try { this.socket.close() } catch { /* already closed */ } }
  /** Drops the connection without a closing handshake, as a lost network does. */
  drop() { (this.socket as unknown as { terminate(): void }).terminate() }
}
export const transcriptText = (note: NoteDTO, channel?: "mic" | "system") => note.transcriptSegments.filter(segment => !channel || segment.channel === channel).map(segment => segment.text).join(" ")
/** How many times each scripted line appears, word for word, in the stored transcript. */
export const occurrences = (note: NoteDTO, line: string) => transcriptText(note).split(line).length - 1
