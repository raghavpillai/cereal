import { createHash } from "node:crypto"
import type { ImportNoteInput } from "@meeting-notes/contracts"

/** The largest import request, in bytes of JSON. The server as a whole accepts a little more (server.ts). */
export const MAX_IMPORT_BYTES = 24 * 1024 * 1024
/** A whole import is one transaction; 50,000 segments take a few seconds, far inside this. */
export const IMPORT_TRANSACTION_MS = 60_000
const EARLIEST_CREATED_AT = Date.UTC(2000, 0, 1)
const CLOCK_TOLERANCE_MS = 5 * 60_000

/** JSON with every object's keys in order and no white space: one text for one value, however it was sent. */
export function canonicalJson(value: unknown): string {
  if (Array.isArray(value)) return "[" + value.map(canonicalJson).join(",") + "]"
  if (value && typeof value === "object") return "{" + Object.entries(value).filter(([, entry]) => entry !== undefined).sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0).map(([key, entry]) => JSON.stringify(key) + ":" + canonicalJson(entry)).join(",") + "}"
  return JSON.stringify(value) ?? "null"
}
/** What ties a repeated import to the request that made the note. */
export const importDigest = (input: ImportNoteInput): string => createHash("sha256").update(canonicalJson(input)).digest("hex")

const bytes = (uuid: string) => Buffer.from(uuid.replaceAll("-", ""), "hex")
/** RFC 4122 version 5: SHA-1 of the namespace's sixteen bytes followed by the name. */
export function uuidV5(namespace: string, name: string): string {
  const hash = createHash("sha1").update(bytes(namespace)).update(name, "utf8").digest().subarray(0, 16)
  hash[6] = (hash[6]! & 0x0f) | 0x50
  hash[8] = (hash[8]! & 0x3f) | 0x80
  const hex = hash.toString("hex")
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`
}
/** A segment's id in an imported note: derived from the note's id and the id the segment had in the request
 * (both in their lower-case text form), so it is the same at every repeat, the client can work it out
 * itself, and no id that exists on this server is ever looked at. */
export const importedSegmentId = (noteId: string, requestSegmentId: string): string => uuidV5(noteId.toLowerCase(), requestSegmentId.toLowerCase())

/** A creation time is carried, within reason: never before 2000, never more than five minutes ahead of this
 * server's clock (a source or device clock that runs a little fast is tolerated). Moved, never refused. */
export const clampCreatedAt = (createdAt: string, now: Date): Date =>
  new Date(Math.min(Math.max(Date.parse(createdAt), EARLIEST_CREATED_AT), now.getTime() + CLOCK_TOLERANCE_MS))

export interface NoteStamp { readonly updatedAt: number; readonly transcriptRevision: number; readonly generatedRevision: number | null }
export interface DeletePreconditions { readonly stamp?: NoteStamp }
/** `updatedAt|transcriptRevision|generatedRevision`, the last part empty (or "null") for a note without
 * generated notes. Null for anything else: a stamp that cannot be read is refused, never ignored. */
export function parseNoteStamp(stamp: string): NoteStamp | null {
  const parts = stamp.split("|")
  if (parts.length !== 3) return null
  const [time, transcript, generated] = parts as [string, string, string]
  const revision = /^(0|[1-9][0-9]{0,9})$/
  if (!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,9})?(?:Z|[+-]\d{2}:\d{2})$/.test(time) || !Number.isFinite(Date.parse(time)) || !revision.test(transcript)) return null
  if (generated !== "" && generated !== "null" && !revision.test(generated)) return null
  return { updatedAt: Date.parse(time), transcriptRevision: Number(transcript), generatedRevision: generated === "" || generated === "null" ? null : Number(generated) }
}
export const matchesStamp = (stamp: NoteStamp, note: { updatedAt: number; transcriptRevision: number; generatedRevision: number | null }) =>
  stamp.updatedAt === note.updatedAt && stamp.transcriptRevision === note.transcriptRevision && stamp.generatedRevision === note.generatedRevision
