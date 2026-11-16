/**
 * The team space (`/api/v1/team-space`): the notes the members of a workspace have chosen to show to the
 * whole workspace, and the people those meetings were with.
 *
 * A note is in the space because its owner put it there, and for no other reason. While it is there every
 * member of the workspace may read its title, the owner's notes and the generated notes, the same
 * documents a shared link carries; nobody but the owner may change or delete it, and the transcript and
 * the audio stay the owner's. The owner, or an owner or administrator of the workspace, takes it out
 * again, and from that moment it is the owner's alone. Someone who is not a member is told 404 for the
 * space and for everything in it.
 */
import { Schema } from "effect"
import { NoteAttendeesDTO, NoteDate, NoteMeetingTimeDTO, NoteText, NoteTitle, OrganizationRole, Timestamp, UUID } from "./index"

const Count = Schema.Number.check(Schema.isInt32(), Schema.isGreaterThanOrEqualTo(0))
/** Notes one request may put in the space. */
export const MAX_TEAM_SPACE_ADD = 100

/** A member of the workspace, or the owner of a note in its space. `image` is as in `UserDTO`. */
export const TeamSpaceMemberDTO = Schema.Struct({
  userId: UUID,
  name: Schema.String,
  email: Schema.String,
  image: Schema.optionalKey(Schema.String.check(Schema.isMaxLength(2048), Schema.isPattern(/^https:\/\//))),
}).annotate({ identifier: "TeamSpaceMemberDTO" })
export type TeamSpaceMemberDTO = typeof TeamSpaceMemberDTO.Type

/** One row of the space's list. `mine`: the reader owns it (it opens as their own note). `canRemove`: the
 * reader may take it out of the space (its owner, or an owner or administrator of the workspace). */
export const TeamSpaceNoteDTO = Schema.Struct({
  id: UUID,
  title: NoteTitle,
  date: NoteDate,
  createdAt: Timestamp,
  meeting: Schema.NullOr(NoteMeetingTimeDTO),
  attendees: NoteAttendeesDTO,
  owner: TeamSpaceMemberDTO,
  addedAt: Timestamp,
  mine: Schema.Boolean,
  canRemove: Schema.Boolean,
}).annotate({ identifier: "TeamSpaceNoteDTO" })
export type TeamSpaceNoteDTO = typeof TeamSpaceNoteDTO.Type

/** Someone a meeting was with. People are told apart by address, or by name when they have none.
 * `lastNoteDate` is the day of the newest note they are on. */
export const TeamSpacePersonDTO = Schema.Struct({
  name: Schema.String.check(Schema.isMaxLength(254)),
  email: Schema.NullOr(Schema.String.check(Schema.isMaxLength(254))),
  initials: Schema.String.check(Schema.isMaxLength(8)),
  notes: Count,
  lastNoteDate: NoteDate,
  noteIds: Schema.Array(UUID),
}).annotate({ identifier: "TeamSpacePersonDTO" })
export type TeamSpacePersonDTO = typeof TeamSpacePersonDTO.Type

/** The whole page. `notes` are newest meeting first. `people` are everyone who attended a meeting whose note
 * is in the space (`peopleSource` "team"); while the space has nobody to list they are instead the people on
 * the reader's own notes (`peopleSource` "own"), which only the reader is shown. */
export const TeamSpaceDTO = Schema.Struct({
  organization: Schema.Struct({ id: UUID, name: Schema.String }),
  role: OrganizationRole,
  members: Schema.Array(TeamSpaceMemberDTO),
  notes: Schema.Array(TeamSpaceNoteDTO),
  people: Schema.Array(TeamSpacePersonDTO),
  peopleSource: Schema.Literals(["team", "own"]),
}).annotate({ identifier: "TeamSpaceDTO" })
export type TeamSpaceDTO = typeof TeamSpaceDTO.Type

/** A note in the space as a member reads it: the documents a shared link carries, read as they are now.
 * No transcript, no audio, nothing that can be written back. */
export const TeamSpaceNoteContentDTO = Schema.Struct({
  id: UUID,
  title: NoteTitle,
  body: NoteText,
  enhanced: NoteText,
  date: NoteDate,
  duration: Schema.String.check(Schema.isMaxLength(32)),
  owner: TeamSpaceMemberDTO,
  addedAt: Timestamp,
  mine: Schema.Boolean,
}).annotate({ identifier: "TeamSpaceNoteContentDTO" })
export type TeamSpaceNoteContentDTO = typeof TeamSpaceNoteContentDTO.Type

/** Notes of the caller's own to put in the space. One that is not theirs refuses the whole request (404). */
export const AddTeamSpaceNotesInput = Schema.Struct({
  noteIds: Schema.Array(UUID).check(Schema.isMinLength(1), Schema.isMaxLength(MAX_TEAM_SPACE_ADD)),
}).annotate({ identifier: "AddTeamSpaceNotesInput" })
export type AddTeamSpaceNotesInput = typeof AddTeamSpaceNotesInput.Type

export const TeamSpaceWireSchemas = {
  TeamSpaceMemberDTO, TeamSpaceNoteDTO, TeamSpacePersonDTO, TeamSpaceDTO, TeamSpaceNoteContentDTO, AddTeamSpaceNotesInput,
} as const
