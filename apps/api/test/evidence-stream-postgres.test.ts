import { afterAll, describe, expect, test } from "bun:test"
import { Effect } from "effect"
import { makeApiHandler } from "../src/api"
import { makePrismaRepository } from "../src/prisma-repository"
import { makeProcessingService } from "../src/processing"
import type { ProviderServices } from "../src/providers"

const url = process.env.DATABASE_URL
const enabled = process.env.RUN_PG_TESTS === "1" && Boolean(url)

describe.skipIf(!enabled)("streamed source summary on PostgreSQL", () => {
  const resources = enabled ? makePrismaRepository(url!) : null
  const ownerToken = "evidence-stream-postgres-owner-at-least-32-characters"
  // The model names the segment the test puts in `named`: a real one, or one the transcript never had.
  let named = ""
  const providers: ProviderServices = {
    generate: () => Effect.succeed(""), chat: () => Effect.succeed(""),
    async *evidenceStream() { const answer = JSON.stringify({ summary: "Cedar moved to Monday.", segmentIds: [named] }); for (let i = 0; i < answer.length; i += 4) yield answer.slice(i, i + 4) },
  }
  const api = resources ? makeApiHandler(resources.repository, ownerToken, { processing: makeProcessingService(resources.repository, { providers }) }) : null
  const owned: string[] = []
  afterAll(async () => {
    if (resources) for (const id of owned) await Effect.runPromise(resources.repository.deleteNote(id))
    await api?.dispose()
    await resources?.close()
  })
  const call = (path: string, body: unknown) => api!.handler(new Request(`http://localhost${path}`, { method: "POST", headers: { Authorization: `Bearer ${ownerToken}`, "Content-Type": "application/json" }, body: JSON.stringify(body) }))
  const events = (body: string) => body.split("\n\n").map(frame => frame.split("\n").filter(line => line.startsWith("data:")).map(line => line.slice(5).trimStart()).join("\n")).filter(Boolean).map(data => JSON.parse(data))

  test("quotations are the stored segments the model named; an unissued ID ends in error", async () => {
    const segment = { id: crypto.randomUUID(), channel: "system", speakerId: "speaker-4", speakerLabel: "Speaker 4", text: "We moved Cedar to Monday.", startMs: 47_000, endMs: 49_000, isFinal: true }
    const note = await (await call("/api/v1/notes", { title: "Evidence stream fixture", enhanced: "- Cedar moved to Monday", transcriptSegments: [segment] })).json()
    owned.push(note.id)
    named = segment.id
    const list = events(await (await call(`/api/v1/notes/${note.id}/evidence/stream`, { paragraph: "Cedar moved to Monday", expectedTranscriptRevision: note.transcriptRevision })).text())
    expect(list.filter(event => event.type === "delta").map(event => event.text).join("")).toBe("Cedar moved to Monday.")
    expect(list.at(-1)).toEqual({ type: "done", text: "", evidence: { summary: "Cedar moved to Monday.", quotes: [{ segmentId: segment.id, text: segment.text, speakerLabel: "Speaker 4", startMs: 47_000 }], transcriptRevision: note.transcriptRevision } })
    named = crypto.randomUUID()
    const refused = events(await (await call(`/api/v1/notes/${note.id}/evidence/stream`, { paragraph: "Cedar moved to Monday", expectedTranscriptRevision: note.transcriptRevision })).text())
    expect(refused.at(-1)!.type).toBe("error")
    expect(refused.some(event => event.type === "done")).toBe(false)
  })
})
