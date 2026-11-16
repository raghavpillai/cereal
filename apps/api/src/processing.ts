import { resolve } from "node:path";
import { Effect, Stream } from "effect";
import type {
  ChatInput,
  ChatStreamEventDTO,
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

import {prepareMeetingChat,checkChatCitations,CitationStreamFilter} from "./chat-retrieval";
import type { LiveHub } from "./live/hub";
import { liveChatState } from "./live/chat";
import type { LiveConversationStore } from "./ai/live-conversation";
import { cleanNoteTitle, isUntitled } from "./ai/note-title";
import { audioPrefix, makeStorage, type StorageService } from "./storage";
import type { DeletePreconditions } from "./note-import";
import { makeEvidenceStream } from "./evidence-stream";

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
  return {
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
    evidenceStream: makeEvidenceStream({ getNote, providers, requestSignal: options.requestSignal }),
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
