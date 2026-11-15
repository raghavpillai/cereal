import {transcribeOpenAi} from "./openai-transcription"
import {languageTransport} from "./language-transport"
import { Context, Effect } from "effect";
import type {
  ChatInput,
  NoteDTO,
  MeetingContextDTO,
  TranscriptSegmentDTO,
  ChatCitationDTO,
} from "@meeting-notes/contracts";
import { completionSettings, DEFAULT_LANGUAGE_PARAMETERS, type LanguageRequestParameters } from "./ai/shared";
import { chatAnswerMessages, chatAnswerParameters, chatFeature, chatLayout } from "./ai";
import { cachedRequest, logLanguageUsage, readUsage, type LanguageUsage, type LanguageUsageEvent } from "./ai/prompt-cache";
import { generationMessages, GENERATION_PARAMETERS } from "./ai/notes-generation";
import { evidenceMessages, EVIDENCE_PARAMETERS } from "./ai/evidence";
import { PLANNER_PARAMETERS } from "./ai/workspace-chat";

export class ProviderFailure extends Error {
  readonly _tag = "ProviderFailure";
  constructor(
    readonly service: "transcription" | "language",
    readonly reason:
      | "not_configured"
      | "unavailable"
      | "invalid_response" = "unavailable",
  ) {
    super(`${service} ${reason}`);
  }
}
export type ProviderNote = NoteDTO & { readonly meetingContext?:MeetingContextDTO|null;readonly citations?:ReadonlyArray<ChatCitationDTO>;readonly retrievalCoverage?:{bodyTruncated:boolean;summaryTruncated:boolean;transcriptTruncated:boolean};readonly chatScope?:"workspace" }
export interface ChatToolCall {id:string;name:string;arguments:string}
export interface ChatToolMessage {role:"system"|"user"|"assistant"|"tool";content:string|null;
  /** This message ends a prefix other requests repeat (`ai/prompt-cache.ts`). Never sent as such. */
  cache?:true;tool_call_id?:string;tool_calls?:ReadonlyArray<{id:string;type:"function";function:{name:string;arguments:string}}>}
export interface ChatToolDefinition {type:"function";function:{name:string;description:string;parameters:Record<string,unknown>}}

/** A batch result. `limitedToKeep` is set by a provider that honoured `keep`; one that cannot (no word
 * times) returns the whole part and leaves it unset, and the worker then replaces the live text instead. */
export type TranscriptionResult = ReadonlyArray<TranscriptSegmentDTO> & { readonly limitedToKeep?: boolean };
export interface ProviderServices {
  readonly transcribe: (input: {
    audio: Uint8Array;
    channel: "mic" | "system";
    chunkId: string;
    startMs: number;
    durationMs: number;
    /** Note-timeline ranges the live path has not covered. When given, only words whose midpoint falls
     * inside one of them belong in the result. */
    keep?: ReadonlyArray<{ readonly fromMs: number; readonly toMs: number }>;
  }) => Effect.Effect<TranscriptionResult, ProviderFailure>;
  readonly generate: (note: ProviderNote) => Effect.Effect<string, ProviderFailure>;
  readonly chat: (
    note: ProviderNote,
    input: ChatInput,
  ) => Effect.Effect<string, ProviderFailure>;
  readonly chatStream?: (note: ProviderNote, input: ChatInput, signal: AbortSignal) => AsyncIterable<string>;
  readonly evidence?: (note: ProviderNote, paragraph: string) => Effect.Effect<{ summary: string; segmentIds: ReadonlyArray<string> }, ProviderFailure>;
  readonly chatToolStep?: (messages:ReadonlyArray<ChatToolMessage>,tools:ReadonlyArray<ChatToolDefinition>,signal:AbortSignal,model?:string)=>Promise<ReadonlyArray<ChatToolCall>>;
}
export class Providers extends Context.Service<Providers, ProviderServices>()(
  "MeetingNotesProviders",
) {}

const object = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v);
// Prompt text, context serialisation and request parameters live in ./ai, one
// module per feature. Re-exported for importers.
export { providerMeetingContext, serializeProviderContext } from "./ai/shared";
export { serializeGenerationContext } from "./ai/notes-generation";

/** Credentials remain in server closure and are never sent to native/web clients or error payloads. */
export function makeProviderServices(
  options: {
    elevenLabsKey?: string;
    openRouterKey?: string;
    openAiKey?:string;
    anthropicKey?:string;
    languageProvider?:"openrouter"|"openai"|"anthropic";
    transcriptionProvider?:"elevenlabs"|"openai";
    transcriptionModel?: string;
    languageModel?: string;
    toolModel?:string;
    fetch?: typeof fetch;
    /** Receives the provider's token usage for every streamed chat answer. Default: one log line (`ai/prompt-cache.ts`). */
    onLanguageUsage?: (event: LanguageUsageEvent) => void;
  } = {},
): ProviderServices {
  const rawTransport = options.fetch ?? globalThis.fetch;
  const languageProvider=options.languageProvider??"openrouter";
  const openAiKey=options.openAiKey??process.env.OPENAI_API_KEY;
  const languageKey=languageProvider==="openai"?openAiKey:languageProvider==="anthropic"?options.anthropicKey??process.env.ANTHROPIC_API_KEY:options.openRouterKey??process.env.OPENROUTER_API_KEY;
  const transport = languageTransport(rawTransport,languageProvider,languageKey??"");
  const elevenKey = options.elevenLabsKey ?? process.env.ELEVENLABS_API_KEY;
  const routerKey = languageKey;
  const transcribe: ProviderServices["transcribe"] = (input) =>
    Effect.tryPromise({
      async try() {
        if(options.transcriptionProvider==="openai")return transcribeOpenAi(input,options.transcriptionModel??"gpt-transcribe",openAiKey??"",rawTransport)
        if (!elevenKey)
          throw new ProviderFailure("transcription", "not_configured");
        const form = new FormData();
        form.set(
          "model_id",
          options.transcriptionModel ??
            process.env.ELEVENLABS_STT_MODEL ??
            "scribe_v3",
        );
        form.set("diarize", "true");
        form.set("language_code", "en");
        form.set("timestamps_granularity", "word");
        // Each upload is one source, which can contain multiple people. Channel
        // identification would incorrectly treat that whole source as one voice.
        form.set("use_multi_channel", "false");
        form.set("tag_audio_events", "false");
        form.set(
          "file",
          new Blob([new Uint8Array(input.audio)], { type: "audio/wav" }),
          "capture.wav",
        );
        const response = await transport(
          "https://api.elevenlabs.io/v1/speech-to-text",
          {
            method: "POST",
            headers: { "xi-api-key": elevenKey },
            body: form,
            signal: AbortSignal.timeout(120000),
            redirect: "error",
          },
        );
        if (!response.ok) throw new ProviderFailure("transcription");
        const raw: unknown = await response.json();
        if (
          !object(raw) ||
          !Array.isArray(raw.words) ||
          raw.words.length > 100000
        )
          throw new ProviderFailure("transcription", "invalid_response");
        // Group contiguous words by provider speaker. Remote identities are scoped to
        // this upload, because batch diarization cannot identify a person across files.
        const result: Array<TranscriptSegmentDTO> = [];
        const speakers = new Map<string, number>();
        let current:
          | { speaker: string; text: string; start: number; end: number }
          | undefined;
        const flush = () => {
          if (!current || !current.text.trim()) return;
          let number = speakers.get(current.speaker);
          if (current.speaker !== "unknown" && number === undefined) {
            number = speakers.size + 1;
            speakers.set(current.speaker, number);
          }
          result.push({
            id: crypto.randomUUID(),
            channel: input.channel,
            speakerId: `${input.channel}:${input.chunkId}:${number === undefined ? "unknown" : `speaker_${number}`}`,
            // A microphone can hear several people in the same room. Source
            // channel alone never proves that every diarized voice is the owner.
            speakerLabel: number === undefined
              ? "Unknown speaker"
              : `Speaker ${number}`,
            text: current.text.trim(),
            isFinal: true,
            startMs: input.startMs + Math.round(current.start * 1000),
            endMs: input.startMs + Math.round(current.end * 1000),
          });
        };
        for (const word of raw.words) {
          if (
            !object(word) ||
            typeof word.text !== "string" ||
            typeof word.start !== "number" ||
            typeof word.end !== "number" ||
            !Number.isFinite(word.start) ||
            !Number.isFinite(word.end) ||
            word.start < 0 ||
            word.end < word.start ||
            word.end * 1000 > input.durationMs + 5000
          )
            throw new ProviderFailure("transcription", "invalid_response");
          if (word.type === "audio_event") continue;
          // Whitespace has no voice. Missing attribution on a spacing token
          // must not split a turn or invent an additional unknown speaker.
          if (word.type === "spacing" || !word.text.trim()) {
            if (current) {
              current.text += word.text;
              current.end = Math.max(current.end, word.end);
            }
            continue;
          }
          // The live path already stored this stretch: leave the word out, and end the turn so text
          // from the two paths never runs together in one segment.
          const middle = input.startMs + ((word.start + word.end) / 2) * 1000;
          if (input.keep && !input.keep.some((range) => middle >= range.fromMs && middle < range.toMs)) {
            flush();
            current = undefined;
            continue;
          }
          const speaker =
            typeof word.speaker_id === "string" && word.speaker_id.trim()
              ? word.speaker_id : "unknown";
          if (
            !current ||
            current.speaker !== speaker ||
            current.text.length + word.text.length > 18000
          ) {
            flush();
            current = {
              speaker,
              text: word.text,
              start: word.start,
              end: word.end,
            };
          } else {
            current.text += word.text;
            current.end = Math.max(current.end, word.end);
          }
        }
        flush();
        return input.keep ? Object.assign(result, { limitedToKeep: true }) : result;
      },
      catch: (e) =>
        e instanceof ProviderFailure ? e : new ProviderFailure("transcription"),
    });
  const complete = (
    messages: ReadonlyArray<{ role: string; content: string; cache?: true }>, json = false,
    parameters: LanguageRequestParameters = DEFAULT_LANGUAGE_PARAMETERS,
  ) =>
    Effect.tryPromise({
      async try(signal) {
        if (!routerKey) throw new ProviderFailure("language", "not_configured");
        const model = options.languageModel ?? process.env.OPENROUTER_MODEL ?? "openai/gpt-6-sol";
        // A feature's cache settings become this provider's fields; a feature without settings is sent as before.
        const wire = cachedRequest(languageProvider, model, messages, parameters.cache);
        const response = await transport(
          "https://openrouter.ai/api/v1/chat/completions",
          {
            method: "POST",
            headers: {
              Authorization: `Bearer ${routerKey}`,
              "Content-Type": "application/json",
            },
            body: JSON.stringify({
              model,
              messages: wire.messages,
              ...completionSettings(model,parameters.maxTokens,parameters.temperature),
              ...(json ? { response_format: { type: "json_object" } } : {}),
              ...wire.extra,
            }),
            signal: AbortSignal.any([signal, AbortSignal.timeout(90000)]),
            redirect: "error",
          },
        );
        if (!response.ok) throw new ProviderFailure("language");
        const raw: unknown = await response.json();
        if (
          !object(raw) ||
          !Array.isArray(raw.choices) ||
          !object(raw.choices[0]) ||
          !object(raw.choices[0].message) ||
          raw.choices[0].finish_reason === "length" ||
          typeof raw.choices[0].message.content !== "string" ||
          !raw.choices[0].message.content.trim() ||
          raw.choices[0].message.content.length > 1000000
        )
          throw new ProviderFailure("language", "invalid_response");
        return raw.choices[0].message.content;
      },
      catch: (e) =>
        e instanceof ProviderFailure ? e : new ProviderFailure("language"),
    });
  return {
    transcribe,
    async chatToolStep(messages,tools,signal) {
      if(!routerKey)throw new ProviderFailure("language","not_configured")
      const model=options.toolModel??options.languageModel??process.env.OPENROUTER_TOOL_MODEL??"openai/gpt-6-sol"
      // The research loop only appends, so each round can read the one before it from the provider's cache.
      const wire=cachedRequest(languageProvider,model,messages,PLANNER_PARAMETERS.cache)
      const response=await transport("https://openrouter.ai/api/v1/chat/completions",{
        method:"POST",headers:{Authorization:`Bearer ${routerKey}`,"Content-Type":"application/json"},
        body:JSON.stringify({model,messages:wire.messages,tools,tool_choice:"auto",parallel_tool_calls:false,...completionSettings(model,PLANNER_PARAMETERS.maxTokens,PLANNER_PARAMETERS.temperature),...wire.extra}),
        signal:AbortSignal.any([signal,AbortSignal.timeout(30_000)]),redirect:"error",
      })
      if(!response.ok)throw new ProviderFailure("language")
      const raw:unknown=await response.json()
      if(!object(raw)||!Array.isArray(raw.choices)||!object(raw.choices[0])||!object(raw.choices[0].message))throw new ProviderFailure("language","invalid_response")
      const calls=raw.choices[0].message.tool_calls
      if(calls===undefined)return[]
      if(!Array.isArray(calls)||calls.length>4)throw new ProviderFailure("language","invalid_response")
      return calls.map(call=>{
        if(!object(call)||typeof call.id!=="string"||call.id.length>200||!object(call.function))throw new ProviderFailure("language","invalid_response")
        const fn=call.function
        if(typeof fn.name!=="string"||typeof fn.arguments!=="string"||fn.arguments.length>20_000||!tools.some(tool=>tool.function.name===fn.name))throw new ProviderFailure("language","invalid_response")
        return{id:call.id,name:fn.name,arguments:fn.arguments}
      })
    },
    async *chatStream(note, input, signal) {
      if (!routerKey) throw new ProviderFailure("language", "not_configured");
      const model = options.languageModel ?? process.env.OPENROUTER_MODEL ?? "openai/gpt-6-sol", parameters = chatAnswerParameters(note);
      // Cache hints from the feature become this provider's request fields; without hints the request is unchanged.
      const wire = cachedRequest(languageProvider, model, chatAnswerMessages(note, input), parameters.cache);
      const started = performance.now();
      let usage: LanguageUsage | null = null, firstTokenMs: number | null = null;
      const response = await transport("https://openrouter.ai/api/v1/chat/completions", {
        method: "POST", headers: { Authorization: `Bearer ${routerKey}`, "Content-Type": "application/json" },
        body: JSON.stringify({ model, stream: true, ...completionSettings(model,parameters.maxTokens,parameters.temperature),
          messages: wire.messages, ...wire.extra }),
        signal: AbortSignal.any([signal, AbortSignal.timeout(90_000)]), redirect: "error",
      });
      if (!response.ok || !response.body) throw new ProviderFailure("language");
      const reader = response.body.getReader();
      const decoder = new TextDecoder();
      let buffer = "", characters = 0, complete = false;
      try {
        while (!complete) {
          const next = await reader.read();
          if (next.done) break;
          buffer = (buffer + decoder.decode(next.value, { stream: true })).replace(/\r\n/g, "\n");
          if (buffer.length > 1_000_000) throw new ProviderFailure("language", "invalid_response");
          let end: number;
          while ((end = buffer.indexOf("\n\n")) >= 0) {
            const frame = buffer.slice(0, end); buffer = buffer.slice(end + 2);
            const data = frame.split("\n").filter(line => line.startsWith("data:")).map(line => line.slice(5).trimStart()).join("\n");
            if (!data) continue;
            if (data === "[DONE]") { complete = true; break; }
            let raw: unknown; try { raw = JSON.parse(data); } catch { throw new ProviderFailure("language", "invalid_response"); }
            if (!object(raw) || raw.error) throw new ProviderFailure("language");
            // The last frame that carries usage wins: it holds the prompt, cached and written token counts.
            usage = readUsage(raw.usage) ?? usage;
            if (!Array.isArray(raw.choices) || raw.choices.length === 0) continue; // Usage-only final frame.
            const choice = raw.choices[0];
            if (!object(choice) || choice.finish_reason === "length") throw new ProviderFailure("language", "invalid_response");
            const text = object(choice.delta) ? choice.delta.content : undefined;
            if (typeof text === "string" && text) {
              characters += text.length; if (characters > 1_000_000) throw new ProviderFailure("language", "invalid_response");
              firstTokenMs ??= Math.round(performance.now() - started);
              yield text;
            }
          }
        }
        if (!complete || characters === 0) throw new ProviderFailure("language", "invalid_response");
      } finally {
        await reader.cancel().catch(() => {});
        if (firstTokenMs !== null) {
          const layout = chatLayout(note);
          (options.onLanguageUsage ?? logLanguageUsage)({ feature: chatFeature(note), provider: languageProvider, model, usage, firstTokenMs, totalMs: Math.round(performance.now() - started), ...(layout ? { layout } : {}) });
        }
      }
    },
    evidence: (note, paragraph) => Effect.flatMap(complete(evidenceMessages(note, paragraph), true, EVIDENCE_PARAMETERS), raw => Effect.try({ try: () => {
      const value: unknown = JSON.parse(raw);
      if (!object(value) || typeof value.summary !== "string" || !value.summary.trim() || value.summary.length > 20_000 || !Array.isArray(value.segmentIds) || value.segmentIds.length > 8 || !value.segmentIds.every(id => typeof id === "string" && note.transcriptSegments.some(s => s.id === id))) throw new ProviderFailure("language", "invalid_response");
      return { summary: value.summary, segmentIds: [...new Set(value.segmentIds as string[])] };
    }, catch: () => new ProviderFailure("language", "invalid_response") })),
    generate: (note) =>
      complete(generationMessages(note), false, GENERATION_PARAMETERS),
    chat: (note, input) =>
      complete(chatAnswerMessages(note, input), false, chatAnswerParameters(note)),
  };
}
