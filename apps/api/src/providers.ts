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
import { titleMessages, TITLE_PARAMETERS, TITLE_TIMEOUT_MS } from "./ai/note-title";
import { evidenceMessages, EVIDENCE_PARAMETERS, readEvidence } from "./ai/evidence";
import { PLANNER_PARAMETERS } from "./ai/workspace-chat";
import { streamCompletion } from "./ai/completion-stream";

export class ProviderFailure extends Error {
  readonly _tag = "ProviderFailure";
  constructor(
    readonly service: "language",
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

export interface ProviderServices {
  readonly generate: (note: ProviderNote) => Effect.Effect<string, ProviderFailure>;
  /** What to call a meeting nobody named, from its finished notes (`ai/note-title.ts`): the model's answer as
   * it came, cleaned and judged by the caller. A provider without it leaves untitled notes untitled. */
  readonly title?: (note: ProviderNote, notes: string) => Effect.Effect<string, ProviderFailure>;
  readonly chat: (
    note: ProviderNote,
    input: ChatInput,
  ) => Effect.Effect<string, ProviderFailure>;
  readonly chatStream?: (note: ProviderNote, input: ChatInput, signal: AbortSignal) => AsyncIterable<string>;
  readonly evidence?: (note: ProviderNote, paragraph: string) => Effect.Effect<{ summary: string; segmentIds: ReadonlyArray<string> }, ProviderFailure>;
  /** The same request as `evidence`, streamed: the model's JSON answer as it is written, unchecked (`evidence-stream.ts` reads and validates it). */
  readonly evidenceStream?: (note: ProviderNote, paragraph: string, signal: AbortSignal) => AsyncIterable<string>;
  readonly chatToolStep?: (messages:ReadonlyArray<ChatToolMessage>,tools:ReadonlyArray<ChatToolDefinition>,signal:AbortSignal,model?:string)=>Promise<ReadonlyArray<ChatToolCall>>;
}
export class Providers extends Context.Service<Providers, ProviderServices>()(
  "MeetingNotesProviders",
) {}

const object = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v);
/** The most of a language provider's answer that is read: the same 1 MB a streamed answer and the model test request
 * are held to. A provider an administrator points at (an OpenAI-compatible server) cannot make the server buffer more. */
export const LANGUAGE_RESPONSE_LIMIT = 1_000_000;
/** A JSON answer read up to `limit` bytes; anything longer, or not JSON, is an invalid response. */
export async function readJsonCapped(response: Response, limit = LANGUAGE_RESPONSE_LIMIT): Promise<unknown> {
  const declared = Number(response.headers.get("content-length"));
  if (Number.isFinite(declared) && declared > limit) { await response.body?.cancel().catch(() => {}); throw new ProviderFailure("language", "invalid_response"); }
  const reader = response.body?.getReader();
  if (!reader) throw new ProviderFailure("language", "invalid_response");
  const decoder = new TextDecoder(); let text = "", size = 0;
  try {
    while (true) {
      const next = await reader.read(); if (next.done) break;
      size += next.value.byteLength; if (size > limit) throw new ProviderFailure("language", "invalid_response");
      text += decoder.decode(next.value, { stream: true });
    }
    text += decoder.decode();
  } finally { await reader.cancel().catch(() => {}); }
  try { return JSON.parse(text); } catch { throw new ProviderFailure("language", "invalid_response"); }
}
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
    languageProvider?:"openrouter"|"openai"|"anthropic"|"openai_compatible";
    /** Where an OpenAI-compatible server's API starts, already checked; and its key, when it wants one. */
    openAiCompatibleBaseUrl?:string;
    openAiCompatibleKey?:string;
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
  const languageKey=languageProvider==="openai_compatible"?options.openAiCompatibleKey??"":languageProvider==="openai"?openAiKey:languageProvider==="anthropic"?options.anthropicKey??process.env.ANTHROPIC_API_KEY:options.openRouterKey??process.env.OPENROUTER_API_KEY;
  const compatibleBase=options.openAiCompatibleBaseUrl??"";
  const transport = languageTransport(rawTransport,languageProvider,languageKey??"",compatibleBase);
  const routerKey = languageKey;
  // A language provider is ready with its key; an OpenAI-compatible server with its address (the key is optional).
  const languageReady = languageProvider==="openai_compatible" ? !!compatibleBase : !!routerKey;
  /** The Authorization header for the shared request; the compatible transport sets its own (none without a key). */
  const authorization: Record<string, string> = languageProvider==="openai_compatible" ? {} : { Authorization: `Bearer ${routerKey}` };
  const complete = (
    messages: ReadonlyArray<{ role: string; content: string; cache?: true }>, json = false,
    parameters: LanguageRequestParameters = DEFAULT_LANGUAGE_PARAMETERS,
    timeoutMs = 90000,
  ) =>
    Effect.tryPromise({
      async try(signal) {
        if (!languageReady) throw new ProviderFailure("language", "not_configured");
        const model = options.languageModel ?? process.env.OPENROUTER_MODEL ?? "openai/gpt-6-sol";
        // A feature's cache settings become this provider's fields; a feature without settings is sent as before.
        const wire = cachedRequest(languageProvider, model, messages, parameters.cache);
        const response = await transport(
          "https://openrouter.ai/api/v1/chat/completions",
          {
            method: "POST",
            headers: {
              ...authorization,
              "Content-Type": "application/json",
            },
            body: JSON.stringify({
              model,
              messages: wire.messages,
              ...completionSettings(model,parameters.maxTokens,parameters.temperature),
              ...(json ? { response_format: { type: "json_object" } } : {}),
              ...wire.extra,
            }),
            signal: AbortSignal.any([signal, AbortSignal.timeout(timeoutMs)]),
            redirect: "error",
          },
        );
        if (!response.ok) throw new ProviderFailure("language");
        const raw: unknown = await readJsonCapped(response);
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
    async chatToolStep(messages,tools,signal) {
      if(!languageReady)throw new ProviderFailure("language","not_configured")
      const model=options.toolModel??options.languageModel??process.env.OPENROUTER_TOOL_MODEL??"openai/gpt-6-sol"
      // The research loop only appends, so each round can read the one before it from the provider's cache.
      const wire=cachedRequest(languageProvider,model,messages,PLANNER_PARAMETERS.cache)
      const response=await transport("https://openrouter.ai/api/v1/chat/completions",{
        method:"POST",headers:{...authorization,"Content-Type":"application/json"},
        body:JSON.stringify({model,messages:wire.messages,tools,tool_choice:"auto",parallel_tool_calls:false,...completionSettings(model,PLANNER_PARAMETERS.maxTokens,PLANNER_PARAMETERS.temperature),...wire.extra}),
        signal:AbortSignal.any([signal,AbortSignal.timeout(30_000)]),redirect:"error",
      })
      if(!response.ok)throw new ProviderFailure("language")
      const raw:unknown=await readJsonCapped(response)
      if(!object(raw)||!Array.isArray(raw.choices)||!object(raw.choices[0])||!object(raw.choices[0].message))throw new ProviderFailure("language","invalid_response")
      const calls=raw.choices[0].message.tool_calls
      // A server without tool calling answers in prose with no calls (or `null`): nothing more to look up.
      if(calls===undefined||calls===null)return[]
      if(!Array.isArray(calls)||calls.length>4)throw new ProviderFailure("language","invalid_response")
      return calls.map(call=>{
        if(!object(call)||typeof call.id!=="string"||call.id.length>200||!object(call.function))throw new ProviderFailure("language","invalid_response")
        const fn=call.function
        if(typeof fn.name!=="string"||typeof fn.arguments!=="string"||fn.arguments.length>20_000||!tools.some(tool=>tool.function.name===fn.name))throw new ProviderFailure("language","invalid_response")
        return{id:call.id,name:fn.name,arguments:fn.arguments}
      })
    },
    async *chatStream(note, input, signal) {
      if (!languageReady) throw new ProviderFailure("language", "not_configured");
      const model = options.languageModel ?? process.env.OPENROUTER_MODEL ?? "openai/gpt-6-sol", parameters = chatAnswerParameters(note);
      // Cache hints from the feature become this provider's request fields; without hints the request is unchanged.
      const wire = cachedRequest(languageProvider, model, chatAnswerMessages(note, input), parameters.cache);
      const started = performance.now();
      let usage: LanguageUsage | null = null, firstTokenMs: number | null = null;
      const response = await transport("https://openrouter.ai/api/v1/chat/completions", {
        method: "POST", headers: { ...authorization, "Content-Type": "application/json" },
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
    // The same reading and checks as the streamed answer (`ai/evidence.ts`), fenced output included.
    evidence: (note, paragraph) => Effect.flatMap(complete(evidenceMessages(note, paragraph), true, EVIDENCE_PARAMETERS), raw => {
      const result = readEvidence(raw, note);
      return result ? Effect.succeed(result) : Effect.fail(new ProviderFailure("language", "invalid_response"));
    }),
    evidenceStream: (note, paragraph, signal) => streamCompletion({ transport, ready: !!routerKey, headers: { Authorization: `Bearer ${routerKey}` }, provider: languageProvider, model: options.languageModel ?? process.env.OPENROUTER_MODEL ?? "openai/gpt-6-sol", onUsage: options.onLanguageUsage ?? logLanguageUsage }, evidenceMessages(note, paragraph), EVIDENCE_PARAMETERS, { feature: "evidence", json: true, signal }),
    generate: (note) =>
      complete(generationMessages(note), false, GENERATION_PARAMETERS),
    title: (note, notes) =>
      complete(titleMessages(note, notes), false, TITLE_PARAMETERS, TITLE_TIMEOUT_MS),
    chat: (note, input) =>
      complete(chatAnswerMessages(note, input), false, chatAnswerParameters(note)),
  };
}
