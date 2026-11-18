import {useEffect,useRef,useState} from 'react'
import {copyText} from '../../lib/clipboard'
import {GOOGLE_NEEDS_HTTPS} from '@meeting-notes/contracts/transport'
import type {AdminSettingsDTO,SignInSettingsDTO} from '@meeting-notes/contracts'
import {AdminClientError,type AdminClient} from '../../lib/admin-client'
import {googleClientProblem,GOOGLE_STUCK_HINT,proveWithGoogle,startGoogleSwitch,type SignInMethod} from '../../lib/onboarding'
import {addAllowedEmails,googleClientReady,savedGoogleFields,teamDomain,withTeamDomain,type SignInDraft} from '../../lib/instance-settings'
import {Button} from '../ui/button'
import {Input} from '../ui/input'
import {Card,CardContent,CardHeader,CardTitle} from '../ui/card'
import {Reveal,Swap} from './shared'
import {useFlow} from './settings-motion'
import {Check,Copy,Loader2,Plus,X} from 'lucide-react'

/** How people sign in: one method, email and password or Google. The method is changed here only by proving the
 * new one first, so nobody locks themselves out: switching to Google runs one real Google sign-in with the client
 * typed in, and switching to email and password sets a password for the administrator who is switching. Who may
 * join with Google, and the Google client, are part of the settings form like everything else. */
export function SignInSettings({draft,saved,change,client,switched,expired,viewerEmail}:{/** The signed-in administrator's address: offers "anyone at their domain". */viewerEmail?:string;draft:SignInDraft;saved:SignInSettingsDTO;change:(next:SignInDraft)=>void;client:AdminClient;switched:(settings:AdminSettingsDTO)=>void;expired:()=>void}){
 const[entry,setEntry]=useState(''),[entryProblem,setEntryProblem]=useState<string|null>(null),[copied,setCopied]=useState<string|null>(null)
 const timer=useRef<number|undefined>(undefined),proving=useRef<AbortController|null>(null)
 useEffect(()=>()=>{window.clearTimeout(timer.current);proving.current?.abort()},[])
 const current:SignInMethod=saved.google?'google':'password'
 const[wanted,setWanted]=useState<SignInMethod>(current),[google,setGoogle]=useState({clientId:saved.googleClientId??'',clientSecret:''}),[password,setPassword]=useState('')
 const[busy,setBusy]=useState<null|'google'|'password'>(null),[problem,setProblem]=useState<string|null>(null)
 const ready=googleClientReady(draft,saved.googleClientSecretSet),domain=teamDomain(viewerEmail)
 // The allowed addresses travel when one is added or taken out (./settings-motion).
 const allowedList=useRef<HTMLUListElement>(null)
 useFlow(allowedList,'pop')
 function add(){const result=addAllowedEmails(draft.allowedEmails,entry);setEntryProblem(result.problem);if(!result.problem){change({...draft,allowedEmails:result.list});setEntry('')}}
 async function copy(uri:string){try{await copyText(uri);setCopied(uri);window.clearTimeout(timer.current);timer.current=window.setTimeout(()=>setCopied(null),2000)}catch{setCopied(null)}}
 async function toGoogle(){
  const trouble=googleClientProblem(google);if(trouble){setProblem(trouble.message);return}
  const controller=new AbortController();proving.current=controller;setBusy('google');setProblem(null)
  try{
   const outcome=await proveWithGoogle(signal=>startGoogleSwitch(google,signal),controller.signal)
   if(controller.signal.aborted)return
   if(!outcome.ok){setProblem(outcome.failure.message);return}
   switched(await client.getSettings(controller.signal))
  }catch(error){if(error instanceof AdminClientError&&error.status===401)expired();else if(!controller.signal.aborted)setProblem('The switch could not be finished. Nothing was changed; try again.')}
  finally{proving.current=null;if(!controller.signal.aborted)setBusy(null);else setBusy(null)}
 }
 async function toPassword(){
  if(password.length<12){setProblem('Use at least 12 characters for your password.');return}
  setBusy('password');setProblem(null)
  // The password leaves the page as soon as it is sent.
  const sent=password;setPassword('')
  try{switched(await client.switchToPassword({password:sent}))}
  catch(error){if(error instanceof AdminClientError&&error.status===401)expired();else setProblem(error instanceof AdminClientError&&error.status===403?'Sign in again in this browser, then switch.':'The switch could not be finished. Nothing was changed; try again.')}
  finally{setBusy(null)}
 }
 /** Whether the person changed the choice since the form was drawn: only then does the line under the choice change with a fade.
  * The parts that come and go with the choice open and close by themselves (Reveal), and are simply there when drawn. */
 const[chosen,setChosen]=useState(false)
 // Picking email and password hides the Google sections, and what is hidden is not saved: their unsaved edits are
 // set aside and the draft goes back to the saved values. Picking Google again, before saving, brings them back.
 const setAside=useRef<Pick<SignInDraft,'allowedEmails'|'googleClientId'|'googleClientSecret'|'removeGoogleSecret'>|null>(null)
 const choose=(method:SignInMethod)=>{
  if(busy)return
  setWanted(method);setProblem(null);setChosen(true)
  if(current!=='google')return
  if(method==='password'&&!setAside.current){
   setAside.current={allowedEmails:draft.allowedEmails,googleClientId:draft.googleClientId,googleClientSecret:draft.googleClientSecret,removeGoogleSecret:draft.removeGoogleSecret}
   change(savedGoogleFields(draft,saved))
  }else if(method==='google'&&setAside.current){change({...draft,...setAside.current});setAside.current=null}
 }
 return <Card><CardHeader><CardTitle>Sign-in</CardTitle></CardHeader><CardContent>
  <div className="model-section"><div className="model-section-heading"><h3>Method</h3><span><Swap text={current==='google'?'Google':'Email and password'}/></span></div><div className="model-controls sign-in-block">
   <div role="radiogroup" aria-label="How people sign in" className="storage-choice">
    <label className="live-toggle sign-in-method"><input type="radio" name="sign-in-method" checked={wanted==='password'} disabled={busy!==null} onChange={()=>choose('password')}/>Email and password</label>
    <label className="live-toggle sign-in-method"><input type="radio" name="sign-in-method" checked={wanted==='google'} disabled={busy!==null||(!!saved.googleNeedsHttps&&current!=='google')} aria-describedby="sign-in-methods-help" onChange={()=>choose('google')}/>Google</label>
   </div>
   <p key={chosen?wanted:'drawn'} className={chosen?'settings-help late-detail':'settings-help'} id="sign-in-methods-help">{saved.googleNeedsHttps&&current!=='google'?`${GOOGLE_NEEDS_HTTPS}. This server is reached over plain HTTP, and Google only signs people in to HTTPS addresses.`:wanted===current?(current==='google'?'Everyone signs in with Google. Nobody has a password here.':'Everyone signs in with an email address and a password. An administrator makes each account.'):wanted==='google'?'Switching is proved first: you sign in with Google once, and only if that works does Google replace passwords.':'Switching is proved first: set the password you will sign in with, and only then do passwords replace Google.'}</p>
   <Reveal className="reveal-row">{wanted==='google'&&current!=='google'&&<div className="sign-in-switch">
    <div className="google-client-fields">
     <div><label className="settings-label" htmlFor="switch-google-client-id">Client ID</label><Input id="switch-google-client-id" type="text" autoComplete="off" autoCapitalize="none" spellCheck={false} maxLength={255} placeholder="…apps.googleusercontent.com" disabled={busy!==null} value={google.clientId} onChange={e=>setGoogle({...google,clientId:e.target.value})}/></div>
     <div><label className="settings-label" htmlFor="switch-google-client-secret">Client secret</label><Input id="switch-google-client-secret" type="password" autoComplete="new-password" maxLength={512} disabled={busy!==null} value={google.clientSecret} onChange={e=>setGoogle({...google,clientSecret:e.target.value})} onKeyDown={e=>{if(e.key==='Enter'){e.preventDefault();void toGoogle()}}}/></div>
    </div>
    <div className="redirect-uris"><p className="settings-label" id="switch-redirect-uris-label">Authorized redirect URIs</p><ul aria-labelledby="switch-redirect-uris-label">{saved.googleRedirectUris.map((uri,index)=><li className="redirect-uri" key={uri}><span className="redirect-uri-purpose">{index===0?'Sign-in':'Calendar'}</span><code>{uri}</code><Button type="button" variant="ghost" size="sm" aria-label={`Copy the ${index===0?'sign-in':'calendar'} redirect URI`} onClick={()=>void copy(uri)}>{copied===uri?<span key="done" className="copy-label swap-in"><Check/>Copied</span>:<span key="copy" className="copy-label"><Copy/>Copy</span>}</Button></li>)}</ul><p className="settings-help">Add both to the OAuth client in Google Cloud, exactly as written.</p></div>
    <div className="sign-in-switch-actions"><Button type="button" disabled={busy!==null} onClick={()=>void toGoogle()}>{busy==='google'?<Loader2 className="animate-spin"/>:null}Verify with Google and switch</Button>{busy==='google'&&<Button type="button" variant="ghost" size="sm" onClick={()=>{proving.current?.abort();setBusy(null)}}>Cancel</Button>}</div>
    <Reveal className="reveal-row">{busy==='google'&&<p className="settings-help" role="status">Waiting for Google. Sign in to Google, as yourself, in the window that opened. {GOOGLE_STUCK_HINT}</p>}</Reveal>
   </div>}</Reveal>
   <Reveal className="reveal-row">{wanted==='password'&&current!=='password'&&<div className="sign-in-switch">
    <div className="google-client-fields"><div><label className="settings-label" htmlFor="switch-password">Your new password</label><Input id="switch-password" type="password" autoComplete="new-password" maxLength={128} disabled={busy!==null} aria-describedby="switch-password-help" value={password} onChange={e=>setPassword(e.target.value)} onKeyDown={e=>{if(e.key==='Enter'){e.preventDefault();void toPassword()}}}/><p className="settings-help" id="switch-password-help">At least 12 characters. You will sign in with your email address and this password; make accounts for everyone else under Administrators and users.</p></div></div>
    <div className="sign-in-switch-actions"><Button type="button" disabled={busy!==null} onClick={()=>void toPassword()}>{busy==='password'?<Loader2 className="animate-spin"/>:null}Set my password and switch</Button></div>
   </div>}</Reveal>
   <Reveal className="reveal-error">{problem&&<p role="alert" className="settings-error">{problem}</p>}</Reveal>
  </div></div>
  {/* Google-only settings, shown while Google is the method and stays chosen: picking email and password hides them. */}
  <Reveal className="reveal-part">{current==='google'&&wanted==='google'&&<div className="admin-google-only">
  <div className="model-section"><div className="model-section-heading"><h3>Allowed emails</h3><span><Swap text={draft.allowedEmails.length===1?'1 entry':`${draft.allowedEmails.length} entries`}/></span></div><div className="model-controls sign-in-block">
   {domain&&<label className="live-toggle sign-in-method"><input type="checkbox" checked={draft.allowedEmails.includes('@'+domain)} onChange={e=>change({...draft,allowedEmails:withTeamDomain(draft.allowedEmails,domain,e.target.checked)})}/>Anyone at {domain} with a Google Workspace account can join</label>}
   <div className="allowed-add"><Input id="allowed-email" type="text" inputMode="email" autoComplete="off" autoCapitalize="none" spellCheck={false} aria-label="Email address or domain to allow" aria-invalid={entryProblem?true:undefined} aria-describedby={entryProblem?'allowed-email-problem':'allowed-email-help'} placeholder="someone@example.com or @example.com" value={entry} onChange={e=>{setEntry(e.target.value);setEntryProblem(null)}} onKeyDown={e=>{if(e.key==='Enter'){e.preventDefault();add()}}}/><Button type="button" variant="outline" onClick={add}><Plus/>Add</Button></div>
   <Reveal className="reveal-error">{entryProblem&&<p id="allowed-email-problem" role="alert" className="settings-error">{entryProblem}</p>}</Reveal>
   <Reveal className="reveal-row">{draft.allowedEmails.length>0&&<ul ref={allowedList} className="allowed-list" aria-label="Allowed emails and domains">{draft.allowedEmails.map(email=><li className="allowed-chip" key={email}><span>{email}</span><button type="button" aria-label={`Remove ${email}`} onClick={()=>change({...draft,allowedEmails:draft.allowedEmails.filter(other=>other!==email)})}><X/></button></li>)}</ul>}</Reveal>
   <p id="allowed-email-help" className="settings-help allowed-help">Only these addresses and domains can create an account by signing in with Google; people who already have an account can always sign in.</p>
  </div></div>
  <div className="model-section"><div className="model-section-heading"><h3>Google client</h3><span><Swap text={ready?(saved.googleClientSecretSet&&!draft.googleClientSecret&&!draft.removeGoogleSecret&&draft.googleClientId.trim()===(saved.googleClientId??'')?'Saved':'Ready to save'):'Not set'}/></span></div><div className="model-controls sign-in-block">
   <div className="google-client-fields">
    <div><label className="settings-label" htmlFor="google-client-id">Client ID</label><Input id="google-client-id" type="text" autoComplete="off" autoCapitalize="none" spellCheck={false} maxLength={255} placeholder="…apps.googleusercontent.com" value={draft.googleClientId} onChange={e=>change({...draft,googleClientId:e.target.value})}/></div>
    <div><label className="settings-label" htmlFor="google-client-secret">Client secret</label><Input id="google-client-secret" type="password" autoComplete="new-password" maxLength={512} disabled={draft.removeGoogleSecret} placeholder={saved.googleClientSecretSet?`${saved.googleClientSecretHint??'A secret'} is saved. Type a new secret to replace it`:'Enter the client secret'} value={draft.googleClientSecret} onChange={e=>change({...draft,googleClientSecret:e.target.value})}/></div>
   </div>
   <p className="settings-help">Used for Google sign-in and the Google Calendar connection. The secret is encrypted on the server and never returned to the browser.</p>
   {saved.googleClientSecretSet&&<label className="remove-key"><input type="checkbox" checked={draft.removeGoogleSecret} onChange={e=>change({...draft,removeGoogleSecret:e.target.checked,googleClientSecret:''})}/>Remove the saved secret</label>}
   <div className="redirect-uris"><p className="settings-label" id="redirect-uris-label">Authorized redirect URIs</p><ul aria-labelledby="redirect-uris-label">{saved.googleRedirectUris.map((uri,index)=><li className="redirect-uri" key={uri}><span className="redirect-uri-purpose">{index===0?'Sign-in':'Calendar'}</span><code>{uri}</code><Button type="button" variant="ghost" size="sm" aria-label={`Copy the ${index===0?'sign-in':'calendar'} redirect URI`} onClick={()=>void copy(uri)}>{copied===uri?<span key="done" className="copy-label swap-in"><Check/>Copied</span>:<span key="copy" className="copy-label"><Copy/>Copy</span>}</Button></li>)}</ul><p className="settings-help">Add both to the OAuth client in Google Cloud, exactly as written.</p></div>
  </div></div>
  </div>}</Reveal>
 </CardContent></Card>
}
