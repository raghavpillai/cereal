import {useCallback,useEffect,useRef,useState,type ReactNode} from 'react'
import type {AdminSettingsDTO,AuthSessionDTO,UpdateAdminSettingsInput} from '@meeting-notes/contracts'
import {createAdminClient,type AdminClient} from '../../lib/admin-client'
import {fetchAuthMethods} from '../../lib/auth-client'
import {needsOnboarding} from '../../lib/onboarding'
import {SessionGate} from '../account/session-gate'
import {Onboarding} from '../onboarding/onboarding'
import {AdministratorsPanel} from './administrators-panel'
import {SettingsPanel} from './settings-panel'
import {Loading,Notice,useAdminTask,useResource} from './shared'
import {Button} from '../ui/button'
import {ArrowUpRight,LogOut} from 'lucide-react'
import '../../admin.css'

/** The settings page's own frame. The setup flow is drawn outside it: it fills the window and has its own controls. */
const Page=({children,arriving=false}:{children:ReactNode;/** Just out of setup: the page's parts settle in. */arriving?:boolean})=><div className={`admin-page instance-settings${arriving?' instance-arrival':''}`}>{children}</div>
/** What the panel opens with after setup: the settings it finished with, and whether to greet them. */
type Arrival={settings:AdminSettingsDTO;celebrate:boolean}|null

export function AdminPortal(){
 // A server nobody has claimed has no one to sign in: the panel opens on "Claim this server" instead.
 const[gate,setGate]=useState<'checking'|'claim'|'session'>('checking'),arrival=useRef<Arrival>(null),claimClient=useRef<AdminClient|null>(null)
 useEffect(()=>{const controller=new AbortController();fetchAuthMethods(controller.signal).then(methods=>{if(!controller.signal.aborted)setGate(methods.setupRequired?'claim':'session')},()=>{if(!controller.signal.aborted)setGate('session')});return()=>controller.abort()},[])
 if(gate==='checking')return <Page><div className="account-loading"><Loading label="Checking your session…"/></div></Page>
 if(gate==='claim'){
  claimClient.current??=createAdminClient(window.location.origin)
  return <Onboarding client={claimClient.current} initial={{kind:'claim',instanceName:'Cereal'}} expired={()=>window.location.reload()} finished={settings=>{arrival.current={settings,celebrate:true};setGate('session')}}/>
 }
 // Straight from setup the session is already known to be good: nothing is shown while it is confirmed.
 return <SessionGate frame={content=><Page>{content}</Page>} fallback={arrival.current?<Page><div className="account-loading" aria-busy="true"/></Page>:undefined}>{(session,client,expired,signOut)=>session.user.role==='user'
  ?<Page><section className="admin-login"><p className="eyebrow">Instance settings</p><h1>Administrator access required.</h1><p>Ask your instance owner for access to these settings.</p><a href="/notes" className="button">Back to notes</a></section></Page>
  :<LoadWorkspace session={session} client={client} expired={expired} signOut={signOut} arrival={arrival}/>}</SessionGate>
}
function LoadWorkspace({session,client,expired,signOut,arrival}:{session:AuthSessionDTO;client:AdminClient;expired:()=>void;signOut:()=>Promise<void>;arrival:{current:Arrival}}){
 // Taken once: a later visit to this page loads the settings afresh.
 const[arrived]=useState(()=>arrival.current)
 useEffect(()=>{arrival.current=null},[arrival])
 const load=useCallback((signal:AbortSignal)=>arrived?Promise.resolve(arrived.settings):client.getSettings(signal),[client,arrived]),resource=useResource(load,expired)
 // Setup in progress, and how the panel should open once it ends.
 const[setup,setSetup]=useState<{settings:AdminSettingsDTO;fromStart:boolean}|null>(null),[opened,setOpened]=useState<Arrival>(arrived)
 const loaded=opened?.settings??resource.data
 if(setup)return <Onboarding client={client} initial={{kind:'ready',settings:setup.settings,fromStart:setup.fromStart}} expired={expired} finished={settings=>{setOpened({settings,celebrate:true});setSetup(null)}}/>
 if(!loaded)return <Page><section className="admin-card">{resource.error?<Notice error>{resource.error}<button onClick={()=>void resource.reload()}>Retry</button></Notice>:<Loading label="Loading instance settings…"/>}</section></Page>
 // While setup is unfinished it is all this page shows: there is no way to the settings except through it.
 if(needsOnboarding(loaded))return <Onboarding client={client} initial={{kind:'ready',settings:loaded}} expired={expired} finished={settings=>setOpened({settings,celebrate:true})}/>
 return <Page arriving={opened?.celebrate??false}><Workspace key={opened?.settings.onboarding?.completedAt??'loaded'} session={session} client={client} initialSettings={loaded} expired={expired} signOut={signOut} restart={settings=>setSetup({settings,fromStart:true})}/></Page>
}
function Workspace({session,client,initialSettings,expired,signOut,restart}:{session:AuthSessionDTO;client:AdminClient;initialSettings:AdminSettingsDTO;expired:()=>void;signOut:()=>Promise<void>;restart:(settings:AdminSettingsDTO)=>void}){
 const[settings,setSettings]=useState(initialSettings),[notice,setNotice]=useState<string|null>(null),tasks=useAdminTask(expired)
 async function save(patch:UpdateAdminSettingsInput){const result=await tasks.run('settings',signal=>client.updateSettings(patch,signal));if(result.ok){setSettings(result.value);setNotice('Instance settings saved.')}}
 async function runSetupAgain(){const result=await tasks.run('setup',signal=>client.restartOnboarding(signal));if(result.ok)restart(result.value)}
 return <>
  <div className="instance-heading"><div><h1>Instance settings</h1><p>{settings.instanceName}</p></div><span className="instance-owner">{session.user.role==='owner'?'Owner':'Administrator'}</span></div>
  {notice&&<Notice>{notice}</Notice>}{tasks.error&&<Notice error>{tasks.error}</Notice>}
  <SettingsPanel key={settings.updatedAt??'initial'} settings={settings} client={client} expired={expired} busy={tasks.pending.has('settings')} save={save}><AdministratorsPanel client={client} viewer={session.user} expired={expired}/></SettingsPanel>
  <div className="instance-footer"><span>Signed in as {session.user.email}</span><div>{session.user.role==='owner'&&settings.onboarding&&<button type="button" className="instance-setup-again" disabled={tasks.pending.has('setup')} onClick={()=>void runSetupAgain()}>Run setup again</button>}<a href="/account">Google Calendar <ArrowUpRight size={12}/></a><Button variant="ghost" size="sm" onClick={()=>void signOut()}><LogOut/>Sign out</Button></div></div>
 </>
}
