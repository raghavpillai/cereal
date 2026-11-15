import {afterAll,beforeAll,describe,expect,test} from "bun:test"
import {randomBytes,createHash} from "node:crypto"
import {Effect} from "effect"
import {makeAuthenticationService} from "../src/auth"
import {makePrismaRepository} from "../src/prisma-repository"
import {createMcpStore} from "../src/mcp-store"
const enabled=process.env.RUN_AUTH_PG_TESTS==="1"&&!!process.env.DATABASE_URL
/** Fictional accounts and clients only, in the isolated PostgreSQL runner. */
describe.skipIf(!enabled)("MCP official OAuth PKCE, durable consent and account isolation",()=>{
 const resources=enabled?makePrismaRepository(process.env.DATABASE_URL!):null
 const db=resources?.database
 let server:ReturnType<typeof Bun.serve>,authentication:ReturnType<typeof makeAuthenticationService>,baseURL:string
 const password="fixture-only-"+randomBytes(16).toString("hex"),email="mcp-"+randomBytes(8).toString("hex")+"@example.test"
 let cookie="",native="",userId="",otherUserId="",clientId="",access="",refresh="",connectionId="",fixtureOrganizationId=""
 const callback="http://127.0.0.1:47777/oauth/callback"
 const json=async(path:string,body?:unknown,headers:Record<string,string>={})=>fetch(baseURL+path,{method:body===undefined?"GET":"POST",headers:{"x-forwarded-for":"192.0.2.203",...(body===undefined?{}:{"Content-Type":"application/json"}),...headers},...(body===undefined?{}:{body:JSON.stringify(body)}),redirect:"manual"})
 const form=async(path:string,body:Record<string,string>)=>fetch(baseURL+path,{method:"POST",headers:{"x-forwarded-for":"192.0.2.203","Content-Type":"application/x-www-form-urlencoded"},body:new URLSearchParams(body),redirect:"manual"})
 async function login(oauthQuery?:string){const response=await json("/api/auth/sign-in/email",{email,password,...(oauthQuery?{oauth_query:oauthQuery}:{})});expect(response.status).toBe(200);cookie=response.headers.getSetCookie().map(c=>c.split(";")[0]).join("; ");native=response.headers.get("set-auth-token")!;expect(Boolean(native)).toBe(true);return response.json()}
 async function authorize(scope="notes:read offline_access"){
  const verifier=randomBytes(40).toString("base64url"),challenge=createHash("sha256").update(verifier).digest("base64url")
  const query=new URLSearchParams({client_id:clientId,redirect_uri:callback,response_type:"code",scope,resource:baseURL+"/mcp",state:"fictional-state",code_challenge:challenge,code_challenge_method:"S256"})
  const response=await json("/api/auth/oauth2/authorize?"+query,undefined,{Cookie:cookie,Accept:"text/html"})
  expect([302,303]).toContain(response.status)
  const location=response.headers.get("location");expect(Boolean(location)).toBe(true)
  const next=new URL(location!,baseURL);expect(next.pathname).toBe("/mcp/consent")
  return{verifier,oauthQuery:next.searchParams.toString()}
 }
 async function grant(scope="notes:read offline_access"){
  const authorization=await authorize(scope)
  const response=await json("/api/auth/oauth2/consent",{accept:true,oauth_query:authorization.oauthQuery},{Cookie:cookie,Origin:baseURL})
  expect(response.status).toBe(200);const body=await response.json();const redirect=new URL(body.url);expect(redirect.origin+redirect.pathname).toBe(new URL(callback).origin+new URL(callback).pathname)
  expect(redirect.searchParams.get("state")).toBe("fictional-state")
  const token=await form("/api/auth/oauth2/token",{grant_type:"authorization_code",client_id:clientId,code:redirect.searchParams.get("code")!,redirect_uri:callback,code_verifier:authorization.verifier,resource:baseURL+"/mcp"})
  const tokens=await token.json();expect(token.status).toBe(200);expect(Boolean(tokens.access_token)).toBe(true);return tokens
 }
 const protect=(token:string)=>fetch(baseURL+"/mcp",{headers:{Authorization:"Bearer "+token}})
 beforeAll(async()=>{
  server=Bun.serve({hostname:"127.0.0.1",port:0,fetch:async request=>{
   const path=new URL(request.url).pathname
   if(path==="/mcp")return authentication.mcp.protect(request,async(_req,grant)=>Response.json({id:grant.principal.id,workspaceId:grant.workspaceId,scopes:[...grant.scopes],connectionId:grant.connectionId,clientId:grant.clientId}))
   if(path.startsWith("/.well-known/"))return authentication.mcp.metadata(request)
   return authentication.handler(request)
  }})
  baseURL="http://127.0.0.1:"+server.port
  authentication=makeAuthenticationService(db!,"fictional-bootstrap-not-a-session-32chars",{secret:randomBytes(32).toString("base64url"),baseURL})
  const user=await Effect.runPromise(authentication.createUser({name:"MCP Fixture",email,password}));userId=user.id;await login()
 })
 afterAll(async()=>{server?.stop(true);if(fixtureOrganizationId)await db?.organization.delete({where:{id:fixtureOrganizationId}});if(otherUserId)await db?.user.delete({where:{id:otherUserId}});if(userId)await db?.user.delete({where:{id:userId}});await resources?.close()})
 test("discovery and challenges are resource-bound; raw session JWT, signup and plugin admin routes remain closed",async()=>{
  const denied=await json("/mcp");expect(denied.status).toBe(401);expect(denied.headers.get("www-authenticate")).toContain("resource_metadata")
  const metadata=await(await json("/.well-known/oauth-protected-resource/mcp")).json();expect(metadata.resource).toBe(baseURL+"/mcp");expect(metadata.scopes_supported.includes("notes:read")).toBe(true)
  const discovery=await(await json("/.well-known/oauth-authorization-server/api/auth")).json();expect(discovery.issuer).toBe(baseURL+"/api/auth");expect(discovery.code_challenge_methods_supported).toEqual(["S256"]);expect(discovery.client_id_metadata_document_supported).toBe(true)
  expect((await protect(native)).status).toBe(401)
  for(const path of["/api/auth/token","/api/auth/sign-up/email","/api/auth/oauth2/create-client","/api/auth/admin/oauth2/resources"])expect((await json(path,{})).status).toBe(404)
 })
 test("explicit bounded public registration, exact callback and signed consent preview",async()=>{
  expect((await json("/api/auth/oauth2/register",{client_name:"x".repeat(33000)})).status).toBe(413)
  const response=await json("/api/auth/oauth2/register",{client_name:"Fictional MCP Client",redirect_uris:[callback],token_endpoint_auth_method:"none",application_type:"native",grant_types:["authorization_code","refresh_token"],response_types:["code"],scope:"notes:read offline_access"})
  const body=await response.json();expect(response.status).toBe(201);clientId=body.client_id;expect(Boolean(clientId)).toBe(true)
  const authorization=await authorize()
  const preview=await json("/api/auth/mcp/authorize-info?"+new URLSearchParams({oauth_query:authorization.oauthQuery}),undefined,{Cookie:cookie})
  expect(preview.status).toBe(200);const info=await preview.json();expect(info.clientName).toBe("Fictional MCP Client");expect(info.redirectUri).toBe(callback);expect(info.globalWorkspaceSwitch).toBe(true)
  const tampered=authorization.oauthQuery.replace("notes%3Aread","notes%3Awrite")
  expect((await json("/api/auth/mcp/authorize-info?"+new URLSearchParams({oauth_query:tampered}),undefined,{Cookie:cookie})).status).toBe(400)
  expect((await json("/api/auth/oauth2/consent",{accept:true,oauth_query:authorization.oauthQuery},{Cookie:cookie,Origin:"https://evil.example"})).status).toBe(403)
  expect((await json("/api/auth/mcp/connections",undefined,{Authorization:"Bearer fictional-bootstrap-not-a-session-32chars"})).status).toBe(401)
  const denied=await json("/api/auth/oauth2/consent",{accept:false,oauth_query:authorization.oauthQuery},{Cookie:cookie,Origin:baseURL});expect(denied.status).toBe(200);const denial=new URL((await denied.json()).url);expect(denial.searchParams.get("error")).toBe("access_denied");expect(denial.searchParams.has("code")).toBe(false)
  expect((await(await json("/api/auth/mcp/connections",undefined,{Cookie:cookie})).json()).connections).toHaveLength(0)
 })
 test("official code exchange verifies PKCE, resource, consent and narrow scopes",async()=>{
  const tokens=await grant();access=tokens.access_token;refresh=tokens.refresh_token;expect(Boolean(refresh)).toBe(true)
  expect(await authentication.resolve(new Headers({Authorization:"Bearer "+access}))).toBeNull()
  const response=await protect(access);expect(response.status).toBe(200);const context=await response.json();expect(context.id).toBe(userId);expect(context.scopes.includes("notes:write")).toBe(false);connectionId=context.connectionId
  const silent=new URLSearchParams({client_id:clientId,redirect_uri:callback,response_type:"code",scope:"notes:read",resource:baseURL+"/mcp",prompt:"none"})
  const noInteraction=await json("/api/auth/oauth2/authorize?"+silent,undefined,{Cookie:cookie,Accept:"application/json"});expect(noInteraction.status).toBe(400);expect((await noInteraction.json()).error).toBe("consent_required")
  const otherResource=await form("/api/auth/oauth2/token",{grant_type:"refresh_token",client_id:clientId,refresh_token:refresh,resource:"https://evil.example/mcp"});expect(otherResource.status).toBe(400)
  const authorization=await authorize(),consent=await json("/api/auth/oauth2/consent",{accept:true,oauth_query:authorization.oauthQuery},{Cookie:cookie,Origin:baseURL}),redirect=new URL((await consent.json()).url)
  const wrong=await form("/api/auth/oauth2/token",{grant_type:"authorization_code",client_id:clientId,code:redirect.searchParams.get("code")!,redirect_uri:callback,code_verifier:randomBytes(40).toString("base64url"),resource:baseURL+"/mcp"});expect(wrong.status).toBe(400)
 })
 test("first MCP connection preserves an app-selected workspace and account-info cannot reset it",async()=>{
  fixtureOrganizationId=crypto.randomUUID()
  await db!.organization.create({data:{id:fixtureOrganizationId,name:"Fictional selected workspace",slug:"mcp-fixture-"+randomBytes(8).toString("hex"),createdAt:new Date()}})
  await db!.member.create({data:{id:crypto.randomUUID(),organizationId:fixtureOrganizationId,userId,role:"member",createdAt:new Date()}})
  await db!.session.updateMany({where:{userId},data:{activeOrganizationId:fixtureOrganizationId}})
  await db!.userWorkspaceState.deleteMany({where:{userId}})
  const first=await protect(access);expect(first.status).toBe(200);expect((await first.json()).workspaceId).toBe(fixtureOrganizationId)
  expect((await db!.userWorkspaceState.findUniqueOrThrow({where:{userId}})).activeOrganizationId).toBe(fixtureOrganizationId)
  expect((await createMcpStore(db!).accountInfo(userId)).activeWorkspace?.id).toBe(fixtureOrganizationId)
  // A subsequent explicit switch must survive the older originating session.
  const defaultMember=await db!.member.findFirstOrThrow({where:{userId,organizationId:{not:fixtureOrganizationId}}})
  await db!.userWorkspaceState.update({where:{userId},data:{activeOrganizationId:defaultMember.organizationId}})
  expect((await(await protect(access)).json()).workspaceId).toBe(defaultMember.organizationId)
 })
 test("refresh is durable across browser logout; new JWT remains account/consent bound",async()=>{
  const persisted=await db!.oauthRefreshToken.findMany({where:{userId,clientId}});expect(persisted.some(t=>t.scopes.includes("offline_access")&&!t.revoked)).toBe(true)
  expect((await json("/api/auth/sign-out",{}, {Cookie:cookie,Origin:baseURL})).status).toBe(200)
  const response=await form("/api/auth/oauth2/token",{grant_type:"refresh_token",client_id:clientId,refresh_token:refresh,resource:baseURL+"/mcp"})
  const tokens=await response.json();if(response.status!==200)throw new Error("OAuth refresh fixture: "+tokens.error+" "+tokens.error_description);expect(response.status).toBe(200);access=tokens.access_token;refresh=tokens.refresh_token;expect((await protect(access)).status).toBe(200)
  // Reconnect after an existing consent must return a signed same-origin review
  // page on login, never bypass the UI directly to the external callback.
  const resumeVerifier=randomBytes(40).toString("base64url"),resumeParams=new URLSearchParams({client_id:clientId,redirect_uri:callback,response_type:"code",scope:"notes:read offline_access",resource:baseURL+"/mcp",code_challenge:createHash("sha256").update(resumeVerifier).digest("base64url"),code_challenge_method:"S256"})
  const pending=await json("/api/auth/oauth2/authorize?"+resumeParams,undefined,{Accept:"text/html"});expect([302,303]).toContain(pending.status);const loginPage=new URL(pending.headers.get("location")!,baseURL);expect(loginPage.pathname).toBe("/mcp/connect")
  const resumed=await login(loginPage.searchParams.toString());expect(resumed.redirect).toBe(true);const review=new URL(resumed.url,baseURL);expect(review.origin).toBe(baseURL);expect(review.pathname).toBe("/mcp/consent")
 })
 test("another signed account cannot inspect or disconnect this account's authorization",async()=>{
  const otherEmail="mcp-other-"+randomBytes(8).toString("hex")+"@example.test",other=await Effect.runPromise(authentication.createUser({name:"Other fictional account",email:otherEmail,password}));otherUserId=other.id
  const signed=await json("/api/auth/sign-in/email",{email:otherEmail,password}),otherCookie=signed.headers.getSetCookie().map(c=>c.split(";")[0]).join("; ")
  expect((await(await json("/api/auth/mcp/connections",undefined,{Cookie:otherCookie})).json()).connections).toHaveLength(0)
  expect((await json("/api/auth/mcp/connections/"+connectionId+"/revoke",{},{Cookie:otherCookie,Origin:baseURL})).status).toBe(404)
  expect((await protect(access)).status).toBe(200)
 })
 test("disconnect instantly blocks issued JWTs and refresh, preserves browser session, regrant cannot revive old JWT",async()=>{
  const response=await json("/api/auth/mcp/connections",undefined,{Cookie:cookie});expect(response.status).toBe(200);expect((await response.json()).connections.some((c:any)=>c.id===connectionId)).toBe(true)
  const disconnected=await json("/api/auth/mcp/connections/"+connectionId+"/revoke",{},{Cookie:cookie,Origin:baseURL});expect(disconnected.status).toBe(200)
  expect((await protect(access)).status).toBe(401)
  expect((await form("/api/auth/oauth2/token",{grant_type:"refresh_token",client_id:clientId,refresh_token:refresh,resource:baseURL+"/mcp"})).status).toBe(400)
  expect((await json("/api/auth/get-session",undefined,{Cookie:cookie})).status).toBe(200)
  const fresh=await grant();expect((await protect(fresh.access_token)).status).toBe(200);expect((await protect(access)).status).toBe(401)
  const stale=await form("/api/auth/oauth2/token",{grant_type:"refresh_token",client_id:clientId,refresh_token:refresh,resource:baseURL+"/mcp"});expect(stale.status).toBe(400)
  const stillFresh=await form("/api/auth/oauth2/token",{grant_type:"refresh_token",client_id:clientId,refresh_token:fresh.refresh_token,resource:baseURL+"/mcp"});expect(stillFresh.status).toBe(200)
  // Membership revocation must immediately close the resource even with a valid JWT.
  await db!.member.deleteMany({where:{userId}});expect((await protect(fresh.access_token)).status).toBe(403)
 })
})
