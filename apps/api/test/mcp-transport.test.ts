import {describe,test,expect} from "bun:test"
import {Client,StreamableHTTPClientTransport,type FetchLike} from "@modelcontextprotocol/client"
import {makeMcpEndpoint,type McpEndpointOptions} from "../src/mcp"
import {MCP_TOOL_DEFINITIONS,McpToolError,type McpToolContext,type McpToolResult} from "../src/mcp-tools"
import type {McpGrant} from "../src/mcp-auth"

const base="http://127.0.0.1:3000",workspace="00000000-0000-4000-8000-000000000002"
const grant=(id:string,write=false):McpGrant=>({principal:{id,name:"Fictional MCP user",email:"fixture@example.invalid",role:"user",source:"session",createdAt:"2026-01-01T00:00:00Z",expiresAt:"2027-01-01T00:00:00Z"},workspaceId:workspace,clientId:"fictional-client",connectionId:"fictional-consent",scopes:new Set(["account:read","notes:read",...(write?["notes:write"]:[])])})
const alice=grant("00000000-0000-4000-8000-000000000003"),bob=grant("00000000-0000-4000-8000-000000000004",true)
function setup(execute:(name:string,args:unknown,context:McpToolContext)=>Promise<McpToolResult>=async(_name,_args,context)=>({content:[{type:"text",text:context.principal.id}],structuredContent:{ownerId:context.principal.id,workspaceId:context.workspaceId}}),options:Pick<McpEndpointOptions,"bodyReadTimeoutMs"|"toolTimeoutMs">={}){
 const endpoint=makeMcpEndpoint({...options,baseURL:base,executor:{execute},protect:async(req,next)=>{
  const value=req.headers.get("authorization")
  if(value!=="Bearer alice"&&value!=="Bearer bob")return new Response(null,{status:401,headers:{"WWW-Authenticate":`Bearer resource_metadata="${base}/.well-known/oauth-protected-resource/mcp"`}})
  return next(req,value==="Bearer alice"?alice:bob)
 }})
 const fetcher:FetchLike=async(input,init)=>endpoint.fetch(new Request(input,init))
 const client=async(identity="alice",modern=false)=>{
  const c=new Client({name:"official-sdk-fixture",version:"1.0.0"},modern?{versionNegotiation:{mode:{pin:"2026-07-28"}}}:{})
  const transport=new StreamableHTTPClientTransport(new URL(base+"/mcp"),{fetch:fetcher,requestInit:{headers:{Authorization:"Bearer "+identity}}})
  await c.connect(transport)
  return c
 }
 return {endpoint,client}
}
describe("official MCP SDK Streamable HTTP interoperability",()=>{
 for(const modern of [false,true])test(modern?"modern protocol discovery and tools":"legacy initialize and tools",async()=>{
  const {endpoint,client}=setup(),c=await client("alice",modern)
  try{
   const tools=await c.listTools();expect(tools.tools.some(t=>t.name==="get_account_info")).toBe(true)
   expect(tools.tools.some(t=>t.name==="update_private_notes")).toBe(false)
   const answer=await c.callTool({name:"get_account_info",arguments:{}})
   expect(answer.structuredContent).toEqual({ownerId:alice.principal.id,workspaceId:workspace})
   expect(JSON.stringify(answer)).not.toContain("Bearer alice")
  }finally{await c.close();await endpoint.close()}
 })
 test("two clients do not share their authenticated factory or writable tool list",async()=>{
  const {endpoint,client}=setup(),a=await client(),b=await client("bob")
  try{
   expect((await b.listTools()).tools.some(t=>t.name==="update_private_notes")).toBe(true)
   const [first,second]=await Promise.all([a.callTool({name:"get_account_info",arguments:{}}),b.callTool({name:"get_account_info",arguments:{}})])
   expect(first.structuredContent).toEqual({ownerId:alice.principal.id,workspaceId:workspace});expect(second.structuredContent).toEqual({ownerId:bob.principal.id,workspaceId:workspace})
  }finally{await a.close();await b.close();await endpoint.close()}
 })
 test("ungranted tools and excess JSON arguments never execute",async()=>{
  let calls=0;const {endpoint,client}=setup(async()=>{calls++;return{content:[{type:"text",text:"unexpected"}]}}),c=await client()
  try{
   await expect(c.callTool({name:"update_private_notes",arguments:{meeting_id:crypto.randomUUID(),text:"Do not write",position:"end"}})).rejects.toBeDefined()
   const invalid=await c.callTool({name:"get_account_info",arguments:{extra:"not in schema"}});expect(invalid.isError).toBe(true)
   expect(calls).toBe(0)
  }finally{await c.close();await endpoint.close()}
 })
 test("domain errors are bounded and safely returned as tool errors",async()=>{
  const {endpoint,client}=setup(async()=>{throw new McpToolError("not_found","Meeting not found.")}),c=await client()
  try{const result=await c.callTool({name:"get_account_info",arguments:{}});expect(result.isError).toBe(true);expect(result.structuredContent).toMatchObject({error:"not_found"})}
  finally{await c.close();await endpoint.close()}
 })
 for(const modern of [false,true])test(`official ${modern?"modern":"legacy"} client cancellation aborts only its running streamed tool`,async()=>{
  let started!:()=>void,aborted!:()=>void
  const ready=new Promise<void>(resolve=>{started=resolve}),cancelled=new Promise<void>(resolve=>{aborted=resolve})
  const {endpoint,client}=setup(async(_name,_args,context)=>{
   context.onProgress?.({type:"tool",text:"",tool:{callId:crypto.randomUUID(),name:"get_meetings",status:"running",label:"Reading fictional fixture"}})
   started()
   await new Promise<void>((_resolve,reject)=>{context.signal.addEventListener("abort",()=>{aborted();reject(new Error("cancelled"))},{once:true})})
   return {content:[]}
  }),c=await client("alice",modern),abort=new AbortController()
  try{
   const pending=c.callTool({name:"get_account_info",arguments:{},_meta:{progressToken:"fixture-progress"}},{signal:abort.signal})
   await ready
   // Another authenticated client cannot cancel this account's request even
   // when it guesses the numeric request id used by the legacy SDK.
   for(let id=0;id<5;id++)await endpoint.fetch(new Request(base+"/mcp",{method:"POST",headers:{Authorization:"Bearer bob","Content-Type":"application/json",Accept:"application/json, text/event-stream"},body:JSON.stringify({jsonrpc:"2.0",method:"notifications/cancelled",params:{requestId:id}})}))
   expect(await Promise.race([cancelled.then(()=>true),Bun.sleep(20).then(()=>false)])).toBe(false)
   abort.abort()
   await expect(pending).rejects.toBeDefined()
   expect(await Promise.race([cancelled.then(()=>true),Bun.sleep(1000).then(()=>false)])).toBe(true)
  }finally{await c.close();await endpoint.close()}
 })
 test("the endpoint answers at /api/mcp and at its previous address /mcp, and nowhere else",async()=>{
  const {endpoint}=setup()
  try{
   for(const path of["/api/mcp","/mcp"]){
    const c=new Client({name:"official-sdk-fixture",version:"1.0.0"})
    await c.connect(new StreamableHTTPClientTransport(new URL(base+path),{fetch:async(input,init)=>endpoint.fetch(new Request(input,init)),requestInit:{headers:{Authorization:"Bearer alice"}}}))
    try{
     expect((await c.listTools()).tools.some(tool=>tool.name==="get_account_info")).toBe(true)
     const result=await c.callTool({name:"get_account_info",arguments:{}});expect((result.structuredContent as {ownerId:string}).ownerId).toBe(alice.principal.id)
    }finally{await c.close()}
    const unauthorized=await endpoint.fetch(new Request(base+path));expect(unauthorized.status).toBe(401)
    const preflight=await endpoint.fetch(new Request(base+path,{method:"OPTIONS",headers:{Origin:base}}));expect(preflight.status).toBe(204)
   }
   // The pages under /mcp/ and near-misses are not the endpoint.
   for(const path of["/mcp/","/mcp/connect","/mcp/consent","/api/mcp/","/api/mcp/extra","/api/mcpx","/api"]){
    const response=await endpoint.fetch(new Request(base+path,{headers:{Authorization:"Bearer alice"}}));expect(response.status).toBe(404);expect(await response.json()).toEqual({error:"not_found"})
   }
  }finally{await endpoint.close()}
 })
 test("Host/Origin validation, OAuth challenges and body bounds precede tool execution",async()=>{
  let calls=0;const {endpoint}=setup(async()=>{calls++;return{content:[]}})
  try{
   expect((await endpoint.fetch(new Request(base+"/mcp",{headers:{Host:"attacker.example",Authorization:"Bearer alice"}}))).status).toBe(403)
   expect((await endpoint.fetch(new Request(base+"/mcp",{headers:{Origin:"https://attacker.example",Authorization:"Bearer alice"}}))).status).toBe(403)
   const unauthorized=await endpoint.fetch(new Request(base+"/mcp"));expect(unauthorized.status).toBe(401);expect(unauthorized.headers.get("www-authenticate")).toContain("resource_metadata")
   const huge=await endpoint.fetch(new Request(base+"/mcp",{method:"POST",headers:{Authorization:"Bearer alice","Content-Type":"application/json",Accept:"application/json, text/event-stream"},body:" ".repeat(1024*1024+1)}));expect(huge.status).toBe(413)
   expect(huge.headers.get("cache-control")).toBe("no-store");expect(calls).toBe(0)
  }finally{await endpoint.close()}
 })
 test("two legacy clients sharing one OAuth grant have independent cancellation sessions",async()=>{
  let starts=0,aborts=0,ready!:()=>void
  const both=new Promise<void>(resolve=>{ready=resolve})
  const {endpoint,client}=setup(async(_name,_args,context)=>{
   if(++starts===2)ready()
   await new Promise<void>((_resolve,reject)=>context.signal.addEventListener("abort",()=>{aborts++;reject(new Error("cancelled"))},{once:true}))
   return {content:[]}
  }),a=await client(),b=await client(),first=new AbortController(),second=new AbortController()
  const pendingA=a.callTool({name:"get_account_info",arguments:{}},{signal:first.signal}).catch(()=>null),pendingB=b.callTool({name:"get_account_info",arguments:{}},{signal:second.signal}).catch(()=>null)
  try{
   expect(await Promise.race([both.then(()=>true),Bun.sleep(1000).then(()=>false)])).toBe(true)
   first.abort();await pendingA;await Bun.sleep(20);expect(aborts).toBe(1)
   second.abort();await pendingB;await Bun.sleep(20);expect(aborts).toBe(2)
  }finally{first.abort();second.abort();await a.close();await b.close();await endpoint.close()}
 })
 test("body upload deadline cancels a stalled request independently of tool execution",async()=>{
  let cancelled=false,calls=0
  const {endpoint}=setup(async()=>{calls++;return {content:[]}},{bodyReadTimeoutMs:10})
  try{
   const body=new ReadableStream<Uint8Array>({start(stream){stream.enqueue(new TextEncoder().encode('{"jsonrpc":'))},cancel(){cancelled=true}})
   const response=await endpoint.fetch(new Request(base+"/mcp",{method:"POST",headers:{Authorization:"Bearer alice","Content-Type":"application/json"},body}))
   expect(response.status).toBe(408);expect(cancelled).toBe(true);expect(calls).toBe(0)
  }finally{await endpoint.close()}
 })
 test("deadline releases an unread legacy call even when its executor ignores cancellation",async()=>{
  let calls=0
  const {endpoint}=setup(async(_name,_args,context)=>{if(++calls===1){context.onProgress?.({type:"tool",text:"",tool:{callId:"fixture",name:"get_meetings",status:"running",label:"Fixture"}});await new Promise(()=>{})}return {content:[{type:"text",text:"completed"}]}},{toolTimeoutMs:20})
  const post=(message:unknown,session?:string)=>endpoint.fetch(new Request(base+"/mcp",{method:"POST",headers:{Authorization:"Bearer alice","Content-Type":"application/json",Accept:"application/json, text/event-stream",...(session?{"Mcp-Session-Id":session}:{})},body:JSON.stringify(message)}))
  try{
   const initialized=await post({jsonrpc:"2.0",id:0,method:"initialize",params:{protocolVersion:"2025-11-25",capabilities:{},clientInfo:{name:"fictional",version:"1"}}}),session=initialized.headers.get("mcp-session-id")!
   await initialized.text();expect(!!session).toBe(true)
   const request={jsonrpc:"2.0",id:7,method:"tools/call",params:{name:"get_account_info",arguments:{},_meta:{progressToken:"fixture"}}}
   const unread=await post(request,session);expect(unread.status).toBe(200)
   await Bun.sleep(40)
   const next=await post(request,session);expect(next.status).toBe(200);expect(await next.text()).toContain("completed");expect(calls).toBe(2)
  }finally{await endpoint.close()}
 })
 test("published tool definitions remain explicit, validated JSON schemas",()=>{
  expect(new Set(MCP_TOOL_DEFINITIONS.map(t=>t.name)).size).toBe(MCP_TOOL_DEFINITIONS.length)
  for(const definition of MCP_TOOL_DEFINITIONS){expect(definition.inputSchema.$schema).toBe("https://json-schema.org/draft/2020-12/schema");expect(definition.requiredScopes.length).toBeGreaterThan(0)}
 })
 test("a tool name that discovery does not list is refused and never executes",async()=>{
  const calls:string[]=[],scope={...alice,scopes:new Set([...alice.scopes,"transcripts:read"])}
  const endpoint=makeMcpEndpoint({baseURL:base,protect:async(request,next)=>next(request,scope),executor:{execute:async(name)=>{calls.push(name);return {content:[{type:"text",text:"Fictional query result"}]}}}})
  const c=new Client({name:"Cereal unknown tool fixture",version:"1"})
  await c.connect(new StreamableHTTPClientTransport(new URL(base+"/mcp"),{fetch:async(input,init)=>endpoint.fetch(new Request(input,init))}))
  try{
   const list=await c.listTools()
   expect(list.tools.some(tool=>tool.name==="query_cereal_meetings")).toBe(true)
   expect(list.tools.some(tool=>tool.name==="query_meetings")).toBe(false)
   let refused=false
   try{refused=(await c.callTool({name:"query_meetings",arguments:{query:"Fictional question"}})).isError===true}catch{refused=true}
   expect(refused).toBe(true);expect(calls).toEqual([])
   const reply=await c.callTool({name:"query_cereal_meetings",arguments:{query:"Fictional question"}})
   expect(reply.isError).not.toBe(true);expect(calls).toEqual(["query_cereal_meetings"])
  }finally{await c.close();await endpoint.close()}
 })
})
