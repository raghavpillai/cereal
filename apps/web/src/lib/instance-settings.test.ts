import {expect,test} from 'bun:test'
import {addAllowedEmails,allowedEmailEntry,defaultLiveModel,googleClientReady,instanceDraft,instancePatch,instanceProblem,storageProblem,storageTestInput,bucketReady,appsProblem,liveLimitsProblem} from './instance-settings'
import type {AdminSettingsDTO} from '@meeting-notes/contracts'
const saved:AdminSettingsDTO={instanceName:'Team',instanceLogo:null,providers:[{id:'openai',configured:true,source:'stored'}],chatModels:[{provider:'openai',model:'gpt-test'}],generationModel:{provider:'openai',model:'gpt-test'},transcriptionModel:{provider:'elevenlabs',model:'scribe_v3'},languageModel:'legacy/model',openRouterConfigured:false,elevenLabsConfigured:false,openRouterSource:'none',elevenLabsSource:'none',calendarRefreshMinutes:15,calendarLookAheadDays:30,updatedAt:null}
test('instance edits preserve untouched keys and native calendar preferences',()=>{
 const draft=instanceDraft(saved);expect(instancePatch(draft,saved)).toEqual({})
 expect(instancePatch({...draft,instanceName:' New name '},saved)).toEqual({instanceName:'New name'})
 expect(instancePatch({...draft,removed:{...draft.removed,openai:true}},saved)).toEqual({openAiApiKey:null})
 expect(instancePatch({...draft,keys:{...draft.keys,anthropic:'draft-provider-key'}},saved)).toEqual({anthropicApiKey:'draft-provider-key'})
 expect(instanceProblem({...draft,chatModels:[]})).toContain('at least one')
 expect(instanceProblem({...draft,keys:{...draft.keys,openai:'line-one\nline-two'}})).toContain('single-line')
})
test('live transcription is patched only when it changes, and null turns it off',()=>{
 const on:AdminSettingsDTO={...saved,liveTranscriptionModel:{provider:'elevenlabs',model:'scribe_v3_live'}},draft=instanceDraft(on)
 expect(draft.liveTranscriptionModel).toEqual({provider:'elevenlabs',model:'scribe_v3_live'})
 expect(instancePatch(draft,on)).toEqual({})
 expect(instancePatch({...draft,liveTranscriptionModel:null},on)).toEqual({liveTranscriptionModel:null})
 expect(instancePatch({...draft,liveTranscriptionModel:{provider:'elevenlabs',model:'scribe_v2_realtime'}},on)).toEqual({liveTranscriptionModel:{provider:'elevenlabs',model:'scribe_v2_realtime'}})
 // A server that does not report the setting reads as off, and stays untouched until it is turned on.
 const older=instanceDraft(saved);expect(older.liveTranscriptionModel).toBeNull();expect(instancePatch(older,saved)).toEqual({})
 expect(instancePatch({...older,liveTranscriptionModel:defaultLiveModel},saved)).toEqual({liveTranscriptionModel:defaultLiveModel})
})
const signIn={password:true,google:false,allowedEmails:['someone@example.com'],googleClientId:null,googleClientSecretSet:false,googleRedirectUris:['https://notes.example.test/api/v1/auth/google/callback','https://notes.example.test/api/integrations/google/callback']}
const clientId='1234567890-fixture.apps.googleusercontent.com',clientSecret='GOCSPX-fixture-client-secret-value'
test('sign-in is patched field by field, the secret is write-only, and an older server is left alone',()=>{
 const withSignIn:AdminSettingsDTO={...saved,signIn},draft=instanceDraft(withSignIn)
 expect(draft.signIn).toEqual({password:true,google:false,allowedEmails:['someone@example.com'],googleClientId:'',googleClientSecret:'',removeGoogleSecret:false})
 expect(instancePatch(draft,withSignIn)).toEqual({})
 const edit=(change:Partial<NonNullable<typeof draft.signIn>>)=>({...draft,signIn:{...draft.signIn!,...change}})
 expect(instancePatch(edit({allowedEmails:['someone@example.com','@example.org']}),withSignIn)).toEqual({signIn:{allowedEmails:['someone@example.com','@example.org']}})
 expect(instancePatch(edit({google:true,googleClientId:` ${clientId} `,googleClientSecret:` ${clientSecret} `}),withSignIn)).toEqual({signIn:{google:true,googleClientId:clientId,googleClientSecret:clientSecret}})
 // With a client already stored: an untouched secret is not sent, clearing the ID sends null, and removal sends null once.
 const stored:AdminSettingsDTO={...saved,signIn:{...signIn,google:true,googleClientId:clientId,googleClientSecretSet:true}},storedDraft=instanceDraft(stored)
 expect(storedDraft.signIn!.googleClientSecret).toBe('')
 expect(instancePatch(storedDraft,stored)).toEqual({})
 expect(instancePatch({...storedDraft,signIn:{...storedDraft.signIn!,password:false}},stored)).toEqual({signIn:{password:false}})
 expect(instancePatch({...storedDraft,signIn:{...storedDraft.signIn!,google:false,googleClientId:'',removeGoogleSecret:true}},stored)).toEqual({signIn:{google:false,googleClientId:null,googleClientSecret:null}})
 expect(instancePatch({...storedDraft,signIn:{...storedDraft.signIn!,removeGoogleSecret:true,googleClientSecret:'typed-then-removed'}},stored).signIn).toEqual({googleClientSecret:null})
 expect(instancePatch(edit({removeGoogleSecret:true}),withSignIn)).toEqual({}) // nothing stored, nothing to remove
 // A server that does not report sign-in settings gets no sign-in patch and no sign-in problem.
 const older=instanceDraft(saved);expect(older.signIn).toBeNull();expect(instancePatch(older,saved)).toEqual({});expect(instanceProblem(older,saved)).toBeNull()
})
test('sign-in problems are caught before saving: no method left, Google without a client, malformed client values',()=>{
 const withSignIn:AdminSettingsDTO={...saved,signIn},draft=instanceDraft(withSignIn),edit=(change:Partial<NonNullable<typeof draft.signIn>>)=>({...draft,signIn:{...draft.signIn!,...change}})
 expect(instanceProblem(draft,withSignIn)).toBeNull()
 expect(instanceProblem(edit({password:false}),withSignIn)).toContain('at least one')
 expect(instanceProblem(edit({google:true}),withSignIn)).toContain('client ID and client secret')
 expect(instanceProblem(edit({google:true,googleClientId:clientId}),withSignIn)).toContain('client ID and client secret')
 expect(instanceProblem(edit({google:true,googleClientId:clientId,googleClientSecret:clientSecret}),withSignIn)).toBeNull()
 expect(instanceProblem(edit({password:false,google:true,googleClientId:clientId,googleClientSecret:clientSecret}),withSignIn)).toBeNull()
 expect(instanceProblem(edit({googleClientId:'has spaces'}),withSignIn)).toContain('client ID')
 expect(instanceProblem(edit({googleClientId:clientId,googleClientSecret:'short'}),withSignIn)).toContain('client secret')
 // A stored secret counts, unless it is being removed.
 const stored:AdminSettingsDTO={...saved,signIn:{...signIn,google:true,googleClientId:clientId,googleClientSecretSet:true}},storedDraft=instanceDraft(stored)
 expect(instanceProblem(storedDraft,stored)).toBeNull()
 expect(googleClientReady(storedDraft.signIn!,true)).toBe(true)
 expect(instanceProblem({...storedDraft,signIn:{...storedDraft.signIn!,removeGoogleSecret:true}},stored)).toContain('client ID and client secret')
 expect(instanceProblem({...storedDraft,signIn:{...storedDraft.signIn!,googleClientId:''}},stored)).toContain('client ID and client secret')
})
test('the allow list takes addresses and domains, lower-cased, several at once, with a reason when something is refused',()=>{
 expect(allowedEmailEntry('  Someone@Example.COM ')).toBe('someone@example.com');expect(allowedEmailEntry('@Example.org')).toBe('@example.org')
 for(const bad of['','example.org','someone@','someone@example','@','a@b@c.test','some one@example.test'])expect(allowedEmailEntry(bad)).toBeNull()
 expect(addAllowedEmails([],'New@Example.com')).toEqual({list:['new@example.com'],problem:null})
 expect(addAllowedEmails(['a@example.com'],'b@example.com, @Example.org;c@example.com  a@example.com')).toEqual({list:['a@example.com','b@example.com','@example.org','c@example.com'],problem:null})
 expect(addAllowedEmails(['a@example.com'],'A@example.com')).toEqual({list:['a@example.com'],problem:'This is already on the list.'})
 expect(addAllowedEmails(['a@example.com'],'   ').problem).toContain('Enter an email address')
 expect(addAllowedEmails(['a@example.com'],'example.org').problem).toContain('full email address or a domain')
 // One bad entry refuses the whole paste and names it; nothing is half-added.
 const mixed=addAllowedEmails(['a@example.com'],'b@example.com nonsense');expect(mixed.list).toEqual(['a@example.com']);expect(mixed.problem).toContain('nonsense')
 const full=Array.from({length:500},(_,i)=>`p${i}@example.com`);expect(addAllowedEmails(full,'one-more@example.com')).toEqual({list:full,problem:'The list can hold up to 500 entries.'})
})

const storage={backend:'local',source:'default',endpoint:null,region:'us-east-1',bucket:null,prefix:'',forcePathStyle:false,accessKeyId:null,secretAccessKeySet:false} as const
test('storage is patched field by field, the secret is write-only, and an older server is left alone',()=>{
 const withStorage={...saved,storage},draft=instanceDraft(withStorage)
 expect(draft.storage).toEqual({backend:'local',endpoint:'',region:'us-east-1',bucket:'',prefix:'',forcePathStyle:false,accessKeyId:'',secretAccessKey:'',removeSecret:false})
 expect(instancePatch(draft,withStorage)).toEqual({})
 const edited={...draft,storage:{...draft.storage!,backend:'s3' as const,endpoint:' https://store.example.com/ ',bucket:' cereal-files ',prefix:'/team/',forcePathStyle:true,accessKeyId:'AKIAEXAMPLE0001',secretAccessKey:' secret-access-key-value '}}
 expect(instancePatch(edited,withStorage)).toEqual({storage:{backend:'s3',endpoint:'https://store.example.com/',bucket:'cereal-files',prefix:'/team/',forcePathStyle:true,accessKeyId:'AKIAEXAMPLE0001',secretAccessKey:'secret-access-key-value'}})
 expect(instanceProblem(edited,withStorage)).toBeNull()
 // Saved settings come back without the secret: an untouched draft changes nothing, and a blank secret keeps the stored one.
 const stored={...saved,storage:{backend:'s3',source:'stored',endpoint:'https://store.example.com',region:'auto',bucket:'cereal-files',prefix:'team/',forcePathStyle:true,accessKeyId:'AKIAEXAMPLE0001',secretAccessKeySet:true}} as const
 const again=instanceDraft(stored)
 expect(again.storage!.secretAccessKey).toBe('')
 expect(instancePatch(again,stored)).toEqual({})
 expect(instancePatch({...again,storage:{...again.storage!,prefix:'team'}},stored)).toEqual({})
 expect(instancePatch({...again,storage:{...again.storage!,backend:'local'}},stored)).toEqual({storage:{backend:'local'}})
 expect(instancePatch({...again,storage:{...again.storage!,endpoint:'',region:''}},stored)).toEqual({storage:{endpoint:null,region:null}})
 expect(instancePatch({...again,storage:{...again.storage!,removeSecret:true}},stored)).toEqual({storage:{secretAccessKey:null}})
 expect(instanceProblem({...again,storage:{...again.storage!,removeSecret:true}},stored)).toBe('Add the bucket, access key ID and secret access key to use S3-compatible storage.')
 // "Test connection" sends what is on screen; a blank secret is left out so the server uses the stored one.
 expect(storageTestInput(again.storage!)).toEqual({backend:'s3',endpoint:'https://store.example.com',region:'auto',bucket:'cereal-files',prefix:'team/',forcePathStyle:true,accessKeyId:'AKIAEXAMPLE0001'})
 expect(storageTestInput({...again.storage!,secretAccessKey:'another-secret-value'}).secretAccessKey).toBe('another-secret-value')
 expect(instanceDraft(saved).storage).toBeNull()
 expect(instancePatch(instanceDraft(saved),saved)).toEqual({})
})
test('storage problems are caught before saving: S3 without a bucket or keys, an insecure or malformed endpoint, a bad bucket name',()=>{
 const base={backend:'s3' as const,endpoint:'',region:'',bucket:'cereal-files',prefix:'',forcePathStyle:false,accessKeyId:'AKIAEXAMPLE0001',secretAccessKey:'secret-access-key-value',removeSecret:false}
 expect(storageProblem(base,false)).toBeNull()
 expect(storageProblem({...base,secretAccessKey:''},true)).toBeNull()
 expect(storageProblem({...base,secretAccessKey:''},false)).toBe('Add the bucket, access key ID and secret access key to use S3-compatible storage.')
 expect(storageProblem({...base,bucket:''},false)).toBe('Add the bucket, access key ID and secret access key to use S3-compatible storage.')
 expect(storageProblem({...base,backend:'local',bucket:'',accessKeyId:'',secretAccessKey:''},false)).toBeNull()
 for(const endpoint of ['http://store.example.com','store.example.com','https://user:pass@store.example.com','https://store.example.com/?a=1'])expect(storageProblem({...base,endpoint},false),endpoint).not.toBeNull()
 for(const endpoint of ['https://store.example.com','http://localhost:9000','http://127.0.0.1:9100','https://host.example/storage/v1/s3'])expect(storageProblem({...base,endpoint},false),endpoint).toBeNull()
 for(const bucket of ['My_Bucket','ab','a..b','-cereal'])expect(storageProblem({...base,bucket},false),bucket).toBe('Bucket names are 3 to 63 lower-case letters, digits, hyphens and dots.')
 expect(storageProblem({...base,region:'US East'},false)).not.toBeNull()
 expect(storageProblem({...base,prefix:'a b'},false)).not.toBeNull()
 expect(storageProblem({...base,secretAccessKey:'short'},false)).toBe('Enter the secret access key exactly as the store shows it.')
 expect(bucketReady({...base,removeSecret:true},true)).toBe(false)
})

test('live limits and the iPhone app are patched only when they change, and checked before saving',()=>{
 const withAll={...saved,liveMaxSessions:20,liveHoldSeconds:300,apps:{iosAppIds:['LZWN2FF7YB.dev.cereal.mobile'],iosAppStoreId:null}} as const,draft=instanceDraft(withAll)
 // The built-in app ID is shown as a blank field, and an untouched draft changes nothing.
 expect(draft).toMatchObject({liveMaxSessions:'20',liveHoldSeconds:'300',apps:{iosAppIds:'',iosAppStoreId:''}})
 expect(instancePatch(draft,withAll)).toEqual({})
 expect(instancePatch({...draft,liveMaxSessions:' 5 ',liveHoldSeconds:'0',apps:{iosAppIds:'ABCDE12345.com.example.notes,\n ABCDE12345.com.example.notes FGHIJ67890.org.example.app',iosAppStoreId:' 6471234567 '}},withAll)).toEqual({liveMaxSessions:5,liveHoldSeconds:0,apps:{iosAppIds:['ABCDE12345.com.example.notes','FGHIJ67890.org.example.app'],iosAppStoreId:'6471234567'}})
 const custom={...withAll,apps:{iosAppIds:['ABCDE12345.com.example.notes'],iosAppStoreId:'6471234567'}} as const
 expect(instanceDraft(custom).apps).toEqual({iosAppIds:'ABCDE12345.com.example.notes',iosAppStoreId:'6471234567'})
 expect(instancePatch({...instanceDraft(custom),apps:{iosAppIds:'',iosAppStoreId:''}},custom)).toEqual({apps:{iosAppIds:[],iosAppStoreId:null}})
 for(const [sessions,hold] of [['0','300'],['201','300'],['2.5','300'],['many','300'],['20','3601'],['20','-1']])expect(liveLimitsProblem({liveMaxSessions:sessions!,liveHoldSeconds:hold!}),`${sessions} ${hold}`).not.toBeNull()
 for(const [sessions,hold] of [['1','0'],['200','3600'],['',''],[' 20 ','300']])expect(liveLimitsProblem({liveMaxSessions:sessions!,liveHoldSeconds:hold!}),`${sessions} ${hold}`).toBeNull()
 expect(appsProblem({iosAppIds:'not an id',iosAppStoreId:''})).not.toBeNull()
 expect(appsProblem({iosAppIds:'',iosAppStoreId:'id123'})).toBe('The App Store ID is the number in the app’s App Store address.')
 expect(appsProblem({iosAppIds:'LZWN2FF7YB.dev.cereal.mobile',iosAppStoreId:'123'})).toBeNull()
 expect(instanceProblem({...draft,liveMaxSessions:'0'},withAll)).toBe('Live recordings at once: enter a whole number from 1 to 200.')
 // An older server reports none of this and is left alone.
 expect(instanceDraft(saved)).toMatchObject({apps:null,liveMaxSessions:'',liveHoldSeconds:''})
 expect(instancePatch({...instanceDraft(saved),liveMaxSessions:'5'},saved)).toEqual({})
})
