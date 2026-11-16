import { afterEach, expect, test } from "bun:test";
import { Effect } from "effect";
import { mkdir, mkdtemp, readdir, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { makeMemoryRepository } from "../src/repository";
import { makeApiHandler } from "../src/api";
import { makeProcessingService } from "../src/processing";
import {
  makeProviderServices,
  type ProviderServices,
} from "../src/providers";

const token = "pipeline-test-owner-token-at-least-32-characters";
const cleanup: Array<() => Promise<unknown>> = [];
afterEach(async () => {
  for (const f of cleanup.splice(0).reverse()) await f();
});
async function fixture(overrides: Partial<ProviderServices> = {}) {
  const dir = await mkdtemp(join(tmpdir(), "meeting-pipeline-"));
  cleanup.push(() => rm(dir, { recursive: true, force: true }));
  const repository = makeMemoryRepository();
  const providers: ProviderServices = {
    generate: (n) =>
      Effect.succeed(
        `## Decisions\n${n.transcriptSegments.map((s) => s.text).join("\n")}`,
      ),
    chat: () => Effect.succeed("Mira owns the prototype, due Friday."),
    ...overrides,
  };
  const processing = makeProcessingService(repository, {
    storageDir: dir,
    providers,
  });
  const api = makeApiHandler(repository, token, { processing });
  cleanup.push(api.dispose);
  const request = (
    path: string,
    method = "GET",
    body?: unknown,
    owner = true,
  ) =>
    api.handler(
      new Request(`http://localhost/api/v1${path}`, {
        method,
        headers: {
          ...(owner ? { Authorization: `Bearer ${token}` } : {}),
          ...(body === undefined ? {} : { "Content-Type": "application/json" }),
        },
        ...(body === undefined ? {} : { body: JSON.stringify(body) }),
      }),
    );
  // What the live session stores: the only transcript a recording gets.
  const note = await Effect.runPromise(repository.createNote({
    title: "Synthetic planning",
    transcriptSegments: [{ id: crypto.randomUUID(), channel: "mic", speakerId: "mic:n:1", speakerLabel: "Me", text: "Mira will ship the prototype on Friday.", isFinal: true, startMs: 0, endMs: 100 }],
  }));
  return {
    dir,
    repository,
    processing,
    providers,
    request,
    note,
  };
}

test("audio parts are not accepted: there is no upload, list or retry route", async () => {
  const { request, note } = await fixture();
  const part = `/notes/${note.id}/audio/chunks/${crypto.randomUUID()}`;
  for (const [path, method] of [[part, "PUT"], [`/notes/${note.id}/audio/chunks`, "GET"], [`${part}/retry`, "POST"]] as const)
    expect((await request(path, method, method === "PUT" ? { channel: "mic", sequence: 0, startMs: 0, audioBase64: "UklGRg==" } : undefined)).status, `${method} ${path}`).toBe(404);
});

test("deleting a note removes audio an older version stored for it", async () => {
  const { request, note, dir } = await fixture();
  await mkdir(join(dir, note.id), { recursive: true });
  await writeFile(join(dir, note.id, `${"a".repeat(64)}.wav`), new Uint8Array(44));
  expect((await request(`/notes/${note.id}`, "DELETE")).status).toBe(200);
  expect(await readdir(dir)).toEqual([]);
});

test("Stop is followed straight away by generation from the live transcript", async () => {
  const { request, note } = await fixture();
  expect((await request(`/notes/${note.id}/recording`, "POST", { command: "start" })).status).toBe(200);
  expect((await request(`/notes/${note.id}/generate`, "POST", { expectedTranscriptRevision: 1 })).status).toBe(400);
  await request(`/notes/${note.id}/recording`, "POST", { command: "stop" });
  const generated = await (await request(`/notes/${note.id}/generate`, "POST", { expectedTranscriptRevision: 1 })).json();
  expect(generated.generatedRevision).toBe(1);
  expect(generated.generationState).toBe("ready");
  expect(generated.enhanced).toContain("Mira will ship the prototype");
  expect((await (await request(`/notes/${note.id}/chat`, "POST", { message: "Who owns the prototype?", history: [] })).json()).answer).toContain("Mira");
});

test("a recording with no live transcript is refused generation clearly instead of waiting", async () => {
  const { request, repository } = await fixture();
  const empty = await Effect.runPromise(repository.createNote({ title: "No connection" }));
  await request(`/notes/${empty.id}/recording`, "POST", { command: "start" });
  const stopped = await (await request(`/notes/${empty.id}/recording`, "POST", { command: "stop" })).json();
  expect(stopped.recordingState).toBe("stopped");
  expect(stopped.transcriptSegments).toEqual([]);
  const refused = await request(`/notes/${empty.id}/generate`, "POST", { expectedTranscriptRevision: 0 });
  expect(refused.status).toBe(400);
  expect((await refused.json()).message).toBe("This recording has no transcript to write notes from");
  const after = await (await request(`/notes/${empty.id}`)).json();
  expect(after.generationState).toBe("idle");
});

test("in-flight generation cannot overwrite a resumed note or leave generation stuck", async () => {
  let release!: (x: string) => void;
  const generated = new Promise<string>((resolve) => {
    release = resolve;
  });
  const { request, note } = await fixture({
    generate: () => Effect.promise(() => generated),
  });
  await request(`/notes/${note.id}/recording`, "POST", { command: "stop" });
  const pending = request(`/notes/${note.id}/generate`, "POST", {
    expectedTranscriptRevision: 1,
  });
  for (let i = 0; i < 30; i++) {
    if (
      (await (await request(`/notes/${note.id}`)).json()).generationState ===
      "writing_notes"
    )
      break;
    await new Promise((resolve) => setTimeout(resolve, 1));
  }
  await request(`/notes/${note.id}/recording`, "POST", { command: "resume" });
  release("Stale notes");
  expect((await pending).status).toBe(400);
  const n = await (await request(`/notes/${note.id}`)).json();
  expect(n.enhanced).toBe("");
  expect(n.generationState).toBe("idle");
});
