import {useEffect,useRef,useState} from 'react'
import type {SignInSettingsDTO} from '@meeting-notes/contracts'
import {addAllowedEmails,googleClientReady,type SignInDraft} from '../../lib/instance-settings'
import {Button} from '../ui/button'
import {Input} from '../ui/input'
import {Card,CardContent,CardHeader,CardTitle} from '../ui/card'
import {Check,Copy,Plus,X} from 'lucide-react'

/** Sign-in methods, who may create an account with Google, and the Google OAuth client. Part of the
 * settings form: everything here is a draft until "Save changes". */
export function SignInSettings({draft,saved,change}:{draft:SignInDraft;saved:SignInSettingsDTO;change:(next:SignInDraft)=>void}){
 const[entry,setEntry]=useState(''),[entryProblem,setEntryProblem]=useState<string|null>(null),[copied,setCopied]=useState<string|null>(null)
 const timer=useRef<number|undefined>(undefined)
 useEffect(()=>()=>window.clearTimeout(timer.current),[])
 const ready=googleClientReady(draft,saved.googleClientSecretSet),methods=Number(draft.password)+Number(draft.google)
 function add(){const result=addAllowedEmails(draft.allowedEmails,entry);setEntryProblem(result.problem);if(!result.problem){change({...draft,allowedEmails:result.list});setEntry('')}}
 async function copy(uri:string){try{await navigator.clipboard.writeText(uri);setCopied(uri);window.clearTimeout(timer.current);timer.current=window.setTimeout(()=>setCopied(null),2000)}catch{setCopied(null)}}
 return <Card id="sign-in"><CardHeader><CardTitle>Sign-in</CardTitle></CardHeader><CardContent>
  <div className="model-section"><div className="model-section-heading"><h3>Methods</h3><span>{methods===2?'Both on':methods===1?'1 on':'None on'}</span></div><div className="model-controls sign-in-block">
   <label className="live-toggle sign-in-method"><input type="checkbox" checked={draft.password} onChange={e=>change({...draft,password:e.target.checked})}/>Email and password</label>
   <label className="live-toggle sign-in-method"><input type="checkbox" checked={draft.google} onChange={e=>change({...draft,google:e.target.checked})}/>Google</label>
   <p className="settings-help">{!draft.password&&!draft.google?'At least one way to sign in has to stay on.':draft.google&&!ready?'Add the Google client ID and secret below before saving.':!draft.password?'Passwords stop working for everyone, including you. The server’s owner token keeps working.':'People who are already signed in stay signed in.'}</p>
  </div></div>
  <div className="model-section"><div className="model-section-heading"><h3>Allowed emails</h3><span>{draft.allowedEmails.length===1?'1 entry':`${draft.allowedEmails.length} entries`}</span></div><div className="model-controls sign-in-block">
   <div className="allowed-add"><Input id="allowed-email" type="text" inputMode="email" autoComplete="off" autoCapitalize="none" spellCheck={false} aria-label="Email address or domain to allow" aria-invalid={entryProblem?true:undefined} aria-describedby={entryProblem?'allowed-email-problem':'allowed-email-help'} placeholder="someone@example.com or @example.com" value={entry} onChange={e=>{setEntry(e.target.value);setEntryProblem(null)}} onKeyDown={e=>{if(e.key==='Enter'){e.preventDefault();add()}}}/><Button type="button" variant="outline" onClick={add}><Plus/>Add</Button></div>
   {entryProblem&&<p id="allowed-email-problem" role="alert" className="settings-error">{entryProblem}</p>}
   {draft.allowedEmails.length>0&&<ul className="allowed-list" aria-label="Allowed emails and domains">{draft.allowedEmails.map(email=><li className="allowed-chip" key={email}><span>{email}</span><button type="button" aria-label={`Remove ${email}`} onClick={()=>change({...draft,allowedEmails:draft.allowedEmails.filter(other=>other!==email)})}><X/></button></li>)}</ul>}
   <p id="allowed-email-help" className="settings-help allowed-help">Only these addresses and domains can create an account by signing in with Google; people who already have an account can always sign in.</p>
  </div></div>
  <div className="model-section"><div className="model-section-heading"><h3>Google client</h3><span>{ready?(saved.googleClientSecretSet&&!draft.googleClientSecret&&!draft.removeGoogleSecret&&draft.googleClientId.trim()===(saved.googleClientId??'')?'Saved':'Ready to save'):'Not set'}</span></div><div className="model-controls sign-in-block">
   <div className="google-client-fields">
    <div><label className="settings-label" htmlFor="google-client-id">Client ID</label><Input id="google-client-id" type="text" autoComplete="off" autoCapitalize="none" spellCheck={false} maxLength={255} placeholder="…apps.googleusercontent.com" value={draft.googleClientId} onChange={e=>change({...draft,googleClientId:e.target.value})}/></div>
    <div><label className="settings-label" htmlFor="google-client-secret">Client secret</label><Input id="google-client-secret" type="password" autoComplete="new-password" maxLength={512} disabled={draft.removeGoogleSecret} placeholder={saved.googleClientSecretSet?'Leave blank to keep the current secret':'Enter the client secret'} value={draft.googleClientSecret} onChange={e=>change({...draft,googleClientSecret:e.target.value})}/></div>
   </div>
   <p className="settings-help">Used for Google sign-in and the Google Calendar connection. The secret is encrypted on the server and never returned to the browser.</p>
   {saved.googleClientSecretSet&&<label className="remove-key"><input type="checkbox" checked={draft.removeGoogleSecret} onChange={e=>change({...draft,removeGoogleSecret:e.target.checked,googleClientSecret:''})}/>Remove the saved secret</label>}
   <div className="redirect-uris"><p className="settings-label" id="redirect-uris-label">Authorized redirect URIs</p><ul aria-labelledby="redirect-uris-label">{saved.googleRedirectUris.map((uri,index)=><li className="redirect-uri" key={uri}><span className="redirect-uri-purpose">{index===0?'Sign-in':'Calendar'}</span><code>{uri}</code><Button type="button" variant="ghost" size="sm" aria-label={`Copy the ${index===0?'sign-in':'calendar'} redirect URI`} onClick={()=>void copy(uri)}>{copied===uri?<><Check/>Copied</>:<><Copy/>Copy</>}</Button></li>)}</ul><p className="settings-help">Add both to the OAuth client in Google Cloud, exactly as written.</p></div>
  </div></div>
 </CardContent></Card>
}
