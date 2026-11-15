import type {AdminSettingsDTO,AIProvider,LanguageModelSelection,LiveTranscriptionModelSelection,StorageSettingsDTO,StorageTestInput,TranscriptionModelSelection,UpdateAdminSettingsInput,UpdateSignInSettingsInput,UpdateStorageSettingsInput} from '@meeting-notes/contracts'
export const providerNames:Record<AIProvider,string>={openrouter:'OpenRouter',openai:'OpenAI',anthropic:'Anthropic',elevenlabs:'ElevenLabs'}
export const providerKeyFields={openrouter:'openRouterApiKey',openai:'openAiApiKey',anthropic:'anthropicApiKey',elevenlabs:'elevenLabsApiKey'} as const
/** Sign-in as it is being edited. The client secret is write-only: typed to replace it, or marked for removal. */
export interface SignInDraft {password:boolean;google:boolean;allowedEmails:string[];googleClientId:string;googleClientSecret:string;removeGoogleSecret:boolean}
/** Where files are kept. Text fields hold what is typed; an empty secret means "keep the stored one". */
export interface StorageDraft {backend:'local'|'s3';endpoint:string;region:string;bucket:string;prefix:string;forcePathStyle:boolean;accessKeyId:string;secretAccessKey:string;removeSecret:boolean}
/** The iPhone app's identifiers as typed: app IDs separated by commas or new lines. */
export interface AppsDraft {iosAppIds:string;iosAppStoreId:string}
export interface InstanceDraft {/** Null when the server does not report them (an older server). */apps:AppsDraft|null;/** Text as typed; empty when the server does not report the limits. */liveMaxSessions:string;liveHoldSeconds:string;/** Null when the server does not report storage settings (an older server). */storage:StorageDraft|null;/** Null when the server does not report sign-in settings (an older server). */signIn:SignInDraft|null;instanceName:string;instanceLogo:string|null;keys:Record<AIProvider,string>;removed:Record<AIProvider,boolean>;chatModels:LanguageModelSelection[];generationModel:LanguageModelSelection;transcriptionModel:TranscriptionModelSelection;/** Null when live transcription is off. */liveTranscriptionModel:LiveTranscriptionModelSelection|null}
export const modelKey=(m:LanguageModelSelection|TranscriptionModelSelection)=>m.provider+':'+m.model
/** What turning live transcription back on selects. */
export const defaultLiveModel:LiveTranscriptionModelSelection={provider:'elevenlabs',model:'scribe_v3_live'}
/** One allow-list entry as the server stores it: a full address or "@domain", lower-case. Null when it is neither. */
export function allowedEmailEntry(raw:string):string|null{const entry=raw.trim().toLowerCase();return entry.length<=254&&/^[^\s@]*@[^\s@.]+(?:\.[^\s@.]+)+$/.test(entry)?entry:null}
/** Adds what was typed or pasted (several entries may be separated by commas, semicolons or spaces). */
export function addAllowedEmails(list:readonly string[],typed:string):{list:string[];problem:string|null}{
 const parts=typed.split(/[\s,;]+/).filter(Boolean),next=[...list]
 if(!parts.length)return{list:next,problem:'Enter an email address or a domain such as @example.com.'}
 for(const part of parts){const entry=allowedEmailEntry(part);if(!entry)return{list:[...list],problem:parts.length>1?`“${part}” is not an email address or a domain such as @example.com.`:'Enter a full email address or a domain such as @example.com.'};if(!next.includes(entry))next.push(entry)}
 if(next.length===list.length)return{list:next,problem:parts.length>1?'These are already on the list.':'This is already on the list.'}
 if(next.length>500)return{list:[...list],problem:'The list can hold up to 500 entries.'}
 return{list:next,problem:null}
}
const googleClientIdPattern=/^[A-Za-z0-9._-]{10,255}$/
function signInPatch(d:SignInDraft,s:NonNullable<AdminSettingsDTO['signIn']>):UpdateSignInSettingsInput{
 const patch:{-readonly[K in keyof UpdateSignInSettingsInput]:UpdateSignInSettingsInput[K]}={}
 if(d.password!==s.password)patch.password=d.password
 if(d.google!==s.google)patch.google=d.google
 if(JSON.stringify(d.allowedEmails)!==JSON.stringify(s.allowedEmails))patch.allowedEmails=d.allowedEmails
 if(d.googleClientId.trim()!==(s.googleClientId??''))patch.googleClientId=d.googleClientId.trim()||null
 if(d.removeGoogleSecret){if(s.googleClientSecretSet)patch.googleClientSecret=null}else if(d.googleClientSecret.trim())patch.googleClientSecret=d.googleClientSecret.trim()
 return patch
}
export const storageDraft=(s:StorageSettingsDTO):StorageDraft=>({backend:s.backend,endpoint:s.endpoint??'',region:s.region??'',bucket:s.bucket??'',prefix:s.prefix,forcePathStyle:s.forcePathStyle,accessKeyId:s.accessKeyId??'',secretAccessKey:'',removeSecret:false})
/** Only what differs from the saved settings. A prefix is compared the way the server stores it: no leading slash, one trailing slash. */
const tidyPrefix=(prefix:string)=>{const bare=prefix.trim().replace(/^\/+/,'').replace(/\/+$/,'');return bare?bare+'/':''}
export function storagePatch(d:StorageDraft,s:StorageSettingsDTO):UpdateStorageSettingsInput{
 const patch:{-readonly[K in keyof UpdateStorageSettingsInput]:UpdateStorageSettingsInput[K]}={}
 if(d.backend!==s.backend)patch.backend=d.backend
 if(d.endpoint.trim().replace(/\/+$/,'')!==(s.endpoint??''))patch.endpoint=d.endpoint.trim()||null
 if((d.region.trim()||'us-east-1')!==(s.region??'us-east-1'))patch.region=d.region.trim()||null
 if(d.bucket.trim()!==(s.bucket??''))patch.bucket=d.bucket.trim()||null
 if(tidyPrefix(d.prefix)!==s.prefix)patch.prefix=d.prefix.trim()
 if(d.forcePathStyle!==s.forcePathStyle)patch.forcePathStyle=d.forcePathStyle
 if(d.accessKeyId.trim()!==(s.accessKeyId??''))patch.accessKeyId=d.accessKeyId.trim()||null
 if(d.removeSecret){if(s.secretAccessKeySet)patch.secretAccessKey=null}else if(d.secretAccessKey.trim())patch.secretAccessKey=d.secretAccessKey.trim()
 return patch
}
/** What "Test connection" sends: the whole draft, so the server tries exactly what is on screen. A blank secret is left out and the stored one is used. */
export function storageTestInput(d:StorageDraft):StorageTestInput{
 return{backend:d.backend,endpoint:d.endpoint.trim()||null,region:d.region.trim()||null,bucket:d.bucket.trim()||null,prefix:d.prefix.trim(),forcePathStyle:d.forcePathStyle,accessKeyId:d.accessKeyId.trim()||null,...(d.removeSecret?{secretAccessKey:null}:d.secretAccessKey.trim()?{secretAccessKey:d.secretAccessKey.trim()}:{})}
}
/** Whether the draft would leave a usable bucket stored: what the server requires before S3 can be selected. */
export function bucketReady(d:StorageDraft,secretStored:boolean):boolean{return Boolean(d.bucket.trim()&&d.accessKeyId.trim())&&(d.removeSecret?false:Boolean(d.secretAccessKey.trim())||secretStored)}
export function storageProblem(d:StorageDraft,secretStored:boolean):string|null{
 const endpoint=d.endpoint.trim(),bucket=d.bucket.trim(),secret=d.secretAccessKey.trim()
 if(endpoint){
  let url:URL|null=null;try{url=new URL(endpoint)}catch{url=null}
  if(!url||url.username||url.password||url.search||url.hash)return 'Enter the storage endpoint as a full address, such as https://s3.example.com.'
  if(url.protocol!=='https:'&&!(url.protocol==='http:'&&['localhost','127.0.0.1','[::1]'].includes(url.hostname)))return 'The storage endpoint must use https.'
 }
 if(bucket&&(!/^[a-z0-9][a-z0-9.-]{1,61}[a-z0-9]$/.test(bucket)||bucket.includes('..')))return 'Bucket names are 3 to 63 lower-case letters, digits, hyphens and dots.'
 if(d.region.trim()&&!/^[a-z0-9-]{1,40}$/.test(d.region.trim()))return 'Enter the storage region as the store names it, such as us-east-1.'
 if(d.prefix.trim()&&!/^[A-Za-z0-9!_.*'()\/-]{1,200}$/.test(d.prefix.trim()))return 'The storage prefix may contain letters, digits and - _ . / only.'
 if(!d.removeSecret&&secret&&(secret.length<8||secret.length>512||/\s/.test(secret)))return 'Enter the secret access key exactly as the store shows it.'
 if(d.backend==='s3'&&!bucketReady(d,secretStored))return 'Add the bucket, access key ID and secret access key to use S3-compatible storage.'
 return null
}
const DEFAULT_IOS_APP_ID='LZWN2FF7YB.dev.cereal.mobile',iosAppIdPattern=/^[A-Z0-9]{10}\.[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+$/
/** The app IDs in a draft, in order, without repeats. */
export const appIdList=(text:string)=>[...new Set(text.split(/[\s,]+/).map(id=>id.trim()).filter(Boolean))]
const wholeNumber=(text:string)=>/^\d{1,5}$/.test(text.trim())?Number(text.trim()):null
export function appsProblem(d:AppsDraft):string|null{
 const ids=appIdList(d.iosAppIds)
 if(ids.some(id=>!iosAppIdPattern.test(id)))return 'An iPhone app ID is the team ID, a dot and the bundle ID, such as '+DEFAULT_IOS_APP_ID+'.'
 if(ids.length>10)return 'List up to 10 iPhone app IDs.'
 if(d.iosAppStoreId.trim()&&!/^\d{1,20}$/.test(d.iosAppStoreId.trim()))return 'The App Store ID is the number in the app’s App Store address.'
 return null
}
export function liveLimitsProblem(d:Pick<InstanceDraft,'liveMaxSessions'|'liveHoldSeconds'>):string|null{
 const sessions=wholeNumber(d.liveMaxSessions),hold=wholeNumber(d.liveHoldSeconds)
 if(d.liveMaxSessions!==''&&(sessions===null||sessions<1||sessions>200))return 'Live recordings at once: enter a whole number from 1 to 200.'
 if(d.liveHoldSeconds!==''&&(hold===null||hold>3600))return 'Seconds to keep speakers after Stop: enter a whole number from 0 to 3600.'
 return null
}
export function instanceDraft(s:AdminSettingsDTO):InstanceDraft{return{apps:s.apps?{iosAppIds:s.apps.iosAppIds.length===1&&s.apps.iosAppIds[0]===DEFAULT_IOS_APP_ID?'':s.apps.iosAppIds.join(', '),iosAppStoreId:s.apps.iosAppStoreId??''}:null,liveMaxSessions:s.liveMaxSessions===undefined?'':String(s.liveMaxSessions),liveHoldSeconds:s.liveHoldSeconds===undefined?'':String(s.liveHoldSeconds),storage:s.storage?storageDraft(s.storage):null,signIn:s.signIn?{password:s.signIn.password,google:s.signIn.google,allowedEmails:[...s.signIn.allowedEmails],googleClientId:s.signIn.googleClientId??'',googleClientSecret:'',removeGoogleSecret:false}:null,instanceName:s.instanceName,instanceLogo:s.instanceLogo,keys:{openrouter:'',openai:'',anthropic:'',elevenlabs:''},removed:{openrouter:false,openai:false,anthropic:false,elevenlabs:false},chatModels:[...s.chatModels],generationModel:{...s.generationModel},transcriptionModel:{...s.transcriptionModel},liveTranscriptionModel:s.liveTranscriptionModel?{...s.liveTranscriptionModel}:null}}
export function instancePatch(d:InstanceDraft,s:AdminSettingsDTO):UpdateAdminSettingsInput{
 const patch:{-readonly[K in keyof UpdateAdminSettingsInput]:UpdateAdminSettingsInput[K]}={}
 if(d.instanceName.trim()!==s.instanceName)patch.instanceName=d.instanceName.trim()
 if(d.instanceLogo!==s.instanceLogo)patch.instanceLogo=d.instanceLogo
 for(const id of Object.keys(providerNames) as AIProvider[])if(d.removed[id])patch[providerKeyFields[id]]=null;else if(d.keys[id].trim())patch[providerKeyFields[id]]=d.keys[id].trim()
 if(JSON.stringify(d.chatModels)!==JSON.stringify(s.chatModels))patch.chatModels=d.chatModels
 if(modelKey(d.generationModel)!==modelKey(s.generationModel))patch.generationModel=d.generationModel
 if(modelKey(d.transcriptionModel)!==modelKey(s.transcriptionModel))patch.transcriptionModel=d.transcriptionModel
 const live=(m:LiveTranscriptionModelSelection|null|undefined)=>m?modelKey(m):''
 if(live(d.liveTranscriptionModel)!==live(s.liveTranscriptionModel))patch.liveTranscriptionModel=d.liveTranscriptionModel
 if(d.signIn&&s.signIn){const signIn=signInPatch(d.signIn,s.signIn);if(Object.keys(signIn).length)patch.signIn=signIn}
 if(d.storage&&s.storage){const storage=storagePatch(d.storage,s.storage);if(Object.keys(storage).length)patch.storage=storage}
 const sessions=wholeNumber(d.liveMaxSessions),hold=wholeNumber(d.liveHoldSeconds)
 if(s.liveMaxSessions!==undefined&&sessions!==null&&sessions!==s.liveMaxSessions)patch.liveMaxSessions=sessions
 if(s.liveHoldSeconds!==undefined&&hold!==null&&hold!==s.liveHoldSeconds)patch.liveHoldSeconds=hold
 if(d.apps&&s.apps){
  const ids=appIdList(d.apps.iosAppIds),savedIds=s.apps.iosAppIds.length===1&&s.apps.iosAppIds[0]===DEFAULT_IOS_APP_ID?[]:s.apps.iosAppIds,storeId=d.apps.iosAppStoreId.trim()||null
  const apps:{iosAppIds?:string[];iosAppStoreId?:string|null}={}
  if(JSON.stringify(ids)!==JSON.stringify(savedIds))apps.iosAppIds=ids
  if(storeId!==s.apps.iosAppStoreId)apps.iosAppStoreId=storeId
  if(Object.keys(apps).length)patch.apps=apps
 }
 return patch
}
/** Whether the draft would leave a Google client stored: what the server requires before Google sign-in can be on. */
export function googleClientReady(d:SignInDraft,secretStored:boolean):boolean{return Boolean(d.googleClientId.trim())&&(d.removeGoogleSecret?false:Boolean(d.googleClientSecret.trim())||secretStored)}
export function signInProblem(d:SignInDraft,secretStored:boolean):string|null{
 if(!d.password&&!d.google)return 'Keep at least one sign-in method turned on.'
 const id=d.googleClientId.trim(),secret=d.googleClientSecret.trim()
 if(id&&!googleClientIdPattern.test(id))return 'Enter the Google client ID exactly as Google Cloud shows it.'
 if(!d.removeGoogleSecret&&secret&&(secret.length<8||secret.length>512||/\s/.test(secret)))return 'Enter the Google client secret exactly as Google Cloud shows it.'
 if(d.google&&!googleClientReady(d,secretStored))return 'Add a Google client ID and client secret to turn on Google sign-in.'
 if(d.allowedEmails.length>500)return 'The allowed list can hold up to 500 entries.'
 return null
}
export function instanceProblem(d:InstanceDraft,s?:AdminSettingsDTO):string|null{
 if(!d.instanceName.trim()||d.instanceName.trim().length>100)return 'Enter a team name of 1–100 characters.'
 if(!d.chatModels.length)return 'Keep at least one chat model.'
 if(d.chatModels.length>20)return 'Choose up to 20 chat models.'
 for(const id of Object.keys(providerNames) as AIProvider[])if(!d.removed[id]&&d.keys[id].trim()&&(d.keys[id].trim().length<10||d.keys[id].trim().length>1000||/[\r\n]/.test(d.keys[id])))return 'Enter a valid, single-line API key.'
 if(d.signIn){const problem=signInProblem(d.signIn,s?.signIn?.googleClientSecretSet??false);if(problem)return problem}
 if(d.storage){const problem=storageProblem(d.storage,s?.storage?.secretAccessKeySet??false);if(problem)return problem}
 const limits=liveLimitsProblem(d);if(limits)return limits
 if(d.apps)return appsProblem(d.apps)
 return null
}
