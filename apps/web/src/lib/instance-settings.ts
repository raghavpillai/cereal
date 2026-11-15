import type {AdminSettingsDTO,AIProvider,LanguageModelSelection,LiveTranscriptionModelSelection,TranscriptionModelSelection,UpdateAdminSettingsInput} from '@meeting-notes/contracts'
export const providerNames:Record<AIProvider,string>={openrouter:'OpenRouter',openai:'OpenAI',anthropic:'Anthropic',elevenlabs:'ElevenLabs'}
export const providerKeyFields={openrouter:'openRouterApiKey',openai:'openAiApiKey',anthropic:'anthropicApiKey',elevenlabs:'elevenLabsApiKey'} as const
export interface InstanceDraft {instanceName:string;instanceLogo:string|null;keys:Record<AIProvider,string>;removed:Record<AIProvider,boolean>;chatModels:LanguageModelSelection[];generationModel:LanguageModelSelection;transcriptionModel:TranscriptionModelSelection;/** Null when live transcription is off. */liveTranscriptionModel:LiveTranscriptionModelSelection|null}
export const modelKey=(m:LanguageModelSelection|TranscriptionModelSelection)=>m.provider+':'+m.model
/** What turning live transcription back on selects. */
export const defaultLiveModel:LiveTranscriptionModelSelection={provider:'elevenlabs',model:'scribe_v3_live'}
export function instanceDraft(s:AdminSettingsDTO):InstanceDraft{return{instanceName:s.instanceName,instanceLogo:s.instanceLogo,keys:{openrouter:'',openai:'',anthropic:'',elevenlabs:''},removed:{openrouter:false,openai:false,anthropic:false,elevenlabs:false},chatModels:[...s.chatModels],generationModel:{...s.generationModel},transcriptionModel:{...s.transcriptionModel},liveTranscriptionModel:s.liveTranscriptionModel?{...s.liveTranscriptionModel}:null}}
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
 return patch
}
export function instanceProblem(d:InstanceDraft):string|null{
 if(!d.instanceName.trim()||d.instanceName.trim().length>100)return 'Enter a team name of 1–100 characters.'
 if(!d.chatModels.length)return 'Keep at least one chat model.'
 if(d.chatModels.length>20)return 'Choose up to 20 chat models.'
 for(const id of Object.keys(providerNames) as AIProvider[])if(!d.removed[id]&&d.keys[id].trim()&&(d.keys[id].trim().length<10||d.keys[id].trim().length>1000||/[\r\n]/.test(d.keys[id])))return 'Enter a valid, single-line API key.'
 return null
}
