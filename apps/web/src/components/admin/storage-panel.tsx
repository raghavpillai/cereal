import {useEffect,useRef,useState} from 'react'
import type {StorageSettingsDTO,StorageTestDTO,StorageTestInput} from '@meeting-notes/contracts'
import {bucketReady,storageProblem,storageTestInput,type StorageDraft} from '../../lib/instance-settings'
import {Button} from '../ui/button'
import {Input} from '../ui/input'
import {Card,CardContent,CardHeader,CardTitle} from '../ui/card'
import {Check,Loader2,PlugZap,TriangleAlert} from 'lucide-react'

/** Where recorded audio and published app builds are kept: this server's disk or an S3-compatible bucket.
 * Part of the settings form: everything here is a draft until "Save changes". "Test connection" tries the
 * draft as it stands, without saving it. */
export function StorageSettings({draft,saved,change,test}:{draft:StorageDraft;saved:StorageSettingsDTO;change:(next:StorageDraft)=>void;test:(input:StorageTestInput,signal:AbortSignal)=>Promise<StorageTestDTO>}){
 const[result,setResult]=useState<StorageTestDTO|{ok:false;message:string}|null>(null),[testing,setTesting]=useState(false)
 const running=useRef<AbortController|null>(null)
 useEffect(()=>()=>running.current?.abort(),[])
 const problem=storageProblem(draft,saved.secretAccessKeySet),ready=bucketReady(draft,saved.secretAccessKeySet),s3=draft.backend==='s3'
 // A result describes the settings that were tried: any edit makes it stale.
 const set=(next:Partial<StorageDraft>)=>{running.current?.abort();setTesting(false);setResult(null);change({...draft,...next})}
 async function run(){
  running.current?.abort();const controller=new AbortController();running.current=controller;setTesting(true);setResult(null)
  try{const answer=await test(storageTestInput(draft),controller.signal);if(!controller.signal.aborted)setResult(answer)}
  catch(error){if(!controller.signal.aborted)setResult({ok:false,message:error instanceof Error&&error.message?error.message:'The test could not be run. Try again.'})}
  finally{if(!controller.signal.aborted)setTesting(false)}
 }
 const text=(id:string,label:string,key:'endpoint'|'region'|'bucket'|'prefix'|'accessKeyId',placeholder:string,maxLength:number)=><div><label className="settings-label" htmlFor={id}>{label}</label><Input id={id} type="text" autoComplete="off" autoCapitalize="none" spellCheck={false} maxLength={maxLength} placeholder={placeholder} value={draft[key]} onChange={e=>set({[key]:e.target.value})}/></div>
 return <Card id="storage"><CardHeader><CardTitle>Storage</CardTitle></CardHeader><CardContent>
  <div className="model-section"><div className="model-section-heading"><h3>Files</h3><span>{s3?'S3-compatible':'Local disk'}</span></div><div className="model-controls sign-in-block">
   <div role="radiogroup" aria-label="Where files are kept" className="storage-choice">
    <label className="live-toggle sign-in-method"><input type="radio" name="storage-backend" checked={!s3} onChange={()=>set({backend:'local'})}/>Local disk</label>
    <label className="live-toggle sign-in-method"><input type="radio" name="storage-backend" checked={s3} onChange={()=>set({backend:'s3'})}/>S3-compatible storage</label>
   </div>
   <p className="settings-help">Recorded audio and the app builds this server offers. New files go to the storage chosen here; files already stored stay where they are and keep working. Nothing is moved.</p>
   {saved.source==='environment'&&<p className="settings-help">These settings come from the server’s environment. Saving here replaces them.</p>}
  </div></div>
  <div className="model-section"><div className="model-section-heading"><h3>Bucket</h3><span>{ready?(saved.secretAccessKeySet&&!draft.secretAccessKey&&!draft.removeSecret?'Saved':'Ready to save'):'Not set'}</span></div><div className="model-controls sign-in-block">
   <div className="storage-fields">
    {text('storage-endpoint','Endpoint','endpoint','Leave blank for Amazon S3',2048)}
    {text('storage-region','Region','region','us-east-1',40)}
    {text('storage-bucket','Bucket','bucket','my-cereal-files',63)}
    {text('storage-prefix','Prefix (optional)','prefix','cereal/',200)}
    {text('storage-access-key','Access key ID','accessKeyId','',128)}
    <div><label className="settings-label" htmlFor="storage-secret">Secret access key</label><Input id="storage-secret" type="password" autoComplete="new-password" maxLength={512} disabled={draft.removeSecret} placeholder={saved.secretAccessKeySet?'Leave blank to keep the current secret':'Enter the secret access key'} value={draft.secretAccessKey} onChange={e=>set({secretAccessKey:e.target.value})}/></div>
   </div>
   <label className="live-toggle sign-in-method"><input type="checkbox" checked={draft.forcePathStyle} onChange={e=>set({forcePathStyle:e.target.checked})}/>Path-style addresses (MinIO and most self-hosted stores)</label>
   <p className="settings-help">Works with Amazon S3, Cloudflare R2, MinIO and Backblaze B2. The secret is encrypted on the server and never returned to the browser.</p>
   {saved.secretAccessKeySet&&<label className="remove-key"><input type="checkbox" checked={draft.removeSecret} onChange={e=>set({removeSecret:e.target.checked,secretAccessKey:''})}/>Remove the saved secret</label>}
   <div className="storage-test"><Button type="button" variant="outline" disabled={testing||!!problem&&!(draft.backend==='local')} onClick={()=>void run()}>{testing?<Loader2 className="animate-spin"/>:<PlugZap/>}Test connection</Button>
    <p className={result&&!result.ok?'settings-error storage-test-result':'settings-help storage-test-result'} role="status" aria-live="polite">{testing?'Writing, reading and deleting a small test file…':result?<>{result.ok?<Check size={12}/>:<TriangleAlert size={12}/>}{result.message}</>:s3||ready?'Tries these settings as they are on screen, before you save them.':'Checks that the server can write to its disk.'}</p>
   </div>
  </div></div>
 </CardContent></Card>
}
