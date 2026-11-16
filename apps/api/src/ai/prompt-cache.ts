/**
 * Prompt caching at the provider boundary: how a feature's cache hints become request fields, and how
 * the provider's own count of cached tokens is read back.
 *
 * A feature says two things. Which messages end a prefix that other requests will send again
 * (`LanguageMessage.cache`), and whether the request is a conversation whose next request repeats it
 * and appends (`cache.tail`) or a one-off whose end is unique. This module knows what each provider
 * needs for that. Verified against the providers on 2026-11-14 (documentation read and requests
 * measured that day):
 *
 *  - Anthropic models, direct or through OpenRouter, cache nothing unless asked. A `cache_control`
 *    marker goes on each marked message, and for a conversation a top-level `cache_control`
 *    (automatic caching) follows its growing end. At most four breakpoints.
 *  - OpenAI GPT-5.6 and later (`prompt_cache_options` and `prompt_cache_breakpoint` are in OpenAI's
 *    chat completions reference and in OpenRouter's, for those models only). By default they write a
 *    cache entry at the end of the newest user message, at 1.25 times the input price, and find
 *    earlier entries where earlier messages end. That is exactly right for a conversation, which
 *    therefore needs no field for its end, only an explicit `prompt_cache_breakpoint` on a prefix
 *    that *other* conversations share. For a one-off it is a surcharge on text nobody reads again,
 *    so a one-off is sent with `prompt_cache_options: {mode: "explicit"}`: only the marked messages
 *    are written, and with none marked nothing is.
 *  - Anything else caches by itself or not at all, and gets the request unchanged. No field is ever
 *    sent to a model outside the two families above.
 *
 * `language-transport.ts` turns the Anthropic form produced here into the Messages API.
 */
import type {LanguageRequestParameters} from "./shared"

/** `openai_compatible` gets no cache fields at all: such a server caches by itself or not at all. */
export type LanguageProvider="openrouter"|"openai"|"anthropic"|"openai_compatible"
export type CacheStyle="anthropic"|"openai-breakpoints"|"automatic"
const GPT_5_6_OR_LATER=/^gpt-(?:5\.(?:[6-9]|\d{2,})|[6-9]|\d{2,})(?:[.\/-]|$)/
export const cacheStyle=(provider:LanguageProvider,model:string):CacheStyle=>
  provider==="anthropic"||provider==="openrouter"&&/^~?anthropic\//.test(model)?"anthropic"
  :provider==="openrouter"&&model.startsWith("openai/")&&GPT_5_6_OR_LATER.test(model.slice(7))||provider==="openai"&&GPT_5_6_OR_LATER.test(model)?"openai-breakpoints":"automatic"

/** A message as a feature writes it. Anything besides the role, the text and the hint (tool calls, a tool call ID) is passed through. */
export type HintedMessage={role:string;content:string|null;cache?:true}
export type WireMessage<M extends HintedMessage=HintedMessage>=Omit<M,"cache"|"content">&{content:string|null|Array<Record<string,unknown>>}
/** The messages and extra top-level fields to send for `messages` under a feature's cache settings.
 * Consecutive system messages are one system message on the wire: a feature splits its instructions
 * only to mark the part that never changes, and a provider that takes no marker gets the same single
 * string it always did. */
export function cachedRequest<M extends HintedMessage>(provider:LanguageProvider,model:string,messages:ReadonlyArray<M>,cache:LanguageRequestParameters["cache"]):{messages:Array<WireMessage<M>>;extra:Record<string,unknown>} {
  type Part={text:string;marked:boolean;system:boolean}
  const units:Array<{message:M;parts:Part[]}>=[]
  for(const message of messages){
    const previous=units.at(-1),part:Part={text:message.content??"",marked:!!message.cache,system:message.role==="system"}
    if(previous&&part.system&&previous.parts[0]!.system)previous.parts.push(part)
    else units.push({message,parts:[part]})
  }
  const style=cache?cacheStyle(provider,model):"automatic",explicit=style==="openai-breakpoints"&&!cache!.tail
  // Where a breakpoint may go. A model that caches a conversation's instructions by itself needs none on them.
  const candidates=style==="automatic"?[]:units.flatMap(unit=>unit.parts).filter(part=>part.marked&&!(style==="openai-breakpoints"&&!explicit&&part.system))
  // At most four per request; an automatic one for the end of a conversation takes one of them. The ones furthest in win.
  const kept=new Set(candidates.slice(-(style==="openai-breakpoints"&&explicit||style==="anthropic"&&!cache!.tail?4:3)))
  const control=style==="anthropic"?{cache_control:{type:"ephemeral",...(cache!.ttl==="1h"?{ttl:"1h"}:{})}}:{prompt_cache_breakpoint:{mode:"explicit"}}
  // Every field of a message stays where it was; only `content` may change form, and the hint is not sent.
  const wire=({message,parts}:{message:M;parts:Part[]})=>Object.fromEntries(Object.entries(message).filter(([name])=>name!=="cache").map(([name,value])=>[name,name!=="content"?value
    :parts.some(part=>kept.has(part))?parts.map(part=>({type:"text",text:part.text,...(kept.has(part)?control:{})})):message.content===null?null:parts.map(part=>part.text).join("")])) as WireMessage<M>
  return {
    messages:units.map(wire),
    extra:style==="anthropic"?(cache!.tail?control:{}):explicit?{prompt_cache_options:{mode:"explicit"}}:{},
  }
}

/** What a provider reported for one request. `promptTokens` is the whole prompt; `cachedTokens` of it
 * were read from the provider's cache and `cacheWriteTokens` were written to it. */
export interface LanguageUsage {promptTokens:number;cachedTokens:number;cacheWriteTokens:number;outputTokens:number|null;costUsd:number|null}
const count=(value:unknown)=>typeof value==="number"&&Number.isFinite(value)&&value>=0?value:null
const object=(value:unknown):value is Record<string,unknown>=>typeof value==="object"&&value!==null&&!Array.isArray(value)
/** Reads an OpenAI-style `usage` object (OpenRouter, OpenAI, and what `language-transport.ts` builds for Anthropic). */
export function readUsage(raw:unknown):LanguageUsage|null {
  if(!object(raw))return null
  const promptTokens=count(raw.prompt_tokens)
  if(promptTokens===null)return null
  const details=object(raw.prompt_tokens_details)?raw.prompt_tokens_details:{}
  return {promptTokens,cachedTokens:count(details.cached_tokens)??0,cacheWriteTokens:count(details.cache_write_tokens)??0,outputTokens:count(raw.completion_tokens),costUsd:count(raw.cost)}
}
/** Anthropic's own usage fields in the OpenAI shape, so one reader serves every provider. */
export function anthropicUsage(raw:unknown):Record<string,unknown>|null {
  if(!object(raw)||count(raw.input_tokens)===null)return null
  const read=count(raw.cache_read_input_tokens)??0,written=count(raw.cache_creation_input_tokens)??0
  return {prompt_tokens:count(raw.input_tokens)!+read+written,completion_tokens:count(raw.output_tokens)??0,prompt_tokens_details:{cached_tokens:read,cache_write_tokens:written}}
}

export interface LanguageUsageEvent {feature:string;provider:LanguageProvider;model:string;usage:LanguageUsage|null;firstTokenMs:number|null;totalMs:number;
  /** How the request was laid out, when the feature says (live chat: `new`, `append`, `rebase`). */
  layout?:string}
/** One line per streamed answer, so cache hits can be read from the server log:
 * `language usage feature=live-chat layout=append model=openai/gpt-6-sol prompt=7638 cached=7122 (93%) written=513 output=31 first_token_ms=640 total_ms=1010`.
 * Silent under `bun test`, and with `CEREAL_LANGUAGE_USAGE_LOG=off`. */
export function logLanguageUsage(event:LanguageUsageEvent):void {
  if(process.env.NODE_ENV==="test"||process.env.CEREAL_LANGUAGE_USAGE_LOG==="off")return
  const {usage}=event
  console.info(`language usage feature=${event.feature}${event.layout?` layout=${event.layout}`:""} provider=${event.provider} model=${event.model} `+
    (usage?`prompt=${usage.promptTokens} cached=${usage.cachedTokens} (${usage.promptTokens?Math.round(100*usage.cachedTokens/usage.promptTokens):0}%) written=${usage.cacheWriteTokens} output=${usage.outputTokens??"?"}${usage.costUsd===null?"":` cost_usd=${usage.costUsd}`}`:"usage=not-reported")+
    ` first_token_ms=${event.firstTokenMs??"?"} total_ms=${event.totalMs}`)
}
