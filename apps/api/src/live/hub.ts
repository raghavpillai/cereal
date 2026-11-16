/**
 * The live sessions of this process, by note. The WebSocket route creates and feeds them; Stop ends them
 * and chat consults them.
 *
 * Sessions are in memory only. Another process, or this one after a restart, sees no session: chat then
 * answers from stored segments, and a reconnecting client resumes from the stored ledger.
 *
 * The hub also holds the provider sessions of a live session that ended in an orderly way, for
 * `holdMs`, so that a Resume keeps the provider's speaker identities (link.ts). A held provider session
 * is billed for its keep-alive audio only, but it counts against the provider's limit on concurrent
 * sessions, which is why the hold is bounded and why holds give way when another note needs the room.
 */
import { Effect } from "effect"
import type { LiveHelloMessage, LiveTranscriber } from "@meeting-notes/contracts/live"
import { LiveCloseCode } from "@meeting-notes/contracts/live"
import type { NotesRepositoryService } from "../repository"
import type { ProviderLink } from "./link"
import { LiveProviderError } from "./providers/queue"
import { DEFAULT_LIVE_CONFIG, LiveSession, LiveSessionRefusal, type LiveConfig, type LiveHold, type LiveSnapshot, type LiveSocket, type RecentAudio } from "./session"

export interface LiveHub {
  readonly size: number
  /** True when a new session for this note would exceed `maxSessions`. */
  full(noteId: string): boolean
  /** Attaches a socket to the note's session, creating or resuming it. Throws `LiveSessionRefusal`. */
  connect(noteId: string, hello: LiveHelloMessage, socket: LiveSocket, transcriber: LiveTranscriber): Promise<LiveSession>
  active(noteId: string): boolean
  /** Finalise what has been heard (rate-limited) and wait briefly for it to be stored. */
  flush(noteId: string): Promise<void>
  snapshot(noteId: string): LiveSnapshot | null
  /** Ends the note's session, storing its last finals. Resolves when nothing more will be written. */
  end(noteId: string): Promise<void>
  /** Ends every session. With `abandon` nothing is flushed or stored, as when the process dies. */
  dispose(options?: { abandon?: boolean }): Promise<void>
}

export function makeLiveHub(repository: NotesRepositoryService, options: { config?: Partial<LiveConfig>; maxSessions?: number; /** The limits in force now (the admin panel sets them); read whenever one is applied, so a change needs no restart. */ limits?: () => { maxSessions?: number; holdMs?: number } } = {}): LiveHub & { /** Notes whose provider sessions are being held for a Resume. */ readonly holding: number } {
  const base = { ...DEFAULT_LIVE_CONFIG, ...options.config }
  const limit = () => options.limits?.().maxSessions ?? options.maxSessions ?? 20, holdMs = () => options.limits?.().holdMs ?? base.holdMs
  /** The configuration a session starts with: the hold in force at that moment. */
  const configured = () => ({ ...base, holdMs: holdMs() })
  const sessions = new Map<string, LiveSession>(), connecting = new Map<string, Promise<unknown>>()
  const store = repository.live
  // Provider sessions between two live sessions of a note, oldest note first.
  const held = new Map<string, { links: ProviderLink[]; until: number; keptAt: number }>()
  let tending: ReturnType<typeof setInterval> | undefined
  const release = (noteId: string) => { const entry = held.get(noteId); held.delete(noteId); for (const link of entry?.links ?? []) void link.close(); if (!held.size) { clearInterval(tending); tending = undefined } }
  function tend() {
    const now = performance.now()
    for (const [noteId, entry] of [...held]) {
      entry.links = entry.links.filter(link => !link.lost)
      if (now >= entry.until || !entry.links.length) { release(noteId); continue }
      // The provider closes a session that hears nothing for about 15 seconds.
      if (now - entry.keptAt >= base.keepAliveMs) { entry.keptAt = now; for (const link of entry.links) link.keepAlive() }
    }
  }
  // The newest audio of a note's channels after its live session ended (session.ts, PRIMER_MS), kept as long as a hold.
  const recent = new Map<string, { audio: RecentAudio; until: number }>()
  const holdFor = (noteId: string): LiveHold => ({
    keepAudio(channel, audio) {
      const now = performance.now()
      for (const [key, entry] of recent) if (entry.until < now) recent.delete(key)
      recent.set(`${noteId}:${channel}`, { audio, until: now + Math.max(holdMs(), 300_000) })
    },
    recentAudio(channel) { const entry = recent.get(`${noteId}:${channel}`); return entry && entry.until >= performance.now() ? entry.audio : undefined },
    take(key) {
      const entry = held.get(noteId), link = entry?.links.find(item => item.fits(key))
      if (entry && link) entry.links = entry.links.filter(item => item !== link)
      return link
    },
    park(links) {
      release(noteId)
      const now = performance.now()
      held.set(noteId, { links: [...links], until: now + holdMs(), keptAt: now })
      tending ??= setInterval(tend, Math.max(50, Math.min(500, base.keepAliveMs / 2, Math.max(100, holdMs() / 2))))
    },
  })
  const current = (noteId: string) => { const session = sessions.get(noteId); return session && !session.ended ? session : undefined }
  async function connect(noteId: string, hello: LiveHelloMessage, socket: LiveSocket, transcriber: LiveTranscriber) {
    if (!store) throw new LiveSessionRefusal(LiveCloseCode.providerUnavailable, "Live transcription storage is unavailable")
    const existing = current(noteId)
    if (existing) {
      if (existing.compatible(hello)) { await existing.attach(socket); return existing }
      // One publisher per note. A session that lost its socket belongs to a capture that is over.
      if (existing.attached) throw new LiveSessionRefusal(LiveCloseCode.replaced, "Another device is streaming this recording")
      await existing.end()
    }
    if (sessions.size >= limit()) throw new LiveSessionRefusal(LiveCloseCode.overLimit, "Too many live sessions")
    // A recording that is running matters more than a Resume that may never come.
    for (const other of [...held.keys()]) if (other !== noteId && sessions.size + held.size >= limit()) release(other)
    const begin = () => LiveSession.start({ noteId, hello, transcriber, store, config: configured(), hold: holdFor(noteId),
      onEnded: ended => { if (sessions.get(noteId) === ended) sessions.delete(noteId) },
      transcriptRevision: async () => (await Effect.runPromise(repository.getNote(noteId)))?.transcriptRevision ?? 0 })
    let session: LiveSession
    try {
      session = await begin().catch(error => {
        // The provider is at its limit and other notes' held sessions are part of the count: let them go and try once more.
        if (!(error instanceof LiveProviderError && error.reason === "over_limit") || ![...held.keys()].some(other => other !== noteId)) throw error
        for (const other of [...held.keys()]) if (other !== noteId) release(other)
        return begin()
      })
    } catch (error) {
      release(noteId)
      if (error instanceof LiveSessionRefusal) throw error
      const over = error instanceof LiveProviderError && error.reason === "over_limit"
      throw new LiveSessionRefusal(over ? LiveCloseCode.overLimit : LiveCloseCode.providerUnavailable, over ? "The transcription provider is at its limit" : "The transcription provider is unavailable")
    }
    // What the new session did not take over (another sample rate, a channel it does not send) is of no further use.
    release(noteId)
    sessions.set(noteId, session)
    await session.attach(socket)
    return session
  }
  return {
    get size() { return sessions.size },
    get holding() { return held.size },
    full: noteId => !current(noteId) && sessions.size >= limit(),
    connect(noteId, hello, socket, transcriber) {
      // Two sockets for one note are handled one after the other, so exactly one wins.
      const previous = connecting.get(noteId) ?? Promise.resolve()
      const next = previous.then(() => connect(noteId, hello, socket, transcriber), () => connect(noteId, hello, socket, transcriber))
      const settled = next.catch(() => undefined).finally(() => { if (connecting.get(noteId) === settled) connecting.delete(noteId) })
      connecting.set(noteId, settled)
      return next
    },
    active: noteId => !!current(noteId),
    flush: async noteId => { await current(noteId)?.flushForChat() },
    snapshot: noteId => current(noteId)?.snapshot() ?? null,
    end: async noteId => { await connecting.get(noteId); await sessions.get(noteId)?.endAfterAudio() },
    dispose: async options => {
      if (options?.abandon) for (const session of [...sessions.values()]) session.abandon()
      else await Promise.all([...sessions.values()].map(session => session.end(1001)))
      for (const noteId of [...held.keys()]) release(noteId)
    },
  }
}
