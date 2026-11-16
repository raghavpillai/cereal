import { Effect } from "effect"
import { mkdtemp, rm } from "node:fs/promises"
import { join } from "node:path"
import { tmpdir } from "node:os"
import type { ImportNoteInput, MeetingContextDTO } from "@meeting-notes/contracts"
import type { PrincipalData } from "@meeting-notes/contracts/api"
import type { Database } from "@meeting-notes/database"
import { makeApiHandler } from "../src/api"
import { makeMemoryRepository } from "../src/repository"
import { makePrismaRepository } from "../src/prisma-repository"
import { makeProcessingService } from "../src/processing"
import { makeAdministrationService } from "../src/administration"
import { makeMemoryCalendarStore, makePrismaCalendarStore, type CalendarStore } from "../src/calendar-store"
import { makeMemoryGoogleStore, makePrismaGoogleStore } from "../src/google-store"
import { makeGoogleService } from "../src/google"
import { makeMemoryNoteParticipantsService, makeNoteParticipantsService } from "../src/note-participants"
import { parseCalendarFeed } from "../src/calendar-feed"
import { secretBoxFromKey } from "../src/admin-secrets"

/** The PostgreSQL suites run only against a throwaway database named mn_test_<hex>. */
export const postgresEnabled = process.env.RUN_PG_TESTS === "1" && !!process.env.DATABASE_URL && /^\/mn_test_[a-f0-9]+$/.test(new URL(process.env.DATABASE_URL).pathname)
export type Kind = "memory" | "postgres"
export type Account = "alice" | "bob"

/**
 * The whole API over HTTP (`api.handler`), with two signed-in people, shared ICS calendars, the meeting
 * context store and the attendee service: in memory, or on PostgreSQL through Prisma exactly as server.ts
 * wires it. Every request goes through the typed routes, so schema limits and the import's own size limit
 * are exercised.
 */
export async function stack(kind: Kind, options: { clock?: () => Date } = {}) {
  const cleanup: Array<() => Promise<unknown>> = []
  const dir = await mkdtemp(join(tmpdir(), "meeting-import-"))
  cleanup.push(() => rm(dir, { recursive: true, force: true }))
  const ids: Record<Account, string> = { alice: crypto.randomUUID(), bob: crypto.randomUUID() }
  const resources = kind === "postgres" ? makePrismaRepository(process.env.DATABASE_URL!) : null
  const repository = resources?.repository ?? makeMemoryRepository([], options.clock)
  const db: Database | null = resources?.database ?? null
  if (db) {
    for (const [name, id] of Object.entries(ids)) await db.user.create({ data: { id, name, email: `${name}-${id}@example.test`, role: "user" } })
    cleanup.push(async () => { await db.calendarSubscription.deleteMany({ where: { sourceHost: `import-${ids.alice}.example.test` } }); await db.user.deleteMany({ where: { id: { in: Object.values(ids) } } }); await resources!.close() })
  }
  const shared: CalendarStore = db ? makePrismaCalendarStore(db) : makeMemoryCalendarStore(repository)
  const contexts = db ? makePrismaGoogleStore(db) : makeMemoryGoogleStore(repository, shared)
  const administration = makeAdministrationService(shared, undefined, { clock: options.clock })
  const google = makeGoogleService(contexts, secretBoxFromKey(Buffer.alloc(32, 3)), { clientId: "client", clientSecret: "secret", baseURL: "https://notes.example.test", ...(options.clock ? { clock: options.clock } : {}) })
  const processing = makeProcessingService(repository, { storageDir: dir, providers: {
    generate: () => Effect.succeed("## Generated"), chat: () => Effect.succeed("answer"),
  } })
  const token = "import-test-bootstrap-token-at-least-32-characters"
  const api = makeApiHandler(repository, token, {
    processing, administration, google, ...(options.clock ? { clock: options.clock } : {}),
    noteParticipants: db ? makeNoteParticipantsService(db) : makeMemoryNoteParticipantsService(repository, google),
    principalResolver: async headers => {
      const account = (headers.get("authorization") ?? "").replace("Bearer ", "") as Account
      return account in ids ? { id: ids[account], name: account, email: `${account}@example.test`, role: "user", source: "session", expiresAt: "2099-01-01T00:00:00Z", createdAt: "2026-01-01T00:00:00Z" } satisfies PrincipalData : null
    },
  })
  cleanup.unshift(api.dispose)
  const send = (account: Account | null, path: string, method = "GET", body?: unknown) => api.handler(new Request(`http://localhost/api/v1${path}`, {
    method, headers: { ...(account ? { Authorization: `Bearer ${account}` } : {}), ...(body === undefined ? {} : { "Content-Type": "application/json" }) },
    ...(body === undefined ? {} : { body: typeof body === "string" ? body : JSON.stringify(body) }),
  }))
  /** Status and parsed JSON of one request. */
  const call = async (account: Account | null, path: string, method = "GET", body?: unknown) => {
    const response = await send(account, path, method, body), text = await response.text()
    let json: any = null
    try { json = JSON.parse(text) } catch { /* not JSON */ }
    return { status: response.status, json, text }
  }
  /** A shared calendar with the given events, as a refresh would store them. */
  const calendar = async (source: string, now: Date) => {
    const subscription = await shared.createCalendar({ name: "Team", provider: "ics", color: "#87e5e8", enabled: true, encryptedURL: "sealed-" + crypto.randomUUID(), sourceHost: `import-${ids.alice}.example.test` })
    const events = await parseCalendarFeed(subscription.id, source, now)
    await shared.replaceEvents(subscription.id, subscription.encryptedURL, events, now.toISOString())
    return events
  }
  return {
    kind, ids, db, repository, call, send, calendar, processing, dir, sendRaw: (request: Request) => api.handler(request),
    /** A stretch of recording: the words the live session stores for it. */
    record: async (_account: Account, noteId: string, sequence = 0) => {
      const startMs = sequence * 100
      await Effect.runPromise(repository.live!.append({ noteId, sessionId: crypto.randomUUID(), channel: "system", speakers: {}, coverage: [{ fromMs: startMs, toMs: startMs + 100 }],
        turns: [{ speakerKey: "speaker_0", words: [{ text: "Recorded words ", startMs, endMs: startMs + 100 }] }] }))
    },
    close: async () => { for (const dispose of cleanup) await dispose().catch(() => undefined) },
  }
}
export type Stack = Awaited<ReturnType<typeof stack>>

export const participant = (name: string, email: string | null, extra: Partial<MeetingContextDTO["participants"][number]> = {}): MeetingContextDTO["participants"][number] => ({
  id: crypto.randomUUID(), email, displayName: name, givenName: null, familyName: null, nameSource: "calendar", isSelf: false, isOrganizer: false, isResource: false, responseStatus: "accepted", ...extra,
})
export const meeting = (overrides: Partial<MeetingContextDTO> = {}): MeetingContextDTO => ({
  eventId: crypto.randomUUID(), provider: "google", title: "Q4 planning", calendarName: "Work",
  startAt: "2026-11-12T15:00:00.000Z", endAt: "2026-11-12T16:00:00.000Z", allDay: false, timeZone: "America/New_York",
  location: "Room 4", meetingUrl: "https://meet.google.com/abc-defg-hij", eventUrl: "https://www.google.com/calendar/event?eid=abc",
  organizer: participant("Raghav", "raghav@acme.test", { isOrganizer: true, isSelf: true }),
  participants: [participant("Raghav", "raghav@acme.test", { isOrganizer: true, isSelf: true }), participant("Ada Lovelace", "ada@acme.test"), participant("Grace Hopper", "grace@acme.test", { responseStatus: "tentative" }), participant("Room 4", "room4@acme.test", { isResource: true })],
  ...overrides,
})
export const segment = (index: number, text = `Line ${index}: we agreed to ship the importer on Friday.`): ImportNoteInput["transcriptSegments"][number] => ({
  id: crypto.randomUUID(), channel: index % 2 ? "mic" : "system", speakerId: `speaker_${index % 3}`, speakerLabel: index % 3 ? `Speaker ${index % 3}` : null,
  text, isFinal: true, startMs: index * 1000, endMs: index % 7 ? index * 1000 + 900 : null,
})
export const importBody = (overrides: Partial<ImportNoteInput> = {}): ImportNoteInput => ({
  id: crypto.randomUUID(), title: "Q4 planning", body: "My own notes\n\n- one\n- two", enhanced: "## Decisions\n\nShip on Friday.", date: "2026-11-12", duration: "58:04",
  createdAt: "2026-11-12T15:01:02.345Z", generated: true, recordingState: "stopped",
  transcriptSegments: [0, 1, 2, 3].map(index => segment(index)), meeting: meeting(), ...overrides,
})
