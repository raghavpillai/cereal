import {useCallback,useEffect,useId,useRef,useState,type ReactNode} from 'react'
import type {AdminSettingsDTO,AIProvider,LanguageModelSelection,LiveTranscriptionModelSelection,ModelCatalogDTO,UpdateAdminSettingsInput} from '@meeting-notes/contracts'
import type {AdminClient} from '../../lib/admin-client'
import {AdminClientError} from '../../lib/admin-client'
import {compatibleBaseUrlProblem} from '@meeting-notes/contracts/compatible'
import {instanceDraft,instancePatch,instanceProblem,modelKey,plausibleModelId,providerNames,providerShortNames,savedKeyText,tidyBaseUrl,type InstanceDraft} from '../../lib/instance-settings'
import {Button} from '../ui/button'
import {Input} from '../ui/input'
import {Badge} from '../ui/badge'
import {Card,CardContent,CardHeader,CardTitle} from '../ui/card'
import {Select,SelectContent,SelectItem,SelectTrigger} from '../ui/select'
import {SignInSettings} from './sign-in-panel'
import {StorageSettings} from './storage-panel'
import {Check,ChevronRight,ImagePlus,Loader2,Plus,RefreshCw,Trash2,X} from 'lucide-react'

export function ProviderLogo({provider}:{provider:AIProvider}){return <img className="provider-logo" src={`/providers/${provider}.svg`} width="20" height="20" alt=""/>}
export const modelLogo=(provider:AIProvider,model:string):AIProvider=>provider==='openrouter'&&/^(openai|anthropic)\//.test(model)?model.split('/')[0] as AIProvider:provider
export type CatalogState={data?:ModelCatalogDTO;busy?:boolean;error?:string}
const providers=Object.keys(providerNames) as AIProvider[]
export function SettingsPanel({settings,client,expired,busy,save,replace,children,viewerEmail}:{viewerEmail?:string;children?:ReactNode;settings:AdminSettingsDTO;client:AdminClient;expired:()=>void;busy:boolean;save:(patch:UpdateAdminSettingsInput)=>Promise<void>;/** The settings changed outside the form (the sign-in method was switched): show them. */replace:(settings:AdminSettingsDTO)=>void}){
 const formId=useId()
 const[draft,setDraft]=useState(()=>instanceDraft(settings)),[editing,setEditing]=useState<AIProvider|null>(null),[catalogs,setCatalogs]=useState<Partial<Record<AIProvider,CatalogState>>>({}),[logoError,setLogoError]=useState<string|null>(null)
 const requests=useRef(new Map<AIProvider,AbortController>()),logoVersion=useRef(0),logoInput=useRef<HTMLInputElement>(null)
 const draftRef=useRef(draft);draftRef.current=draft
 const problem=instanceProblem(draft,settings),patch=instancePatch(draft,settings),dirty=Object.keys(patch).length>0
 const update=<K extends keyof InstanceDraft>(key:K,value:InstanceDraft[K])=>setDraft(old=>({...old,[key]:value}))
 const loadCatalog=useCallback(async(provider:AIProvider,refresh=false)=>{
  requests.current.get(provider)?.abort();const controller=new AbortController();requests.current.set(provider,controller)
  const current=draftRef.current,key=current.removed[provider]?'':current.keys[provider].trim(),saved=settings.providers.find(p=>p.id===provider)
  // The OpenAI-compatible server is asked at the address on screen, with a key only when one is typed (else the saved one).
  const compatible=provider==='openai_compatible',base=current.compatibleBaseUrl.trim()
  if(compatible?!base||!!compatibleBaseUrlProblem(base):!key&&(!saved?.configured||current.removed[provider])&&provider!=='openrouter')return
  setCatalogs(old=>({...old,[provider]:{...old[provider],busy:true,error:undefined}}))
  try{const data=await client.getModelCatalog({provider,...(key?{apiKey:key}:{}),...(compatible?{baseUrl:base}:{}),refresh},controller.signal);if(!controller.signal.aborted)setCatalogs(old=>({...old,[provider]:{data,busy:false}}))}
  catch(error){if(controller.signal.aborted)return;if(error instanceof AdminClientError&&error.status===401){expired();return}setCatalogs(old=>({...old,[provider]:{busy:false,error:compatible&&error instanceof AdminClientError&&error.serverMessage?error.serverMessage:'Couldn’t load models. Check the API key and try again.'}}))}
 },[client,expired,settings.providers])
 useEffect(()=>{for(const p of settings.providers)if(p.configured||p.id==='openrouter')void loadCatalog(p.id);return()=>{requests.current.forEach(c=>c.abort());logoVersion.current++}},[loadCatalog,settings.providers])
 async function readLogo(file:File|undefined){
  if(!file)return;const version=++logoVersion.current;setLogoError(null)
  if(!['image/png','image/jpeg','image/webp'].includes(file.type)||file.size>250_000){setLogoError('Choose a PNG, JPG, or WebP smaller than 250 KB.');return}
  const reader=new FileReader();reader.onload=()=>{if(version!==logoVersion.current)return;const value=reader.result;if(typeof value==='string'&&/^data:image\/(png|jpeg|webp);base64,/.test(value))update('instanceLogo',value);else setLogoError('This image could not be read.')};reader.onerror=()=>setLogoError('This image could not be read.');reader.readAsDataURL(file)
 }
 function setKey(provider:AIProvider,value:string){requests.current.get(provider)?.abort();setCatalogs(old=>({...old,[provider]:{}}));setDraft(old=>({...old,keys:{...old.keys,[provider]:value},removed:{...old.removed,[provider]:false}}))}
 function setBaseUrl(value:string){requests.current.get('openai_compatible')?.abort();setCatalogs(old=>({...old,openai_compatible:{}}));update('compatibleBaseUrl',value)}
 // The page's sections in order. Administrators is drawn by the caller, and only for the owner.
 const sections:Array<[string,string]>=[['identity','Identity'],['providers','AI providers'],['models','Models'],...(settings.signIn?[['sign-in','Sign-in'] as [string,string]]:[]),...(settings.storage?[['storage','Storage'] as [string,string]]:[]),...(settings.apps?[['apps','Apps'] as [string,string]]:[]),['administrators','Administrators']]
 const[administratorsShown,setAdministratorsShown]=useState(false)
 useEffect(()=>{setAdministratorsShown(document.getElementById('administrators')!==null)})
 // The OpenAI-compatible server is usable with a valid address; the others with a key that is not being removed.
 const available=(id:AIProvider)=>id==='openai_compatible'?!!draft.compatibleBaseUrl.trim()&&!compatibleBaseUrlProblem(draft.compatibleBaseUrl):!draft.removed[id]&&(Boolean(draft.keys[id].trim())||Boolean(settings.providers.find(p=>p.id===id)?.configured))
 const compatibleSaved=settings.openAiCompatible,compatibleChanged=tidyBaseUrl(draft.compatibleBaseUrl)!==(compatibleSaved?.baseUrl??'')
 const status=(id:AIProvider)=>id==='openai_compatible'?(compatibleChanged&&available(id)?'Address ready':available(id)?'Connected':'Not connected'):draft.keys[id]?'Key ready':available(id)?'Connected':'Not connected'
 return <><form id={formId} className="instance-form" onSubmit={event=>{event.preventDefault();if(!problem&&dirty)void save(patch)}}>
 <fieldset disabled={busy} className="admin-fieldset">
  <nav className="settings-index" aria-label="Sections">{sections.filter(([id])=>id!=='administrators'||administratorsShown).map(([id,label])=><a key={id} href={`#${id}`}>{label}</a>)}</nav>
  {settings.unusedEnvironment&&settings.unusedEnvironment.length>0&&<p className="environment-note" role="note">These are still set in the server’s environment but are no longer read, because their settings now live on this page: <code>{settings.unusedEnvironment.join(', ')}</code>. You can delete them.</p>}
  <Card id="identity"><CardHeader><CardTitle>Identity</CardTitle></CardHeader><CardContent>
   <div className="identity-fields"><div><label className="settings-label" htmlFor="instance-name">Team name</label><Input id="instance-name" value={draft.instanceName} maxLength={100} required onChange={e=>update('instanceName',e.target.value)}/></div>
    <div><label className="settings-label">Team logo</label><div className="logo-control"><div className="team-logo-preview">{draft.instanceLogo?<img src={draft.instanceLogo} alt="Team logo"/>:<img src="/mark.svg" width="26" height="26" alt="Cereal"/>}</div><div><Button type="button" variant="outline" onClick={()=>logoInput.current?.click()}><ImagePlus/>Upload logo</Button><p className="settings-help">PNG, JPG, WebP · 250 KB max</p></div>{draft.instanceLogo&&<Button type="button" variant="ghost" size="icon" aria-label="Remove team logo" onClick={()=>{logoVersion.current++;update('instanceLogo',null)}}><X/></Button>}</div><input ref={logoInput} type="file" className="sr-only" accept="image/png,image/jpeg,image/webp" aria-label="Upload team logo" onChange={e=>void readLogo(e.target.files?.[0])}/></div>
   </div>{logoError&&<p role="alert" className="settings-error">{logoError}</p>}
  </CardContent></Card>
  <Card id="providers"><CardHeader><CardTitle>AI providers</CardTitle>  </CardHeader><CardContent><div className="provider-list">{providers.map(id=>{const saved=settings.providers.find(p=>p.id===id),connected=available(id),state=catalogs[id],hint=saved?.configured&&!draft.removed[id]?saved.hint:null;return <div className="provider-entry" key={id}>
   <div className="provider-row"><div className="provider-icon"><ProviderLogo provider={id}/></div><div className="provider-details"><strong title={providerNames[id]}>{providerShortNames[id]?<><span className="provider-name-long">{providerNames[id]}</span><span className="provider-name-short" aria-hidden="true">{providerShortNames[id]}</span></>:providerNames[id]}</strong>{hint&&<span className="provider-hint" title={hint}>{hint}{saved?.source==='environment'?' · server':''}</span>}</div><Badge variant="outline" className={connected?'provider-connected':'provider-disconnected'}><span/>{status(id)}</Badge><Button type="button" variant="ghost" size="sm" aria-expanded={editing===id} onClick={()=>setEditing(editing===id?null:id)}>{connected?'Manage':'Connect'}<ChevronRight className={editing===id?'rotate-90':''}/></Button></div>
   {editing===id&&id!=='openai_compatible'&&<div className="provider-editor"><label className="settings-label" htmlFor={`${id}-key`}>{providerNames[id]} API key</label><div className="provider-key-row"><Input id={`${id}-key`} type="password" autoComplete="new-password" value={draft.keys[id]} disabled={draft.removed[id]} placeholder={saved?.configured?'Type a new key to replace it':'Enter your API key'} onChange={e=>setKey(id,e.target.value)}/>{(connected||id==='openrouter')&&<Button type="button" variant="outline" disabled={state?.busy} onClick={()=>void loadCatalog(id,true)}>{state?.busy?<Loader2 className="animate-spin"/>:<RefreshCw/>}Load models</Button>}</div>
   <p className="settings-help">{saved?.configured&&!draft.removed[id]?savedKeyText(saved)?.replace(/\. Type.*$/,'. '):''}{saved?.source==='environment'?'A key saved here overrides it.':'Keys are encrypted on the server and never returned to the browser.'}</p>
   {saved?.source==='stored'&&<label className="remove-key"><input type="checkbox" checked={draft.removed[id]} onChange={e=>update('removed',{...draft.removed,[id]:e.target.checked})}/>Remove saved key</label>}
   {state?.error&&<p role="alert" className="settings-error">{state.error}</p>}{state?.data&&<p className="settings-help"><Check size={12}/>{state.data.models.length} models available{draft.keys[id]?' · Save changes to connect':''}</p>}
   </div>}
   {editing===id&&id==='openai_compatible'&&<div className="provider-editor">
    <label className="settings-label" htmlFor="compatible-base-url">Server address</label><div className="provider-key-row"><Input id="compatible-base-url" type="url" inputMode="url" autoComplete="off" autoCapitalize="none" spellCheck={false} maxLength={2048} placeholder="http://localhost:11434/v1" aria-describedby="compatible-base-url-help" value={draft.compatibleBaseUrl} onChange={e=>setBaseUrl(e.target.value)}/>{connected&&<Button type="button" variant="outline" disabled={state?.busy} onClick={()=>void loadCatalog(id,true)}>{state?.busy?<Loader2 className="animate-spin"/>:<RefreshCw/>}Load models</Button>}</div>
    <p id="compatible-base-url-help" className="settings-help">{draft.compatibleBaseUrl.trim()&&compatibleBaseUrlProblem(draft.compatibleBaseUrl)||'Ollama, vLLM, LM Studio, or any /v1 API: where its API starts. Requests go to …/chat/completions and …/models there.'}{compatibleSaved?.baseUrl&&!draft.compatibleBaseUrl.trim()?' Saving with no address disconnects it.':''}</p>
    {connected&&<><label className="settings-label" htmlFor="compatible-key">API key <span className="settings-optional">optional</span></label><Input id="compatible-key" type="password" autoComplete="new-password" value={draft.keys[id]} disabled={draft.removed[id]} placeholder={compatibleSaved?.apiKeySet?'Type a new key to replace it':'Leave blank if not needed'} onChange={e=>setKey(id,e.target.value)}/>
    <p className="settings-help">{compatibleSaved?.apiKeySet&&!draft.removed[id]?compatibleChanged&&!draft.keys[id].trim()?'The saved key is only sent to the saved address: saving a new address removes it, so type it again if this server needs one. ':`${compatibleSaved.apiKeyHint??'A key'} is saved. `:''}Ollama and LM Studio need none. A key is encrypted on the server and never returned to the browser.</p>
    {compatibleSaved?.apiKeySet&&<label className="remove-key"><input type="checkbox" checked={draft.removed[id]} onChange={e=>update('removed',{...draft.removed,[id]:e.target.checked})}/>Remove saved key</label>}</>}
    {state?.error&&<p role="alert" className="settings-error">{state.error}</p>}{state?.data&&<p className="settings-help"><Check size={12}/>{state.data.models.length?`${state.data.models.length} models available`:'No models listed. Type a model name in the model choices below.'}{compatibleChanged||draft.keys[id]?' · Save changes to connect':''}</p>}
   </div>}
  </div>})}</div><p className="provider-security">API keys are encrypted and kept on your server.</p></CardContent></Card>
  <Card id="models"><CardHeader><CardTitle>Models</CardTitle></CardHeader><CardContent>
   <div className="model-section"><div className="model-section-heading"><h3>Chat</h3><span>{draft.chatModels.length} enabled</span></div><div className="model-controls">
    <div className="chat-model-list">{draft.chatModels.map((model,i)=><div className="chat-model-row" key={modelKey(model)}><ModelPicker value={model} kind="language" catalogs={catalogs} available={available} load={loadCatalog} label={`Chat model ${i+1}`} change={next=>{if(draft.chatModels.some((m,index)=>index!==i&&modelKey(m)===modelKey(next)))return;update('chatModels',draft.chatModels.map((m,index)=>index===i?next as LanguageModelSelection:m))}}/>{i===0?<Badge variant="secondary" className="default-model">Default</Badge>:<Button type="button" variant="ghost" size="icon" aria-label={`Make ${model.model} the default`} onClick={()=>update('chatModels',[model,...draft.chatModels.filter((_,index)=>index!==i)])}><Check size={14}/></Button>}<Button type="button" variant="ghost" size="icon" disabled={draft.chatModels.length===1} aria-label={`Remove ${model.model}`} onClick={()=>update('chatModels',draft.chatModels.filter((_,index)=>index!==i))}><Trash2/></Button></div>)}</div>
    {draft.chatModels.length<20&&<ModelPicker kind="language" catalogs={catalogs} available={available} load={loadCatalog} label="Add chat model" excluded={draft.chatModels.map(modelKey)} change={model=>update('chatModels',[...draft.chatModels,model as LanguageModelSelection])}/>}
   </div></div>
   {/* Transcription is live only and always on: only the live model can change. */}
   <div className="model-section"><div className="model-section-heading"><h3>Live transcription</h3><span>{available('elevenlabs')?'Always on':'Needs ElevenLabs'}</span></div><div className="model-controls">
    <ModelPicker value={draft.liveTranscriptionModel} kind="live_transcription" catalogs={catalogs} available={available} load={loadCatalog} label="Live transcription model" change={model=>update('liveTranscriptionModel',model as LiveTranscriptionModelSelection)}/>
    <p className="settings-help">{available('elevenlabs')?'Words appear within about a second, with speaker labels.':'Connect ElevenLabs above to start transcribing.'}</p>
    {/* The limits only matter once live transcription can run. */}
    {settings.liveMaxSessions!==undefined&&available('elevenlabs')&&<div className="live-limits">
     <div><label className="settings-label" htmlFor="live-max-sessions">Live recordings at once</label><Input id="live-max-sessions" type="text" inputMode="numeric" autoComplete="off" maxLength={3} aria-describedby="live-max-sessions-help" value={draft.liveMaxSessions} onChange={e=>update('liveMaxSessions',e.target.value)}/><p id="live-max-sessions-help" className="settings-help">1 to 200. A recording started beyond this gets no live transcript until a place frees up; its app keeps retrying.</p></div>
     <div><label className="settings-label" htmlFor="live-hold-seconds">Seconds to keep speakers after Stop</label><Input id="live-hold-seconds" type="text" inputMode="numeric" autoComplete="off" maxLength={4} aria-describedby="live-hold-seconds-help" value={draft.liveHoldSeconds} onChange={e=>update('liveHoldSeconds',e.target.value)}/><p id="live-hold-seconds-help" className="settings-help">0 to 3600. A Resume within this time keeps its speaker labels; 0 turns it off.</p></div>
    </div>}
   </div></div>
   <div className="model-section"><div className="model-section-heading"><h3>Meeting notes</h3></div><ModelPicker value={draft.generationModel} kind="language" catalogs={catalogs} available={available} load={loadCatalog} label="Meeting notes model" change={model=>update('generationModel',model as LanguageModelSelection)}/></div>
  </CardContent></Card>
  {draft.signIn&&settings.signIn&&<SignInSettings viewerEmail={viewerEmail} draft={draft.signIn} saved={settings.signIn} change={next=>update('signIn',next)} client={client} switched={replace} expired={expired}/>}
  {draft.storage&&settings.storage&&<StorageSettings draft={draft.storage} saved={settings.storage} change={next=>update('storage',next)} test={async(input,signal)=>{try{return await client.testStorage(input,signal)}catch(error){if(error instanceof AdminClientError&&error.status===401)expired();throw error}}}/>}
  {draft.apps&&settings.apps&&<Card id="apps"><CardHeader><CardTitle>Apps</CardTitle></CardHeader><CardContent>
   <div className="model-section"><div className="model-section-heading"><h3>iPhone app</h3><span>{draft.apps.iosAppStoreId.trim()?'On the App Store':'Not published'}</span></div><div className="model-controls sign-in-block"><div className="storage-fields">
    <div><label className="settings-label" htmlFor="ios-app-ids">App IDs that open note links</label><Input id="ios-app-ids" type="text" autoComplete="off" autoCapitalize="none" spellCheck={false} aria-describedby="ios-app-ids-help" placeholder="LZWN2FF7YB.dev.cereal.mobile" value={draft.apps.iosAppIds} onChange={e=>update('apps',{...draft.apps!,iosAppIds:e.target.value})}/><p id="ios-app-ids-help" className="settings-help">Team ID, a dot, the bundle ID; several separated by commas. Leave blank for the Cereal app.</p></div>
    <div><label className="settings-label" htmlFor="ios-app-store-id">App Store ID</label><Input id="ios-app-store-id" type="text" inputMode="numeric" autoComplete="off" maxLength={20} aria-describedby="ios-app-store-id-help" placeholder="Not published yet" value={draft.apps.iosAppStoreId} onChange={e=>update('apps',{...draft.apps!,iosAppStoreId:e.target.value})}/><p id="ios-app-store-id-help" className="settings-help">The number in the app’s App Store address. Adds the iPhone link on the download page and the “open in the app” banner in Safari.</p></div>
   </div></div></div>
  </CardContent></Card>}
 </fieldset>
 </form>
 {children}
  <div className="instance-save"><span aria-live="polite">{busy?<><Loader2 size={14} className="animate-spin"/>Saving changes…</>:problem??(dirty?'You have unsaved changes.':<><Check size={14}/>All changes saved</>)}</span><Button form={formId} type="submit" disabled={busy||!!problem||!dirty}>Save changes</Button></div>
 </>
}
export function ModelPicker({value,kind,catalogs,available,load,label,change,excluded=[]}:{value?:LanguageModelSelection|LiveTranscriptionModelSelection;kind:'language'|'live_transcription';catalogs:Partial<Record<AIProvider,CatalogState>>;available:(id:AIProvider)=>boolean;load:(id:AIProvider,refresh?:boolean)=>Promise<void>;label:string;change:(m:LanguageModelSelection|LiveTranscriptionModelSelection)=>void;excluded?:string[]}){
 const[query,setQuery]=useState(''),id=useId(),supported:readonly AIProvider[]=kind==='language'?['openrouter','openai','anthropic','openai_compatible']:['elevenlabs']
 const all=supported.flatMap(provider=>(catalogs[provider]?.data?.models??[]).filter(m=>m.capabilities.includes(kind)&&!excluded.includes(modelKey({provider,model:m.id}))).map(m=>({...m,provider})))
 const matches=all.filter(m=>(m.name+' '+m.id+' '+providerNames[m.provider]).toLowerCase().includes(query.toLowerCase())).slice(0,100)
 const current=value?all.find(m=>modelKey({provider:m.provider,model:m.id})===modelKey(value)):undefined
 const errors=supported.filter(p=>catalogs[p]?.error),pending=supported.some(p=>catalogs[p]?.busy),selected=value?modelKey(value):''
 // An OpenAI-compatible server may list nothing, or not everything it serves: its model can be named by hand. The
 // server tries it before saving.
 const typed=query.trim(),manual=kind==='language'&&available('openai_compatible')&&plausibleModelId(typed)&&!all.some(m=>m.provider==='openai_compatible'&&m.id===typed)&&!excluded.includes('openai_compatible:'+typed)?'openai_compatible:'+typed:null
 return <div className={`model-picker${!value?' add-model-picker':''}`}><Select value={selected} onValueChange={key=>{const match=all.find(m=>modelKey({provider:m.provider,model:m.id})===key);if(match)change({provider:match.provider,model:match.id} as LanguageModelSelection);else if(manual&&key===manual)change({provider:'openai_compatible',model:typed})}} onOpenChange={open=>{if(!open){setQuery('');return}for(const p of supported)if(!catalogs[p]?.data&&!catalogs[p]?.busy&&(available(p)||p==='openrouter'))void load(p)}}>
  <SelectTrigger aria-label={label} className="model-select-trigger">{value?<span className="selected-model"><ProviderLogo provider={modelLogo(value.provider,value.model)}/><span>{current?.name??value.model}</span><span className="selected-provider">{providerNames[value.provider]}</span></span>:<span className="add-model-label"><Plus size={14}/>{label}</span>}</SelectTrigger>
  <SelectContent className="instance-select model-select-content" position="popper"><div className="model-search" onKeyDown={e=>e.stopPropagation()}><Input id={id} aria-label={`Search ${label.toLowerCase()}`} placeholder="Search models…" value={query} onChange={e=>setQuery(e.target.value)}/></div>
   {value&&!matches.some(m=>modelKey({provider:m.provider,model:m.id})===selected)&&<SelectItem value={selected}><span className="provider-choice"><ProviderLogo provider={modelLogo(value.provider,value.model)}/>{value.model}</span></SelectItem>}
   {matches.map(m=><SelectItem key={modelKey({provider:m.provider,model:m.id})} value={modelKey({provider:m.provider,model:m.id})}><span className="provider-choice"><ProviderLogo provider={modelLogo(m.provider,m.id)}/><span className="catalog-model-name">{m.name}<small>{providerNames[m.provider]} · {m.id}</small></span></span></SelectItem>)}
   {manual&&<SelectItem value={manual}><span className="provider-choice"><ProviderLogo provider="openai_compatible"/><span className="catalog-model-name">Use “{typed}”<small>{providerNames.openai_compatible} · typed by hand</small></span></span></SelectItem>}
   {!matches.length&&!manual&&<div className="catalog-message">{pending?'Loading available models…':errors.length?'Couldn’t load models. Retry below.':kind==='language'&&available('openai_compatible')?'No models found. Type a model name to use it with your OpenAI-compatible server.':'Connect a provider to see its models.'}</div>}
   {all.length>100&&!query&&<div className="catalog-message">Search to find more models.</div>}
   {supported.map(p=>catalogs[p]?.data?.notice?<p className="catalog-message" key={p}>{catalogs[p]!.data!.notice}</p>:null)}
   <div className="catalog-refresh"><Button type="button" variant="ghost" size="sm" disabled={pending} onClick={e=>{e.preventDefault();for(const p of supported)if(available(p)||p==='openrouter')void load(p,true)}}><RefreshCw className={pending?'animate-spin':''}/>Refresh models</Button><span>Live provider catalogs</span></div>
  </SelectContent>
 </Select></div>
}
