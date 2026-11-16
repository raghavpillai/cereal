import { randomBytes } from "node:crypto"
import { Effect } from "effect"
import type { AIProvider, ModelCatalogInput, LanguageModelSelection, AdminSettingsDTO,UpdateAdminSettingsInput,CreateCalendarInput,UpdateCalendarInput,CalendarDTO,CalendarEventDTO } from "@meeting-notes/contracts"
import type { ApiError } from "@meeting-notes/contracts/api"
import { secretBoxFromKey,type SecretBox } from "./admin-secrets"
import { fetchCalendarFeed,normalizedFeedURL,parseCalendarFeed } from "./calendar-feed"
import { CalendarLimitError, type CalendarStore, type StoredCalendar } from "./calendar-store"
import { makeProviderServices,ProviderFailure,type ProviderServices } from "./providers"

import {makeModelCatalog,CatalogFailure} from "./model-catalog"
import {normalizeAllowedEmail,type SignInPolicy} from "./sign-in"
import {classifyOrigin,googleNeedsHttps,GOOGLE_NEEDS_HTTPS} from "@meeting-notes/contracts/transport"
import {normalizeS3Endpoint,normalizeS3Prefix,validBucketName,validS3Region,validAccessKeyId,validSecretAccessKey,S3SettingsError,type S3Config} from "./s3"
import type {StorageConfiguration} from "./storage"
import type {StorageSettingsDTO,UpdateStorageSettingsInput,OnboardingDTO,OnboardingStepInput} from "@meeting-notes/contracts"

/** Settings that used to come from the environment and are now made on the admin panel. */
export const MOVED_VARIABLES=["OPENROUTER_API_KEY","OPENAI_API_KEY","ANTHROPIC_API_KEY","ELEVENLABS_API_KEY","OPENROUTER_MODEL","OPENROUTER_TOOL_MODEL","ELEVENLABS_STT_MODEL","LIVE_MAX_SESSIONS","LIVE_HOLD_SECONDS","IOS_APP_IDS","IOS_APP_STORE_ID"] as const
type MovedVariable=typeof MOVED_VARIABLES[number]
/** The Cereal iPhone app: team ID, a dot, the bundle ID. */
export const DEFAULT_IOS_APP_ID="LZWN2FF7YB.dev.cereal.mobile"
const IOS_APP_ID=/^[A-Z0-9]{10}\.[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+$/
class AdminFailure extends Error {constructor(readonly payload:ApiError){super(payload.message)}}
const missing=()=>new AdminFailure({error:"not_found",message:"Calendar or event not found"})
const invalid=(message:string)=>new AdminFailure({error:"invalid_request",message})
/** What a new instance streams with until an administrator chooses otherwise. It only runs once ElevenLabs has a key. */
/** What a new instance uses until an administrator chooses models. */
export const DEFAULT_LANGUAGE_MODEL="openai/gpt-6-sol",DEFAULT_TRANSCRIPTION_MODEL="scribe_v3"
export const defaultLiveTranscriptionModel={provider:"elevenlabs",model:"scribe_v3_live"} as const
/** The name an instance has until someone gives it one. */
export const DEFAULT_INSTANCE_NAME="Cereal"
/** Setup cannot be finished without these; the other steps can be skipped. */
export const REQUIRED_ONBOARDING_STEPS=["owner","language","speech"] as const
/** Steps that are finished because someone confirmed or skipped them, not because of what is stored. */
const RECORDED_ONBOARDING_STEPS:ReadonlyArray<string>=["name","models","signIn","storage"]
/** Whether the owner account has a password. Without an account service (tests) there is nothing to claim. */
export type OwnerReady=()=>Promise<boolean>
const ownerAlwaysReady:OwnerReady=async()=>true
export function makeAdministrationService(store:CalendarStore,box:SecretBox=secretBoxFromKey(randomBytes(32)),options:{clock?:()=>Date;fetchFeed?:(url:string)=>Promise<string>;worker?:boolean;environment?:NodeJS.ProcessEnv;fetch?:typeof fetch;/** The public origin, for the redirect URIs shown on the admin panel. */baseURL?:string}={}) {
 const now=options.clock??(()=>new Date()),env=options.environment??process.env,fetchFeed=options.fetchFeed??fetchCalendarFeed
 const origin=new URL(options.baseURL??env.PUBLIC_BASE_URL??"http://localhost:"+(env.PORT??"3000")).origin
 const googleRedirectUris=[origin+"/api/v1/auth/google/callback",origin+"/api/integrations/google/callback"]
 /** How the public address is reached, and whether Google will redirect to it (it will not over plain HTTP off this machine). */
 const transport=classifyOrigin(origin),noGoogle=googleNeedsHttps(origin)
 const running=new Map<string,Promise<CalendarDTO|null>>();let disposed=false,busy=false
 const safe=(c:StoredCalendar):CalendarDTO=>{const{encryptedURL,...dto}=c;return dto}
 const boundary=<A>(task:()=>Promise<A>)=>Effect.tryPromise({try:task,catch:(error):ApiError=>error instanceof AdminFailure?error.payload:error instanceof CalendarLimitError?{error:"invalid_request",message:"A maximum of 20 calendars can be linked"}:{error:"unavailable",message:"Settings or calendar service is temporarily unavailable"}})
 const catalog=makeModelCatalog(options.fetch,now)
 const secretFields={openrouter:"openRouterSecret",openai:"openAiSecret",anthropic:"anthropicSecret",elevenlabs:"elevenLabsSecret"} as const
 const envFields={openrouter:"OPENROUTER_API_KEY",openai:"OPENAI_API_KEY",anthropic:"ANTHROPIC_API_KEY",elevenlabs:"ELEVENLABS_API_KEY"} as const
 type Stored=Awaited<ReturnType<CalendarStore["getSettings"]>>
 /** A moved setting's value in the environment, until it has been taken into the settings at start-up
  * (`importEnvironment`). After that the environment is never read for it again: what is stored decides. */
 const fromEnv=(s:Stored,name:MovedVariable)=>s?.importedEnvironment?.includes(name)?undefined:env[name]?.trim()||undefined
 async function providerKey(provider:AIProvider){const s=await store.getSettings(),secret=s?.[secretFields[provider]];return secret?box.open(secret):fromEnv(s,envFields[provider])??""}
 /** Takes each moved setting that has a value in the environment and nothing stored into the settings, once.
  * Every variable seen is remembered, imported or not: a stored value is never replaced, and a setting that is
  * cleared later is not brought back by the next start. Returns the names that were imported (never a value). */
 async function importEnvironment():Promise<string[]>{
  const s=await store.getSettings(),seen=new Set(s?.importedEnvironment??[]),changes:Partial<Parameters<CalendarStore["saveSettings"]>[0]> ={},imported:string[]=[]
  const take=(name:MovedVariable,stored:unknown,read:(value:string)=>Partial<Parameters<CalendarStore["saveSettings"]>[0]>|null)=>{
   const value=env[name]?.trim()
   if(!value||seen.has(name))return
   seen.add(name)
   const change=stored===null||stored===undefined||stored===""?read(value):null
   if(change){Object.assign(changes,change);imported.push(name)}
  }
  for(const provider of Object.keys(envFields) as AIProvider[])take(envFields[provider],s?.[secretFields[provider]],value=>value.length>=10&&value.length<=1000&&!/\s/.test(value)?{[secretFields[provider]]:box.seal(value)}:null)
  take("OPENROUTER_MODEL",s?.languageModel,value=>/^[A-Za-z0-9_.:/-]{1,150}$/.test(value)?{languageModel:value}:null)
  take("ELEVENLABS_STT_MODEL",s?.transcriptionModel,value=>/^[A-Za-z0-9_.:/-]{1,150}$/.test(value)?{transcriptionModel:{provider:"elevenlabs",model:value}}:null)
  // The tool planner uses the chat model that was chosen; there is nothing to import, only to stop reading.
  take("OPENROUTER_TOOL_MODEL","unused",()=>null)
  take("LIVE_MAX_SESSIONS",s?.liveMaxSessions,value=>{const n=Number(value);return Number.isInteger(n)&&n>=1&&n<=200?{liveMaxSessions:n}:null})
  take("LIVE_HOLD_SECONDS",s?.liveHoldSeconds,value=>{const n=Number(value);return Number.isInteger(n)&&n>=0&&n<=3600?{liveHoldSeconds:n}:null})
  take("IOS_APP_IDS",s?.iosAppIds?.length?s.iosAppIds:null,value=>{const ids=value.split(",").map(id=>id.trim()).filter(id=>IOS_APP_ID.test(id));return ids.length?{iosAppIds:[...new Set(ids)].slice(0,10)}:null})
  take("IOS_APP_STORE_ID",s?.iosAppStoreId,value=>/^\d{1,20}$/.test(value)?{iosAppStoreId:value}:null)
  // A server whose provider keys came from its environment was set up before the setup flow existed: it is finished.
  const configuredBefore=!s?.onboardingCompletedAt&&imported.some(name=>(Object.values(envFields) as string[]).includes(name))
  if(seen.size!==(s?.importedEnvironment??[]).length)await store.saveSettings({...changes,importedEnvironment:[...seen],...(configuredBefore?{onboardingCompletedAt:now().toISOString()}:{})})
  return imported
 }
 /** First-run setup as it stands. Every step but sign-in and storage is read from what is really stored, so a
  * step finished some other way (the panel, the environment) counts, and nothing here can be made true by asking. */
 function onboardingOf(s:Stored,ownerReady:boolean):OnboardingDTO{
  const recorded=s?.onboardingSteps??{},configured=(id:AIProvider)=>!!(s?.[secretFields[id]]||fromEnv(s,envFields[id]))
  return{completedAt:s?.onboardingCompletedAt??null,steps:{
   owner:ownerReady,
   name:!!recorded.name||(s?.instanceName??DEFAULT_INSTANCE_NAME)!==DEFAULT_INSTANCE_NAME,
   language:configured("openrouter")||configured("openai")||configured("anthropic"),
   // ElevenLabs, or OpenAI when it is the provider chosen for speech-to-text.
   speech:configured("elevenlabs")||(s?.transcriptionModel?.provider==="openai"&&configured("openai")),
   models:!!recorded.models||!!(s?.chatModels&&s.generationModel&&s.transcriptionModel),
   // "Who may join" only exists for Google; with email and password an administrator adds people, so there is no step.
   signIn:!!recorded.signIn||!googleOn(s),storage:!!recorded.storage,
  }}
 }
 /** The settings with the setup state: what the admin endpoints answer with. */
 async function adminSettings(owner:OwnerReady=ownerAlwaysReady):Promise<AdminSettingsDTO>{
  const[dto,stored,ownerReady]=await Promise.all([settings(),store.getSettings(),owner()])
  return{...dto,onboarding:onboardingOf(stored,ownerReady)}
 }
 async function recordOnboardingStep(input:OnboardingStepInput,owner?:OwnerReady){
  const required=(REQUIRED_ONBOARDING_STEPS as ReadonlyArray<string>).includes(input.step)
  if(required&&input.state==="skipped")throw invalid("This step is needed before Cereal can work and cannot be skipped")
  const s=await store.getSettings()
  // A required step is finished by what is stored, never by saying so.
  if(required&&!(await adminSettings(owner)).onboarding!.steps[input.step])throw new AdminFailure({error:"conflict",message:"This step is not finished yet"})
  if(RECORDED_ONBOARDING_STEPS.includes(input.step))await store.saveSettings({onboardingSteps:{...(s?.onboardingSteps??{}),[input.step]:input.state}})
  return adminSettings(owner)
 }
 async function completeOnboarding(owner?:OwnerReady){
  const current=await adminSettings(owner),missing=REQUIRED_ONBOARDING_STEPS.filter(step=>!current.onboarding!.steps[step])
  if(missing.length)throw new AdminFailure({error:"conflict",message:"Finish the required setup steps first: "+missing.join(", ")})
  // Finishing twice keeps the first time.
  if(!current.onboarding!.completedAt)await store.saveSettings({onboardingCompletedAt:now().toISOString()})
  return adminSettings(owner)
 }
 /** Shows the setup flow again. Nothing that was configured is touched: only the record of having finished. */
 async function restartOnboarding(owner?:OwnerReady){await store.saveSettings({onboardingCompletedAt:null,onboardingSteps:{}});return adminSettings(owner)}
 /** Moved variables that are still set in the environment after they were taken in: ignored, safe to delete. */
 const unusedEnvironment=(s:Stored)=>MOVED_VARIABLES.filter(name=>env[name]?.trim()&&s?.importedEnvironment?.includes(name))
 const liveLimits=(s:Stored)=>({maxSessions:s?.liveMaxSessions??(Number.isInteger(Number(fromEnv(s,"LIVE_MAX_SESSIONS")))&&Number(fromEnv(s,"LIVE_MAX_SESSIONS"))>=1?Math.min(200,Number(fromEnv(s,"LIVE_MAX_SESSIONS"))):20),
  holdSeconds:s?.liveHoldSeconds??(fromEnv(s,"LIVE_HOLD_SECONDS")!==undefined&&Number.isFinite(Number(fromEnv(s,"LIVE_HOLD_SECONDS")))&&Number(fromEnv(s,"LIVE_HOLD_SECONDS"))>=0?Math.min(3600,Math.floor(Number(fromEnv(s,"LIVE_HOLD_SECONDS")))):300)})
 const appsOf=(s:Stored)=>{
  const fallback=(fromEnv(s,"IOS_APP_IDS")??"").split(",").map(id=>id.trim()).filter(id=>IOS_APP_ID.test(id)),storeId=s?.iosAppStoreId??fromEnv(s,"IOS_APP_STORE_ID")??null
  return{iosAppIds:s?.iosAppIds?.length?[...s.iosAppIds]:fallback.length?fallback:[DEFAULT_IOS_APP_ID],iosAppStoreId:storeId&&/^\d{1,20}$/.test(storeId)?storeId:null}
 }
 /** The stored Google OAuth client. An unreadable secret (a replaced settings key) counts as not stored. */
 function storedGoogleClient(s:Stored){if(!s?.googleClientId||!s.googleClientSecret)return null;try{return{clientId:s.googleClientId,clientSecret:box.open(s.googleClientSecret)}}catch{return null}}
 /** Google Calendar and Google sign-in share one client: the stored one, else the server environment. */
 async function googleCredentials(){const stored=storedGoogleClient(await store.getSettings());if(stored)return stored;return env.GOOGLE_CLIENT_ID&&env.GOOGLE_CLIENT_SECRET?{clientId:env.GOOGLE_CLIENT_ID,clientSecret:env.GOOGLE_CLIENT_SECRET}:null}
 // Both methods off can only be left behind by two administrators saving at once; password then stays on.
 // Where Google cannot be used at all (plain HTTP), password sign-in cannot be the method that is off either.
 // A server has exactly one way to sign in. Google is it only when it was chosen, password was turned off with it,
 // and Google can be used at this address; in every other case (a new server, an older one that had both on, plain
 // HTTP) it is email and password.
 const googleOn=(s:Stored)=>(s?.signInGoogle??false)&&s?.signInPassword===false&&!noGoogle
 const signInDTO=(s:Stored)=>({password:!googleOn(s),google:googleOn(s),allowedEmails:[...(s?.signInAllowedEmails??[])],googleClientId:s?.googleClientId??null,googleClientSecretSet:!!s?.googleClientSecret,googleRedirectUris,googleNeedsHttps:noGoogle})
 async function signInPolicy():Promise<SignInPolicy>{const s=await store.getSettings(),client=storedGoogleClient(s),dto=signInDTO(s);return{password:dto.password,google:dto.google&&!!client&&!noGoogle,allowedEmails:dto.allowedEmails,googleCredentials:client,instanceName:s?.instanceName??"Cereal"}}
 async function settings():Promise<AdminSettingsDTO>{
  const s=await store.getSettings(),legacyModel=s?.languageModel??fromEnv(s,"OPENROUTER_MODEL")??DEFAULT_LANGUAGE_MODEL,limits=liveLimits(s)
  return{instanceName:s?.instanceName??DEFAULT_INSTANCE_NAME,instanceLogo:s?.instanceLogo??null,languageModel:legacyModel,
   providers:(["openrouter","openai","anthropic","elevenlabs"] as const).map(id=>({id,configured:!!(s?.[secretFields[id]]||fromEnv(s,envFields[id])),source:s?.[secretFields[id]]?"stored":fromEnv(s,envFields[id])?"environment":"none"})),
   chatModels:s?.chatModels??[{provider:"openrouter",model:legacyModel}],generationModel:s?.generationModel??{provider:"openrouter",model:legacyModel},transcriptionModel:s?.transcriptionModel??{provider:"elevenlabs",model:fromEnv(s,"ELEVENLABS_STT_MODEL")??DEFAULT_TRANSCRIPTION_MODEL},
   // Chosen on the admin panel and kept in the database; on by default, and null once an administrator turns it off.
   liveTranscriptionModel:s?.liveTranscriptionModel===undefined?defaultLiveTranscriptionModel:s.liveTranscriptionModel,
   openRouterConfigured:!!(s?.openRouterSecret||fromEnv(s,"OPENROUTER_API_KEY")),elevenLabsConfigured:!!(s?.elevenLabsSecret||fromEnv(s,"ELEVENLABS_API_KEY")),openRouterSource:s?.openRouterSecret?"stored":fromEnv(s,"OPENROUTER_API_KEY")?"environment":"none",elevenLabsSource:s?.elevenLabsSecret?"stored":fromEnv(s,"ELEVENLABS_API_KEY")?"environment":"none",calendarRefreshMinutes:s?.calendarRefreshMinutes??15,calendarLookAheadDays:s?.calendarLookAheadDays??30,signIn:signInDTO(s),storage:storageDTO(storageState(s)),liveMaxSessions:limits.maxSessions,liveHoldSeconds:limits.holdSeconds,apps:appsOf(s),unusedEnvironment:unusedEnvironment(s),updatedAt:s?.updatedAt??null}
 }
 /** The storage settings in force, with the secret in the clear for the server's own use: what was saved on the admin
  * panel, else the environment (STORAGE_BACKEND, S3_*), else local disk. A value that does not pass the same checks
  * as a saved one is treated as not given. */
 type StorageState={backend:"local"|"s3";source:"stored"|"environment"|"default";endpoint:string|null;region:string;bucket:string;prefix:string;forcePathStyle:boolean;accessKeyId:string;secret:string|null}
 function storageState(s:Stored):StorageState{
  if(s?.storageBackend){
   const b=s.storageS3;let secret:string|null=null
   // An unreadable secret (a replaced settings key) counts as not stored.
   try{secret=s.storageS3Secret?box.open(s.storageS3Secret):null}catch{secret=null}
   return{backend:s.storageBackend,source:"stored",endpoint:b?.endpoint??null,region:b?.region||"us-east-1",bucket:b?.bucket??"",prefix:b?.prefix??"",forcePathStyle:b?.forcePathStyle??false,accessKeyId:b?.accessKeyId??"",secret}
  }
  const given=env.STORAGE_BACKEND||env.S3_BUCKET||env.S3_ENDPOINT
  if(!given)return{backend:"local",source:"default",endpoint:null,region:"us-east-1",bucket:"",prefix:"",forcePathStyle:false,accessKeyId:"",secret:null}
  const attempt=<A>(read:()=>A,fallback:A)=>{try{return read()}catch{return fallback}}
  const bucket=env.S3_BUCKET?.trim()??"",region=env.S3_REGION?.trim()||"us-east-1",accessKeyId=env.S3_ACCESS_KEY_ID?.trim()??""
  return{backend:env.STORAGE_BACKEND?.trim().toLowerCase()==="s3"?"s3":"local",source:"environment",endpoint:env.S3_ENDPOINT?.trim()?attempt(()=>normalizeS3Endpoint(env.S3_ENDPOINT!),null):null,region:validS3Region(region)?region:"us-east-1",bucket:validBucketName(bucket)?bucket:"",
   prefix:attempt(()=>normalizeS3Prefix(env.S3_PREFIX??""),""),forcePathStyle:/^(?:1|true|yes|on)$/i.test(env.S3_FORCE_PATH_STYLE?.trim()??""),accessKeyId:validAccessKeyId(accessKeyId)?accessKeyId:"",secret:env.S3_SECRET_ACCESS_KEY?.trim()||null}
 }
 const storageDTO=(state:StorageState):StorageSettingsDTO=>({backend:state.backend,source:state.source,endpoint:state.endpoint,region:state.region,bucket:state.bucket||null,prefix:state.prefix,forcePathStyle:state.forcePathStyle,accessKeyId:state.accessKeyId||null,secretAccessKeySet:!!state.secret})
 /** The bucket as the storage layer uses it, or null while something it needs is missing. */
 const bucketOf=(state:StorageState):S3Config|null=>state.bucket&&state.accessKeyId&&state.secret?{endpoint:state.endpoint,region:state.region,bucket:state.bucket,prefix:state.prefix,forcePathStyle:state.forcePathStyle,accessKeyId:state.accessKeyId,secretAccessKey:state.secret}:null
 const storageOf=(state:StorageState):StorageConfiguration=>({backend:state.backend,s3:bucketOf(state)})
 /** `input` laid over the settings in force and checked. Nothing is written. */
 function storageDraft(current:StorageState,input:UpdateStorageSettingsInput):StorageState{
  const next={...current}
  try{
   if(input.backend!==undefined)next.backend=input.backend
   if(input.endpoint!==undefined)next.endpoint=input.endpoint?.trim()?normalizeS3Endpoint(input.endpoint):null
   if(input.region!==undefined){const region=input.region?.trim()||"us-east-1";if(!validS3Region(region))throw new S3SettingsError("Enter the region as the store names it, such as us-east-1 or auto");next.region=region}
   if(input.bucket!==undefined){const bucket=input.bucket?.trim()??"";if(bucket&&!validBucketName(bucket))throw new S3SettingsError("Bucket names are 3 to 63 lower-case letters, digits, hyphens and dots");next.bucket=bucket}
   if(input.prefix!==undefined)next.prefix=normalizeS3Prefix(input.prefix)
   if(input.forcePathStyle!==undefined)next.forcePathStyle=input.forcePathStyle
   if(input.accessKeyId!==undefined){const id=input.accessKeyId?.trim()??"";if(id&&!validAccessKeyId(id))throw new S3SettingsError("Enter the access key ID exactly as the store shows it");next.accessKeyId=id}
   if(input.secretAccessKey!==undefined){const secret=input.secretAccessKey?.trim()||null;if(secret&&!validSecretAccessKey(secret))throw new S3SettingsError("Enter the secret access key exactly as the store shows it");next.secret=secret}
  }catch(error){if(error instanceof S3SettingsError)throw invalid(error.message);throw error}
  return next
 }
 /** Checks a storage change and returns the columns to write. Saving records everything in force, so settings that
  * came from the environment are kept once the panel has been used. The secret is sealed here. */
 async function storageChange(input:UpdateStorageSettingsInput){
  const next=storageDraft(storageState(await store.getSettings()),input)
  if(next.backend==="s3"&&!bucketOf(next))throw invalid("Enter the bucket, access key ID and secret access key before choosing S3-compatible storage")
  return{storageBackend:next.backend,storageS3:{endpoint:next.endpoint,region:next.region,bucket:next.bucket,prefix:next.prefix,forcePathStyle:next.forcePathStyle,accessKeyId:next.accessKeyId},storageS3Secret:next.secret?box.seal(next.secret):null}
 }
 /** Checks a sign-in change against what is stored and returns the columns to write. The secret is sealed here. */
 async function signInChange(input:NonNullable<UpdateAdminSettingsInput["signIn"]>){
  const current=signInDTO(await store.getSettings()),changes:{signInPassword?:boolean;signInGoogle?:boolean;signInAllowedEmails?:string[];googleClientId?:string|null;googleClientSecret?:string|null}={}
  // One method, never both and never neither: choosing one turns the other off.
  if(input.password===true&&input.google===true)throw invalid("A server has one way to sign in: choose email and password, or Google")
  if(input.password===false&&input.google===false)throw invalid("Keep one sign-in method turned on")
  const wantsGoogle=input.google===true||input.password===false?true:input.google===false||input.password===true?false:undefined
  if(wantsGoogle===true&&noGoogle)throw invalid(GOOGLE_NEEDS_HTTPS+". This server is reached over plain HTTP; put it behind HTTPS to use Google.")
  if(wantsGoogle!==undefined){changes.signInGoogle=wantsGoogle;changes.signInPassword=!wantsGoogle}
  if(input.allowedEmails!==undefined){
   const entries:string[]=[]
   for(const raw of input.allowedEmails){const entry=normalizeAllowedEmail(raw);if(!entry)throw invalid("Allowed entries are a full email address or a domain such as @example.com");if(!entries.includes(entry))entries.push(entry)}
   if(entries.length>500)throw invalid("Keep the allowed list to 500 entries or fewer")
   changes.signInAllowedEmails=entries
  }
  if(input.googleClientId!==undefined){const id=input.googleClientId?.trim()||null;if(id&&!/^[A-Za-z0-9._-]{10,255}$/.test(id))throw invalid("Enter the Google client ID exactly as Google Cloud shows it");changes.googleClientId=id}
  if(input.googleClientSecret!==undefined){const secret=input.googleClientSecret?.trim()||null;if(secret&&(secret.length<8||secret.length>512||/\s/.test(secret)))throw invalid("Enter the Google client secret exactly as Google Cloud shows it");changes.googleClientSecret=secret?box.seal(secret):null}
  const google=changes.signInGoogle??current.google
  const clientId=changes.googleClientId===undefined?current.googleClientId:changes.googleClientId,secretSet=changes.googleClientSecret===undefined?current.googleClientSecretSet:changes.googleClientSecret!==null
  if(google&&!(clientId&&secretSet))throw invalid(current.google?"Turn off Google sign-in before removing its client ID or secret":"Add a Google client ID and client secret before turning on Google sign-in")
  return changes
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
 configuration,liveConfiguration,googleCredentials,signInPolicy,
 /** Makes Google, with a client Google has just accepted in a real sign-in, the one way to sign in. Returns how to
  * put the settings back, for a caller whose next step fails. */
 async useGoogle(client:{clientId:string;clientSecret:string}){
  const before=await store.getSettings()
  await store.saveSettings({googleClientId:client.clientId,googleClientSecret:box.seal(client.clientSecret),signInGoogle:true,signInPassword:false})
  return async()=>{await store.saveSettings({googleClientId:before?.googleClientId??null,googleClientSecret:before?.googleClientSecret??null,signInGoogle:before?.signInGoogle??false,signInPassword:before?.signInPassword??true})}
 },
 /** Makes email and password the one way to sign in. The Google client stays stored, unused. */
 async usePassword(){await store.saveSettings({signInPassword:true,signInGoogle:false})},
 /** "secure", "private" or "public-http": how the public address is reached (contracts/transport.ts). */
 transport,
 importEnvironment,
 /** How many recordings may be transcribed live at once, and how long a stopped one's provider sessions are kept. */
 liveLimits:async()=>liveLimits(await store.getSettings()),
 /** The iPhone app's identifiers, for universal links, the download page and the Smart App Banner. */
 apps:async()=>appsOf(await store.getSettings()),
 /** Where files are kept right now; read by the storage layer for every operation. */
 storageConfiguration:async():Promise<StorageConfiguration>=>storageOf(storageState(await store.getSettings())),
 /** The configuration "Test connection" tries: the settings in force with a draft laid over them. */
 storageDraft:(input:UpdateStorageSettingsInput)=>boundary(async()=>storageOf(storageDraft(storageState(await store.getSettings()),input))),
 unlinkNote:(userId:string,noteId:string)=>boundary(()=>store.unlinkNote(userId,noteId)),
 eventContext:(id:string)=>boundary(()=>store.getEventContext(id)),
 noteContext:(userId:string,noteId:string)=>boundary(()=>store.getNoteContext(userId,noteId)),
 eventRange:(query:{from?:string;to?:string})=>boundary(()=>eventRange(query)),
 /** With `owner`, the answer for the admin endpoints: `onboarding.steps.owner` is read through it. */
 getSettings:(owner?:OwnerReady)=>boundary(()=>adminSettings(owner)),
 recordOnboardingStep:(input:OnboardingStepInput,owner?:OwnerReady)=>boundary(()=>recordOnboardingStep(input,owner)),
 completeOnboarding:(owner?:OwnerReady)=>boundary(()=>completeOnboarding(owner)),
 restartOnboarding:(owner?:OwnerReady)=>boundary(()=>restartOnboarding(owner)),
 getChatModels:()=>boundary(async()=>({models:(await settings()).chatModels})),
 getModelCatalog:(input:ModelCatalogInput)=>boundary(()=>modelCatalog(input)),
 updateSettings:(input:UpdateAdminSettingsInput,owner?:OwnerReady)=>boundary(async()=>{
  const{openRouterApiKey,elevenLabsApiKey,openAiApiKey,anthropicApiKey,signIn,storage,apps,...fields}=input
  const appChanges:{iosAppIds?:string[];iosAppStoreId?:string|null}={}
  if(apps?.iosAppIds!==undefined){const ids=[...new Set(apps.iosAppIds.map(id=>id.trim()).filter(Boolean))];if(ids.some(id=>!IOS_APP_ID.test(id)))throw invalid("An app ID is the team ID, a dot and the bundle ID, such as LZWN2FF7YB.dev.cereal.mobile");appChanges.iosAppIds=ids}
  if(apps?.iosAppStoreId!==undefined){const id=apps.iosAppStoreId?.trim()||null;if(id&&!/^\d{1,20}$/.test(id))throw invalid("The App Store ID is the number in the app's App Store address");appChanges.iosAppStoreId=id}
  const signInChanges=signIn?await signInChange(signIn):{},storageChanges=storage?await storageChange(storage):{}
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
   const key=keys[model.provider]===null?(fromEnv(await store.getSettings(),envFields[model.provider])??""):keys[model.provider]??await providerKey(model.provider)
   if(!key&&model.provider!=="openrouter")throw invalid("Connect the provider before selecting its model")
   const available=await modelCatalog({provider:model.provider,...(key?{apiKey:key}:{})})
   if(!available.models.some(m=>m.id===model.model&&m.capabilities.includes(capability as "language"|"transcription"|"live_transcription")))throw invalid("Choose an available model for this task")
  }
  const secrets:Record<string,string|null>={};for(const[id,key]of Object.entries(keys))if(key!==undefined)secrets[secretFields[id as AIProvider]]=key===null?null:box.seal(key)
  await store.saveSettings({...fields,...(fields.instanceName!==undefined?{instanceName:fields.instanceName.trim()}:{}),...secrets,...signInChanges,...storageChanges,...appChanges})
  return adminSettings(owner)
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
 return {chatToolStep:async(messages,tools,signal,model)=>{const p=makeProviderServices({...await admin.configuration("chat",model),fetch:transport});return p.chatToolStep!(messages,tools,signal)},transcribe:input=>Effect.flatMap(provider("transcription"),p=>p.transcribe(input)),generate:note=>Effect.flatMap(provider("language","generation"),p=>p.generate(note)),title:(note,notes)=>Effect.flatMap(provider("language","generation"),p=>p.title!(note,notes)),chat:(note,input)=>Effect.flatMap(provider("language","chat",input.model),p=>p.chat(note,input)),evidence:(note,paragraph)=>Effect.flatMap(provider("language","generation"),p=>p.evidence!(note,paragraph)),async *chatStream(note,input,signal){let p:ProviderServices;try{p=makeProviderServices({...await admin.configuration("chat",input.model),fetch:transport})}catch{throw new ProviderFailure("language")}yield* p.chatStream!(note,input,signal)}}
}
