import {useCallback,useState} from 'react'
import type {UserDTO} from '@meeting-notes/contracts'
import type {AdminClient} from '../../lib/admin-client'
import {Button} from '../ui/button'
import {Badge} from '../ui/badge'
import {Card,CardContent,CardHeader,CardTitle} from '../ui/card'
import {Modal,Notice,useAdminTask,useResource} from './shared'
import {Plus,ShieldCheck} from 'lucide-react'

export function AdministratorsPanel(props:{client:AdminClient;viewer:UserDTO;expired:()=>void}){
 return props.viewer.role==='owner'?<OwnerAdministrators {...props}/>:null
}
function OwnerAdministrators({client,viewer,expired}:{client:AdminClient;viewer:UserDTO;expired:()=>void}){
 const load=useCallback((signal:AbortSignal)=>client.listUsers(signal),[client]),resource=useResource(load,expired),tasks=useAdminTask(expired)
 const[adding,setAdding]=useState(false),[revoking,setRevoking]=useState<UserDTO|null>(null),[notice,setNotice]=useState<string|null>(null)
 const users=resource.data?.users??[],busy=tasks.pending.size>0
 async function promote(userId:string){const result=await tasks.run('admin',signal=>client.updateUserRole(userId,{role:'admin'},signal));if(result.ok){setAdding(false);setNotice('Administrator added.');await resource.reload()}}
 async function revoke(user:UserDTO){const result=await tasks.run('admin',signal=>client.updateUserRole(user.id,{role:'user'},signal));if(result.ok){setRevoking(null);setNotice('Administrator access revoked.');await resource.reload()}}
 return <>
  <Card id="administrators" className="administrators-card"><CardHeader className="section-with-action"><div><CardTitle>Administrators</CardTitle><span className="section-caption">Only you can manage admin access.</span></div><Button type="button" variant="outline" size="sm" disabled={busy||!resource.data} onClick={()=>{tasks.clearError();setAdding(true);void resource.reload()}}><Plus/>Add admin</Button></CardHeader><CardContent>
   {notice&&<p className="admin-inline-notice" role="status">{notice}</p>}
   {resource.error&&<Notice error>{resource.error}<Button type="button" variant="ghost" size="sm" onClick={()=>void resource.reload()}>Retry</Button></Notice>}
   {!resource.data&&!resource.error?<p className="settings-help">Loading administrators…</p>:<AdministratorsList users={users} viewer={viewer} busy={busy} revoke={user=>{tasks.clearError();setRevoking(user)}}/>}
  </CardContent></Card>
  {adding&&<AddAdministrator users={users} busy={busy} loading={resource.busy} error={tasks.error??resource.error} close={()=>setAdding(false)} promote={promote} refresh={resource.reload}/>}
  {revoking&&<Modal title="Revoke admin access?" busy={busy} close={()=>setRevoking(null)}><div className="admin-dialog-body"><p><strong>{revoking.email}</strong> will lose instance-settings access. Their notes are kept.</p>{tasks.error&&<Notice error>{tasks.error}</Notice>}</div><div className="admin-dialog-actions"><Button type="button" variant="outline" disabled={busy} onClick={()=>setRevoking(null)}>Cancel</Button><Button type="button" variant="destructive" disabled={busy} onClick={()=>void revoke(revoking)}>{busy?'Revoking…':'Revoke admin'}</Button></div></Modal>}
 </>
}
export function AdministratorsList({users,viewer,busy,revoke}:{users:readonly UserDTO[];viewer:UserDTO;busy:boolean;revoke:(user:UserDTO)=>void}){
 if(viewer.role!=='owner')return null
 return <div className="administrators-list">{users.filter(user=>user.role==='owner'||user.role==='admin').map(user=><div className="administrator-row" key={user.id}><div className="administrator-avatar" aria-hidden="true">{user.name.trim().slice(0,1).toUpperCase()||'A'}</div><span className="administrator-person"><strong>{user.name}{user.id===viewer.id&&<small>you</small>}</strong><span>{user.email}</span></span>{user.role==='owner'?<Badge variant="secondary"><ShieldCheck size={11}/>Owner</Badge>:<Button type="button" variant="ghost" size="sm" disabled={busy} aria-label={`Revoke admin access for ${user.email}`} onClick={()=>revoke(user)}>Revoke</Button>}</div>)}</div>
}
export function AddAdministrator({users,busy,loading=false,error,close,promote,refresh}:{users:readonly UserDTO[];busy:boolean;loading?:boolean;error:string|null;close:()=>void;promote:(id:string)=>Promise<void>;refresh:()=>Promise<void>}){
 const[selected,setSelected]=useState('')
 const eligible=users.filter(user=>user.role==='user').sort((a,b)=>(a.name||a.email).localeCompare(b.name||b.email))
 const valid=eligible.some(user=>user.id===selected)
 return <Modal title="Add administrator" busy={busy} close={close}><form onSubmit={e=>{e.preventDefault();if(!busy&&!loading&&valid)void promote(selected)}}>
  <fieldset className="admin-fieldset admin-dialog-body" disabled={busy||loading}>
   <p>Choose an existing user to manage this instance’s settings.</p>
   <label className="settings-label">User<select className="admin-account-select" aria-label="User to promote" required disabled={!eligible.length||busy||loading} value={valid?selected:''} onChange={e=>setSelected(e.target.value)}><option value="" disabled>{eligible.length?'Choose a user':'No eligible users'}</option>{eligible.map(user=><option key={user.id} value={user.id}>{user.name.trim()?`${user.name} · ${user.email}`:user.email}</option>)}</select></label>
   {!eligible.length&&!busy&&!loading&&!error&&<p className="settings-help">Users must already belong to this instance. Current admins and the owner are excluded.</p>}
   {error&&<Notice error>{error}<Button type="button" variant="ghost" size="sm" onClick={()=>void refresh()}>Refresh users</Button></Notice>}
  </fieldset>
  <div className="admin-dialog-actions"><Button type="button" variant="outline" disabled={busy} onClick={close}>Cancel</Button><Button type="submit" disabled={busy||loading||!valid}>{busy?'Granting…':loading?'Loading users…':'Grant admin'}</Button></div>
 </form></Modal>
}
