import {useCallback,useState} from 'react'
import type {AdminSettingsDTO,AuthSessionDTO,UpdateAdminSettingsInput} from '@meeting-notes/contracts'
import type {AdminClient} from '../../lib/admin-client'
import {SessionGate} from '../account/session-gate'
import {AdministratorsPanel} from './administrators-panel'
import {SettingsPanel} from './settings-panel'
import {Loading,Notice,useAdminTask,useResource} from './shared'
import {Button} from '../ui/button'
import {ArrowUpRight,LogOut} from 'lucide-react'
import '../../admin.css'

export function AdminPortal(){return <div className="admin-page instance-settings"><SessionGate>{(session,client,expired,signOut)=>session.user.role==='user'
 ?<section className="admin-login"><p className="eyebrow">Instance settings</p><h1>Administrator access required.</h1><p>Ask your instance owner for access to these settings.</p><a href="/account" className="button">Your account</a></section>
 :<LoadWorkspace session={session} client={client} expired={expired} signOut={signOut}/>}</SessionGate></div>}
function LoadWorkspace({session,client,expired,signOut}:{session:AuthSessionDTO;client:AdminClient;expired:()=>void;signOut:()=>Promise<void>}){
 const load=useCallback((signal:AbortSignal)=>client.getSettings(signal),[client]),resource=useResource(load,expired)
 if(!resource.data)return <section className="admin-card">{resource.error?<Notice error>{resource.error}<button onClick={()=>void resource.reload()}>Retry</button></Notice>:<Loading label="Loading instance settings…"/>}</section>
 return <Workspace session={session} client={client} initialSettings={resource.data} expired={expired} signOut={signOut}/>
}
function Workspace({session,client,initialSettings,expired,signOut}:{session:AuthSessionDTO;client:AdminClient;initialSettings:AdminSettingsDTO;expired:()=>void;signOut:()=>Promise<void>}){
 const[settings,setSettings]=useState(initialSettings),[notice,setNotice]=useState<string|null>(null),tasks=useAdminTask(expired)
 async function save(patch:UpdateAdminSettingsInput){const result=await tasks.run('settings',signal=>client.updateSettings(patch,signal));if(result.ok){setSettings(result.value);setNotice('Instance settings saved.')}}
 return <>
  <div className="instance-heading"><div><h1>Instance settings</h1><p>{settings.instanceName}</p></div><span className="instance-owner">{session.user.role==='owner'?'Owner':'Administrator'}</span></div>
  {notice&&<Notice>{notice}</Notice>}{tasks.error&&<Notice error>{tasks.error}</Notice>}
  <SettingsPanel key={settings.updatedAt??'initial'} settings={settings} client={client} expired={expired} busy={tasks.pending.has('settings')} save={save}><AdministratorsPanel client={client} viewer={session.user} expired={expired}/></SettingsPanel>
  <div className="instance-footer"><span>Signed in as {session.user.email}</span><div><a href="/account">Your account <ArrowUpRight size={12}/></a><Button variant="ghost" size="sm" onClick={()=>void signOut()}><LogOut/>Sign out</Button></div></div>
 </>
}
