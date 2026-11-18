import {useEffect,useId,useRef,useState,type CSSProperties,type ReactNode,type RefObject} from 'react'
import {AdminClientError} from '../../lib/client-error'
import {UPDATE_PATH,availabilityLine,fetchUpdateStatus,pendingSteps,requestUpdate,sameUpdate,updateOffered,updateOutcome,updateSteps,versionLabel,type UpdateJob,type UpdateStatus} from '../../lib/updates'
import {elapsedLabel,jobElapsed,restartExpected,sceneAnnouncement,sceneCaption,sceneHeadline,stepCount,updateScene,versionRoll,type UpdateScene} from '../../lib/update-scene'
import {UpdateBowl} from './update-bowl'
import {reducedMotion,useLeaving,useNow,useStaged,type StageKind} from './update-motion'
import {Button} from '../ui/button'
import {Badge} from '../ui/badge'
import {Card,CardContent,CardHeader,CardTitle} from '../ui/card'
import {Loading,Modal,Notice,timestamp} from './shared'
import {ChevronRight,Loader2,RefreshCw,RotateCw,TriangleAlert} from 'lucide-react'

/** How the page is connected to the server's progress stream. During an update the server restarts, so
 * "reconnecting" is expected then and says so. */
type Link='connecting'|'live'|'reconnecting'
/** A request this page made and the updater has not picked up yet (`at` is the server's clock, in seconds). */
type Asked={action:'check'|'start';at:number}|null
const MAX_LINES=2000

/** The Updates section (owner only): which version runs, whether a newer one is there, the "Update server"
 * action with its live progress, and how the last update ended. The server only passes requests on; the work is
 * done by scripts/updater.sh on the host, and when that is not installed this says so and how to install it. */
export function UpdatesPanel({expired}:{expired:()=>void}){
 const[status,setStatus]=useState<UpdateStatus|null>(null),[lines,setLines]=useState<string[]>([]),[link,setLink]=useState<Link>('connecting')
 const[problem,setProblem]=useState<string|null>(null),[asked,setAsked]=useState<Asked>(null),[confirming,setConfirming]=useState(false),[working,setWorking]=useState(false)
 // The update this page watched from start to end: after it, the page itself is the old version's.
 const[watched,setWatched]=useState<string|null>(null)
 const shown=useRef({job:'',lines:0})
 useEffect(()=>{
  let source:EventSource|null=null,timer:number|undefined,stopped=false
  const connect=()=>{
   if(stopped)return
   // What this page already shows, so a stream opened after the server restarted carries on from there.
   source=new EventSource(`${UPDATE_PATH}/events?job=${encodeURIComponent(shown.current.job)}&after=${shown.current.lines}`)
   source.addEventListener('open',()=>setLink('live'))
   source.addEventListener('status',event=>{const next=JSON.parse((event as MessageEvent<string>).data) as UpdateStatus;shown.current.job=next.job?.id??'';setStatus(next);setLink('live')})
   source.addEventListener('log',event=>{const{line}=JSON.parse((event as MessageEvent<string>).data) as{line:string};shown.current.lines++;setLines(old=>[...old,line].slice(-MAX_LINES))})
   source.addEventListener('reset',()=>{shown.current.lines=0;setLines([])})
   // An EventSource gives up for good when the answer is not a stream (a proxy's 502 while the server restarts):
   // close it and open another.
   source.addEventListener('error',()=>{source?.close();if(stopped)return;setLink('reconnecting');timer=window.setTimeout(connect,1500)})
  }
  const controller=new AbortController()
  // First as an ordinary request: it can say "sign in again" or "not allowed", which a stream cannot.
  fetchUpdateStatus(controller.signal).then(first=>{if(stopped)return;shown.current.job=first.job?.id??'';setStatus(first);connect()},error=>{
   if(stopped||controller.signal.aborted)return
   if(error instanceof AdminClientError&&error.status===401){expired();return}
   setProblem(error instanceof AdminClientError&&error.status===403?'Only the owner can update the server.':'The update status could not be loaded. Try again.')
  })
  return()=>{stopped=true;controller.abort();source?.close();window.clearTimeout(timer)}
 },[expired])
 // A request is answered when the updater's files show something that happened after it.
 const after=(time:string|null|undefined,at:number)=>!!time&&Date.parse(time)>=at*1000
 useEffect(()=>{
  if(!asked||!status)return
  if(asked.action==='check'?after(status.available?.checkedAt,asked.at):after(status.job?.startedAt,asked.at)){if(asked.action==='start')setWatched(status.job!.id);setAsked(null);return}
  const timer=window.setTimeout(()=>{setAsked(null);setProblem('The updater did not answer. Check that it is running on the server.')},20_000)
  return()=>window.clearTimeout(timer)
 },[asked,status])
 async function ask(action:'check'|'start'){
  setProblem(null);setWorking(true)
  try{const answer=await requestUpdate(action);if(action==='start'){shown.current.lines=0;setLines([])}setAsked({action,at:answer.requestedAt});setConfirming(false)}
  catch(error){
   if(error instanceof AdminClientError&&error.status===401){expired();return}
   setConfirming(false);setProblem(error instanceof AdminClientError?error.serverMessage??error.message:'The request could not finish. Please try again.')
  }finally{setWorking(false)}
 }
 if(!status)return <Card><CardHeader><CardTitle>Updates</CardTitle></CardHeader><CardContent>{problem?<Notice error>{problem}</Notice>:<Loading label="Loading update status…"/>}</CardContent></Card>
 return <>
  <UpdatesView status={status} lines={lines} link={link} problem={problem} asked={asked?.action??null} reloadOffered={!!watched&&!!status.job&&sameUpdate(status.job.id,watched)&&status.job.status==='deployed'} check={()=>void ask('check')} update={()=>{setProblem(null);setConfirming(true)}}/>
  {confirming&&status.available&&<Modal title="Update server?" busy={working} close={()=>setConfirming(false)}>
   <div className="admin-dialog-body">
    <p>Cereal goes from <strong>{versionLabel(status.current.version,status.current.commit)}</strong> to <strong>{versionLabel(status.available.version,status.available.commit)}</strong>. Settings, notes and accounts stay as they are.</p>
    <p>The server restarts at the end, for about a minute. Recordings in progress lose their live transcript until their app reconnects.</p>
    <p>The database is backed up first. If the new version does not start, the previous one is put back.</p>
   </div>
   <div className="admin-dialog-actions"><Button type="button" variant="outline" disabled={working} onClick={()=>setConfirming(false)}>Cancel</Button><Button type="button" disabled={working} data-busy={working?'':undefined} onClick={()=>void ask('start')}>{working?'Starting…':'Update server'}</Button></div>
  </Modal>}
 </>
}

/** How long an update's ending is played before the picture relaxes into the summary. */
const ENDING_MS=5200
/** What decides the layout of the progress section. When one of these changes, the change is staged (./update-motion). */
interface Shown{present:boolean;job:UpdateJob|null;asking:boolean;link:Link;lines:readonly string[];logOpen:boolean;scene:UpdateScene;reloadOffered:boolean;rows:number}
const stageKind=(from:Shown,to:Shown):StageKind=>!from.present&&to.present?'launch':from.present&&to.present&&from.scene.compact!==to.scene.compact?'settle':to.rows>from.rows?'extend':'resize'

/** Everything the section shows, from what the server answered. Drawn the same whether the page watched the
 * update happen or opened afterwards; only the ending of an update is played once, for the page that saw it run. */
export function UpdatesView({status,lines,link,problem=null,asked=null,reloadOffered=false,check,update}:{status:UpdateStatus;lines:readonly string[];link:Link;problem?:string|null;asked?:'check'|'start'|null;/** The page saw this update finish: its own code is now the old version's. */reloadOffered?:boolean;check:()=>void;update:()=>void}){
 const{current,updater,available,job}=status,active=!!job?.active||asked==='start',ready=updater.state==='ready',offered=updateOffered(status)
 // From here on, presentation: which update this page has seen run, whether its ending has been played, the log.
 const asking=asked==='start',[ran,setRan]=useState<string|null>(null),[played,setPlayed]=useState<string|null>(null),[logOpen,setLogOpen]=useState(false)
 // The update this page asked for counts as seen even when it is over before the page hears of a single step.
 const before=useRef<string|null>(null)
 useEffect(()=>{
  if(job?.active){setRan(job.id);before.current=null;return}
  if(asking){before.current??=job?.id??'';return}
  if(before.current!==null&&job&&job.id!==before.current)setRan(job.id)
  before.current=null
 },[asking,job?.id,job?.active])
 const over=!!job&&!job.active&&!asking,watched=over&&ran===job.id
 useEffect(()=>{
  if(!watched||!job||played===job.id)return
  const timer=window.setTimeout(()=>setPlayed(job.id),ENDING_MS)
  return()=>window.clearTimeout(timer)
 },[watched,job?.id,played])
 const scene=updateScene({job,asking,reconnecting:link==='reconnecting',watched,played:!!job&&played===job.id})
 const next:Shown={present:!!job||asking,job,asking,link,lines,logOpen,scene,reloadOffered,rows:job&&!asking?updateSteps(job).length:pendingSteps().length}
 const{shown:view,pending}=useStaged(next,`${next.present}|${scene.compact}|${next.rows}|${logOpen}`,stageKind)
 // An update started here is brought into view once it has unfolded: on a phone it begins below the screen's edge.
 // Once per update, and not given up when the updater answers before the page has unfolded.
 const place=useRef<HTMLDivElement>(null),bring=useRef<number|undefined>(undefined)
 useEffect(()=>{
  if(!view.asking)return
  window.clearTimeout(bring.current)
  bring.current=window.setTimeout(()=>place.current?.scrollIntoView({block:'nearest',behavior:reducedMotion()?'auto':'smooth'}),650)
 },[view.asking])
 useEffect(()=>()=>window.clearTimeout(bring.current),[])
 const viewActive=!!view.job?.active||view.asking
 return <Card className="updates-card"><CardHeader><CardTitle>Updates</CardTitle></CardHeader><CardContent>
  {problem&&<Notice error>{problem}</Notice>}
  <div className="model-section updates-this"><div className="model-section-heading"><h3>This server</h3></div><div className="model-controls">
   <p className="update-version"><strong>Cereal {current.version}</strong>{current.commit&&<code title={current.commit}>{current.commit.slice(0,7)}</code>}</p>
  </div></div>

  {updater.state==='missing'&&<div className="model-section updates-source"><div className="model-section-heading"><h3>Update server</h3><span>Not set up</span></div><div className="model-controls update-setup">
   <p className="update-plain"><TriangleAlert size={14}/>The updater is not installed on this server, so it cannot be updated from this page and this page cannot tell whether a newer version exists.</p>
   <p className="settings-help">Updating rebuilds and restarts the server’s containers. Cereal does not give its own container that power: a small program on the machine itself does it, and only when the owner asks here. To set it up, on the server, in the folder Cereal was deployed to:</p>
   <ol className="update-howto">
    <li><span>Tell it where newer versions come from (a git remote the server can read, and a branch or tag):</span><code>./scripts/updater.sh install git@github.com:you/cereal.git main --ssh-key ~/.ssh/cereal_deploy_key</code></li>
    <li><span>Keep it running and restart Cereal once. The command above prints both steps:</span><code>sudo systemctl enable --now cereal-updater</code><code>docker compose up -d</code></li>
   </ol>
   <p className="settings-help">Until then, update from your own machine with <code>scripts/deploy-ssh.sh</code>. <code>docs/server-updates.md</code> has the details.</p>
  </div></div>}

  {updater.state!=='missing'&&<div className="model-section updates-source"><div className="model-section-heading"><h3>Newer version</h3><span>{asked==='check'?'Checking…':available?.error?'Check failed':offered?'Available':available?.relation==='same'?'Up to date':available?'Nothing to install':'Not checked'}</span></div><div className="model-controls">
   {updater.state==='stopped'&&<p className="update-plain"><TriangleAlert size={14}/>The updater is installed but is not running on the server, so nothing can be checked or installed right now.</p>}
   {updater.state==='stopped'&&<ol className="update-howto"><li><span>Start it on the server:</span><code>sudo systemctl start cereal-updater</code></li></ol>}
   <p className={available?.error?'settings-error update-found':'update-found'}>{offered&&<Badge variant="secondary" className="default-model">New</Badge>}{availabilityLine(status)}</p>
   <p className="settings-help update-source">{available?.checkedAt?`Checked ${timestamp(available.checkedAt)}. `:''}Compares the commit this server runs with <code>{updater.ref??'the configured ref'}</code> in <code>{updater.source??'the configured repository'}</code>.</p>
   <div className="update-actions">
    {/* It cannot be pressed from the moment an update is asked for. Until the progress has unfolded from it, it keeps its colour. */}
    {offered&&<Button type="button" className="update-go" data-busy={pending&&active?'':undefined} disabled={!ready||active||asked!==null} onClick={update}><RotateCw/>Update server</Button>}
    <Button type="button" variant="outline" disabled={!ready||active||asked!==null} onClick={check}>{asked==='check'?<Loader2 className="animate-spin"/>:<RefreshCw/>}Check for updates</Button>
   </div>
  </div></div>}

  {view.present&&<div className="model-section update-section"><div className="model-section-heading"><h3>{viewActive?'Update in progress':'Last update'}</h3>{view.job&&!viewActive&&<span>{timestamp(view.job.finishedAt??view.job.startedAt)}</span>}</div><div className="model-controls">
   <UpdateProgress view={view} place={place} toggleLog={()=>setLogOpen(open=>!open)}/>
  </div></div>}
 </CardContent></Card>
}

/** The digit of "Step 3 of 5": a reel of them, turned to the one that counts. */
function Count({at,of}:{at:number;of:number}){
 return <span className="update-count" style={{'--at':at-1} as CSSProperties}><span className="sr-only">{at}</span><span className="update-count-reel" aria-hidden="true">{Array.from({length:of},(_,index)=><span key={index}>{index+1}</span>)}</span></span>
}
/** A version turning from the old number to the new one, the characters that differ rolling over. Without motion it is the new number. */
function VersionRoll({from,to}:{from:string|null;to:string}){
 return <span className="update-roll"><span className="sr-only">{to}</span><span aria-hidden="true">{versionRoll(from,to).map((cell,index)=>cell.changed
  ?<span key={index} className="update-roll-cell" style={{'--i':index} as CSSProperties}><span className="update-roll-reel"><span className="update-roll-old">{cell.from}</span>{cell.to}</span></span>
  :cell.to)}</span></span>
}
/** A line whose words change: the new ones rise in as the old ones leave. */
function Swap({text,children}:{text:string;children?:ReactNode}){
 const leaving=useLeaving(text)
 return <><span key={text} className="update-swap-in">{children??text}</span>{leaving!==null&&<span key={`was ${leaving}`} className="update-swap-out" aria-hidden="true">{leaving}</span>}</>
}

const STEP_WORDS={done:'Done',running:'Running',failed:'Failed',waiting:'',skipped:'Not needed'} as const
function StepNode({state,rollback}:{state:keyof typeof STEP_WORDS;rollback:boolean}){
 return <span className="update-node" aria-hidden="true"><svg viewBox="0 0 16 16" focusable="false">
  {state==='done'&&(rollback?<path className="update-node-line" pathLength={1} d="M4.6 8.2a3.5 3.5 0 1 0 1-2.6M4.4 3.6v2.3h2.3"/>:<path className="update-node-line" pathLength={1} d="M4.5 8.4l2.3 2.3 4.7-5.2"/>)}
  {state==='running'&&<><circle className="update-node-ring" cx="8" cy="8" r="6"/><circle className="update-node-arc" cx="8" cy="8" r="6" pathLength={1}/></>}
  {state==='failed'&&<path className="update-node-line" pathLength={1} d="M5.2 5.2l5.6 5.6M10.8 5.2l-5.6 5.6"/>}
  {state==='skipped'&&<path className="update-node-line" pathLength={1} d="M5 8h6"/>}
 </svg></span>
}

function UpdateProgress({view,place,toggleLog}:{view:Shown;place:RefObject<HTMLDivElement|null>;toggleLog:()=>void}){
 const{job,asking,lines,logOpen,scene,reloadOffered}=view,live=!scene.compact,running=scene.phase!=='ended'
 const now=useNow(running&&!asking),logId=useId(),log=useRef<HTMLPreElement>(null),follow=useRef(true)
 // The log follows its last line until someone scrolls up to read.
 useEffect(()=>{const box=log.current;if(box&&logOpen&&follow.current)box.scrollTop=box.scrollHeight},[lines,logOpen])
 // Opened, the log is brought into view once it has unfolded: it is the last thing in the card and may be below the screen's edge.
 useEffect(()=>{
  if(!logOpen)return
  const timer=window.setTimeout(()=>log.current?.scrollIntoView({block:'nearest',behavior:reducedMotion()?'auto':'smooth'}),650)
  return()=>window.clearTimeout(timer)
 },[logOpen])
 const steps=job&&!asking?updateSteps(job):pendingSteps(),outcome=job&&!asking?updateOutcome(job):{tone:'neutral' as const,title:'Asking the updater to start…',detail:null}
 const count=job&&!asking&&job.status==='running'&&job.step!=='rollback'?stepCount(job):null,elapsed=asking?null:jobElapsed(job,now)
 const caption=sceneCaption(job,asking),headline=sceneHeadline(job,asking),rolled=!asking&&job?.status==='deployed'&&!!job.to.version
 // Steps that did not lead anywhere: the update failed, so the gold that marked the way to it is taken back.
 const stopped=steps.findIndex(step=>step.state==='failed'),unwound=!asking&&!!job&&stopped>=0
 const away=!asking&&!!job?.active&&view.link==='reconnecting'
 return <div ref={place} className={`update-progress is-${live?'live':'compact'} is-${scene.phase}`}>
  <div className="update-hero">
   <UpdateBowl act={scene.act} tone={scene.tone} level={scene.level} pours={scene.pours}/>
   <div className="update-words">
    {live&&<p className="update-caption"><Swap text={count?'step':caption}>{count?<>Step <Count at={count.at} of={count.of}/> of {count.of}</>:caption}</Swap>{elapsed!==null&&<span className="update-elapsed"> · {elapsedLabel(elapsed)}</span>}</p>}
    {live&&<h4 className="update-headline"><Swap text={headline}>{rolled?<>Cereal <VersionRoll from={job!.from.version} to={job!.to.version!}/></>:headline}</Swap></h4>}
    <p className={`update-outcome is-${outcome.tone}`} role="status">{!running&&outcome.tone==='bad'&&<TriangleAlert size={14}/>}<span><strong>{outcome.title}</strong>{outcome.detail&&<> {outcome.detail}</>}</span></p>
    {/* Always there while an update runs, so that what it says is announced, and so that nothing moves when it does. */}
    {live&&running&&<p className="update-link" role="status">{away&&<>{/* The restart follows the build at once, so a connection lost from then on is the restart. */}<Loader2 size={12} className="animate-spin"/>{restartExpected(job!.step)?'The server is restarting. This page reconnects by itself…':'Connection lost. Reconnecting…'}</>}</p>}
    {!live&&!running&&elapsed!==null&&<p className="settings-help update-took">Took {elapsedLabel(elapsed)}.</p>}
    {reloadOffered&&<p className="update-reload">This page is still the previous version’s. <button type="button" onClick={()=>window.location.reload()}>Reload it</button></p>}
   </div>
  </div>
  {/* Said once per step to a screen reader; the picture above is decoration and the list below is read as a list. */}
  <p className="sr-only" role="status" aria-live="polite">{sceneAnnouncement(job,asking)}</p>
  <ol className={`update-steps${unwound?' is-unwound':''}`} aria-label="Steps">{steps.map((step,index)=><li key={step.id} className={`is-${step.state}${step.id==='rollback'?' is-rollback':''}`} style={{'--back':Math.max(0,stopped-1-index)} as CSSProperties}>
   <StepNode state={step.state} rollback={step.id==='rollback'}/><span className="update-step-text"><span className="update-step-name">{step.label}</span>{STEP_WORDS[step.state]&&<small key={step.state}>{STEP_WORDS[step.state]}</small>}</span>
  </li>)}</ol>
  <div className="update-log-wrap">
   <button type="button" className="update-log-toggle" aria-expanded={logOpen} aria-controls={logId} onClick={toggleLog}><span className="update-log-chevron"><ChevronRight aria-hidden="true"/></span><span className="update-log-label">{logOpen?'Hide log':'Show log'}{lines.length>0&&<small>{lines.length} {lines.length===1?'line':'lines'}</small>}</span></button>
   {/* In the page even while it is closed, so opening it loses no line and nothing has to be fetched. */}
   <div id={logId} className="update-log-panel" hidden={!logOpen}><pre ref={log} className="update-log" tabIndex={0} aria-label="Update log" onScroll={event=>{const box=event.currentTarget;follow.current=box.scrollHeight-box.scrollTop-box.clientHeight<24}}>{lines.join('\n')}</pre></div>
  </div>
 </div>
}
