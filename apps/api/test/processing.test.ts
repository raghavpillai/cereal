import { afterEach, expect, test } from "bun:test";
import { Effect } from "effect";
import { mkdtemp, readdir, rm, stat } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { makeMemoryRepository } from "../src/repository";
import { makeApiHandler } from "../src/api";
import { inspectWav, makeProcessingService } from "../src/processing";
import {
  makeProviderServices,
  ProviderFailure,
  type ProviderServices,
} from "../src/providers";

const token = "pipeline-test-owner-token-at-least-32-characters";
const cleanup: Array<() => Promise<unknown>> = [];
afterEach(async () => {
  for (const f of cleanup.splice(0).reverse()) await f();
});
function wav() {
  const b = Buffer.alloc(44 + 3200);
  b.write("RIFF");
  b.writeUInt32LE(b.length - 8, 4);
  b.write("WAVEfmt ", 8);
  b.writeUInt32LE(16, 16);
  b.writeUInt16LE(1, 20);
  b.writeUInt16LE(1, 22);
  b.writeUInt32LE(16000, 24);
  b.writeUInt32LE(32000, 28);
  b.writeUInt16LE(2, 32);
  b.writeUInt16LE(16, 34);
  b.write("data", 36);
  b.writeUInt32LE(3200, 40);
  return b;
}
const upload = (overrides = {}) => ({
  channel: "system",
  sequence: 0,
  startMs: 0,
  audioBase64: wav().toString("base64"),
  ...overrides,
});
async function fixture(overrides: Partial<ProviderServices> = {}) {
  const dir = await mkdtemp(join(tmpdir(), "meeting-pipeline-"));
  cleanup.push(() => rm(dir, { recursive: true, force: true }));
  const repository = makeMemoryRepository();
  let calls = 0;
  const providers: ProviderServices = {
    transcribe: (i) =>
      Effect.sync(() => {
        calls++;
        return [
          {
            id: crypto.randomUUID(),
            channel: i.channel,
            speakerId: `${i.chunkId}:speaker_0`,
            speakerLabel: "Speaker 0",
            text: "Mira will ship the prototype on Friday.",
            isFinal: true,
            startMs: i.startMs,
            endMs: i.startMs + 100,
          },
        ];
      }),
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
    worker: false,
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
  const created = await request("/notes", "POST", {
    title: "Synthetic planning",
  });
  const note = await created.json();
  return {
    dir,
    repository,
    processing,
    providers,
    request,
    note,
    calls: () => calls,
  };
}

test("audio routes authenticate and reject malformed input without creating jobs", async () => {
  const { request, note, dir } = await fixture();
  const p = `/notes/${note.id}/audio/chunks/${crypto.randomUUID()}`;
  expect((await request(p, "PUT", upload(), false)).status).toBe(401);
  expect(
    (await request(p, "PUT", upload({ audioBase64: "@@@@" }))).status,
  ).toBe(400);
  expect(
    (
      await request(
        p,
        "PUT",
        upload({ audioBase64: Buffer.from("not a wav").toString("base64") }),
      )
    ).status,
  ).toBe(400);
  expect((await request(p, "PUT", upload({ startMs: -1 }))).status).toBe(400);
  expect(await readdir(dir)).toEqual([]);
  expect(inspectWav(wav())).toEqual({ durationMs: 100, sampleRate: 16000 });
  const truncated = wav();
  truncated.writeUInt32LE(999999, 40);
  expect(() => inspectWav(truncated)).toThrow();
});

test("durable upload acknowledgement, restart, idempotent append and private cleanup", async () => {
  const { request, note, dir, repository, providers, calls } = await fixture();
  const id = crypto.randomUUID();
  const p = `/notes/${note.id}/audio/chunks/${id}`;
  const first = await (await request(p, "PUT", upload())).json();
  expect(first.state).toBe("queued");
  expect(Object.keys(first).sort()).toEqual(
    ["id", "noteId", "channel", "sequence", "startMs", "durationMs", "state", "error"].sort(),
  );
  expect((await request(p, "PUT", upload())).status).toBe(200);
  const replacement = makeProcessingService(repository, {
    storageDir: dir,
    providers,
    worker: false,
  });
  cleanup.push(replacement.dispose);
  await replacement.runPending();
  expect(calls()).toBe(1);
  const captured = await (await request(`/notes/${note.id}`)).json();
  expect(captured.transcriptRevision).toBe(1);
  expect(captured.transcriptSegments).toHaveLength(1);
  expect((await (await request(p, "PUT", upload())).json()).state).toBe(
    "ready",
  );
  await replacement.runPending();
  expect(calls()).toBe(1);
  const path = join(dir, note.id, (await readdir(join(dir, note.id)))[0]!);
  expect((await stat(path)).mode & 0o777).toBe(0o600);
  expect((await request(`/notes/${note.id}`, "DELETE")).status).toBe(200);
  expect(await readdir(dir)).toEqual([]);
  expect(await Effect.runPromise(repository.listAudioChunks(note.id))).toEqual(
    [],
  );
});

test("conflicting immutable chunk and sequence are rejected", async () => {
  const { request, note } = await fixture();
  const id = crypto.randomUUID(),
    p = `/notes/${note.id}/audio/chunks/${id}`;
  expect((await request(p, "PUT", upload())).status).toBe(200);
  expect((await request(p, "PUT", upload({ startMs: 1 }))).status).toBe(400);
  expect(
    (
      await request(
        `/notes/${note.id}/audio/chunks/${crypto.randomUUID()}`,
        "PUT",
        upload(),
      )
    ).status,
  ).toBe(400);
  const changed = wav();
  changed.writeInt16LE(12, 44);
  expect(
    (
      await request(
        p,
        "PUT",
        upload({ audioBase64: changed.toString("base64") }),
      )
    ).status,
  ).toBe(400);
});

test("Start Stop generate Resume append and regenerate keep revisions and prior transcript", async () => {
  const { request, note, processing } = await fixture();
  expect(
    (await request(`/notes/${note.id}/recording`, "POST", { command: "start" }))
      .status,
  ).toBe(200);
  const p = `/notes/${note.id}/audio/chunks/${crypto.randomUUID()}`;
  await request(p, "PUT", upload());
  await processing.runPending();
  expect(
    (
      await request(`/notes/${note.id}/generate`, "POST", {
        expectedTranscriptRevision: 1,
      })
    ).status,
  ).toBe(400);
  await request(`/notes/${note.id}/recording`, "POST", { command: "stop" });
  const generated = await (
    await request(`/notes/${note.id}/generate`, "POST", {
      expectedTranscriptRevision: 1,
    })
  ).json();
  expect(generated.generatedRevision).toBe(1);
  expect(generated.generationState).toBe("ready");
  await request(`/notes/${note.id}/recording`, "POST", { command: "resume" });
  await request(`/notes/${note.id}/recording`, "POST", { command: "stop" });
  await request(
    `/notes/${note.id}/audio/chunks/${crypto.randomUUID()}`,
    "PUT",
    upload({ sequence: 1, startMs: 100 }),
  );
  expect(
    (
      await request(`/notes/${note.id}/generate`, "POST", {
        expectedTranscriptRevision: 1,
      })
    ).status,
  ).toBe(400);
  await processing.runPending();
  const n = await (await request(`/notes/${note.id}`)).json();
  expect(n.transcriptSegments).toHaveLength(2);
  expect(n.transcriptRevision).toBe(2);
  expect(n.generatedRevision).toBe(1);
  expect(
    (
      await request(`/notes/${note.id}/generate`, "POST", {
        expectedTranscriptRevision: 1,
      })
    ).status,
  ).toBe(400);
  const regenerated = await (
    await request(`/notes/${note.id}/generate`, "POST", {
      expectedTranscriptRevision: 2,
    })
  ).json();
  expect(regenerated.generatedRevision).toBe(2);
  expect(
    (
      await (
        await request(`/notes/${note.id}/chat`, "POST", {
          message: "Who owns the prototype?",
          history: [],
        })
      ).json()
    ).answer,
  ).toContain("Mira");
});

test("provider errors are bounded, failed chunk retries once, no secret crosses API", async () => {
  let fail = true;
  const { request, note, processing } = await fixture({
    transcribe: (i) =>
      fail
        ? Effect.fail(new ProviderFailure("transcription"))
        : Effect.succeed([
            {
              id: crypto.randomUUID(),
              channel: i.channel,
              speakerId: "speaker_0",
              speakerLabel: null,
              text: "Recovered",
              isFinal: true,
              startMs: 0,
              endMs: 100,
            },
          ]),
  });
  const id = crypto.randomUUID();
  await request(`/notes/${note.id}/audio/chunks/${id}`, "PUT", upload());
  await processing.runPending();
  const failed = await (await request(`/notes/${note.id}/audio/chunks`)).json();
  expect(failed[0].state).toBe("failed");
  expect(failed[0].error).toContain("retry");
  fail = false;
  await request(`/notes/${note.id}/audio/chunks/${id}/retry`, "POST");
  await processing.runPending();
  const n = await (await request(`/notes/${note.id}`)).json();
  expect(n.transcriptRevision).toBe(1);
  expect(n.transcriptSegments).toHaveLength(1);
  await request(`/notes/${note.id}/audio/chunks/${id}/retry`, "POST");
  await processing.runPending();
  expect(
    (await (await request(`/notes/${note.id}`)).json()).transcriptRevision,
  ).toBe(1);
});

test("in-flight generation cannot overwrite a resumed note or leave generation stuck", async () => {
  let release!: (x: string) => void;
  const generated = new Promise<string>((resolve) => {
    release = resolve;
  });
  const { request, note, processing } = await fixture({
    generate: () => Effect.promise(() => generated),
  });
  await request(
    `/notes/${note.id}/audio/chunks/${crypto.randomUUID()}`,
    "PUT",
    upload(),
  );
  await processing.runPending();
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

test("expired transcription lease can be recovered; stale worker cannot append", async () => {
  const { request, note, repository, processing } = await fixture();
  const id = crypto.randomUUID();
  await request(`/notes/${note.id}/audio/chunks/${id}`, "PUT", upload());
  const old = await Effect.runPromise(
    repository.claimAudioChunk("2026-01-01T00:00:00Z"),
  );
  expect(old?.state).toBe("processing");
  await processing.runPending();
  expect(
    await Effect.runPromise(
      repository.finishAudioChunk(id, old!.lease!, [], null),
    ),
  ).toBe(false);
  expect(
    (await (await request(`/notes/${note.id}`)).json()).transcriptRevision,
  ).toBe(1);
});

test("ElevenLabs adapter requests diarization and scopes two speakers; OpenRouter keeps credentials server-side", async () => {
  const requests: Array<Request> = [];
  const providers = makeProviderServices({
    elevenLabsKey: "test-hidden-eleven-key",
    openRouterKey: "test-hidden-router-key",
    fetch: (async (input, init) => {
      const request = new Request(input, init);
      requests.push(request);
      if (request.url.includes("elevenlabs"))
        return Response.json({
          words: [
            {
              text: "Hello ",
              start: 0,
              end: 0.04,
              speaker_id: "speaker_0",
              type: "word",
            },
            {
              text: "there",
              start: 0.04,
              end: 0.09,
              speaker_id: "speaker_1",
              type: "word",
            },
          ],
        });
      return Response.json({
        choices: [{ message: { content: "Supported answer" } }],
      });
    }) as typeof fetch,
  });
  const id = crypto.randomUUID();
  const segments = await Effect.runPromise(
    providers.transcribe({
      audio: wav(),
      channel: "system",
      chunkId: id,
      startMs: 1000,
      durationMs: 100,
    }),
  );
  expect(segments).toHaveLength(2);
  expect(segments[0]!.speakerId).toContain(id);
  expect(segments[1]!.startMs).toBe(1040);
  const microphoneSegments = await Effect.runPromise(providers.transcribe({
    audio: wav(), channel: "mic", chunkId: id, startMs: 0, durationMs: 100,
  }));
  expect(new Set(microphoneSegments.map((s) => s.speakerLabel)).size).toBe(2);
  expect(microphoneSegments.every((s) => s.speakerLabel !== "Me")).toBe(true);
  const form = await requests[0]!.formData();
  expect(form.get("diarize")).toBe("true");
  expect(form.get("model_id")).toBe("scribe_v3");
  expect(requests[0]!.headers.get("xi-api-key")).toBe("test-hidden-eleven-key");
  const { repository, note } = await fixture();
  const n = await Effect.runPromise(repository.getNote(note.id));
  expect(
    await Effect.runPromise(
      providers.chat(n!, { message: "Question", history: [] }),
    ),
  ).toBe("Supported answer");
  expect(requests[2]!.headers.get("Authorization")).toBe(
    "Bearer test-hidden-router-key",
  );
});

test("new queued upload invalidates an in-flight generation before transcript revision changes", async () => {
  let release!: (x: string) => void;
  const generated = new Promise<string>((resolve) => {
    release = resolve;
  });
  const { request, note, processing } = await fixture({
    generate: () => Effect.promise(() => generated),
  });
  await request(
    `/notes/${note.id}/audio/chunks/${crypto.randomUUID()}`,
    "PUT",
    upload(),
  );
  await processing.runPending();
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
  await request(
    `/notes/${note.id}/audio/chunks/${crypto.randomUUID()}`,
    "PUT",
    upload({ sequence: 1, startMs: 100 }),
  );
  release("Old incomplete summary");
  expect((await pending).status).toBe(400);
  const n = await (await request(`/notes/${note.id}`)).json();
  expect(n.transcriptRevision).toBe(1);
  expect(n.generatedRevision).toBeNull();
  expect(n.generationState).toBe("idle");
  expect(n.enhanced).toBe("");
});
