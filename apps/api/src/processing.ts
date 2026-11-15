import { createHash } from "node:crypto";
import { resolve } from "node:path";
import { Effect, Stream } from "effect";
import type {
  AudioChunkDTO,
  ChatInput,
  ChatStreamEventDTO,
  NoteDTO,
  MeetingContextDTO,
  UploadAudioChunkInput,
} from "@meeting-notes/contracts";
import type { ApiError } from "@meeting-notes/contracts/api";
import {
  type NotesRepositoryService,
  type StoredAudioChunk,
} from "./repository";
import {
  makeProviderServices,
  type ProviderServices,
  ProviderFailure,
  serializeProviderContext, serializeGenerationContext, providerMeetingContext, type ProviderNote,
} from "./providers";

import {prepareMeetingChat,checkChatCitations,CitationStreamFilter} from "./chat-retrieval";
import type { LiveHub } from "./live/hub";
import { COVERAGE_TOLERANCE_MS, overlapMs, type Interval } from "./live/coverage";
import { liveChatState } from "./live/chat";
import type { LiveConversationStore } from "./ai/live-conversation";
import { cleanNoteTitle, isUntitled } from "./ai/note-title";
import { audioKey, audioPrefix, makeStorage, type StorageService } from "./storage";

const invalid = (message: string): ApiError => ({
  error: "invalid_request",
  message,
});
const missing: ApiError = {
  error: "not_found",
  message: "Note or audio chunk not found",
};
const unavailable: ApiError = {
  error: "unavailable",
  message: "Processing is temporarily unavailable",
};
const persist = <A>(e: Effect.Effect<A, unknown>) =>
  Effect.mapError(e, () => unavailable);
const publicChunk = (c: StoredAudioChunk): AudioChunkDTO => ({
  id: c.id,
  noteId: c.noteId,
  channel: c.channel,
  sequence: c.sequence,
  startMs: c.startMs,
  durationMs: c.durationMs,
  state: c.state,
  error: c.error,
});
const maxBytes = 20 * 1024 * 1024;

/** Parse actual PCM WAV chunks, not just a MIME label. Reject malformed/truncated,
 * compressed, multi-channel, or unreasonable metadata before persisting a job. */
export function inspectWav(audio: Uint8Array) {
  if (audio.byteLength < 44 || audio.byteLength > maxBytes)
    throw invalid("Audio must be a WAV file of at most 20 MiB");
  const b = Buffer.from(audio.buffer, audio.byteOffset, audio.byteLength);
  if (
    b.toString("ascii", 0, 4) !== "RIFF" ||
    b.toString("ascii", 8, 12) !== "WAVE" ||
    b.readUInt32LE(4) + 8 !== b.length
  )
    throw invalid("Invalid or unfinished WAV file");
  let rate = 0,
    align = 0,
    bits = 0,
    channels = 0,
    format = 0,
    dataSize = -1;
  for (let offset = 12; offset + 8 <= b.length; ) {
    const size = b.readUInt32LE(offset + 4),
      start = offset + 8,
      end = start + size;
    if (end > b.length) throw invalid("Truncated WAV data");
    const tag = b.toString("ascii", offset, offset + 4);
    if (tag === "fmt ") {
      if (size < 16) throw invalid("Invalid WAV format");
      format = b.readUInt16LE(start);
      channels = b.readUInt16LE(start + 2);
      rate = b.readUInt32LE(start + 4);
      align = b.readUInt16LE(start + 12);
      bits = b.readUInt16LE(start + 14);
      if (b.readUInt32LE(start + 8) !== rate * align)
        throw invalid("Invalid WAV byte rate");
    }
    if (tag === "data") {
      if (dataSize !== -1)
        throw invalid("Multiple WAV data chunks are unsupported");
      dataSize = size;
    }
    offset = end + (size & 1);
  }
  if (
    channels !== 1 ||
    rate < 8000 ||
    rate > 192000 ||
    ![1, 3].includes(format) ||
    !(format === 1 ? [16, 24, 32] : [32, 64]).includes(bits) ||
    align !== (channels * bits) / 8 ||
    dataSize <= 0 ||
    dataSize % align !== 0
  )
    throw invalid("Expected nonempty mono PCM WAV audio");
  const durationMs = Math.round((dataSize / align / rate) * 1000);
  if (durationMs > 60 * 60 * 1000) throw invalid("Audio chunk is too long");
  return { durationMs, sampleRate: rate };
}

function makeProcessingServiceBase(
  repository: NotesRepositoryService,
  options: {
    storageDir?: string;
    /** Where audio parts are kept (`storage.ts`). Default: local disk only, in `storageDir`. */
    storage?: StorageService;
    providers?: ProviderServices;
    worker?: boolean;
    meetingContext?: (noteId:string)=>Effect.Effect<MeetingContextDTO|null,ApiError>;
    requestSignal?:AbortSignal;
    /** The time a chat request is answered at; tests fix it. */
    clock?:()=>Date;
    /** The live sessions of this process: the worker waits for audio they are still transcribing,
     * Stop lets them store their last words, and chat reads their unfinished utterances. */
    live?: LiveHub;
    /** Chat during a recording remembers its threads so each request can repeat the previous one
     * (`ai/live-conversation.ts`): where they are kept (default: one store for the process), and the
     * caller they belong to. */
    liveConversations?: LiveConversationStore;
    actor?: string;
    /** How long a part waits for a live session that is still working on its audio, and how often it looks again. */
    livePendingWaitMs?: number;
    livePendingRetryMs?: number;
  } = {},
) {
  const providers = options.providers ?? makeProviderServices();
  const root = resolve(
    options.storageDir ?? process.env.AUDIO_STORAGE_DIR ?? ".local/audio",
  );
  let closed = false,
    running: Promise<void> | undefined;
  const storage = options.storage ?? makeStorage({ audioDir: root, downloadsDir: process.env.DOWNLOADS_DIR || "/data/downloads" });
  /** One part, under a key made of the note's ID and the audio's own SHA-256: the same audio stored
   * twice is one file, and nothing a client names ever becomes part of a path. */
  const store = (audio: Uint8Array, hash: string, noteId: string) =>
    storage.audio.put(audioKey(noteId, hash), audio, { sha256: hash, contentType: "audio/wav" });
  // Parts held back for a live session, by chunk ID, with the time they were first held.
  const held = new Map<string, number>();
  /** What a claimed part still needs from the batch provider, given what the live path has stored.
   * `false`: nothing to do in this pass. */
  async function coveragePlan(c: StoredAudioChunk): Promise<false | { keep: ReadonlyArray<Interval> | null }> {
    const store = repository.live;
    if (!store) return { keep: null };
    const part = { fromMs: c.startMs, toMs: c.startMs + c.durationMs };
    const pending = options.live?.pendingRange(c.noteId, c.channel);
    if (pending && overlapMs(pending, part) > COVERAGE_TOLERANCE_MS) {
      // The live session has this audio and has not finalised it yet. Look again shortly; give up
      // waiting after a while and let the ledger keep the two paths apart.
      const since = held.get(c.id) ?? Date.now();
      held.set(c.id, since);
      if (Date.now() - since < (options.livePendingWaitMs ?? 40_000)) {
        await Effect.runPromise(store.deferAudioChunk(c.id, c.lease!, new Date(Date.now() + (options.livePendingRetryMs ?? 5000)).toISOString()));
        return false;
      }
    }
    held.delete(c.id);
    const plan = await Effect.runPromise(store.planAudioChunk(c.id, c.lease!));
    // Fully covered parts were marked ready by the plan itself: no provider call.
    if (!plan || plan.kind === "covered") return false;
    return { keep: plan.keep };
  }
  async function drain() {
    while (!closed) {
      const c = await Effect.runPromise(
        repository.claimAudioChunk(new Date().toISOString()),
      );
      if (!c) return;
      try {
        const plan = await coveragePlan(c);
        if (!plan) continue;
        // Wherever the part is: the selected storage, or the other one if it was recorded before a switch.
        const audio = await storage.audio.bytes(audioKey(c.noteId, c.hash));
        if (!audio || createHash("sha256").update(audio).digest("hex") !== c.hash)
          throw new Error("Corrupt audio storage");
        const segments = await Effect.runPromise(
          providers.transcribe({
            audio,
            channel: c.channel,
            chunkId: c.id,
            startMs: c.startMs,
            durationMs: c.durationMs,
            ...(plan.keep ? { keep: plan.keep } : {}),
          }),
        );
        await Effect.runPromise(
          plan.keep
            // Partly covered: keep the words the live path missed. A provider without word times
            // returns the whole part, which then replaces the live text inside it.
            ? repository.finishAudioChunk(c.id, c.lease!, segments, null, segments.limitedToKeep ? { resolution: "mixed" } : { resolution: "batch", replaceLive: true })
            : repository.finishAudioChunk(c.id, c.lease!, segments, null),
        );
      } catch {
        await Effect.runPromise(
          repository.finishAudioChunk(
            c.id,
            c.lease!,
            [],
            "Transcription failed. Check provider configuration and retry.",
          ),
        );
      }
    }
  }
  function kick() {
    if (closed || running || options.worker === false) return;
    running = drain()
      .catch(() => {
        /* Keep errors bounded and secret-free; next poll retries storage access. */
      })
      .finally(() => {
        running = undefined;
      });
  }
  const timer = options.worker === false ? undefined : setInterval(kick, 1000);
  timer?.unref();
  kick();
  const getNote = (id: string) =>
    Effect.gen(function* () {
      const n = yield* persist(repository.getNote(id));
      if (!n) return yield* Effect.fail(missing);
      return n;
    });
  const providerNote = (note:NoteDTO) => Effect.gen(function*(){
    const meeting=options.meetingContext?yield* options.meetingContext(note.id):null
    if(meeting&&JSON.stringify(providerMeetingContext(meeting)).length>100_000)return yield* Effect.fail(invalid("Attached meeting context is too large for processing"))
    return {...note,...(meeting?{meetingContext:meeting}:{})} satisfies ProviderNote
  })
  /** The model's title for a note without one, or null. Never fails, whatever the provider does. The log
   * line names the note and the reason, never the title, the notes or the transcript. */
  const suggestTitle = (note: ProviderNote, notes: string) => {
    if (!providers.title) return Effect.succeed<string | null>(null);
    const skipped = (reason: string) => {
      if (process.env.NODE_ENV !== "test") console.warn(`note title not saved note=${note.id} reason=${reason}`);
      return null;
    };
    return Effect.map(Effect.exit(Effect.suspend(() => providers.title!(note, notes))), (exit) =>
      exit._tag === "Success" ? (cleanNoteTitle(exit.value) ?? skipped("unusable_answer")) : skipped("request_failed"),
    );
  };
  return {
    deleteNote: (id: string) =>
      Effect.gen(function* () {
        const deleted = yield* persist(repository.deleteNote(id));
        if (!deleted) return yield* Effect.fail(missing);
        yield* Effect.tryPromise({
          try: () => storage.audio.deletePrefix(audioPrefix(id)),
          catch: () => unavailable,
        });
        return { deleted };
      }),
    recording: (id: string, command: "start" | "stop" | "resume") =>
      Effect.gen(function* () {
        yield* getNote(id);
        // Stop first lets the live session store its last words, so the transcript the client reads
        // back (and generates notes from) is complete.
        if (command === "stop" && options.live?.active(id))
          yield* Effect.promise(() => options.live!.end(id).catch(() => undefined));
        const n = yield* persist(repository.recording(id, command));
        return (
          n ??
          (yield* Effect.fail(
            invalid("Recording command does not match the current state"),
          ))
        );
      }),
    upload: (noteId: string, id: string, input: UploadAudioChunkInput) =>
      Effect.gen(function* () {
        yield* getNote(noteId);
        if (
          input.audioBase64.length % 4 !== 0 ||
          /[^A-Za-z0-9+/=]/.test(input.audioBase64) ||
          input.audioBase64.slice(0, -2).includes("=")
        )
          return yield* Effect.fail(invalid("Invalid base64 audio"));
        const audio = Buffer.from(input.audioBase64, "base64");
        if (audio.toString("base64") !== input.audioBase64)
          return yield* Effect.fail(invalid("Invalid base64 audio"));
        const wav = yield* Effect.try({
          try: () => inspectWav(audio),
          catch: (e) => e as ApiError,
        });
        if (input.startMs + wav.durationMs > 2147483647)
          return yield* Effect.fail(
            invalid("Audio timeline exceeds supported duration"),
          );
        const hash = createHash("sha256").update(audio).digest("hex");
        yield* Effect.tryPromise({
          try: () => store(audio, hash, noteId),
          catch: () => unavailable,
        });
        const c = yield* persist(
          repository.putAudioChunk({
            id,
            noteId,
            channel: input.channel,
            sequence: input.sequence,
            startMs: input.startMs,
            durationMs: wav.durationMs,
            hash,
            state: "queued",
            error: null,
            lease: null,
            leaseUntil: null,
          }),
        );
        if (!c)
          return yield* Effect.fail(
            invalid(
              "Chunk ID or sequence already exists with different audio or metadata",
            ),
          );
        kick();
        return publicChunk(c);
      }),
    list: (id: string) =>
      Effect.gen(function* () {
        yield* getNote(id);
        return (yield* persist(repository.listAudioChunks(id))).map(
          publicChunk,
        );
      }),
    retry: (noteId: string, id: string) =>
      Effect.gen(function* () {
        yield* getNote(noteId);
        const c = yield* persist(repository.retryAudioChunk(noteId, id));
        if (!c) return yield* Effect.fail(missing);
        kick();
        return publicChunk(c);
      }),
    generate: (id: string, revision: number) =>
      Effect.gen(function* () {
        const source = yield* providerNote(yield* getNote(id));
        if (!source.transcriptSegments.some((s) => s.text.trim()))
          return yield* Effect.fail(
            invalid("Transcribe audio before generating notes"),
          );
        if (serializeGenerationContext(source).length > 500_000)
          return yield* Effect.fail(invalid("Note context is too large for generation"));
        const runId = crypto.randomUUID();
        const n = yield* persist(
          repository.beginGeneration(id, revision, runId),
        );
        if (!n)
          return yield* Effect.fail(
            invalid(
              "Stop recording, finish transcription, and use the current transcript revision before generating",
            ),
          );
        const enriched={...n,...(source.meetingContext?{meetingContext:source.meetingContext}:{})};
        const result = yield* Effect.result(providers.generate(enriched));
        if (result._tag === "Failure") {
          yield* persist(
            repository.finishGeneration(id, revision, runId, null),
          );
          return yield* Effect.fail(unavailable);
        }
        // A note nobody has named is named from its finished notes. This never costs the notes: no
        // answer, or one that is not a title, leaves the note untitled.
        const title = isUntitled(n.title) ? yield* suggestTitle(enriched, result.success) : null;
        const contextCheck=yield* Effect.result(Effect.flatMap(getNote(id),providerNote));
        if(contextCheck._tag==="Failure"){yield* persist(repository.finishGeneration(id,revision,runId,null));return yield* Effect.fail(contextCheck.failure)}
        const latestContext=contextCheck.success;
        if(JSON.stringify(providerMeetingContext(latestContext.meetingContext))!==JSON.stringify(providerMeetingContext(source.meetingContext))){
          yield* persist(repository.finishGeneration(id,revision,runId,null));
          return yield* Effect.fail(invalid("Meeting context changed during generation; generate again using its current association"));
        }
        const finished = yield* persist(
          // The title is only a suggestion: the write keeps a title the note has been given meanwhile.
          repository.finishGeneration(id, revision, runId, result.success, title),
        );
        return (
          finished ??
          (yield* Effect.fail(
            invalid(
              "Note inputs or document changed during generation; review your edits and generate again",
            ),
          ))
        );
      }),
    evidence: (id: string, input: { paragraph: string; expectedTranscriptRevision: number }) => Effect.gen(function*() {
      const note = yield* getNote(id);
      if (note.transcriptRevision !== input.expectedTranscriptRevision) return yield* Effect.fail(invalid("Transcript changed; reopen the explanation"));
      if (!note.transcriptSegments.some(s => s.text.trim())) return { summary: "No supporting transcript is available for this note.", quotes: [], transcriptRevision: note.transcriptRevision };
      if (!providers.evidence) return yield* Effect.fail(unavailable);
      if (note.transcriptSegments.reduce((sum, s) => sum + s.text.length, 0) > 500_000) return yield* Effect.fail(invalid("Transcript is too large for an explanation"));
      const result = yield* Effect.mapError(providers.evidence(note, input.paragraph), () => unavailable);
      const latest = yield* getNote(id);
      if (latest.transcriptRevision !== note.transcriptRevision) return yield* Effect.fail(invalid("Transcript changed; reopen the explanation"));
      const quotes = result.segmentIds.map(segmentId => {
        const segment = note.transcriptSegments.find(s => s.id === segmentId);
        return segment ? { segmentId, text: segment.text, speakerLabel: segment.speakerLabel, startMs: segment.startMs } : undefined;
      });
      if (quotes.some(q => !q) || quotes.length > 8) return yield* Effect.fail(unavailable);
      return { summary: result.summary, quotes: quotes.filter((q): q is NonNullable<typeof q> => q !== undefined), transcriptRevision: note.transcriptRevision };
    }),
    chatStream: (id: string | null, input: ChatInput) => Effect.gen(function*() {
      if(id!==null)yield* getNote(id)
      if(input.timeZone){try{new Intl.DateTimeFormat("en-US",{timeZone:input.timeZone}).format(new Date())}catch{return yield* Effect.fail(invalid("Invalid time zone"))}}
      if (!providers.chatStream) return yield* Effect.fail(unavailable);
      const abort = new AbortController();
      const signal=AbortSignal.any([abort.signal,AbortSignal.timeout(120_000),...(options.requestSignal?[options.requestSignal]:[])])
      const readable = new ReadableStream<ChatStreamEventDTO>({
        start(output) { void (async () => {
          const emit=(event:ChatStreamEventDTO)=>{if(!signal.aborted)output.enqueue(event)}
          try {
            // During a recording: have the live session finalise what it has heard, then take its
            // unfinished utterances before the note is read, so a sentence that becomes final in
            // between is seen at least once.
            let snapshot:ReturnType<LiveHub["snapshot"]>=null
            if(id!==null&&options.live?.active(id)){await options.live.flush(id).catch(()=>undefined);snapshot=options.live.snapshot(id)}
            const prepared=await prepareMeetingChat({noteId:id,input,repository,providers,signal,now:options.clock?.()??new Date(),
              ...(options.liveConversations?{liveConversations:options.liveConversations}:{}),...(options.actor?{conversationScope:options.actor}:{}),
              enrich:async note=>{const enriched=await Effect.runPromise(providerNote(note),{signal});const live=snapshot&&liveChatState(note,snapshot);return live?{...enriched,live}:enriched},emit})
            // A request that knows its own size is measured as sent (a thread on a meeting in progress repeats earlier
            // messages, which the note's JSON form does not show); any other by the note, thread and question.
            const total=prepared.requestCharacters??serializeProviderContext(prepared.note).length+input.history.reduce((sum,message)=>sum+message.content.length,0)+input.message.length
            if(total>500_000)throw new Error("Chat context budget exceeded")
            const filter=new CitationStreamFilter(new Set(prepared.citations.map(citation=>citation.id)))
            let characters=0,answer=""
            for await (const delta of providers.chatStream!(prepared.note, input, signal)) {
              signal.throwIfAborted()
              const text=input.includeSources?filter.push(delta):delta
              characters+=text.length;answer+=text
              if(text)emit({type:"delta",text})
            }
            if(input.includeSources){const tail=filter.push("",true);characters+=tail.length;answer+=tail;if(tail)emit({type:"delta",text:tail})}
            if(!characters)throw new Error("Empty chat answer")
            // The answer exactly as the client received it: the next request of this thread repeats it.
            prepared.commit?.(answer)
            if(input.includeSources){
              // Each citation is judged on its own passage (ai/citations.ts). One whose
              // passage was edited away is dropped and the answer still completes; speech
              // appended while the answer streamed invalidates nothing. The answer is
              // only refused when a cited note can no longer be read at all, or when not
              // one of the passages it relied on is still there.
              const selected=prepared.citations.filter(citation=>filter.used.has(citation.id))
              const {valid:citations,withdrawn}=await checkChatCitations(selected,repository,signal)
              emit({type:"citations",text:"",citations})
              if(withdrawn||selected.length>0&&citations.length===0){
                emit({type:"error",text:"A source changed while answering. Retry to use the current meeting notes."})
                output.close();return
              }
            }
            emit({type:"done",text:""});output.close()
          } catch {
            if(!abort.signal.aborted){output.enqueue({type:"error",text:"Chat could not finish. Please retry."});output.close()}
          }
        })(); },
        cancel() { abort.abort(); },
      });
      return Stream.fromReadableStream({ evaluate: () => readable, onError: () => unavailable }).pipe(
        Stream.ensuring(Effect.sync(()=>abort.abort())),
        Stream.catch(() => Stream.make({ type: "error" as const, text: "Chat could not finish. Please retry." })),
      );
    }),
    chat: (id: string, input: ChatInput) =>
      Effect.gen(function* () {
        const n = yield* providerNote(yield* getNote(id));
        const total =
          serializeProviderContext(n).length +
          input.history.reduce((sum, m) => sum + m.content.length, 0) +
          input.message.length;
        if (total > 500000)
          return yield* Effect.fail(
            invalid("Note context is too large for chat"),
          );
        const answer = yield* Effect.mapError(
          providers.chat(n, input),
          () => unavailable,
        );
        return { answer };
      }),
    runPending: async () => {
      await running;
      await drain();
    },
    dispose: async () => {
      closed = true;
      if (timer) clearInterval(timer);
      await running;
    },
  };
}
/** Share provider configuration and storage, never the caller's note scope.
 * Request adapters do not start background workers or retain per-user state. */
export function makeProcessingService(repository: NotesRepositoryService, options: NonNullable<Parameters<typeof makeProcessingServiceBase>[1]> = {}) {
  const configured = { ...options, providers: options.providers ?? makeProviderServices() };
  return {
    ...makeProcessingServiceBase(repository, configured),
    forRepository: (scoped: NotesRepositoryService, context?:{meetingContext?:NonNullable<Parameters<typeof makeProcessingServiceBase>[1]>["meetingContext"];requestSignal?:AbortSignal;actor?:string}) => makeProcessingServiceBase(scoped, { ...configured, ...context, worker: false }),
  };
}
export type ProcessingService = ReturnType<typeof makeProcessingService>;
