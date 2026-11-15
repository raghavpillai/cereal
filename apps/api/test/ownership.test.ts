import { expect, test } from "bun:test"
import { Effect, Stream } from "effect"
import { BOOTSTRAP_OWNER_ID, makeMemoryRepository, type StoredShare } from "../src/repository"
import { makeProcessingService } from "../src/processing"

const alice = "10000000-0000-4000-8000-000000000001"
const bob = "10000000-0000-4000-8000-000000000002"
const run = Effect.runPromise

test("account scopes isolate note discovery, mutations, recordings and derived jobs", async () => {
  const root = makeMemoryRepository()
  const a = root.forUser(alice), b = root.forUser(bob)
  const note = await run(a.createNote({ title: "Alice confidential", body: "private project" }))
  expect((await run(a.listNotes())).map(n => n.id)).toEqual([note.id])
  expect(await run(b.listNotes())).toEqual([])
  expect(await run(root.forUser(BOOTSTRAP_OWNER_ID).getNote(note.id))).toBeNull()
  expect(await run(b.getNote(note.id))).toBeNull()
  expect(await run(b.updateNote(note.id, { body: "overwritten" }))).toBeNull()
  expect(await run(b.recording(note.id, "start"))).toBeNull()
  expect(await run(b.beginGeneration(note.id, 0, crypto.randomUUID()))).toBeNull()
  expect(await run(b.finishGeneration(note.id, 0, crypto.randomUUID(), "leak"))).toBeNull()
  const chunk = { id: crypto.randomUUID(), noteId: note.id, channel: "mic" as const,
    sequence: 0, startMs: 0, durationMs: 1000, hash: "a".repeat(64), state: "queued" as const,
    error: null, lease: null, leaseUntil: null }
  expect(await run(b.putAudioChunk(chunk))).toBeNull()
  expect(await run(a.putAudioChunk(chunk))).toEqual(chunk)
  expect(await run(b.listAudioChunks(note.id))).toEqual([])
  expect(await run(b.retryAudioChunk(note.id, chunk.id))).toBeNull()
  // Neither account can run the trusted, global transcription worker APIs.
  expect(await run(a.claimAudioChunk(new Date().toISOString()))).toBeNull()
  expect(await run(b.finishAudioChunk(chunk.id, "forged", [], null))).toBe(false)
  expect(await run(b.deleteNote(note.id))).toBe(false)
  expect((await run(a.getNote(note.id)))?.body).toBe("private project")
  const claimed = await run(root.claimAudioChunk(new Date().toISOString()))
  expect(claimed?.id).toBe(chunk.id)
  expect(await run(root.finishAudioChunk(chunk.id, claimed!.lease!, [], null))).toBe(true)
})

test("share listing and revocation require ownership; public capabilities use only trusted lookup", async () => {
  const root = makeMemoryRepository(), a = root.forUser(alice), b = root.forUser(bob)
  const n = await run(a.createNote({ title: "Alice share" }))
  const now = new Date().toISOString()
  const share: StoredShare = { metadata: { id: crypto.randomUUID(), noteId: n.id, createdAt: now, expiresAt: null, revokedAt: null }, tokenHash: "b".repeat(64),
    snapshot: { shareId: crypto.randomUUID(), title: n.title, body: "", enhanced: "", date: n.date, duration: "", sharedAt: now, expiresAt: null } }
  await run(a.createShare(share))
  expect(await run(b.listShares(n.id))).toEqual([])
  expect(await run(b.revokeShare(share.metadata.id, now))).toBeNull()
  expect((await run(a.listShares(n.id)))[0]?.revokedAt).toBeNull()
  expect((await run(Effect.result(b.createShare({ ...share, tokenHash: "c".repeat(64) }))))._tag).toBe("Failure")
  expect(await run(a.findShare(share.tokenHash))).toBeNull()
  expect((await run(root.findShare(share.tokenHash)))?.snapshot.title).toBe("Alice share")
  expect((await run(a.revokeShare(share.metadata.id, now)))?.revokedAt).toBe(now)
})

test("workspace chat passes only the authenticated account's context to its provider", async () => {
  const root = makeMemoryRepository(), a = root.forUser(alice), b = root.forUser(bob)
  const aliceNote = await run(a.createNote({ title: "Alice private", body: "SECRET_ALICE_CONTENT" }))
  await run(b.createNote({ title: "Bob visible", body: "Bob assigned Friday" }))
  const contexts: string[] = []
  const processing = makeProcessingService(root, { worker: false, providers: {
    transcribe: () => Effect.succeed([]), generate: () => Effect.succeed("generated"), chat: () => Effect.succeed("reply"),
    async *chatStream(note) { contexts.push(note.body); yield "Bob assigned Friday" },
  } })
  const scoped = processing.forRepository(b)
  const stream = await run(scoped.chatStream(null, { message: "Assigned?", history: [] }))
  await run(Stream.runCollect(stream))
  expect(contexts).toHaveLength(1)
  expect(contexts[0]).toContain("Bob assigned Friday")
  expect(contexts[0]).not.toContain("SECRET_ALICE_CONTENT")
  expect((await run(Effect.result(scoped.chat(aliceNote.id, { message: "Read it", history: [] }))))._tag).toBe("Failure")
  expect((await run(Effect.result(scoped.upload(aliceNote.id, crypto.randomUUID(), { channel: "system", sequence: 0, startMs: 0, audioBase64: "" }))))._tag).toBe("Failure")
  await processing.dispose()
})
