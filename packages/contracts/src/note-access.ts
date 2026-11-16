/**
 * Who a note is shared with (`/api/v1/notes/:id/access`, `/api/v1/shared-with-me`).
 *
 * A note belongs to its owner and to one workspace (its organization). Sharing never leaves that workspace:
 *   - people: members of the workspace the owner added by name. Each of them reads the note, as it is now,
 *     from "Shared with me" and from any link to it;
 *   - level: who else a link reaches. "invited": nobody. "organization": every signed-in member of the
 *     workspace who holds a link.
 * There is no level that reaches anyone outside the workspace. A reader gets the title, the owner's notes
 * and the generated notes: never the transcript or the audio, and nothing they can write back. Only the
 * owner changes any of this, and a change counts from the next request.
 */
import { Schema } from "effect"
import { NoteDate, NoteText, NoteTitle, Timestamp, UUID } from "./index"

/** People one request may add, and candidates one search answers with. */
export const MAX_NOTE_ACCESS_ADD = 100
export const MAX_NOTE_ACCESS_CANDIDATES = 20

export const NoteAccessLevel = Schema.Literals(["invited", "organization"]).annotate({ identifier: "NoteAccessLevel" })
export type NoteAccessLevel = typeof NoteAccessLevel.Type

const Picture = Schema.optionalKey(Schema.String.check(Schema.isMaxLength(2048), Schema.isPattern(/^https:\/\//)))
const Email = Schema.String.check(Schema.isMaxLength(254))

/** A member of the note's workspace. `image` is as in `UserDTO`. */
export const NoteAccessPersonDTO = Schema.Struct({
  userId: UUID,
  name: Schema.String,
  email: Schema.String,
  image: Picture,
}).annotate({ identifier: "NoteAccessPersonDTO" })
export type NoteAccessPersonDTO = typeof NoteAccessPersonDTO.Type

/** Someone the note is shared with, and since when. */
export const NoteAccessGrantDTO = Schema.Struct({
  userId: UUID,
  name: Schema.String,
  email: Schema.String,
  image: Picture,
  addedAt: Timestamp,
}).annotate({ identifier: "NoteAccessGrantDTO" })
export type NoteAccessGrantDTO = typeof NoteAccessGrantDTO.Type

/** An attendee of the note's meeting who is a member of the workspace. `hasAccess`: already shared with. */
export const NoteAccessAttendeeDTO = Schema.Struct({
  userId: UUID,
  name: Schema.String,
  email: Schema.String,
  image: Picture,
  hasAccess: Schema.Boolean,
}).annotate({ identifier: "NoteAccessAttendeeDTO" })
export type NoteAccessAttendeeDTO = typeof NoteAccessAttendeeDTO.Type

/** The workspace sharing stays inside. `domain` is the mail domain its people sign in with ("example.com"),
 * when there is one to name; the apps say the workspace's name otherwise. */
export const NoteAccessOrganizationDTO = Schema.Struct({
  id: UUID,
  name: Schema.String,
  domain: Schema.NullOr(Schema.String.check(Schema.isMaxLength(253))),
}).annotate({ identifier: "NoteAccessOrganizationDTO" })
export type NoteAccessOrganizationDTO = typeof NoteAccessOrganizationDTO.Type

/** A note's sharing, as its owner sees it. `organization` is null for a note in no workspace: it cannot be
 * shared. `attendees` are the meeting's attendees who are members (the owner left out): the people "Share
 * with attendees" offers. `suspended`: the owner has put the note away (in Trash); nobody else can read it
 * until it is taken out again. */
export const NoteAccessDTO = Schema.Struct({
  noteId: UUID,
  level: NoteAccessLevel,
  suspended: Schema.Boolean,
  organization: Schema.NullOr(NoteAccessOrganizationDTO),
  owner: NoteAccessPersonDTO,
  people: Schema.Array(NoteAccessGrantDTO),
  attendees: Schema.Array(NoteAccessAttendeeDTO),
}).annotate({ identifier: "NoteAccessDTO" })
export type NoteAccessDTO = typeof NoteAccessDTO.Type

/** People to share with, by account or by address. Every one must be a member of the note's workspace, or
 * the whole request is refused (400) and nobody is added. */
export const AddNoteAccessInput = Schema.Struct({
  userIds: Schema.optionalKey(Schema.Array(UUID).check(Schema.isMaxLength(MAX_NOTE_ACCESS_ADD))),
  emails: Schema.optionalKey(Schema.Array(Email).check(Schema.isMaxLength(MAX_NOTE_ACCESS_ADD))),
}).annotate({ identifier: "AddNoteAccessInput" })
export type AddNoteAccessInput = typeof AddNoteAccessInput.Type

export const UpdateNoteAccessInput = Schema.Struct({
  level: Schema.optionalKey(NoteAccessLevel),
  suspended: Schema.optionalKey(Schema.Boolean),
}).annotate({ identifier: "UpdateNoteAccessInput" })
export type UpdateNoteAccessInput = typeof UpdateNoteAccessInput.Type

/** A member the note could be shared with. `attendee`: they were at the note's meeting. */
export const NoteAccessCandidateDTO = Schema.Struct({
  userId: UUID,
  name: Schema.String,
  email: Schema.String,
  image: Picture,
  attendee: Schema.Boolean,
}).annotate({ identifier: "NoteAccessCandidateDTO" })
export type NoteAccessCandidateDTO = typeof NoteAccessCandidateDTO.Type

/** Members of the note's workspace who do not have access yet. Without a query: the meeting's attendees
 * first, then everyone else. With one: those whose name or address contains it. Never anyone else. */
export const NoteAccessCandidatesDTO = Schema.Struct({
  people: Schema.Array(NoteAccessCandidateDTO),
}).annotate({ identifier: "NoteAccessCandidatesDTO" })
export type NoteAccessCandidatesDTO = typeof NoteAccessCandidatesDTO.Type

/** One row of "Shared with me": a note somebody shared with the reader by name. */
export const SharedWithMeEntryDTO = Schema.Struct({
  id: UUID,
  title: NoteTitle,
  date: NoteDate,
  owner: NoteAccessPersonDTO,
  sharedAt: Timestamp,
}).annotate({ identifier: "SharedWithMeEntryDTO" })
export type SharedWithMeEntryDTO = typeof SharedWithMeEntryDTO.Type

export const SharedWithMeDTO = Schema.Struct({
  notes: Schema.Array(SharedWithMeEntryDTO),
}).annotate({ identifier: "SharedWithMeDTO" })
export type SharedWithMeDTO = typeof SharedWithMeDTO.Type

/** A note shared with the reader, as it is now. No transcript, no audio, nothing that can be written back. */
export const SharedWithMeNoteDTO = Schema.Struct({
  id: UUID,
  title: NoteTitle,
  body: NoteText,
  enhanced: NoteText,
  date: NoteDate,
  duration: Schema.String.check(Schema.isMaxLength(32)),
  owner: NoteAccessPersonDTO,
  sharedAt: Timestamp,
}).annotate({ identifier: "SharedWithMeNoteDTO" })
export type SharedWithMeNoteDTO = typeof SharedWithMeNoteDTO.Type

export const NoteAccessWireSchemas = {
  NoteAccessLevel, NoteAccessPersonDTO, NoteAccessGrantDTO, NoteAccessAttendeeDTO, NoteAccessOrganizationDTO, NoteAccessDTO,
  AddNoteAccessInput, UpdateNoteAccessInput, NoteAccessCandidateDTO, NoteAccessCandidatesDTO,
  SharedWithMeEntryDTO, SharedWithMeDTO, SharedWithMeNoteDTO,
} as const
