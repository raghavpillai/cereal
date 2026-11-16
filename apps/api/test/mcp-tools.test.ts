import {expect,test} from "bun:test"
import {Effect,Stream} from "effect"
import type {ChatStreamEventDTO} from "@meeting-notes/contracts"
import {createMcpToolExecutor,MCP_SCOPES,MCP_TOOL_DEFINITIONS,McpToolError,type McpToolContext,type McpToolExecutorOptions} from "../src/mcp-tools"
import type {McpStore} from "../src/mcp-store"

const id="00000000-0000-4000-8000-000000000001",workspace="00000000-0000-4000-8000-000000000002"
const context=(scopes:ReadonlyArray<string>=MCP_SCOPES):McpToolContext=>({principal:{id,name:"QA",email:"qa@example.invalid",role:"user",source:"session",createdAt:"2026-11-13T12:00:00Z",expiresAt:null},workspaceId:workspace,scopes:new Set(scopes),connectionId:"consent-only",clientId:"synthetic-mcp-client",signal:new AbortController().signal})
function fixture(events:ReadonlyArray<ChatStreamEventDTO>=[]){
 const calls:string[]=[],progress:ChatStreamEventDTO[]=[]
 const store={assertScope:async()=>{calls.push("scope")},listMeetings:async(_scope:unknown,input:unknown)=>{calls.push("list");return{meetings:[],input}},accountInfo:async()=>({email:"qa@example.invalid"})} as unknown as McpStore
 const options:McpToolExecutorOptions={store,processingForUser:(userId,workspaceId)=>{expect(userId).toBe(id);expect(workspaceId).toBe(workspace);return{generate:()=>Effect.fail({error:"unavailable" as const,message:"Not used"}),chatStream:(_id,input)=>{expect(input.includeSources).toBe(true);return Effect.succeed(Stream.fromIterable(events))}}},signOut:async ctx=>{expect(ctx.connectionId).toBe("consent-only");calls.push("signout")}}
 return{executor:createMcpToolExecutor(options),calls,progress}
}
test("all advertised tools publish the same bounded Effect inputs and granular permissions, without unsupported templates",()=>{
 expect(MCP_TOOL_DEFINITIONS).toHaveLength(13)
 expect(new Set(MCP_TOOL_DEFINITIONS.map(tool=>tool.name)).size).toBe(13)
 expect(MCP_TOOL_DEFINITIONS.some(tool=>String(tool.name)==="list_templates")).toBe(false)
 for(const tool of MCP_TOOL_DEFINITIONS){expect((tool.inputSchema as Record<string,unknown>).type).toBe("object");expect(tool.requiredScopes.length).toBeGreaterThan(0);expect(tool.annotations.openWorldHint).toBe(false)}
 expect(MCP_TOOL_DEFINITIONS.find(tool=>tool.name==="query_cereal_meetings")!.requiredScopes).toContain("transcripts:read")
 expect(MCP_TOOL_DEFINITIONS.find(tool=>tool.name==="update_private_notes")!.annotations.readOnlyHint).toBe(false)
 expect(MCP_TOOL_DEFINITIONS.find(tool=>tool.name==="sign_out")!.requiredScopes).toEqual(["account:read"])
})
test("folders are gone: no folder tool, filter or permission exists, and a former folder tool is unknown",async()=>{
 const f=fixture()
 expect(MCP_TOOL_DEFINITIONS.filter(tool=>/folder/i.test(tool.name+" "+tool.description+" "+JSON.stringify(tool.inputSchema)))).toEqual([])
 expect(MCP_TOOL_DEFINITIONS.some(tool=>(tool.requiredScopes as ReadonlyArray<string>).includes("folders:write"))).toBe(false)
 expect(MCP_SCOPES as ReadonlyArray<string>).not.toContain("folders:write")
 for(const name of ["list_meeting_folders","create_folder","update_folder","add_note_to_folder","remove_note_from_folder"])
  await expect(f.executor.execute(name,{},context())).rejects.toMatchObject({code:"invalid_arguments",message:"Unknown tool."})
 await expect(f.executor.execute("list_meetings",{folder_id:"00000000-0000-4000-8000-000000000001"},context())).rejects.toBeInstanceOf(McpToolError)
 expect(f.calls).not.toContain("list")
})
test("scope rejection precedes any data access and prototype names never resolve as tools",async()=>{
 const f=fixture()
 await expect(f.executor.execute("get_meeting_transcript",{meeting_id:id},context(["notes:read"]))).rejects.toBeInstanceOf(McpToolError)
 expect(f.calls).toEqual([])
 for(const name of ["constructor","__proto__","not_a_tool"])await expect(f.executor.execute(name,{},context())).rejects.toBeInstanceOf(McpToolError)
 expect(f.calls).toEqual([])
})
test("invalid dates, unbounded reads, unsupported arguments and inverted ranges are rejected",async()=>{
 const f=fixture()
 for(const args of [{from:"2026-02-30"},{limit:101},{unexpected:true},{from:"2026-11-14",to:"2026-11-13"}])await expect(f.executor.execute("list_meetings",args,context())).rejects.toBeInstanceOf(McpToolError)
 await expect(f.executor.execute("generate_summary",{meeting_id:id,template_id:id},context())).rejects.toBeInstanceOf(McpToolError)
 expect(f.calls).not.toContain("list")
})
test("notes-only keyword search cannot query transcript contents implicitly",async()=>{
 const f=fixture(),result=await f.executor.execute("search_meetings",{query:"launch"},context(["notes:read"]))
 expect((result.structuredContent?.input as {includeTranscriptMatch:boolean}).includeTranscriptMatch).toBe(false)
 const allowed=await f.executor.execute("search_meetings",{query:"launch"},context(["notes:read","transcripts:read"]))
 expect((allowed.structuredContent?.input as {includeTranscriptMatch:boolean}).includeTranscriptMatch).toBe(true)
})
test("query progress exposes only actual tool steps; withdrawn sources discard buffered answer and quotations",async()=>{
 const events:ChatStreamEventDTO[]=[{type:"tool",text:"",tool:{callId:id,name:"search_meetings",status:"completed",label:"Found 1 meeting",resultCount:1}},{type:"delta",text:"WITHDRAWN_PRIVATE_TEXT [[S1]]"},{type:"citations",text:"",citations:[]},{type:"error",text:"A source changed. Retry."}]
 const f=fixture(events),ctx=context();ctx.onProgress=event=>f.progress.push(event)
 const result=await f.executor.execute("query_cereal_meetings",{query:"Find the launch"},ctx)
 expect(result.isError).toBe(true)
 expect(JSON.stringify(result)).not.toContain("WITHDRAWN_PRIVATE_TEXT")
 expect(result.structuredContent).toEqual({error:"A source changed. Retry.",retryable:true})
 expect(f.progress.map(event=>event.type)).toEqual(["tool"])
})
test("a successful query returns real answer metadata, and sign out targets only this OAuth connection",async()=>{
 const f=fixture([{type:"delta",text:"Actual answer"},{type:"citations",text:"",citations:[]},{type:"done",text:""}])
 expect((await f.executor.execute("query_cereal_meetings",{query:"Launch?",time_zone:"UTC"},context())).structuredContent?.answer).toBe("Actual answer")
 const signed=await f.executor.execute("sign_out",{},context(["account:read"]))
 expect(signed.structuredContent).toEqual({signedOut:true,scope:"this_ai_connection_only"})
 expect(f.calls).toContain("signout")
})
test("already-cancelled requests do not read or mutate storage",async()=>{
 const f=fixture(),controller=new AbortController();controller.abort()
 await expect(f.executor.execute("list_meetings",{}, {...context(),signal:controller.signal})).rejects.toBeDefined()
 expect(f.calls).toEqual([])
})

test("empty tools accept only an empty object and publish a closed object schema",async()=>{
 const f=fixture()
 for(const name of ["get_agent_brief","get_account_info","sign_out"] as const){
  const schema=MCP_TOOL_DEFINITIONS.find(tool=>tool.name===name)!.inputSchema as Record<string,unknown>
  expect(schema.type).toBe("object");expect(schema.additionalProperties).toBe(false)
  for(const args of [{extra:true},null,[],"text",1])await expect(f.executor.execute(name,args,context())).rejects.toBeInstanceOf(McpToolError)
 }
 expect(f.calls).not.toContain("signout")
 expect((await f.executor.execute("get_account_info",{},context())).structuredContent?.email).toBe("qa@example.invalid")
})
