/**
 * Sharing a note with people of its workspace (see packages/contracts/src/note-access.ts).
 *
 * Every rule about who may see or change what is in `makeNoteAccessService`, and each is decided again on
 * every request from what the store says at that moment:
 *   - only the note's owner reads or changes its sharing; to anyone else the note does not exist (404);
 *   - only members of the note's workspace can be given access, by account or by address. One person who is
 *     not a member refuses the whole request, and nobody outside the workspace is ever listed or suggested;
 *   - a person the note is shared with reads it (title, the owner's notes, the generated notes: never the
 *     transcript) while they are still a member, the note is not suspended, and their row is there;
 *   - a link is read by the owner, by the people the note is shared with and, at level "organization" or
 *     while the note is in the workspace's team space, by any member. Never by anyone else;
 *   - nothing here writes to a note. Editing, deleting and sharing stay behind the owner's own repository.
 */
import { Effect } from "effect"
import type { Database } from "@meeting-notes/database"
import type { ApiError, PrincipalData } from "@meeting-notes/contracts/api"
import { personalMailDomain } from "@meeting-notes/contracts/mail-domains"
import { MAX_NOTE_ACCESS_CANDIDATES, type AddNoteAccessInput, type NoteAccessCandidatesDTO, type NoteAccessDTO, type NoteAccessLevel, type NoteAccessPersonDTO, type SharedWithMeDTO, type SharedWithMeNoteDTO, type UpdateNoteAccessInput } from "@meeting-notes/contracts/note-access"
import { decodeMeeting } from "./note-summaries"
import { pictureAddress } from "./sign-in"
import { withoutTranscriptLink } from "./transcript-link"
import type { NotesRepositoryService } from "./repository"

/** A note's sharing as stored. `organizationId` null: the note is in no workspace and cannot be shared. */
export interface NoteSharingState {
  readonly noteId: string; readonly ownerId: string
  readonly organizationId: string | null; readonly organizationName: string
  readonly level: NoteAccessLevel; readonly suspended: boolean
}
export interface NoteAccessDocument { readonly title: string; readonly body: string; readonly enhanced: string; readonly date: string; readonly duration: string }
/** Storage only: it answers what it is asked and decides nothing about who may ask. */
export interface NoteAccessStore {
  /** Any note's sharing, whoever asks. */
  state(noteId: string): Promise<NoteSharingState | null>
  /** Everyone in the workspace, in the order they joined. */
  members(organizationId: string): Promise<ReadonlyArray<NoteAccessPersonDTO>>
  /** The rows of a note, oldest first. People who have since left the workspace may still be among them. */
  grants(noteId: string): Promise<ReadonlyArray<{ readonly userId: string; readonly addedAt: string }>>
  /** The addresses of the people at the note's meeting, as its owner's saved calendar event has them. */
  attendeeEmails(ownerId: string, noteId: string): Promise<ReadonlyArray<string>>
  grant(noteId: string, userIds: ReadonlyArray<string>, addedById: string, now: Date): Promise<void>
  revoke(noteId: string, userId: string): Promise<boolean>
  update(noteId: string, change: { level?: NoteAccessLevel; suspended?: boolean }): Promise<void>
  /** The notes with a row for this person, newest row first. */
  sharedWith(userId: string): Promise<ReadonlyArray<{ readonly noteId: string; readonly addedAt: string }>>
  document(noteId: string): Promise<NoteAccessDocument | null>
  /** Whether the note is in this workspace's team space (team-space.ts): that too lets its members read it. */
  inTeamSpace(noteId: string, organizationId: string): Promise<boolean>
  /** The one mail domain sign-in is limited to ("example.com"), when exactly one is. */
  signInDomain(): Promise<string | null>
}

class NoteAccessFailure extends Error { constructor(readonly payload: ApiError) { super(payload.message) } }
// One answer for "no such note", "not yours" and "not shared with you": it says nothing about what exists.
const missingNote = () => new NoteAccessFailure({ error: "not_found", message: "Note not found" })
const invalid = (message: string) => new NoteAccessFailure({ error: "invalid_request", message })
const boundary = <A>(task: () => Promise<A>) => Effect.tryPromise({ try: task, catch: (error): ApiError => error instanceof NoteAccessFailure ? error.payload : { error: "unavailable", message: "Sharing is temporarily unavailable" } })

const lower = (text: string) => text.trim().toLowerCase()
const domainOf = (email: string) => { const at = email.lastIndexOf("@"); return at < 0 ? "" : lower(email.slice(at + 1)) }
/** The domain a workspace's people sign in with: the one sign-in is limited to, else the one all of its
 * members share, unless that is a mail service anyone can sign up with. Null when there is none to name. */
export function organizationDomain(signInDomain: string | null, members: ReadonlyArray<{ readonly email: string }>): string | null {
  if (signInDomain) return signInDomain
  const domains = new Set(members.map(member => domainOf(member.email)))
  const [only] = domains
  return domains.size === 1 && only && !personalMailDomain(only) ? only : null
}
/** Whether a member answers a search: every word of the query is in their name or address. */
export function matchesMember(person: { readonly name: string; readonly email: string }, query: string): boolean {
  const text = `${person.name} ${person.email}`.toLowerCase()
  return lower(query).split(/\s+/).filter(Boolean).every(word => text.includes(word))
}

export interface NoteAccessService {
  get(principal: PrincipalData, noteId: string): Effect.Effect<NoteAccessDTO, ApiError>
  add(principal: PrincipalData, noteId: string, input: AddNoteAccessInput): Effect.Effect<NoteAccessDTO, ApiError>
  remove(principal: PrincipalData, noteId: string, userId: string): Effect.Effect<NoteAccessDTO, ApiError>
  update(principal: PrincipalData, noteId: string, input: UpdateNoteAccessInput): Effect.Effect<NoteAccessDTO, ApiError>
  candidates(principal: PrincipalData, noteId: string, query: string | undefined): Effect.Effect<NoteAccessCandidatesDTO, ApiError>
  sharedWithMe(principal: PrincipalData): Effect.Effect<SharedWithMeDTO, ApiError>
  read(principal: PrincipalData, noteId: string): Effect.Effect<SharedWithMeNoteDTO, ApiError>
  /** Whether this person may read a link to the note. False says nothing about why. */
  mayReadLink(principal: PrincipalData, noteId: string): Effect.Effect<boolean, ApiError>
}

export function makeNoteAccessService(store: NoteAccessStore, options: { clock?: () => Date } = {}): NoteAccessService {
  const clock = options.clock ?? (() => new Date())
  /** The note, only for its owner. */
  const owned = async (principal: PrincipalData, noteId: string) => {
    const state = await store.state(noteId.toLowerCase())
    if (!state || state.ownerId !== principal.id) throw missingNote()
    return state
  }
  const membersOf = async (state: NoteSharingState) => state.organizationId ? [...await store.members(state.organizationId)] : []
  /** The members among a note's attendees, the owner left out, in the order the workspace lists them. */
  const attendeesAmong = async (state: NoteSharingState, members: ReadonlyArray<NoteAccessPersonDTO>) => {
    const emails = new Set((await store.attendeeEmails(state.ownerId, state.noteId)).map(lower).filter(Boolean))
    return members.filter(member => member.userId !== state.ownerId && emails.has(lower(member.email)))
  }
  const page = async (principal: PrincipalData, state: NoteSharingState): Promise<NoteAccessDTO> => {
    const [members, grants, signInDomain] = await Promise.all([membersOf(state), store.grants(state.noteId), store.signInDomain()])
    const byId = new Map(members.map(member => [member.userId, member]))
    // A row for someone who has left the workspace gives them nothing, so it is not listed either.
    const people = grants.flatMap(grant => { const member = byId.get(grant.userId); return member && member.userId !== state.ownerId ? [{ ...member, addedAt: grant.addedAt }] : [] })
    const shared = new Set(people.map(person => person.userId))
    const image = pictureAddress(principal.image)
    return {
      noteId: state.noteId, level: state.level, suspended: state.suspended,
      organization: state.organizationId ? { id: state.organizationId, name: state.organizationName, domain: organizationDomain(signInDomain, members) } : null,
      owner: byId.get(state.ownerId) ?? { userId: principal.id, name: principal.name, email: principal.email, ...(image ? { image } : {}) },
      people,
      attendees: (await attendeesAmong(state, members)).map(member => ({ ...member, hasAccess: shared.has(member.userId) })),
    }
  }
  /** What lets `userId` read the note, if anything. Decided from the store as it is now. */
  const reach = async (userId: string, noteId: string): Promise<{ state: NoteSharingState; by: "owner" | "person" | "link"; addedAt: string | null } | null> => {
    const state = await store.state(noteId.toLowerCase())
    if (!state) return null
    if (state.ownerId === userId) return { state, by: "owner", addedAt: null }
    if (state.suspended || !state.organizationId) return null
    if (!(await store.members(state.organizationId)).some(member => member.userId === userId)) return null
    const grant = (await store.grants(state.noteId)).find(row => row.userId === userId)
    if (grant) return { state, by: "person", addedAt: grant.addedAt }
    if (state.level === "organization" || await store.inTeamSpace(state.noteId, state.organizationId)) return { state, by: "link", addedAt: null }
    return null
  }
  return {
    get: (principal, noteId) => boundary(async () => page(principal, await owned(principal, noteId))),
    add: (principal, noteId, input) => boundary(async () => {
      const state = await owned(principal, noteId)
      const ids = [...new Set((input.userIds ?? []).map(id => id.toLowerCase()))], emails = [...new Set((input.emails ?? []).map(lower).filter(Boolean))]
      if (!ids.length && !emails.length) throw invalid("Choose someone to share with")
      if (!state.organizationId) throw invalid("This note is not in a workspace, so it cannot be shared")
      const members = await membersOf(state), where = organizationDomain(await store.signInDomain(), members) ?? state.organizationName
      const chosen = new Set<string>()
      for (const id of ids) {
        const member = members.find(candidate => candidate.userId === id)
        // The same words for an account that does not exist and one in another workspace.
        if (!member) throw invalid(`That person is not in ${where}. Notes can only be shared with people in ${where}.`)
        chosen.add(member.userId)
      }
      for (const email of emails) {
        const member = members.find(candidate => lower(candidate.email) === email)
        if (!member) throw invalid(`${email} is not in ${where}. Notes can only be shared with people in ${where}.`)
        chosen.add(member.userId)
      }
      // The owner already has the note.
      chosen.delete(state.ownerId)
      if (chosen.size) await store.grant(state.noteId, [...chosen], principal.id, clock())
      return page(principal, state)
    }),
    remove: (principal, noteId, userId) => boundary(async () => {
      const state = await owned(principal, noteId)
      await store.revoke(state.noteId, userId.toLowerCase())
      return page(principal, state)
    }),
    update: (principal, noteId, input) => boundary(async () => {
      const state = await owned(principal, noteId)
      if (input.level === undefined && input.suspended === undefined) return page(principal, state)
      if (input.level === "organization" && !state.organizationId) throw invalid("This note is not in a workspace, so it cannot be shared")
      await store.update(state.noteId, { ...(input.level === undefined ? {} : { level: input.level }), ...(input.suspended === undefined ? {} : { suspended: input.suspended }) })
      return page(principal, { ...state, level: input.level ?? state.level, suspended: input.suspended ?? state.suspended })
    }),
    candidates: (principal, noteId, query) => boundary(async () => {
      const state = await owned(principal, noteId)
      const members = await membersOf(state), has = new Set((await store.grants(state.noteId)).map(grant => grant.userId))
      const attendees = new Set((await attendeesAmong(state, members)).map(member => member.userId))
      const open = members.filter(member => member.userId !== state.ownerId && !has.has(member.userId))
      const wanted = query?.trim() ? open.filter(member => matchesMember(member, query)) : open
      // Attendees first; within each group the workspace's own order.
      const people = [...wanted.filter(member => attendees.has(member.userId)), ...wanted.filter(member => !attendees.has(member.userId))]
      return { people: people.slice(0, MAX_NOTE_ACCESS_CANDIDATES).map(member => ({ ...member, attendee: attendees.has(member.userId) })) }
    }),
    sharedWithMe: principal => boundary(async () => {
      const notes: Array<SharedWithMeDTO["notes"][number]> = []
      for (const row of await store.sharedWith(principal.id)) {
        const found = await reach(principal.id, row.noteId)
        if (found?.by !== "person") continue
        const document = await store.document(row.noteId)
        if (!document) continue
        const owner = (await membersOf(found.state)).find(member => member.userId === found.state.ownerId)
        notes.push({ id: row.noteId, title: document.title, date: document.date, owner: owner ?? { userId: found.state.ownerId, name: "Former member", email: "" }, sharedAt: row.addedAt })
      }
      return { notes }
    }),
    read: (principal, noteId) => boundary(async () => {
      // By its id a note is read only by someone it was shared with by name: a link level is for links.
      const found = await reach(principal.id, noteId)
      if (found?.by !== "person") throw missingNote()
      const document = await store.document(found.state.noteId)
      if (!document) throw missingNote()
      const owner = (await membersOf(found.state)).find(member => member.userId === found.state.ownerId)
      // Without the chat link line, as on a shared link: only the owner can open that conversation.
      return { id: found.state.noteId, title: document.title, body: document.body, enhanced: withoutTranscriptLink(found.state.noteId, document.enhanced), date: document.date, duration: document.duration, owner: owner ?? { userId: found.state.ownerId, name: "Former member", email: "" }, sharedAt: found.addedAt! }
    }),
    mayReadLink: (principal, noteId) => boundary(async () => await reach(principal.id, noteId) !== null),
  }
}

const person = (user: { id: string; name: string; email: string; image?: string | null }): NoteAccessPersonDTO => {
  const image = pictureAddress(user.image)
  return { userId: user.id, name: user.name, email: user.email, ...(image ? { image } : {}) }
}
const level = (value: string | undefined): NoteAccessLevel => value === "organization" ? "organization" : "invited"

export function makePrismaNoteAccessStore(db: Database, options: { signInDomains?: () => Promise<ReadonlyArray<string>> } = {}): NoteAccessStore {
  return {
    async state(noteId) {
      const note = await db.note.findUnique({ where: { id: noteId }, select: { id: true, ownerId: true, organizationId: true, organization: { select: { name: true } }, sharing: { select: { level: true, suspended: true } } } })
      return note ? { noteId: note.id, ownerId: note.ownerId, organizationId: note.organizationId, organizationName: note.organization?.name ?? "", level: level(note.sharing?.level), suspended: note.sharing?.suspended ?? false } : null
    },
    async members(organizationId) {
      const members = await db.member.findMany({ where: { organizationId, user: { banned: false } }, include: { user: { select: { id: true, name: true, email: true, image: true } } }, orderBy: [{ createdAt: "asc" }, { id: "asc" }], take: 5000 })
      return members.map(member => person(member.user))
    },
    async grants(noteId) {
      const rows = await db.noteAccess.findMany({ where: { noteId }, select: { userId: true, addedAt: true }, orderBy: [{ addedAt: "asc" }, { userId: "asc" }] })
      return rows.map(row => ({ userId: row.userId, addedAt: row.addedAt.toISOString() }))
    },
    async attendeeEmails(ownerId, noteId) {
      const row = await db.noteMeetingContext.findFirst({ where: { noteId, userId: ownerId }, select: { meeting: true } })
      return (decodeMeeting(row?.meeting)?.participants ?? []).filter(participant => !participant.isResource).flatMap(participant => participant.email ? [participant.email] : [])
    },
    async grant(noteId, userIds, addedById, now) {
      // A person already there keeps the moment they were first added.
      await db.noteAccess.createMany({ data: userIds.map(userId => ({ noteId, userId, addedById, addedAt: now })), skipDuplicates: true })
    },
    async revoke(noteId, userId) {
      return (await db.noteAccess.deleteMany({ where: { noteId, userId } })).count > 0
    },
    async update(noteId, change) {
      await db.noteSharing.upsert({ where: { noteId }, create: { noteId, ...change }, update: change })
    },
    async sharedWith(userId) {
      const rows = await db.noteAccess.findMany({ where: { userId }, select: { noteId: true, addedAt: true }, orderBy: [{ addedAt: "desc" }, { noteId: "asc" }], take: 500 })
      return rows.map(row => ({ noteId: row.noteId, addedAt: row.addedAt.toISOString() }))
    },
    async document(noteId) {
      const note = await db.note.findUnique({ where: { id: noteId }, select: { title: true, body: true, enhanced: true, date: true, duration: true } })
      return note ? { title: note.title, body: note.body, enhanced: note.enhanced, date: note.date.toISOString().slice(0, 10), duration: note.duration } : null
    },
    async inTeamSpace(noteId, organizationId) {
      return await db.teamSpaceNote.count({ where: { noteId, organizationId } }) > 0
    },
    async signInDomain() {
      const domains = (await options.signInDomains?.() ?? []).filter(entry => entry.startsWith("@")).map(entry => lower(entry.slice(1)))
      return domains.length === 1 ? domains[0]! : null
    },
  }
}

/** For tests of the HTTP layer without a database: one workspace, whose members are whatever `members` holds
 * at the moment of each request. A member's notes are in that workspace; whose a note is, is asked of each
 * known account's own repository in turn (the memory repository keeps owners to itself). */
export function makeMemoryNoteAccessStore(repository: NotesRepositoryService, options: {
  organization: { id: string; name: string } | null
  members: Array<NoteAccessPersonDTO>
  /** The addresses of the people at a note's meeting. */
  attendees?: (ownerId: string, noteId: string) => Promise<ReadonlyArray<string>>
  inTeamSpace?: (noteId: string) => Promise<boolean>
  signInDomain?: string | null
  /** Accounts that own notes without being members (to test a note in no workspace). */
  outsiders?: ReadonlyArray<string>
}): NoteAccessStore {
  const sharing = new Map<string, { level: NoteAccessLevel; suspended: boolean }>()
  const rows = new Map<string, Array<{ userId: string; addedAt: string }>>()
  const ownerOf = async (noteId: string) => {
    for (const userId of [...options.members.map(member => member.userId), ...(options.outsiders ?? [])]) if (await Effect.runPromise(repository.forUser(userId).getNote(noteId))) return userId
    return null
  }
  return {
    async state(noteId) {
      const ownerId = await ownerOf(noteId)
      // A note that was deleted takes its sharing with it, as the database's cascade has it.
      if (!ownerId) { sharing.delete(noteId); rows.delete(noteId); return null }
      const inside = options.organization && options.members.some(member => member.userId === ownerId)
      return { noteId, ownerId, organizationId: inside ? options.organization!.id : null, organizationName: inside ? options.organization!.name : "", level: sharing.get(noteId)?.level ?? "invited", suspended: sharing.get(noteId)?.suspended ?? false }
    },
    async members(organizationId) { return organizationId === options.organization?.id ? [...options.members] : [] },
    async grants(noteId) { return [...rows.get(noteId) ?? []] },
    async attendeeEmails(ownerId, noteId) { return options.attendees ? options.attendees(ownerId, noteId) : [] },
    async grant(noteId, userIds, _addedById, now) {
      const list = rows.get(noteId) ?? []
      for (const userId of userIds) if (!list.some(row => row.userId === userId)) list.push({ userId, addedAt: now.toISOString() })
      rows.set(noteId, list)
    },
    async revoke(noteId, userId) {
      const list = rows.get(noteId) ?? [], kept = list.filter(row => row.userId !== userId)
      rows.set(noteId, kept)
      return kept.length !== list.length
    },
    async update(noteId, change) { sharing.set(noteId, { level: "invited", suspended: false, ...sharing.get(noteId), ...change }) },
    async sharedWith(userId) {
      return [...rows.entries()].flatMap(([noteId, list]) => list.filter(row => row.userId === userId).map(row => ({ noteId, addedAt: row.addedAt }))).sort((a, b) => b.addedAt.localeCompare(a.addedAt))
    },
    async document(noteId) {
      const note = await Effect.runPromise(repository.getNote(noteId))
      return note ? { title: note.title, body: note.body, enhanced: note.enhanced, date: note.date, duration: note.duration } : null
    },
    async inTeamSpace(noteId) { return options.inTeamSpace ? options.inTeamSpace(noteId) : false },
    async signInDomain() { return options.signInDomain ?? null },
  }
}
