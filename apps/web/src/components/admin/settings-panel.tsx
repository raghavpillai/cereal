import {useCallback,useEffect,useId,useLayoutEffect,useReducer,useRef,useState,type CSSProperties,type ReactNode,type RefObject} from 'react'
import {flushSync} from 'react-dom'
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
import {Reveal,Swap} from './shared'
import {createSectionMotion,saveLine,saveState,sectionDirection,useFlow,type SaveState,type SectionMotion} from './settings-motion'
import {reducedMotion} from './update-motion'
import {Check,ChevronRight,ImagePlus,Loader2,Plus,RefreshCw,Trash2,TriangleAlert,X} from 'lucide-react'

export function ProviderLogo({provider}:{provider:AIProvider}){return <img className="provider-logo" src={`/providers/${provider}.svg`} width="20" height="20" alt=""/>}
export const modelLogo=(provider:AIProvider,model:string):AIProvider=>provider==='openrouter'&&/^(openai|anthropic)\//.test(model)?model.split('/')[0] as AIProvider:provider
export type CatalogState={data?:ModelCatalogDTO;busy?:boolean;error?:string}
const providers=Object.keys(providerNames) as AIProvider[]
/** A section that is not part of the settings form: it acts at once and has no "Save changes" (Administrators, Updates). */
export type ExtraSection={id:string;label:string;content:ReactNode}
/** Which section the address names (`/admin#models`), so reload, back and a shared link all open the same one.
 * An unknown or missing name is the first section. */
export function sectionFromHash(hash:string,ids:readonly string[]):string{
 let name='';try{name=decodeURIComponent(hash.replace(/^#/,''))}catch{/* not a section */}
 return ids.includes(name)?name:ids[0]??''
}
/** The section shown, and how another is chosen. A change is made at once (the page never waits for it) and handed
 * to the page's motion, which moves from the section that was shown: see ./settings-motion. */
function useSection(ids:readonly string[],motion:RefObject<SectionMotion|null>):[string,(next:string)=>void]{
 const key=ids.join(' '),read=()=>sectionFromHash(typeof window==='undefined'?'':window.location.hash,key.split(' '))
 const[section,setSection]=useState(read),shown=useRef(section)
 shown.current=section
 const go=useCallback((next:string)=>{
  const list=key.split(' '),from=shown.current
  if(!list.includes(next)||next===from)return
  shown.current=next
  const update=()=>flushSync(()=>setSection(next))
  if(motion.current)motion.current.change(sectionDirection(list,from,next),update);else update()
 },[key,motion])
 useEffect(()=>{setSection(read());const changed=()=>go(read());window.addEventListener('hashchange',changed);return()=>window.removeEventListener('hashchange',changed)},[key,go])
 return[section,go]
}
/** The highlight behind the chosen section. It is one mark that travels from the old section to the new one; where
 * it goes is measured, and how it gets there is CSS (admin.css, "Sections"). */
function useNavThumb(nav:RefObject<HTMLElement|null>,index:number,count:number){
 const previous=useRef(-1),timer=useRef<number|undefined>(undefined)
 const place=useCallback(()=>{
  const row=nav.current,chosen=row?.querySelector<HTMLElement>('a[aria-current]')
  if(!row)return
  if(!chosen){delete row.dataset.ready;return}
  row.style.setProperty('--thumb-top',`${chosen.offsetTop}px`);row.style.setProperty('--thumb-bottom',`${row.clientHeight-chosen.offsetTop-chosen.offsetHeight}px`)
  row.style.setProperty('--thumb-left',`${chosen.offsetLeft}px`);row.style.setProperty('--thumb-right',`${row.clientWidth-chosen.offsetLeft-chosen.offsetWidth}px`)
  row.dataset.ready=''
 },[nav])
 useLayoutEffect(()=>{
  const row=nav.current
  if(!row)return
  // The first placement is immediate; only a change of section travels, led by the edge that faces where it is going.
  if(previous.current>=0&&previous.current!==index&&index>=0&&!reducedMotion()){
   row.dataset.dir=index>previous.current?'forward':'backward'
   window.clearTimeout(timer.current);timer.current=window.setTimeout(()=>{delete row.dataset.dir},700)
  }
  place();previous.current=index
 },[index,nav,place])
 useEffect(()=>{
  const row=nav.current
  if(!row)return
  // Names change width when the web font arrives, and the row changes shape between a phone and a desk.
  let live=true
  void document.fonts?.ready.then(()=>{if(live)place()})
  const observer=typeof ResizeObserver==='undefined'?null:new ResizeObserver(place)
  observer?.observe(row);row.querySelectorAll('a').forEach(item=>observer?.observe(item))
  return()=>{live=false;observer?.disconnect();window.clearTimeout(timer.current)}
 },[count,nav,place])
}
/** The section a changed setting is shown in, for the mark beside its name in the sidebar. */
export const sectionOfSetting=(key:string):string=>key==='instanceName'||key==='instanceLogo'?'identity':key.endsWith('ApiKey')||key==='openAiCompatibleBaseUrl'?'providers':key==='signIn'?'sign-in':key==='storage'||key==='apps'?key:'models'
/** The sections with an unsaved edit, as the form last drew them: a form drawn again after a save starts from these. */
let lastUnsaved:ReadonlySet<string>=new Set()
const nothingUnsaved:ReadonlySet<string>=new Set()
export function SettingsPanel({settings,client,expired,busy,save,replace,extra=[],viewerEmail,drawn='loaded'}:{/** Why the form is drawn: for the first time on this page, as loaded since, or again because a save just ended. */drawn?:'first'|'loaded'|'saved';viewerEmail?:string;/** Sections after the form's own, in order. */extra?:readonly ExtraSection[];settings:AdminSettingsDTO;client:AdminClient;expired:()=>void;busy:boolean;save:(patch:UpdateAdminSettingsInput)=>Promise<void>;/** The settings changed outside the form (the sign-in method was switched): show them. */replace:(settings:AdminSettingsDTO)=>void}){
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
 // The page's sections in order: the form's own, then the caller's (Administrators and Updates, for the owner).
 // One is shown at a time, chosen in the sidebar; the draft covers all of them, so an edit survives looking elsewhere.
 const formSections:Array<[string,string]>=[['identity','Identity'],['providers','AI providers'],['models','Models'],...(settings.signIn?[['sign-in','Sign-in'] as [string,string]]:[]),...(settings.storage?[['storage','Storage'] as [string,string]]:[]),...(settings.apps?[['apps','Apps'] as [string,string]]:[])]
 const sections:Array<[string,string]>=[...formSections,...extra.map(({id,label})=>[id,label] as [string,string])]
 const pane=useRef<HTMLDivElement>(null),motion=useRef<SectionMotion|null>(null)
 // Only the save row this form is first drawn with arrives from "Saving changes…"; one shown again later is simply there.
 const born=useRef(true)
 useEffect(()=>{born.current=false},[])
 // The marks beside the sections that were saved: drawn once more as the form is drawn again, so they shrink away instead of vanishing.
 const[justSaved,setJustSaved]=useState<ReadonlySet<string>>(()=>drawn==='saved'?lastUnsaved:nothingUnsaved)
 useEffect(()=>{if(justSaved.size)setJustSaved(nothingUnsaved)},[])
 // Saved from a section that has no form: the row stays long enough to say so, then closes.
 const[lingering,setLingering]=useState(drawn==='saved')
 useEffect(()=>{if(!lingering)return;const timer=window.setTimeout(()=>setLingering(false),1800);return()=>window.clearTimeout(timer)},[])
 // Made before the first paint, so the first section's rows are never seen and then hidden again.
 useLayoutEffect(()=>{if(!pane.current)return;const made=createSectionMotion(pane.current);motion.current=made;if(drawn==='first')made.arrive(240);return()=>{made.dispose();motion.current=null}},[])
 const[section,go]=useSection(sections.map(([id])=>id),motion),inForm=formSections.some(([id])=>id===section),unsaved=new Set([...Object.keys(patch).map(sectionOfSetting),...justSaved])
 useEffect(()=>{lastUnsaved=new Set(Object.keys(patch).map(sectionOfSetting))})
 const nav=useRef<HTMLElement>(null),scrolled=useRef(false)
 useNavThumb(nav,sections.findIndex(([id])=>id===section),sections.length)
 // On a phone the sections are one row that scrolls sideways: keep the chosen one in view.
 useEffect(()=>{const row=nav.current,chosen=row?.querySelector<HTMLElement>('[aria-current]');if(row&&chosen&&row.scrollWidth>row.clientWidth)row.scrollTo({left:chosen.offsetLeft-(row.clientWidth-chosen.offsetWidth)/2,behavior:scrolled.current&&!reducedMotion()?'smooth':'auto'});scrolled.current=true},[section])
 // Lists whose rows travel when they change (./settings-motion). A provider's editor closes before its row gives the width back: drawn again then, so the rows travel.
 const providerList=useRef<HTMLDivElement>(null),chatList=useRef<HTMLDivElement>(null),[,relayout]=useReducer((turn:number)=>turn+1,0)
 useFlow(providerList,'rise','settle');useFlow(chatList)
 // The OpenAI-compatible server is usable with a valid address; the others with a key that is not being removed.
 const available=(id:AIProvider)=>id==='openai_compatible'?!!draft.compatibleBaseUrl.trim()&&!compatibleBaseUrlProblem(draft.compatibleBaseUrl):!draft.removed[id]&&(Boolean(draft.keys[id].trim())||Boolean(settings.providers.find(p=>p.id===id)?.configured))
 const compatibleSaved=settings.openAiCompatible,compatibleChanged=tidyBaseUrl(draft.compatibleBaseUrl)!==(compatibleSaved?.baseUrl??'')
 const status=(id:AIProvider)=>id==='openai_compatible'?(compatibleChanged&&available(id)?'Address ready':available(id)?'Connected':'Not connected'):draft.keys[id]?'Key ready':available(id)?'Connected':'Not connected'
 return <div className="settings-layout" data-entrance={drawn==='first'?'':undefined}>
 {/* A plain click changes the section at once, without waiting for the address to change; the address then follows by itself. */}
 <nav ref={nav} className="settings-nav" aria-label="Sections"><span className="settings-nav-thumb" aria-hidden="true"/>{sections.map(([id,label],index)=><a key={id} href={`#${id}`} aria-current={id===section?'page':undefined} style={{'--i':index} as CSSProperties} onClick={event=>{if(event.button===0&&!event.metaKey&&!event.ctrlKey&&!event.shiftKey&&!event.altKey)go(id)}}>
  {/* The name twice, one over the other: the regular one fades into the medium one, and the row never changes width. */}
  <span className="settings-nav-label"><span>{label}</span><span aria-hidden="true">{label}</span></span>
  <span className="settings-nav-unsaved" data-on={unsaved.has(id)?'':undefined} {...(unsaved.has(id)?{role:'img','aria-label':'unsaved changes'}:{'aria-hidden':true})}/>
 </a>)}</nav>
 <div className="settings-pane" ref={pane}><div className="settings-stage">
 <form id={formId} className="instance-form" hidden={!inForm} onSubmit={event=>{event.preventDefault();if(!problem&&dirty)void save(patch)}}>
 <fieldset disabled={busy} className="admin-fieldset">
  {settings.unusedEnvironment&&settings.unusedEnvironment.length>0&&<p className="environment-note" role="note">These are still set in the server’s environment but are no longer read, because their settings now live on this page: <code>{settings.unusedEnvironment.join(', ')}</code>. You can delete them.</p>}
  {section==='identity'&&<Card><CardHeader><CardTitle>Identity</CardTitle></CardHeader><CardContent>
   <div className="identity-fields"><div><label className="settings-label" htmlFor="instance-name">Team name</label><Input id="instance-name" value={draft.instanceName} maxLength={100} required onChange={e=>update('instanceName',e.target.value)}/></div>
    <div><label className="settings-label">Team logo</label><div className="logo-control"><div className="team-logo-preview">{draft.instanceLogo?<img key={draft.instanceLogo.length} className={draft.instanceLogo!==settings.instanceLogo?'is-new':undefined} src={draft.instanceLogo} alt="Team logo"/>:<img key="mark" className={settings.instanceLogo?'is-new':undefined} src="/mark.svg" width="26" height="26" alt="Cereal"/>}</div><div><Button type="button" variant="outline" onClick={()=>logoInput.current?.click()}><ImagePlus/>Upload logo</Button><p className="settings-help">PNG, JPG, WebP · 250 KB max</p></div>{draft.instanceLogo&&<Button type="button" variant="ghost" size="icon" className={draft.instanceLogo!==settings.instanceLogo?'logo-remove late-detail':'logo-remove'} aria-label="Remove team logo" onClick={()=>{logoVersion.current++;update('instanceLogo',null)}}><X/></Button>}</div><input ref={logoInput} type="file" className="sr-only" accept="image/png,image/jpeg,image/webp" aria-label="Upload team logo" onChange={e=>void readLogo(e.target.files?.[0])}/></div>
   </div><Reveal>{logoError&&<p role="alert" className="settings-error">{logoError}</p>}</Reveal>
  </CardContent></Card>}
  {section==='providers'&&<Card><CardHeader><CardTitle>AI providers</CardTitle>  </CardHeader><CardContent><div className="provider-list" ref={providerList}>{providers.map(id=>{const saved=settings.providers.find(p=>p.id===id),connected=available(id),state=catalogs[id],hint=saved?.configured&&!draft.removed[id]?saved.hint:null;return <div className="provider-entry" key={id}>
   <div className="provider-row"><div className="provider-icon"><ProviderLogo provider={id}/></div><div className="provider-details"><strong title={providerNames[id]}>{providerShortNames[id]?<><span className="provider-name-long">{providerNames[id]}</span><span className="provider-name-short" aria-hidden="true">{providerShortNames[id]}</span></>:providerNames[id]}</strong>{hint&&<span className="provider-hint" title={hint}>{hint}{saved?.source==='environment'?' · server':''}</span>}</div><Badge data-edge="" variant="outline" className={connected?'provider-connected':'provider-disconnected'}><span/><span key={status(id)} className="provider-status">{status(id)}</span></Badge><Button data-edge="" type="button" variant="ghost" size="sm" aria-expanded={editing===id} onClick={()=>setEditing(editing===id?null:id)}>{connected?'Manage':'Connect'}<ChevronRight className={editing===id?'rotate-90':''}/></Button></div>
   <Reveal onExited={relayout} abrupt={editing!==null}>{editing===id&&(id!=='openai_compatible'?<div className="provider-editor"><label className="settings-label" htmlFor={`${id}-key`}>{providerNames[id]} API key</label><div className="provider-key-row"><Input id={`${id}-key`} type="password" autoComplete="new-password" value={draft.keys[id]} disabled={draft.removed[id]} placeholder={saved?.configured?'Type a new key to replace it':'Enter your API key'} onChange={e=>setKey(id,e.target.value)}/>{(connected||id==='openrouter')&&<Button type="button" variant="outline" disabled={state?.busy} onClick={()=>void loadCatalog(id,true)}>{state?.busy?<Loader2 className="animate-spin"/>:<RefreshCw/>}Load models</Button>}</div>
   <p className="settings-help">{saved?.configured&&!draft.removed[id]?savedKeyText(saved)?.replace(/\. Type.*$/,'. '):''}{saved?.source==='environment'?'A key saved here overrides it.':'Keys are encrypted on the server and never returned to the browser.'}</p>
   {saved?.source==='stored'&&<label className="remove-key"><input type="checkbox" checked={draft.removed[id]} onChange={e=>update('removed',{...draft.removed,[id]:e.target.checked})}/>Remove saved key</label>}
   <Reveal>{state?.error&&<p role="alert" className="settings-error">{state.error}</p>}</Reveal><Reveal>{state?.data&&<p className="settings-help"><Check size={12}/>{state.data.models.length} models available{draft.keys[id]?' · Save changes to connect':''}</p>}</Reveal>
   </div>:<div className="provider-editor">
    <label className="settings-label" htmlFor="compatible-base-url">Server address</label><div className="provider-key-row"><Input id="compatible-base-url" type="url" inputMode="url" autoComplete="off" autoCapitalize="none" spellCheck={false} maxLength={2048} placeholder="http://localhost:11434/v1" aria-describedby="compatible-base-url-help" value={draft.compatibleBaseUrl} onChange={e=>setBaseUrl(e.target.value)}/>{connected&&<Button type="button" variant="outline" disabled={state?.busy} onClick={()=>void loadCatalog(id,true)}>{state?.busy?<Loader2 className="animate-spin"/>:<RefreshCw/>}Load models</Button>}</div>
    <p id="compatible-base-url-help" className="settings-help">{draft.compatibleBaseUrl.trim()&&compatibleBaseUrlProblem(draft.compatibleBaseUrl)||'Ollama, vLLM, LM Studio, or any /v1 API: where its API starts. Requests go to …/chat/completions and …/models there.'}{compatibleSaved?.baseUrl&&!draft.compatibleBaseUrl.trim()?' Saving with no address disconnects it.':''}</p>
    <Reveal>{connected&&<><label className="settings-label" htmlFor="compatible-key">API key <span className="settings-optional">optional</span></label><Input id="compatible-key" type="password" autoComplete="new-password" value={draft.keys[id]} disabled={draft.removed[id]} placeholder={compatibleSaved?.apiKeySet?'Type a new key to replace it':'Leave blank if not needed'} onChange={e=>setKey(id,e.target.value)}/>
    <p className="settings-help">{compatibleSaved?.apiKeySet&&!draft.removed[id]?compatibleChanged&&!draft.keys[id].trim()?'The saved key is only sent to the saved address: saving a new address removes it, so type it again if this server needs one. ':`${compatibleSaved.apiKeyHint??'A key'} is saved. `:''}Ollama and LM Studio need none. A key is encrypted on the server and never returned to the browser.</p>
    {compatibleSaved?.apiKeySet&&<label className="remove-key"><input type="checkbox" checked={draft.removed[id]} onChange={e=>update('removed',{...draft.removed,[id]:e.target.checked})}/>Remove saved key</label>}</>}</Reveal>
    <Reveal>{state?.error&&<p role="alert" className="settings-error">{state.error}</p>}</Reveal><Reveal>{state?.data&&<p className="settings-help"><Check size={12}/>{state.data.models.length?`${state.data.models.length} models available`:'No models listed. Type a model name in the model choices below.'}{compatibleChanged||draft.keys[id]?' · Save changes to connect':''}</p>}</Reveal>
   </div>)}</Reveal>
  </div>})}</div><p className="provider-security">API keys are encrypted and kept on your server.</p></CardContent></Card>}
  {section==='models'&&<Card><CardHeader><CardTitle>Models</CardTitle></CardHeader><CardContent>
   <div className="model-section"><div className="model-section-heading"><h3>Chat</h3><span><Swap text={`${draft.chatModels.length} enabled`}/></span></div><div className="model-controls">
    <div className="chat-model-list" ref={chatList}>{draft.chatModels.map((model,i)=><div className="chat-model-row" key={modelKey(model)}><ModelPicker value={model} kind="language" catalogs={catalogs} available={available} load={loadCatalog} label={`Chat model ${i+1}`} change={next=>{if(draft.chatModels.some((m,index)=>index!==i&&modelKey(m)===modelKey(next)))return;update('chatModels',draft.chatModels.map((m,index)=>index===i?next as LanguageModelSelection:m))}}/>{i===0?<Badge key="default" variant="secondary" className={settings.chatModels[0]&&modelKey(settings.chatModels[0])===modelKey(model)?'default-model':'default-model is-new'}>Default</Badge>:<Button key="make" type="button" variant="ghost" size="icon" className="make-default" aria-label={`Make ${model.model} the default`} onClick={()=>update('chatModels',[model,...draft.chatModels.filter((_,index)=>index!==i)])}><Check size={14}/></Button>}<Button type="button" variant="ghost" size="icon" disabled={draft.chatModels.length===1} aria-label={`Remove ${model.model}`} onClick={()=>update('chatModels',draft.chatModels.filter((_,index)=>index!==i))}><Trash2/></Button></div>)}</div>
    {draft.chatModels.length<20&&<ModelPicker kind="language" catalogs={catalogs} available={available} load={loadCatalog} label="Add chat model" excluded={draft.chatModels.map(modelKey)} change={model=>update('chatModels',[...draft.chatModels,model as LanguageModelSelection])}/>}
   </div></div>
   {/* Transcription is live only and always on: only the live model can change. */}
   <div className="model-section"><div className="model-section-heading"><h3>Live transcription</h3><span><Swap text={available('elevenlabs')?'Always on':'Needs ElevenLabs'}/></span></div><div className="model-controls">
    <ModelPicker value={draft.liveTranscriptionModel} kind="live_transcription" catalogs={catalogs} available={available} load={loadCatalog} label="Live transcription model" change={model=>update('liveTranscriptionModel',model as LiveTranscriptionModelSelection)}/>
    <p className="settings-help">{available('elevenlabs')?'Words appear within about a second, with speaker labels.':'Connect ElevenLabs above to start transcribing.'}</p>
    {/* The limits only matter once live transcription can run. */}
    <Reveal className="reveal-row">{settings.liveMaxSessions!==undefined&&available('elevenlabs')&&<div className="live-limits">
     <div><label className="settings-label" htmlFor="live-max-sessions">Live recordings at once</label><Input id="live-max-sessions" type="text" inputMode="numeric" autoComplete="off" maxLength={3} aria-describedby="live-max-sessions-help" value={draft.liveMaxSessions} onChange={e=>update('liveMaxSessions',e.target.value)}/><p id="live-max-sessions-help" className="settings-help">1 to 200. A recording started beyond this gets no live transcript until a place frees up; its app keeps retrying.</p></div>
     <div><label className="settings-label" htmlFor="live-hold-seconds">Seconds to keep speakers after Stop</label><Input id="live-hold-seconds" type="text" inputMode="numeric" autoComplete="off" maxLength={4} aria-describedby="live-hold-seconds-help" value={draft.liveHoldSeconds} onChange={e=>update('liveHoldSeconds',e.target.value)}/><p id="live-hold-seconds-help" className="settings-help">0 to 3600. A Resume within this time keeps its speaker labels; 0 turns it off.</p></div>
    </div>}</Reveal>
   </div></div>
   <div className="model-section"><div className="model-section-heading"><h3>Meeting notes</h3></div><ModelPicker value={draft.generationModel} kind="language" catalogs={catalogs} available={available} load={loadCatalog} label="Meeting notes model" change={model=>update('generationModel',model as LanguageModelSelection)}/></div>
  </CardContent></Card>}
  {section==='sign-in'&&draft.signIn&&settings.signIn&&<SignInSettings viewerEmail={viewerEmail} draft={draft.signIn} saved={settings.signIn} change={next=>update('signIn',next)} client={client} switched={replace} expired={expired}/>}
  {section==='storage'&&draft.storage&&settings.storage&&<StorageSettings draft={draft.storage} saved={settings.storage} change={next=>update('storage',next)} test={async(input,signal)=>{try{return await client.testStorage(input,signal)}catch(error){if(error instanceof AdminClientError&&error.status===401)expired();throw error}}}/>}
  {section==='apps'&&draft.apps&&settings.apps&&<Card><CardHeader><CardTitle>Apps</CardTitle></CardHeader><CardContent>
   <div className="model-section"><div className="model-section-heading"><h3>iPhone app</h3><span><Swap text={draft.apps.iosAppStoreId.trim()?'On the App Store':'Not published'}/></span></div><div className="model-controls sign-in-block"><div className="storage-fields">
    <div><label className="settings-label" htmlFor="ios-app-ids">App IDs that open note links</label><Input id="ios-app-ids" type="text" autoComplete="off" autoCapitalize="none" spellCheck={false} aria-describedby="ios-app-ids-help" placeholder="LZWN2FF7YB.dev.cereal.mobile" value={draft.apps.iosAppIds} onChange={e=>update('apps',{...draft.apps!,iosAppIds:e.target.value})}/><p id="ios-app-ids-help" className="settings-help">Team ID, a dot, the bundle ID; several separated by commas. Leave blank for the Cereal app.</p></div>
    <div><label className="settings-label" htmlFor="ios-app-store-id">App Store ID</label><Input id="ios-app-store-id" type="text" inputMode="numeric" autoComplete="off" maxLength={20} aria-describedby="ios-app-store-id-help" placeholder="Not published yet" value={draft.apps.iosAppStoreId} onChange={e=>update('apps',{...draft.apps!,iosAppStoreId:e.target.value})}/><p id="ios-app-store-id-help" className="settings-help">The number in the app’s App Store address. Adds the iPhone link on the download page and the “open in the app” banner in Safari.</p></div>
   </div></div></div>
  </CardContent></Card>}
 </fieldset>
 </form>
 {extra.find(({id})=>id===section)?.content}
  {/* Sections outside the form save nothing, so the bar is there only while an edit made elsewhere is waiting. */}
  <Reveal className="reveal-save">{(inForm||dirty||busy||lingering)&&<SaveRow formId={formId} state={saveState(busy,problem,dirty)} problem={problem} fresh={drawn==='saved'&&born.current}/>}</Reveal>
 </div></div>
 </div>
}
/** The row under a section: what there is to save, and the button that saves it. It moves from one state to the
 * next: the words change places, the mark beside them is the state's own (a check that draws itself once a save
 * has ended), and the button wakes when there is something to save. `fresh`: the form was drawn again because a
 * save just ended, so the row arrives from "Saving changes…". */
function SaveRow({formId,state,problem,fresh}:{formId:string;state:SaveState;problem:string|null;fresh:boolean}){
 // Whether anything has happened since the row was drawn: a row that is simply there does not move.
 const[seen,setSeen]=useState(state),[lively,setLively]=useState(fresh)
 if(seen!==state){setSeen(state);setLively(true)}
 const line=saveLine(state,problem)
 return <div className={lively?'instance-save is-lively':'instance-save'} data-state={state}>
  <span className="save-status" aria-live="polite">
   <span className="save-mark" aria-hidden="true">{state==='saving'?<Loader2 key="saving" size={14} className="animate-spin"/>:state==='saved'?<svg key="saved" className="save-check" viewBox="0 0 14 14" focusable="false"><path pathLength={1} d="M2.6 7.5l3 3 5.8-6.6"/></svg>:state==='problem'?<TriangleAlert key="problem" size={14}/>:<span key="unsaved" className="save-dot"/>}</span>
   <Swap text={line} initial={fresh?saveLine('saving',null):undefined}/>
  </span>
  <Button form={formId} type="submit" disabled={state!=='unsaved'}>Save changes</Button>
 </div>
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
