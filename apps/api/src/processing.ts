import { resolve } from "node:path";
import { Cause, Effect, Stream } from "effect";
import type {
  ChatInput,
  ChatStreamEventDTO,
  GenerationStreamEventDTO,
  NoteDTO,
  MeetingContextDTO,
} from "@meeting-notes/contracts";
import type { ApiError } from "@meeting-notes/contracts/api";
import { type NotesRepositoryService } from "./repository";
import {
  makeProviderServices,
  type ProviderServices,
  ProviderFailure,
  serializeProviderContext, serializeGenerationContext, providerMeetingContext, type ProviderNote,
} from "./providers";

import {prepareMeetingChat,checkChatCitations,CitationStreamFilter,withoutSourceMarkers} from "./chat-retrieval";
import type { LiveHub } from "./live/hub";
import { liveChatState } from "./live/chat";
import type { LiveConversationStore } from "./ai/live-conversation";
import { cleanNoteTitle, isUntitled } from "./ai/note-title";
import { audioPrefix, makeStorage, type StorageService } from "./storage";
import type { DeletePreconditions } from "./note-import";
import { makeEvidenceStream } from "./evidence-stream";
import { GeneratedTitleReader, splitGeneratedTitle } from "./ai/notes-generation";

const invalid = (message: string): ApiError => ({
  error: "invalid_request",
  message,
});
const missing: ApiError = {
  error: "not_found",
  message: "Note not found",
};
const changedSinceRead: ApiError = {
  error: "conflict",
  message: "The note changed since it was read, so it was not deleted",
};
const unavailable: ApiError = {
  error: "unavailable",
  message: "Processing is temporarily unavailable",
};
const persist = <A>(e: Effect.Effect<A, unknown>) =>
  Effect.mapError(e, () => unavailable);
/** Longest a streamed generation may take, first request to saved notes. */
const GENERATION_DEADLINE_MS = 180_000;
const GENERATION_FAILED = "Notes could not be generated. Try again.";
/** The most reasoning text one answer forwards: what a stored answer can hold (40 stretches of 20,000 characters). Anything after is not sent. */
export const MAX_REASONING_CHARACTERS = 800_000;
function makeProcessingServiceBase(
  repository: NotesRepositoryService,
  options: {
    storageDir?: string;
    /** Where audio parts recorded by an older version are kept (`storage.ts`): removed with their note.
     * New recordings keep no audio on the server. Default: local disk only, in `storageDir`. */
    storage?: StorageService;
    providers?: ProviderServices;
    meetingContext?: (noteId:string)=>Effect.Effect<MeetingContextDTO|null,ApiError>;
    requestSignal?:AbortSignal;
    /** The time a chat request is answered at; tests fix it. */
    clock?:()=>Date;
    /** The live sessions of this process: Stop lets them store their last words, and chat reads their
     * unfinished utterances. The live transcript is the only transcript; nothing is transcribed in batch. */
    live?: LiveHub;
    /** Chat during a recording remembers its threads so each request can repeat the previous one
     * (`ai/live-conversation.ts`): where they are kept (default: one store for the process), and the
     * caller they belong to. */
    liveConversations?: LiveConversationStore;
    actor?: string;
    /** A chat about one note in its labelled form: sources issued for its lines, markers kept, citations checked
     * (`MeetingChatOptions.labelledLines`). The server never sets this: such a chat shows no sources. It lets the
     * citation checks, which chats across notes rely on, be tested against one note. */
    labelledNoteChat?: boolean;
  } = {},
) {
  const providers = options.providers ?? makeProviderServices();
  const root = resolve(
    options.storageDir ?? process.env.AUDIO_STORAGE_DIR ?? ".local/audio",
  );
  const storage = options.storage ?? makeStorage({ audioDir: root, downloadsDir: process.env.DOWNLOADS_DIR || "/data/downloads" });
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
  /** A question that names a chat model the administrator has not enabled is refused before anything is asked or
   * stored. Without a choice of models (`chatModelEnabled` absent) the name is ignored, as it always was. */
  const checkChatModel = (model: string | undefined) => Effect.gen(function* () {
    if (!model || !providers.chatModelEnabled) return;
    const enabled = yield* Effect.tryPromise({ try: () => providers.chatModelEnabled!(model), catch: () => unavailable });
    if (!enabled) return yield* Effect.fail(invalid("Choose an enabled chat model"));
  });
  /** What one generation works from: the note as it was when the generation began, and what the model is shown. */
  type GenerationRun = { note: NoteDTO; source: ProviderNote; enriched: ProviderNote };
  /** The checks every generation starts with, then the claim on the note (`repository.beginGeneration`). */
  const beginGeneration = (id: string, revision: number) =>
    Effect.gen(function* () {
      const source = yield* providerNote(yield* getNote(id));
      if (!source.transcriptSegments.some((s) => s.text.trim()))
        return yield* Effect.fail(
          invalid("This recording has no transcript to write notes from"),
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
            "Stop recording and use the current transcript revision before generating",
          ),
        );
      const enriched: ProviderNote = {...n,...(source.meetingContext?{meetingContext:source.meetingContext}:{})};
      return { run: { note: n, source, enriched } satisfies GenerationRun, runId };
    });
  /** Saves the notes the model wrote. `inlineTitle` is the title line of the same answer, as it came; a
   * note nobody has named whose answer had none is named by one more small request (`ai/note-title.ts`).
   * Neither ever costs the notes: no answer, or one that is not a title, leaves the note untitled. */
  const finishGeneration = (run: GenerationRun, runId: string, document: string, inlineTitle: string | null) =>
    Effect.gen(function* () {
      const { note: n, source, enriched } = run, id = n.id, revision = n.transcriptRevision;
      const title = !isUntitled(n.title) ? null : inlineTitle !== null ? cleanNoteTitle(inlineTitle) : yield* suggestTitle(enriched, document);
      const contextCheck=yield* Effect.result(Effect.flatMap(getNote(id),providerNote));
      if(contextCheck._tag==="Failure"){yield* persist(repository.finishGeneration(id,revision,runId,null));return yield* Effect.fail(contextCheck.failure)}
      const latestContext=contextCheck.success;
      if(JSON.stringify(providerMeetingContext(latestContext.meetingContext))!==JSON.stringify(providerMeetingContext(source.meetingContext))){
        yield* persist(repository.finishGeneration(id,revision,runId,null));
        return yield* Effect.fail(invalid("Meeting context changed during generation; generate again using its current association"));
      }
      const finished = yield* persist(
        // The title is only a suggestion: the write keeps a title the note has been given meanwhile.
        repository.finishGeneration(id, revision, runId, document, title),
      );
      return (
        finished ??
        (yield* Effect.fail(
          invalid(
            "Note inputs or document changed during generation; review your edits and generate again",
          ),
        ))
      );
    });
  return {
    checkChatModel,
    /** With preconditions the note is deleted only if it is still what the caller read (409 otherwise). The
     * audio is removed after the row, so a refused delete leaves the note with all of its recording. */
    deleteNote: (id: string, preconditions?: DeletePreconditions) =>
      Effect.gen(function* () {
        const outcome = preconditions
          ? yield* persist(repository.deleteNoteIf(id, preconditions))
          : (yield* persist(repository.deleteNote(id))) ? ("deleted" as const) : ("missing" as const);
        if (outcome === "conflict") return yield* Effect.fail(changedSinceRead);
        const deleted = outcome === "deleted";
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
    generate: (id: string, revision: number) =>
      Effect.gen(function* () {
        const { run, runId } = yield* beginGeneration(id, revision);
        const result = yield* Effect.result(providers.generate(run.enriched));
        if (result._tag === "Failure") {
          yield* persist(
            repository.finishGeneration(id, revision, runId, null),
          );
          return yield* Effect.fail(unavailable);
        }
        // A note nobody has named is named by the same answer (`ai/notes-generation.ts`, the title line).
        const answer = splitGeneratedTitle(result.success);
        return yield* finishGeneration(run, runId, answer.document, answer.title);
      }),
    /** The same generation, with the notes sent as they are written (POST /api/v1/notes/:id/generate/stream):
     *
     *   title   the name the model gave a note nobody had named (at most one, before the first delta)
     *   delta*  the notes' Markdown, in order
     *   done    the note as saved: its notes are the completed stream plus the chat link line, so a client
     *           replaces what it showed with `note.enhanced`
     *   error   instead of done: nothing was saved, and what was streamed is to be dropped
     *
     * The refusals a buffered generation answers with (no transcript, a stale revision, a recording still
     * going) are answered the same way, before any event. Once the stream has begun, the generation belongs
     * to the server: a client that goes away mid-stream stops receiving, and the notes are still finished
     * and saved, so they are there when it next reads the note. A provider that cannot stream, or refuses to
     * before its first word, is asked the buffered way and its notes arrive as one delta. */
    generateStream: (id: string, revision: number) =>
      Effect.gen(function* () {
        const { run, runId } = yield* beginGeneration(id, revision);
        let listening = true;
        const signal = AbortSignal.timeout(GENERATION_DEADLINE_MS);
        const readable = new ReadableStream<GenerationStreamEventDTO>({
          start(output) { void (async () => {
            const emit = (event: GenerationStreamEventDTO) => { if (listening) try { output.enqueue(event) } catch { listening = false } };
            const close = () => { if (listening) try { output.close() } catch { /* already gone */ } listening = false };
            const fail = async (message: string) => {
              await Effect.runPromise(repository.finishGeneration(id, revision, runId, null)).catch(() => null);
              emit({ type: "error", text: message }); close();
            };
            try {
              const reader = new GeneratedTitleReader();
              let document = "", titled = false;
              const take = (text: string) => {
                if (!titled && reader.title !== null) {
                  titled = true;
                  const title = isUntitled(run.note.title) ? cleanNoteTitle(reader.title) : null;
                  if (title) emit({ type: "title", text: title });
                }
                if (text) { document += text; emit({ type: "delta", text }) }
              };
              let streamed = false;
              if (providers.generateStream) {
                try {
                  for await (const piece of providers.generateStream(run.enriched, signal)) { streamed = true; take(reader.push(piece)) }
                  take(reader.push("", true));
                } catch (error) {
                  // Refused before a word was written (a provider or server that does not stream): ask once
                  // more the buffered way. Once text is on screen, a failure is the generation's failure.
                  if (streamed || signal.aborted) throw error;
                }
              }
              if (!streamed) take(reader.push(await Effect.runPromise(providers.generate(run.enriched), { signal }), true));
              if (!document.trim()) throw new Error("Empty notes");
              const finished = await Effect.runPromiseExit(finishGeneration(run, runId, document, reader.title));
              if (finished._tag === "Failure") {
                const failure = Cause.squash(finished.cause) as Partial<ApiError>;
                emit({ type: "error", text: failure?.error === "invalid_request" && typeof failure.message === "string" ? failure.message : GENERATION_FAILED }); close(); return;
              }
              emit({ type: "done", text: "", note: finished.value }); close();
            } catch {
              await fail(GENERATION_FAILED);
            }
          })(); },
          // The client went away: it stops receiving, and the generation carries on.
          cancel() { listening = false; },
        });
        return Stream.fromReadableStream({ evaluate: () => readable, onError: () => unavailable }).pipe(
          Stream.catch(() => Stream.make({ type: "error" as const, text: GENERATION_FAILED })),
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
    evidenceStream: makeEvidenceStream({ getNote, providers, requestSignal: options.requestSignal }),
    chatStream: (id: string | null, input: ChatInput) => Effect.gen(function*() {
      if(id!==null)yield* getNote(id)
      if(input.timeZone){try{new Intl.DateTimeFormat("en-US",{timeZone:input.timeZone}).format(new Date())}catch{return yield* Effect.fail(invalid("Invalid time zone"))}}
      if (!providers.chatStream) return yield* Effect.fail(unavailable);
      yield* checkChatModel(input.model);
      const abort = new AbortController();
      const signal=AbortSignal.any([abort.signal,AbortSignal.timeout(120_000),...(options.requestSignal?[options.requestSignal]:[])])
      const readable = new ReadableStream<ChatStreamEventDTO>({
        start(output) { void (async () => {
          // The model's reasoning is forwarded as it comes, up to what an answer can hold; the rest is dropped.
          let reasoningLeft=MAX_REASONING_CHARACTERS
          const emit=(event:ChatStreamEventDTO)=>{
            if(signal.aborted)return
            if(event.type==="reasoning"){const text=event.text.slice(0,reasoningLeft);if(!text)return;reasoningLeft-=text.length;output.enqueue({type:"reasoning",text});return}
            output.enqueue(event)
          }
          try {
            // During a recording: have the live session finalise what it has heard, then take its
            // unfinished utterances before the note is read, so a sentence that becomes final in
            // between is seen at least once.
            let snapshot:ReturnType<LiveHub["snapshot"]>=null
            if(id!==null&&options.live?.active(id)){await options.live.flush(id).catch(()=>undefined);snapshot=options.live.snapshot(id)}
            const plain=id!==null&&!options.labelledNoteChat
            const prepared=await prepareMeetingChat({noteId:id,input,repository,providers,signal,now:options.clock?.()??new Date(),...(options.labelledNoteChat?{labelledLines:true}:{}),
              ...(options.liveConversations?{liveConversations:options.liveConversations}:{}),...(options.actor?{conversationScope:options.actor}:{}),
              enrich:async note=>{const enriched=await Effect.runPromise(providerNote(note),{signal});const live=snapshot&&liveChatState(note,snapshot);return live?{...enriched,live}:enriched},emit})
            // A request that knows its own size is measured as sent (a thread on a meeting in progress repeats earlier
            // messages, which the note's JSON form does not show); any other by the note, thread and question.
            const total=prepared.requestCharacters??serializeProviderContext(prepared.note).length+input.history.reduce((sum,message)=>sum+message.content.length,0)+input.message.length
            if(total>500_000)throw new Error("Chat context budget exceeded")
            // A chat about one note shows no sources: any marker the model still writes is removed, asked for or not.
            const filtered=!!input.includeSources||plain
            const filter=new CitationStreamFilter(new Set(prepared.citations.map(citation=>citation.id)),undefined,plain)
            let characters=0,answer=""
            for await (const delta of providers.chatStream!(prepared.note, input, signal, text=>emit({type:"reasoning",text}))) {
              signal.throwIfAborted()
              const text=filtered?filter.push(delta):delta
              characters+=text.length;answer+=text
              if(text)emit({type:"delta",text})
            }
            if(filtered){const tail=filter.push("",true);characters+=tail.length;answer+=tail;if(tail)emit({type:"delta",text:tail})}
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
        yield* checkChatModel(input.model);
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
        return { answer: withoutSourceMarkers(answer) };
      }),
    dispose: async () => {},
  };
}
/** Share provider configuration and storage, never the caller's note scope.
 * Request adapters retain no per-user state. */
export function makeProcessingService(repository: NotesRepositoryService, options: NonNullable<Parameters<typeof makeProcessingServiceBase>[1]> = {}) {
  const configured = { ...options, providers: options.providers ?? makeProviderServices() };
  return {
    ...makeProcessingServiceBase(repository, configured),
    forRepository: (scoped: NotesRepositoryService, context?:{meetingContext?:NonNullable<Parameters<typeof makeProcessingServiceBase>[1]>["meetingContext"];requestSignal?:AbortSignal;actor?:string}) => makeProcessingServiceBase(scoped, { ...configured, ...context }),
  };
}
export type ProcessingService = ReturnType<typeof makeProcessingService>;
