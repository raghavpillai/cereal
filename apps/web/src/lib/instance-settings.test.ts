import {expect,test} from 'bun:test'
import {defaultLiveModel,instanceDraft,instancePatch,instanceProblem} from './instance-settings'
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
