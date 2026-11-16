/**
 * The team space: notes their owners show to a whole workspace (see packages/contracts/src/team-space.ts).
 *
 * Every rule about who may see or change what is in `makeTeamSpaceService`, and each one is decided again on
 * every request from what the store says at that moment:
 *   - the reader must be a member of the workspace, or the answer is 404 for the space and everything in it;
 *   - a note is readable only while its row is in that workspace's space, so taking it out, deleting it, or
 *     losing the membership ends access with the next request;
 *   - only a note's owner can put it in; its owner, or an owner or administrator of the workspace, takes it out;
 *   - nothing here writes to a note. Editing and deleting stay behind the owner's own repository.
 */
import { Effect } from "effect"
import type { Database } from "@meeting-notes/database"
import type { MeetingContextDTO } from "@meeting-notes/contracts"
import type { ApiError, PrincipalData } from "@meeting-notes/contracts/api"
import type { AddTeamSpaceNotesInput, TeamSpaceDTO, TeamSpaceMemberDTO, TeamSpaceNoteContentDTO, TeamSpaceNoteDTO, TeamSpacePersonDTO } from "@meeting-notes/contracts/team-space"
import { attendeeSummary, decodeMeeting, initials } from "./note-summaries"
import { resolveNoteOrganization } from "./mcp-store"
import { DEFAULT_ORGANIZATION_ID } from "./organizations"
import { pictureAddress } from "./sign-in"
import { withoutTranscriptLink } from "./transcript-link"
import type { NotesRepositoryService } from "./repository"

type Role = "owner" | "admin" | "member"
export interface TeamSpaceMembership { readonly organizationId: string; readonly organizationName: string; readonly role: Role }
/** A note in a space, without its documents. */
export interface TeamSpaceEntry {
  readonly noteId: string; readonly title: string; readonly date: string; readonly createdAt: string
  readonly owner: TeamSpaceMemberDTO; readonly addedAt: string; readonly meeting: MeetingContextDTO | null
}
export interface TeamSpaceDocument extends TeamSpaceEntry { readonly body: string; readonly enhanced: string; readonly duration: string }
/** Storage only: it answers what it is asked and decides nothing about who may ask. */
export interface TeamSpaceStore {
  /** The workspace whose space this person sees (the one they have selected), if they are a member of any. */
  membership(userId: string): Promise<TeamSpaceMembership | null>
  members(organizationId: string): Promise<ReadonlyArray<TeamSpaceMemberDTO>>
  entries(organizationId: string): Promise<ReadonlyArray<TeamSpaceEntry>>
  /** One note, only if it is in this workspace's space. */
  document(organizationId: string, noteId: string): Promise<TeamSpaceDocument | null>
  /** Puts notes in the space, all or none: false, and nothing written, unless every one is `userId`'s own. */
  add(organizationId: string, userId: string, noteIds: ReadonlyArray<string>, now: Date): Promise<boolean>
  remove(organizationId: string, noteId: string): Promise<boolean>
  /** The saved meetings of the person's own notes, for the People list of a space that has nobody yet. */
  ownMeetings(userId: string): Promise<ReadonlyArray<{ readonly noteId: string; readonly date: string; readonly meeting: MeetingContextDTO | null }>>
}

class TeamSpaceFailure extends Error { constructor(readonly payload: ApiError) { super(payload.message) } }
// One answer for "not a member", "no such note" and "not in the space": it says nothing about what exists.
const missingSpace = () => new TeamSpaceFailure({ error: "not_found", message: "Team space not found" })
const missingNote = () => new TeamSpaceFailure({ error: "not_found", message: "Note not found" })
const boundary = <A>(task: () => Promise<A>) => Effect.tryPromise({ try: task, catch: (error): ApiError => error instanceof TeamSpaceFailure ? error.payload : { error: "unavailable", message: "The team space is temporarily unavailable" } })

const newestFirst = (a: { date: string; createdAt: string; noteId: string }, b: { date: string; createdAt: string; noteId: string }) =>
  b.date.localeCompare(a.date) || b.createdAt.localeCompare(a.createdAt) || b.noteId.localeCompare(a.noteId)

/** Everyone on the given meetings, rooms and equipment left out, newest note first, then by name. */
export function teamSpacePeople(meetings: ReadonlyArray<{ readonly noteId: string; readonly date: string; readonly meeting: MeetingContextDTO | null }>): TeamSpacePersonDTO[] {
  const people = new Map<string, { name: string; email: string | null; noteIds: Set<string>; lastNoteDate: string }>()
  for (const { noteId, date, meeting } of meetings) {
    for (const person of meeting?.participants ?? []) {
      if (person.isResource) continue
      const email = person.email?.trim().toLowerCase() || null
      const name = person.displayName?.trim() || [person.givenName?.trim(), person.familyName?.trim()].filter(Boolean).join(" ") || email
      if (!name) continue
      const key = email ? `e:${email}` : `n:${name.toLowerCase()}`
      const known = people.get(key)
      if (!known) { people.set(key, { name, email, noteIds: new Set([noteId]), lastNoteDate: date }); continue }
      known.noteIds.add(noteId)
      if (date > known.lastNoteDate) known.lastNoteDate = date
      // An address alone is replaced by a real name as soon as one meeting gives it.
      if (known.name === known.email && name !== email) known.name = name
    }
  }
  return [...people.values()]
    .map(person => ({ name: person.name.slice(0, 254), email: person.email?.slice(0, 254) ?? null, initials: initials(person.name).slice(0, 8), notes: person.noteIds.size, lastNoteDate: person.lastNoteDate, noteIds: [...person.noteIds] }))
    .sort((a, b) => b.lastNoteDate.localeCompare(a.lastNoteDate) || a.name.localeCompare(b.name))
}

export interface TeamSpaceService {
  get(principal: PrincipalData): Effect.Effect<TeamSpaceDTO, ApiError>
  add(principal: PrincipalData, input: AddTeamSpaceNotesInput): Effect.Effect<TeamSpaceDTO, ApiError>
  note(principal: PrincipalData, noteId: string): Effect.Effect<TeamSpaceNoteContentDTO, ApiError>
  remove(principal: PrincipalData, noteId: string): Effect.Effect<{ deleted: boolean }, ApiError>
}

export function makeTeamSpaceService(store: TeamSpaceStore, options: { clock?: () => Date } = {}): TeamSpaceService {
  const clock = options.clock ?? (() => new Date())
  const member = async (principal: PrincipalData) => {
    const membership = await store.membership(principal.id)
    if (!membership) throw missingSpace()
    return membership
  }
  const manages = (membership: TeamSpaceMembership) => membership.role === "owner" || membership.role === "admin"
  const page = async (principal: PrincipalData, membership: TeamSpaceMembership): Promise<TeamSpaceDTO> => {
    const [members, entries] = await Promise.all([store.members(membership.organizationId), store.entries(membership.organizationId)])
    const sorted = [...entries].sort(newestFirst)
    const notes: TeamSpaceNoteDTO[] = sorted.map(entry => {
      const mine = entry.owner.userId === principal.id
      return {
        id: entry.noteId, title: entry.title, date: entry.date, createdAt: entry.createdAt,
        meeting: entry.meeting ? { startAt: entry.meeting.startAt, endAt: entry.meeting.endAt, allDay: entry.meeting.allDay } : null,
        attendees: attendeeSummary(entry.meeting), owner: entry.owner, addedAt: entry.addedAt, mine, canRemove: mine || manages(membership),
      }
    })
    const team = teamSpacePeople(sorted)
    // A space nobody has met anyone in yet would show an empty table; the reader's own meetings fill it for them alone.
    const people = team.length ? team : teamSpacePeople(await store.ownMeetings(principal.id))
    return { organization: { id: membership.organizationId, name: membership.organizationName }, role: membership.role, members: [...members], notes, people, peopleSource: team.length ? "team" : "own" }
  }
  return {
    get: principal => boundary(async () => page(principal, await member(principal))),
    add: (principal, input) => boundary(async () => {
      const membership = await member(principal)
      const ids = [...new Set(input.noteIds.map(id => id.toLowerCase()))]
      if (!await store.add(membership.organizationId, principal.id, ids, clock())) throw missingNote()
      return page(principal, membership)
    }),
    note: (principal, noteId) => boundary(async () => {
      const membership = await member(principal)
      const found = await store.document(membership.organizationId, noteId.toLowerCase())
      if (!found) throw missingNote()
      // Without the chat link line, as on a shared link: only the owner can open that conversation.
      return { id: found.noteId, title: found.title, body: found.body, enhanced: withoutTranscriptLink(found.noteId, found.enhanced), date: found.date, duration: found.duration, owner: found.owner, addedAt: found.addedAt, mine: found.owner.userId === principal.id }
    }),
    remove: (principal, noteId) => boundary(async () => {
      const membership = await member(principal)
      const found = await store.document(membership.organizationId, noteId.toLowerCase())
      if (!found) throw missingNote()
      if (found.owner.userId !== principal.id && !manages(membership)) throw new TeamSpaceFailure({ error: "forbidden", message: "Only the note's owner or a workspace administrator can remove it from the team space" })
      return { deleted: await store.remove(membership.organizationId, found.noteId) }
    }),
  }
}

const role = (value: string): Role => value === "owner" || value === "admin" ? value : "member"
const person = (user: { id: string; name: string; email: string; image?: string | null }): TeamSpaceMemberDTO => {
  const image = pictureAddress(user.image)
  return { userId: user.id, name: user.name, email: user.email, ...(image ? { image } : {}) }
}
const day = (date: Date) => date.toISOString().slice(0, 10)

export function makePrismaTeamSpaceStore(db: Database): TeamSpaceStore {
  const owner = { select: { id: true, name: true, email: true, image: true } } as const
  const listed = { id: true, title: true, date: true, createdAt: true, owner, meetingContext: { select: { meeting: true } } } as const
  type Listed = { id: string; title: string; date: Date; createdAt: Date; owner: { id: string; name: string; email: string; image: string | null }; meetingContext: { meeting: unknown } | null }
  const entry = (row: { addedAt: Date; note: Listed }): TeamSpaceEntry => ({
    noteId: row.note.id, title: row.note.title, date: day(row.note.date), createdAt: row.note.createdAt.toISOString(),
    owner: person(row.note.owner), addedAt: row.addedAt.toISOString(), meeting: decodeMeeting(row.note.meetingContext?.meeting),
  })
  return {
    async membership(userId) {
      const organizationId = await resolveNoteOrganization(db, userId)
      if (!organizationId) return null
      const member = await db.member.findUnique({ where: { organizationId_userId: { organizationId, userId } }, include: { organization: { select: { name: true } } } })
      if (!member) return null
      // The workspace every account starts in was never named by anyone: it goes by the instance's name.
      const unnamed = organizationId === DEFAULT_ORGANIZATION_ID && member.organization.name === "Default workspace"
      const organizationName = unnamed ? (await db.instanceSettings.findFirst({ select: { instanceName: true } }))?.instanceName ?? "Cereal" : member.organization.name
      return { organizationId, organizationName, role: role(member.role) }
    },
    async members(organizationId) {
      const members = await db.member.findMany({ where: { organizationId }, include: { user: owner }, orderBy: [{ createdAt: "asc" }, { id: "asc" }], take: 1000 })
      return members.map(member => person(member.user))
    },
    async entries(organizationId) {
      const rows = await db.teamSpaceNote.findMany({ where: { organizationId }, select: { addedAt: true, note: { select: listed } } })
      return rows.map(entry)
    },
    async document(organizationId, noteId) {
      const row = await db.teamSpaceNote.findFirst({ where: { organizationId, noteId }, select: { addedAt: true, note: { select: { ...listed, body: true, enhanced: true, duration: true } } } })
      return row ? { ...entry(row), body: row.note.body, enhanced: row.note.enhanced, duration: row.note.duration } : null
    },
    add: (organizationId, userId, noteIds, now) => db.$transaction(async tx => {
      if (await tx.note.count({ where: { id: { in: [...noteIds] }, ownerId: userId } }) !== noteIds.length) return false
      for (const noteId of noteIds) {
        // A note already in this space keeps the moment it was added; one in another workspace's space moves here.
        const existing = await tx.teamSpaceNote.findUnique({ where: { noteId } })
        if (existing?.organizationId === organizationId) continue
        await tx.teamSpaceNote.upsert({ where: { noteId }, create: { noteId, organizationId, addedById: userId, addedAt: now }, update: { organizationId, addedById: userId, addedAt: now } })
      }
      return true
    }),
    async remove(organizationId, noteId) {
      return (await db.teamSpaceNote.deleteMany({ where: { organizationId, noteId } })).count > 0
    },
    async ownMeetings(userId) {
      const rows = await db.noteMeetingContext.findMany({ where: { userId, note: { ownerId: userId } }, select: { noteId: true, meeting: true, note: { select: { date: true } } }, take: 5000 })
      return rows.map(row => ({ noteId: row.noteId, date: day(row.note.date), meeting: decodeMeeting(row.meeting) }))
    },
  }
}

/** For tests of the HTTP layer without a database: one workspace, whose members are whatever `members` holds
 * at the moment of each request. Notes are read through their owner's own repository. */
export function makeMemoryTeamSpaceStore(repository: NotesRepositoryService, options: {
  organization: { id: string; name: string }
  members: Array<TeamSpaceMemberDTO & { role: Role }>
  meeting?: (ownerId: string, noteId: string) => Promise<MeetingContextDTO | null>
}): TeamSpaceStore {
  const rows = new Map<string, { ownerId: string; addedAt: string }>()
  const meeting = (ownerId: string, noteId: string) => options.meeting ? options.meeting(ownerId, noteId) : Promise.resolve(null)
  const read = async (noteId: string): Promise<TeamSpaceDocument | null> => {
    const row = rows.get(noteId)
    if (!row) return null
    const note = await Effect.runPromise(repository.forUser(row.ownerId).getNote(noteId))
    // A note that was deleted is no longer in the space, as the database's cascade has it.
    if (!note) { rows.delete(noteId); return null }
    const known = options.members.find(member => member.userId === row.ownerId)
    const { role: _role, ...owner } = known ?? { userId: row.ownerId, name: "Former member", email: "", role: "member" as const }
    return { noteId, title: note.title, date: note.date, createdAt: note.createdAt, owner, addedAt: row.addedAt, meeting: await meeting(row.ownerId, noteId), body: note.body, enhanced: note.enhanced, duration: note.duration }
  }
  return {
    async membership(userId) {
      const member = options.members.find(member => member.userId === userId)
      return member ? { organizationId: options.organization.id, organizationName: options.organization.name, role: member.role } : null
    },
    async members() { return options.members.map(({ role: _role, ...member }) => member) },
    async entries() { return (await Promise.all([...rows.keys()].map(read))).flatMap(entry => entry ? [entry] : []) },
    async document(organizationId, noteId) { return organizationId === options.organization.id ? read(noteId) : null },
    async add(_organizationId, userId, noteIds, now) {
      for (const noteId of noteIds) if (!await Effect.runPromise(repository.forUser(userId).getNote(noteId))) return false
      for (const noteId of noteIds) if (!rows.has(noteId)) rows.set(noteId, { ownerId: userId, addedAt: now.toISOString() })
      return true
    },
    async remove(_organizationId, noteId) { return rows.delete(noteId) },
    async ownMeetings(userId) {
      const notes = await Effect.runPromise(repository.forUser(userId).listNotes())
      return Promise.all(notes.map(async note => ({ noteId: note.id, date: note.date, meeting: await meeting(userId, note.id) })))
    },
  }
}
