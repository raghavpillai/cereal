import { randomBytes } from "node:crypto"
import { Effect } from "effect"
import type { AIProvider, ModelCatalogInput, LanguageModelSelection, AdminSettingsDTO,UpdateAdminSettingsInput,CreateCalendarInput,UpdateCalendarInput,CalendarDTO,CalendarEventDTO } from "@meeting-notes/contracts"
import type { ApiError } from "@meeting-notes/contracts/api"
import { secretBoxFromKey,type SecretBox } from "./admin-secrets"
import { fetchCalendarFeed,normalizedFeedURL,parseCalendarFeed } from "./calendar-feed"
import { CalendarLimitError, type CalendarStore, type StoredCalendar } from "./calendar-store"
import { makeProviderServices,ProviderFailure,type ProviderServices } from "./providers"

import {makeModelCatalog,CatalogFailure} from "./model-catalog"

class AdminFailure extends Error {constructor(readonly payload:ApiError){super(payload.message)}}
const missing=()=>new AdminFailure({error:"not_found",message:"Calendar or event not found"})
const invalid=(message:string)=>new AdminFailure({error:"invalid_request",message})
/** What a new instance streams with until an administrator chooses otherwise. It only runs once ElevenLabs has a key. */
export const defaultLiveTranscriptionModel={provider:"elevenlabs",model:"scribe_v3_live"} as const
export function makeAdministrationService(store:CalendarStore,box:SecretBox=secretBoxFromKey(randomBytes(32)),options:{clock?:()=>Date;fetchFeed?:(url:string)=>Promise<string>;worker?:boolean;environment?:NodeJS.ProcessEnv;fetch?:typeof fetch}={}) {
 const now=options.clock??(()=>new Date()),env=options.environment??process.env,fetchFeed=options.fetchFeed??fetchCalendarFeed
 const running=new Map<string,Promise<CalendarDTO|null>>();let disposed=false,busy=false
 const safe=(c:StoredCalendar):CalendarDTO=>{const{encryptedURL,...dto}=c;return dto}
 const boundary=<A>(task:()=>Promise<A>)=>Effect.tryPromise({try:task,catch:(error):ApiError=>error instanceof AdminFailure?error.payload:error instanceof CalendarLimitError?{error:"invalid_request",message:"A maximum of 20 calendars can be linked"}:{error:"unavailable",message:"Settings or calendar service is temporarily unavailable"}})
 const catalog=makeModelCatalog(options.fetch,now)
 const secretFields={openrouter:"openRouterSecret",openai:"openAiSecret",anthropic:"anthropicSecret",elevenlabs:"elevenLabsSecret"} as const
 const envFields={openrouter:"OPENROUTER_API_KEY",openai:"OPENAI_API_KEY",anthropic:"ANTHROPIC_API_KEY",elevenlabs:"ELEVENLABS_API_KEY"} as const
 async function providerKey(provider:AIProvider){const s=await store.getSettings(),secret=s?.[secretFields[provider]];return secret?box.open(secret):env[envFields[provider]]??""}
 async function settings():Promise<AdminSettingsDTO>{
  const s=await store.getSettings(),legacyModel=s?.languageModel??env.OPENROUTER_MODEL??"openai/gpt-6-sol"
  return{instanceName:s?.instanceName??"Cereal",instanceLogo:s?.instanceLogo??null,languageModel:legacyModel,
   providers:(["openrouter","openai","anthropic","elevenlabs"] as const).map(id=>({id,configured:!!(s?.[secretFields[id]]||env[envFields[id]]),source:s?.[secretFields[id]]?"stored":env[envFields[id]]?"environment":"none"})),
   chatModels:s?.chatModels??[{provider:"openrouter",model:legacyModel}],generationModel:s?.generationModel??{provider:"openrouter",model:legacyModel},transcriptionModel:s?.transcriptionModel??{provider:"elevenlabs",model:env.ELEVENLABS_STT_MODEL??"scribe_v3"},
   // Chosen on the admin panel and kept in the database; on by default, and null once an administrator turns it off.
   liveTranscriptionModel:s?.liveTranscriptionModel===undefined?defaultLiveTranscriptionModel:s.liveTranscriptionModel,
   openRouterConfigured:!!(s?.openRouterSecret||env.OPENROUTER_API_KEY),elevenLabsConfigured:!!(s?.elevenLabsSecret||env.ELEVENLABS_API_KEY),openRouterSource:s?.openRouterSecret?"stored":env.OPENROUTER_API_KEY?"environment":"none",elevenLabsSource:s?.elevenLabsSecret?"stored":env.ELEVENLABS_API_KEY?"environment":"none",calendarRefreshMinutes:s?.calendarRefreshMinutes??15,calendarLookAheadDays:s?.calendarLookAheadDays??30,updatedAt:s?.updatedAt??null}
 }
 async function configuration(purpose:"chat"|"generation"="chat",requested?:string){
  const s=await settings();let selected=purpose==="generation"?s.generationModel:s.chatModels[0]!
  if(requested){const found=s.chatModels.find(m=>`${m.provider}:${m.model}`===requested);if(!found)throw invalid("Choose an enabled chat model");selected=found}
  const keys=await Promise.all((["openrouter","openai","anthropic","elevenlabs"] as const).map(providerKey))
  return{openRouterKey:keys[0]!,openAiKey:keys[1]!,anthropicKey:keys[2]!,elevenLabsKey:keys[3]!,languageProvider:selected.provider,languageModel:selected.model,toolModel:selected.model,transcriptionProvider:s.transcriptionModel.provider,transcriptionModel:s.transcriptionModel.model}
 }
 async function modelCatalog(input:ModelCatalogInput){
  try{return await catalog(input.provider,input.apiKey??await providerKey(input.provider),input.refresh)}catch(error){throw error instanceof CatalogFailure?new AdminFailure({error:error.reason==="not_configured"?"invalid_request":"unavailable",message:error.reason==="not_configured"?"Add a provider API key to load its models":"The provider model list is unavailable. Check the key and retry."}):error}
 }
 function validatedURL(raw:string){try{return normalizedFeedURL(raw)}catch{throw invalid("Use a public HTTPS or webcal subscription URL; private network feeds and embedded credentials are not supported")}}
 async function sync(id:string):Promise<CalendarDTO|null>{
  const c=await store.getCalendar(id);if(!c)return null;const key=id+":"+c.encryptedURL;const existing=running.get(key);if(existing)return existing
  const operation=(async()=>{try{const source=await fetchFeed(box.open(c.encryptedURL));const events=await parseCalendarFeed(id,source,now());await store.replaceEvents(id,c.encryptedURL,events,now().toISOString())}catch{await store.failSync(id,c.encryptedURL,"Calendar could not be refreshed. Check the subscription URL and try again.")}const latest=await store.getCalendar(id);return latest?safe(latest):null})()
  running.set(key,operation);try{return await operation}finally{if(running.get(key)===operation)running.delete(key)}
 }
 async function eventRange(query:{from?:string;to?:string}){const s=await settings(),from=query.from??now().toISOString(),to=query.to??new Date(Date.parse(from)+s.calendarLookAheadDays*86400000).toISOString();if(!Number.isFinite(Date.parse(from))||!Number.isFinite(Date.parse(to))||Date.parse(to)<=Date.parse(from)||Date.parse(to)-Date.parse(from)>90*86400000)throw invalid("Event range must be after its start and no more than 90 days");return{from:new Date(from).toISOString(),to:new Date(to).toISOString()}}
 async function tick(){if(disposed||busy)return;busy=true;try{const s=await settings(),calendars=await store.listCalendars();const due=calendars.filter(c=>c.enabled&&(!c.lastSyncedAt||now().getTime()-Date.parse(c.lastSyncedAt)>=s.calendarRefreshMinutes*60000));for(let i=0;i<due.length&&!disposed;i+=2)await Promise.all(due.slice(i,i+2).map(c=>sync(c.id)))}catch{}finally{busy=false}}
 let timer:ReturnType<typeof setInterval>|undefined
 if(options.worker){timer=setInterval(()=>void tick(),60_000);timer.unref();void tick()}
 /** The streaming model and the key to call it with, or null when live transcription is off or its provider has no key. */
 async function liveConfiguration(){const selected=(await settings()).liveTranscriptionModel;if(!selected)return null;return await providerKey(selected.provider)?{provider:selected.provider,model:selected.model,apiKey:()=>providerKey(selected.provider)}:null}
 return {
 configuration,liveConfiguration,
 unlinkNote:(userId:string,noteId:string)=>boundary(()=>store.unlinkNote(userId,noteId)),
 eventContext:(id:string)=>boundary(()=>store.getEventContext(id)),
 noteContext:(userId:string,noteId:string)=>boundary(()=>store.getNoteContext(userId,noteId)),
 eventRange:(query:{from?:string;to?:string})=>boundary(()=>eventRange(query)),
 getSettings:()=>boundary(settings),
 getChatModels:()=>boundary(async()=>({models:(await settings()).chatModels})),
 getModelCatalog:(input:ModelCatalogInput)=>boundary(()=>modelCatalog(input)),
 updateSettings:(input:UpdateAdminSettingsInput)=>boundary(async()=>{
  const{openRouterApiKey,elevenLabsApiKey,openAiApiKey,anthropicApiKey,...fields}=input
  if(fields.instanceName!==undefined&&!fields.instanceName.trim())throw invalid("Enter an instance name")
  if(fields.chatModels&&new Set(fields.chatModels.map(m=>m.provider+":"+m.model)).size!==fields.chatModels.length)throw invalid("Chat models must be unique")
  // Verify newly selected models against their provider's catalog, using a draft
  // key when included. Unrelated edits do not depend on provider availability.
  const current=await settings(),keys={openrouter:openRouterApiKey,elevenlabs:elevenLabsApiKey,openai:openAiApiKey,anthropic:anthropicApiKey}
  const selections=[...(fields.chatModels??[]).map(model=>({model,capability:"language"})),...(fields.generationModel?[{model:fields.generationModel,capability:"language"}]:[]),...(fields.transcriptionModel?[{model:fields.transcriptionModel,capability:"transcription"}]:[]),...(fields.liveTranscriptionModel?[{model:fields.liveTranscriptionModel,capability:"live_transcription"}]:[])]
  const old=[...current.chatModels,current.generationModel,current.transcriptionModel]
  for(const{model,capability}of selections){
   // A streaming model is only "already selected" as the streaming model: a batch model must not pass as one.
   if(capability==="live_transcription"?current.liveTranscriptionModel?.provider===model.provider&&current.liveTranscriptionModel.model===model.model:old.some(m=>m.provider===model.provider&&m.model===model.model))continue
   const key=keys[model.provider]===null?(env[envFields[model.provider]]??""):keys[model.provider]??await providerKey(model.provider)
   if(!key&&model.provider!=="openrouter")throw invalid("Connect the provider before selecting its model")
   const available=await modelCatalog({provider:model.provider,...(key?{apiKey:key}:{})})
   if(!available.models.some(m=>m.id===model.model&&m.capabilities.includes(capability as "language"|"transcription"|"live_transcription")))throw invalid("Choose an available model for this task")
  }
  const secrets:Record<string,string|null>={};for(const[id,key]of Object.entries(keys))if(key!==undefined)secrets[secretFields[id as AIProvider]]=key===null?null:box.seal(key)
  await store.saveSettings({...fields,...(fields.instanceName!==undefined?{instanceName:fields.instanceName.trim()}:{}),...secrets})
  return settings()
 }),
 listCalendars:()=>boundary(async()=>(await store.listCalendars()).map(safe)),
 createCalendar:(input:CreateCalendarInput)=>boundary(async()=>{const url=validatedURL(input.url);const c=await store.createCalendar({name:input.name,color:input.color??"#87e5e8",enabled:input.enabled??true,provider:"ics",encryptedURL:box.seal(url.href),sourceHost:url.hostname});return(await sync(c.id))??safe(c)}),
 updateCalendar:(id:string,input:UpdateCalendarInput)=>boundary(async()=>{const{url,...fields}=input;const normalized=url===undefined?undefined:validatedURL(url);const c=await store.updateCalendar(id,{...fields,...(normalized?{encryptedURL:box.seal(normalized.href),sourceHost:normalized.hostname}:{})});if(!c)throw missing();return safe(c)}),
 deleteCalendar:(id:string)=>boundary(async()=>{if(!await store.deleteCalendar(id))throw missing();return{deleted:true as const}}),
 syncCalendar:(id:string)=>boundary(async()=>{const c=await sync(id);if(!c)throw missing();return c}),
 listEvents:(query:{from?:string;to?:string},userId?:string)=>boundary(async()=>{const{from,to}=await eventRange(query);const events=await store.listEvents(from,to,userId);if(events.length>10000)throw invalid("Too many events; request a shorter date range");return events.map(({sourceKey,meetingKey,...e}):CalendarEventDTO=>e)}),
 createEventNote:(id:string,userId?:string)=>boundary(async()=>{const note=await store.createEventNote(id,userId);if(!note)throw missing();return note}),
 async dispose(){disposed=true;if(timer)clearInterval(timer);await Promise.allSettled([...running.values()])},
 }
}
export type AdministrationService=ReturnType<typeof makeAdministrationService>
/** Load configuration for each operation, so encrypted admin overrides take effect
 * immediately. Calendar data is never attached to language-provider context. */
export function settingsAwareProviders(admin:AdministrationService,transport?:typeof fetch):ProviderServices {
 const provider=(service:"language"|"transcription",purpose:"chat"|"generation"="chat",requested?:string)=>Effect.tryPromise({try:async()=>makeProviderServices({...await admin.configuration(purpose,requested),fetch:transport}),catch:()=>new ProviderFailure(service)})
 return {chatToolStep:async(messages,tools,signal,model)=>{const p=makeProviderServices({...await admin.configuration("chat",model),fetch:transport});return p.chatToolStep!(messages,tools,signal)},transcribe:input=>Effect.flatMap(provider("transcription"),p=>p.transcribe(input)),generate:note=>Effect.flatMap(provider("language","generation"),p=>p.generate(note)),chat:(note,input)=>Effect.flatMap(provider("language","chat",input.model),p=>p.chat(note,input)),evidence:(note,paragraph)=>Effect.flatMap(provider("language","generation"),p=>p.evidence!(note,paragraph)),async *chatStream(note,input,signal){let p:ProviderServices;try{p=makeProviderServices({...await admin.configuration("chat",input.model),fetch:transport})}catch{throw new ProviderFailure("language")}yield* p.chatStream!(note,input,signal)}}
}
