import {Effect,Schema,Stream} from "effect"
import type {PrincipalData} from "@meeting-notes/contracts/api"
import type {ChatStreamEventDTO} from "@meeting-notes/contracts"
import type {ProcessingService} from "./processing"
import type {McpStore} from "./mcp-store"
import {McpStoreError} from "./mcp-store"
import {UUID,NoteDate} from "@meeting-notes/contracts"

export const MCP_SCOPES = ["notes:read","transcripts:read","notes:write","attachments:read","profile:read","account:read","account:write"] as const
export type McpScope = typeof MCP_SCOPES[number]
export interface McpToolContext {
  principal:PrincipalData
  workspaceId:string
  scopes:ReadonlySet<string>
  connectionId:string
  clientId:string
  signal:AbortSignal
  onProgress?:(event:ChatStreamEventDTO)=>void
}
export interface McpToolResult {
  content:Array<{type:"text";text:string}|{type:"image";data:string;mimeType:string}>
  structuredContent?:Record<string,unknown>
  isError?:boolean
}
type Processing = ReturnType<ProcessingService["forRepository"]>
export interface McpToolExecutorOptions {
  store:McpStore
  processingForUser:(userId:string,workspaceId:string,signal:AbortSignal)=>Pick<Processing,"generate"|"chatStream">
  signOut:(context:McpToolContext)=>Promise<void>
}
export class McpToolError extends Error {
  constructor(readonly code:"invalid_arguments"|"forbidden"|"not_found"|"conflict"|"unavailable",message:string,readonly requiredScopes:ReadonlyArray<string>=[]) {super(message);this.name="McpToolError"}
}

const text=(maximum:number)=>Schema.String.check(Schema.isMinLength(1),Schema.isMaxLength(maximum))
const opt=<S extends Schema.Top>(schema:S)=>Schema.optionalKey(schema)
const limit=Schema.Int.check(Schema.isBetween({minimum:1,maximum:100}))
const emptyInput=Schema.Record(Schema.String,Schema.Never)
const search={query:opt(text(300)),from:opt(NoteDate),to:opt(NoteDate),attendee:opt(text(254)),company:opt(text(200)),limit:opt(limit),cursor:opt(text(1000))}
export const MCP_TOOL_INPUTS={
  search_meetings:Schema.Struct({...search,query:text(300)}),
  search_transcripts:Schema.Struct({...search,query:text(300)}),
  list_meetings:Schema.Struct(search),
  get_meetings:Schema.Struct({meeting_ids:Schema.Array(UUID).check(Schema.isMinLength(1),Schema.isMaxLength(4))}),
  get_meeting_transcript:Schema.Struct({meeting_id:UUID,cursor:opt(text(1000)),start_ms:opt(Schema.Int.check(Schema.isBetween({minimum:0,maximum:2147483647}))),limit:opt(limit),max_characters:opt(Schema.Int.check(Schema.isBetween({minimum:256,maximum:50000})))}),
  get_meeting_attachment:Schema.Struct({attachment_id:UUID}),
  query_cereal_meetings:Schema.Struct({query:text(20000),time_zone:opt(text(100))}),
  update_private_notes:Schema.Struct({meeting_id:UUID,text:text(100000),position:Schema.Literals(["append","prepend"])}),
  generate_summary:Schema.Struct({meeting_id:UUID}),
  get_agent_brief:emptyInput,
  get_account_info:emptyInput,
  switch_workspace:Schema.Struct({workspace_id:UUID}),
  sign_out:emptyInput,
} as const
export type McpToolName=keyof typeof MCP_TOOL_INPUTS
const descriptions:Record<McpToolName,string>={
  search_meetings:"Find private meetings you own in the active workspace by keyword, inclusive date range, or explicitly attached attendee/company email or name. Results are paginated. Membership alone never grants another person's notes.",
  search_transcripts:"Find exact source passages and timestamps from your own active-workspace transcripts. Speaker IDs remain scoped to recording part/channel; no attendee identity is inferred.",
  list_meetings:"List your private active-workspace meetings, with dates and explicit calendar attendees. Filter by date/attendee and follow nextCursor for more.",
  get_meetings:"Read up to four owned meeting notes, summaries, explicit attendee metadata and available attachment IDs. Transcript contents are read separately.",
  get_meeting_transcript:"Read a full owned transcript using revision-bound pagination; long utterances are split into exact text parts with textOffset. Follow nextCursor until null. start_ms begins up to30seconds before a known passage.",
  get_meeting_attachment:"Read an actual owner-uploaded PNG/JPEG/WebP meeting image. No screenshot is invented or fetched from external URLs. Obtain attachment IDs from get_meetings.",
  query_cereal_meetings:"Ask the configured meeting assistant an evidence-grounded question across only your own active-workspace meetings. This slower operation uses actual search/read tools and returns issued citations. Requires transcript consent because the assistant may read transcripts.",
  update_private_notes:"When explicitly asked, atomically add text to the beginning or end of existing private notes. Existing text is preserved; concurrent edits are serialized. Does not generate a summary automatically.",
  generate_summary:"When explicitly asked, generate or regenerate a stopped meeting's notes from its current transcript using the configured server processing service. No template support is advertised. May take30–90seconds.",
  get_agent_brief:"Read the explicit profile saved by this account for the active workspace. Empty means no profile has been configured; this service does not silently infer a profile from meeting history.",
  get_account_info:"Read the connected account email, joined workspaces and globally active workspace, plus the private-owned-only access policy.",
  switch_workspace:"When explicitly asked, switch the globally active workspace after checking current membership. This also updates other connected tools and browser sessions; native browsing currently remains across your own notes.",
  sign_out:"When explicitly asked, disconnect only this AI client's OAuth consent and its tokens. Does not sign the user out of native/browser apps or delete notes.",
}
const scopes:Record<McpToolName,ReadonlyArray<McpScope>>={
  search_meetings:["notes:read"],search_transcripts:["notes:read","transcripts:read"],list_meetings:["notes:read"],get_meetings:["notes:read"],get_meeting_transcript:["notes:read","transcripts:read"],get_meeting_attachment:["notes:read","attachments:read"],query_cereal_meetings:["notes:read","transcripts:read"],update_private_notes:["notes:write","notes:read"],generate_summary:["notes:write","notes:read","transcripts:read"],get_agent_brief:["profile:read"],get_account_info:["account:read"],switch_workspace:["account:read","account:write"],sign_out:["account:read"],
}
const writes=new Set<McpToolName>(["update_private_notes","generate_summary","switch_workspace","sign_out"])
export const MCP_TOOL_DEFINITIONS=Object.entries(MCP_TOOL_INPUTS).map(([name,schema])=>{
  const key=name as McpToolName,document=Schema.toJsonSchemaDocument(schema)
  return {name:key,title:name.split("_").map(word=>word[0]!.toUpperCase()+word.slice(1)).join(" "),description:descriptions[key],inputSchema:{$schema:"https://json-schema.org/draft/2020-12/schema",...document.schema,$defs:document.definitions},requiredScopes:scopes[key],annotations:{readOnlyHint:!writes.has(key),destructiveHint:key==="generate_summary"||key==="sign_out",idempotentHint:["switch_workspace"].includes(key),openWorldHint:false}}
})
function decode<S extends Schema.ConstraintDecoder<unknown>>(schema:S,input:unknown):S["Type"] {
  try{return Schema.decodeUnknownSync(schema,{onExcessProperty:"error"})(input)}catch{throw new McpToolError("invalid_arguments","Tool arguments did not match the published schema.")}
}
const result=(value:Record<string,unknown>):McpToolResult=>({content:[{type:"text",text:JSON.stringify(value)}],structuredContent:value})

export function createMcpToolExecutor(options:McpToolExecutorOptions) {
  return {
    async execute(name:string,args:unknown,context:McpToolContext):Promise<McpToolResult> {
      if(!Object.hasOwn(MCP_TOOL_INPUTS,name))throw new McpToolError("invalid_arguments","Unknown tool.")
      const tool=name as McpToolName,required=scopes[tool]
      if(required.some(scope=>!context.scopes.has(scope)))throw new McpToolError("forbidden","This connection has not been granted the tool's required permissions.",required)
      context.signal.throwIfAborted()
      const scope={userId:context.principal.id,workspaceId:context.workspaceId},store=options.store
      try {
        await store.assertScope(scope)
        switch(tool){
          case "search_meetings":case "list_meetings":case "search_transcripts": {
            const input=decode(MCP_TOOL_INPUTS[tool],args)
            if(input.from&&input.to&&input.from>input.to)throw new McpToolError("invalid_arguments","The date range ends before it starts.")
            const search={query:input.query,from:input.from,to:input.to,attendee:input.attendee,company:input.company,limit:Math.min(tool==="search_transcripts"?20:100,input.limit??20),cursor:input.cursor,includeTranscriptMatch:context.scopes.has("transcripts:read")}
            return result(tool==="search_transcripts"?await store.searchTranscripts(scope,search):await store.listMeetings(scope,search))
          }
          case "get_meetings": {const input=decode(MCP_TOOL_INPUTS[tool],args);return result(await store.getMeetings(scope,[...new Set(input.meeting_ids)]))}
          case "get_meeting_transcript": {const input=decode(MCP_TOOL_INPUTS[tool],args);return result(await store.transcript(scope,input.meeting_id,{limit:input.limit??30,cursor:input.cursor,startMs:input.start_ms,maxCharacters:input.max_characters??20000}))}
          case "update_private_notes": {const input=decode(MCP_TOOL_INPUTS[tool],args);if(!input.text.trim())throw new McpToolError("invalid_arguments","Notes text must not be empty.");return result(await store.updatePrivateNotes(scope,input.meeting_id,input.text,input.position))}
          case "generate_summary": {const input=decode(MCP_TOOL_INPUTS[tool],args),note=await store.noteForProcessing(scope,input.meeting_id);const generated=await Effect.runPromise(options.processingForUser(scope.userId,scope.workspaceId,context.signal).generate(note.id,note.transcriptRevision),{signal:context.signal});return result({noteId:generated.id,summary:generated.enhanced,transcriptRevision:generated.transcriptRevision,generatedRevision:generated.generatedRevision})}
          case "get_meeting_attachment": {const input=decode(MCP_TOOL_INPUTS[tool],args),file=await store.getAttachment(scope,input.attachment_id);return {content:[{type:"text",text:JSON.stringify({id:file.id,noteId:file.noteId,name:file.name,mimeType:file.mimeType})},{type:"image",data:Buffer.from(file.data).toString("base64"),mimeType:file.mimeType}],structuredContent:{id:file.id,noteId:file.noteId,name:file.name,mimeType:file.mimeType}}}
          case "get_agent_brief":decode(MCP_TOOL_INPUTS[tool],args);return result(await store.agentBrief(scope))
          case "get_account_info":decode(MCP_TOOL_INPUTS[tool],args);return result(await store.accountInfo(scope.userId))
          case "switch_workspace": {const input=decode(MCP_TOOL_INPUTS[tool],args);return result({...await store.switchWorkspace(scope.userId,input.workspace_id),globalChange:true})}
          case "sign_out":decode(MCP_TOOL_INPUTS[tool],args);await options.signOut(context);return result({signedOut:true,scope:"this_ai_connection_only"})
          case "query_cereal_meetings": {
            const input=decode(MCP_TOOL_INPUTS[tool],args)
            const stream=await Effect.runPromise(options.processingForUser(scope.userId,scope.workspaceId,context.signal).chatStream(null,{message:input.query,history:[],includeSources:true,...(input.time_zone?{timeZone:input.time_zone}:{})}),{signal:context.signal})
            const events:ChatStreamEventDTO[]=[]
            await Effect.runPromise(Stream.runForEach(stream,event=>Effect.sync(()=>{if(event.type==="tool")context.onProgress?.(event);events.push(event)})),{signal:context.signal})
            let answer="",citations:ReadonlyArray<unknown>=[];const tools:unknown[]=[];let done=false
            for(const event of events){if(event.type==="delta")answer+=event.text;else if(event.type==="citations")citations=event.citations??[];else if(event.type==="tool"&&event.tool)tools.push(event.tool);else if(event.type==="done")done=true;else if(event.type==="error")return {...result({error:event.text,retryable:true}),isError:true}}
            if(!done)throw new McpToolError("unavailable","Meeting query did not complete. Retry the request.")
            return result({answer,citations,tools,workspaceId:scope.workspaceId,access:"private-owned-only"})
          }
        }
      }catch(error){
        if(context.signal.aborted)throw error
        if(error instanceof McpToolError)throw error
        if(error instanceof McpStoreError)throw new McpToolError(error.code,error.message)
        throw new McpToolError("unavailable","The tool could not finish. Please retry.")
      }
    },
  }
}
