import {afterAll,beforeAll,describe,expect,test} from "bun:test"
import {Effect,Schema} from "effect"
import {AgentBriefDTO,NoteAttachmentDTO} from "@meeting-notes/contracts"
import type {PrincipalData} from "@meeting-notes/contracts/api"
import {makePrismaRepository} from "../src/prisma-repository"
import {makeApiHandler} from "../src/api"
import {createMcpStore} from "../src/mcp-store"
import {DEFAULT_ORGANIZATION_ID} from "../src/organizations"

const enabled=process.env.RUN_PG_TESTS==="1"&&!!process.env.DATABASE_URL
describe.skipIf(!enabled)("typed MCP attachment/profile input hooks",()=>{
 const resource=enabled?makePrismaRepository(process.env.DATABASE_URL!):null,db=resource?.database
 const alice=crypto.randomUUID(),bob=crypto.randomUUID();let id="",foreignId="",api:ReturnType<typeof makeApiHandler>
 const principal=(userId:string):PrincipalData=>({id:userId,name:"Fictional attachment QA",email:"fixture@example.invalid",role:"user",source:"session",expiresAt:null,createdAt:"2026-01-01T00:00:00Z"})
 const request=(path:string,method="GET",body?:unknown,token="alice")=>api.handler(new Request("http://127.0.0.1:3000"+path,{method,headers:{...(token?{Authorization:"Bearer "+token}:{}),...(body===undefined?{}:{"Content-Type":"application/json"})},body:body===undefined?undefined:JSON.stringify(body)}))
 const png="iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Y9Zl1sAAAAASUVORK5CYII="
 beforeAll(async()=>{
  for(const userId of [alice,bob]){
   await db!.user.create({data:{id:userId,name:"Fictional MCP support fixture",email:userId+"@example.invalid"}})
   await db!.member.create({data:{id:crypto.randomUUID(),organizationId:DEFAULT_ORGANIZATION_ID,userId,role:"member"}})
  }
  id=(await Effect.runPromise(resource!.repository.forUser(alice).createNote({title:"Fictional attachment"}))).id
  foreignId=(await Effect.runPromise(resource!.repository.forUser(bob).createNote({title:"Other owner"}))).id
  api=makeApiHandler(resource!.repository,"operator-fixture-do-not-use-for-mcp",{mcpStore:createMcpStore(db!),principalResolver:async headers=>headers.get("authorization")==="Bearer alice"?principal(alice):headers.get("authorization")==="Bearer bob"?principal(bob):null})
 })
 afterAll(async()=>{await api?.dispose();await db!.user.deleteMany({where:{id:{in:[alice,bob]}}});await resource?.close()})
 test("real private image upload is typed, bounded and owner scoped",async()=>{
  const body={name:"fictional-chart.png",mimeType:"image/png",dataBase64:png}
  expect((await request(`/api/v1/notes/${id}/attachments`,"POST",body,"")).status).toBe(401)
  expect((await request(`/api/v1/notes/${foreignId}/attachments`,"POST",body)).status).toBe(404)
  expect((await request(`/api/v1/notes/${id}/attachments`,"POST",{...body,dataBase64:"ZmFrZQ=="})).status).toBe(400)
  const response=await request(`/api/v1/notes/${id}/attachments`,"POST",body);expect(response.status).toBe(200)
  const saved=Schema.decodeUnknownSync(NoteAttachmentDTO)(await response.json());expect(saved.noteId).toBe(id)
  const file=await createMcpStore(db!).getAttachment({userId:alice,workspaceId:DEFAULT_ORGANIZATION_ID},saved.id)
  expect(Buffer.from(file.data).toString("base64")).toBe(png)
  await expect(createMcpStore(db!).getAttachment({userId:bob,workspaceId:DEFAULT_ORGANIZATION_ID},saved.id)).rejects.toBeDefined()
  expect(await db!.noteAttachment.count({where:{noteId:foreignId}})).toBe(0)
 })
 test("explicit profile remains private and does not claim automatic inference",async()=>{
  const response=await request("/api/v1/profile/agent-brief","PATCH",{body:"I own the fictional Cedar launch."});expect(response.status).toBe(200)
  const brief=Schema.decodeUnknownSync(AgentBriefDTO)(await response.json());expect(brief.source).toBe("explicit_user_profile");expect(brief.automaticallyInferred).toBe(false);expect(brief.workspaceId).toBe(DEFAULT_ORGANIZATION_ID)
  const other=Schema.decodeUnknownSync(AgentBriefDTO)(await(await request("/api/v1/profile/agent-brief","GET",undefined,"bob")).json());expect(other.body).toBe("")
  expect((await request("/api/v1/profile/agent-brief","PATCH",{body:"x".repeat(20_001)})).status).toBe(400)
  expect((await request("/api/v1/profile/agent-brief","GET",undefined,"")).status).toBe(401)
 })
})
