import {createHash,timingSafeEqual} from "node:crypto"
import {readFile} from "node:fs/promises"
import {betterAuth} from "better-auth"
import {APIError,createAuthMiddleware} from "better-auth/api"
import {prismaAdapter} from "better-auth/adapters/prisma"
import {bearer,organization,admin as adminPlugin} from "better-auth/plugins"
import {createAccessControl} from "better-auth/plugins/access"
import {defaultStatements as adminStatements} from "better-auth/plugins/admin/access"
import {defaultAc as organizationAccess} from "better-auth/plugins/organization/access"
import {makeOrganizationService,DEFAULT_ORGANIZATION_ID} from "./organizations"
import {hashPassword} from "better-auth/crypto"
import {Effect} from "effect"
import type {Database} from "@meeting-notes/database"
import type {CreateUserInput,UserDTO} from "@meeting-notes/contracts"
import {AuthPrincipal,type PrincipalData,type ApiError} from "@meeting-notes/contracts/api"
import {loadSecretBox} from "./admin-secrets"
import {withAccountMutation} from "./account-mutations"
import {makeMcpAuthPlugins,makeMcpAuthentication} from "./mcp-auth"
export {AuthPrincipal}
export const BOOTSTRAP_OWNER_ID="00000000-0000-4000-8000-000000000001"
export const bootstrapPrincipal:PrincipalData={id:BOOTSTRAP_OWNER_ID,name:"Owner",email:"owner@localhost.invalid",role:"owner",source:"bootstrap",expiresAt:null,createdAt:"2026-01-01T00:00:00.000Z"}
export function isBootstrapToken(headers:Headers,token:string){const supplied=/^Bearer +(.+)$/i.exec(headers.get("authorization")??"")?.[1];return !!supplied&&timingSafeEqual(createHash("sha256").update(supplied).digest(),createHash("sha256").update(token).digest())}
export async function loadAuthenticationSecret(path:string){await loadSecretBox(path);return(await readFile(path)).toString("base64url")}
const userDTO=(u:{id:string;name:string;email:string;role?:string;createdAt:Date}):UserDTO=>({id:u.id,name:u.name,email:u.email,role:u.role==="owner"?"owner":u.role==="admin"?"admin":"user",createdAt:u.createdAt.toISOString()})
class AuthFailure extends Error{constructor(readonly payload:ApiError){super(payload.message)}}
const fail=(error:"invalid_request"|"conflict"|"not_found"|"forbidden",message:string)=>new AuthFailure({error,message})
const boundary=<A>(f:()=>Promise<A>)=>Effect.tryPromise({try:f,catch:(e):ApiError=>{
 if(e instanceof AuthFailure)return e.payload
 const status=(e as{statusCode?:number})?.statusCode
 if(status===401)return{error:"unauthorized",message:"Sign in again to continue"}
 if(status===403)return{error:"forbidden",message:"Account action is not permitted"}
 if(status===404)return{error:"not_found",message:"Account not found"}
 if(status===409)return{error:"conflict",message:"Account state changed; refresh and try again"}
 return{error:"unavailable",message:"Account service is temporarily unavailable"}
}})
const instanceAccess=createAccessControl(adminStatements)
export function makeAuthenticationEngine(db:Database,options:{secret:string},baseURL:string,origins:string[]){
 return betterAuth({
  disabledPaths:["/token"],database:prismaAdapter(db,{provider:"postgresql",transaction:true}),secret:options.secret,baseURL:baseURL,basePath:"/api/auth",trustedOrigins:origins,
  emailAndPassword:{enabled:true,minPasswordLength:12,maxPasswordLength:128,autoSignIn:false},
  session:{expiresIn:7*86400,updateAge:86400,cookieCache:{enabled:false}},
  advanced:{database:{generateId:()=>crypto.randomUUID()},useSecureCookies:baseURL.startsWith("https:"),defaultCookieAttributes:{httpOnly:true,sameSite:"lax",secure:baseURL.startsWith("https:")}},
  // Supported provider pre-hook adds review before the provider signs browser
  // continuation data. Refresh never visits authorize and remains silent.
  hooks:{before:createAuthMiddleware(async ctx=>{
   if(ctx.path!=="/oauth2/authorize"||!ctx.request||ctx.request.method!=="GET"||new URL(ctx.request.url).pathname!=="/api/auth/oauth2/authorize")return
   const prompts=String(ctx.query?.prompt??"").split(/\s+/).filter(Boolean)
   if(prompts.includes("none"))throw new APIError("BAD_REQUEST",{error:"consent_required",error_description:"Explicit client consent is required"})
   return {context:{query:{...ctx.query,prompt:[...new Set([...prompts,"consent"])].join(" ")}}}
  })},
  databaseHooks:{user:{create:{before:async(user)=>({data:{...user,role:"user"}})}}},
  plugins:[...makeMcpAuthPlugins(db,baseURL),bearer({requireSignature:true}),adminPlugin({defaultRole:"user",ac:instanceAccess,roles:{
   owner:instanceAccess.newRole({user:["create","list","set-role"],session:[]}),
   admin:instanceAccess.newRole({user:["create","list"],session:[]}),
   user:instanceAccess.newRole({user:[],session:[]}),
  },adminRoles:["owner","admin"]}),organization({
   allowUserToCreateOrganization:async(user)=>user.role==="owner",teams:{enabled:false},
   // Accounts are provisioned by operators; invitations are authenticated in-app,
   // not email capability links. Never claim SMTP/email verification took place.
   requireEmailVerificationOnInvitation:false,
   roles:{owner:organizationAccess.newRole({organization:["update"],member:["create","update","delete"],invitation:["create","cancel"]}),
    admin:organizationAccess.newRole({organization:["update"],member:["create","delete"],invitation:["create","cancel"]}),
    member:organizationAccess.newRole({organization:[],member:[],invitation:[]})},
  })],
  rateLimit:{enabled:true,window:60,max:60,customRules:{"/sign-in/email":{window:60,max:10}}},logger:{disabled:true},
 })
}
export type AuthenticationEngine=ReturnType<typeof makeAuthenticationEngine>
/** Only an explicit operator token can initialize the owner. Anonymous signup is
 * never mounted. Better Auth handles hashing, sessions, cookies and revocation. */
export function makeAuthenticationService(db:Database,bootstrapToken:string,options:{secret:string;baseURL:string;trustedOrigins?:ReadonlyArray<string>}){
 const base=new URL(options.baseURL)
 if(!["https:","http:"].includes(base.protocol)||base.username||base.password||base.pathname!=="/"||base.search||base.hash||base.protocol==="http:"&&!["localhost","127.0.0.1","[::1]"].includes(base.hostname))throw new Error("PUBLIC_BASE_URL must use HTTPS except for loopback development")
 if(options.secret.length<32)throw new Error("Authentication secret must contain at least 32 characters")
 const origins=[base.origin];if(base.protocol==="http:"){const alias=new URL(base);alias.hostname=base.hostname==="localhost"?"127.0.0.1":"localhost";origins.push(alias.origin)}
 for(const raw of options.trustedOrigins??[]){const origin=new URL(raw);if(raw!==origin.origin||origin.username||origin.password||origin.protocol!=="https:"&&!(origin.protocol==="http:"&&["localhost","127.0.0.1","[::1]"].includes(origin.hostname)))throw new Error("AUTH_TRUSTED_ORIGINS must contain explicit HTTPS origins or loopback development origins");if(!origins.includes(origin.origin))origins.push(origin.origin)}
 const auth=makeAuthenticationEngine(db,options,base.origin,origins)
 const organizations=makeOrganizationService(db,auth)
 const mcp=makeMcpAuthentication(db,auth,base.origin)
 const allowed=new Map([
  ["/sign-in/email",["POST"]],["/sign-out",["POST"]],["/get-session",["GET"]],
  ["/change-password",["POST"]],["/update-user",["POST"]],["/list-sessions",["GET"]],
  ["/revoke-session",["POST"]],["/revoke-other-sessions",["POST"]],["/revoke-sessions",["POST"]],
  ["/jwks",["GET"]],["/.well-known/oauth-authorization-server",["GET"]],
  ["/oauth2/authorize",["GET"]],["/oauth2/token",["POST"]],["/oauth2/register",["POST"]],
  ["/oauth2/introspect",["POST"]],["/oauth2/revoke",["POST"]],["/oauth2/consent",["POST"]],
  ["/oauth2/continue",["POST"]],["/oauth2/public-client",["GET"]],["/oauth2/public-client-prelogin",["POST"]],
 ])
 return {
  organizations,mcp,
  async resolve(headers:Headers):Promise<PrincipalData|null>{
   if(isBootstrapToken(headers,bootstrapToken)){const owner=await db.user.findUnique({where:{id:BOOTSTRAP_OWNER_ID}});return owner?{...userDTO(owner),source:"bootstrap",expiresAt:null}:null}
   // Explicit bearer credentials take precedence over cookies, so a stale browser
   // cookie cannot silently change the identity of a native/session-token request.
   const sessionHeaders=new Headers(headers);if(sessionHeaders.has("authorization"))sessionHeaders.delete("cookie")
   const session=await auth.api.getSession({headers:sessionHeaders,query:{disableCookieCache:true}})
   if(!session)return null
   const user=await db.user.findUnique({where:{id:session.user.id}})
   if(!user)return null
   return{...userDTO(user),source:"session",expiresAt:new Date(session.session.expiresAt).toISOString(),sessionId:session.session.id}
  },
  async handler(request:Request){
   const path=new URL(request.url).pathname.slice("/api/auth".length)
   const accountRoute=path.startsWith("/mcp/")
   if(!accountRoute&&!allowed.get(path)?.includes(request.method))return Response.json({error:"not_found",message:"Authentication route not available"},{status:404,headers:{"Cache-Control":"no-store"}})
   // Protocol endpoints authenticate OAuth clients and must work cross-origin.
   // Browser account/consent mutations retain strict CSRF protection.
   const protocolPost=["/oauth2/token","/oauth2/register","/oauth2/introspect","/oauth2/revoke"].includes(path)
   const origin=request.headers.get("origin")
   if(request.method!=="GET"&&!protocolPost&&((origin&&!origins.includes(origin))||request.headers.get("sec-fetch-site")==="cross-site"||(!origin&&request.headers.has("cookie")&&!request.headers.has("authorization"))))return Response.json({error:"forbidden",message:"A same-origin request is required"},{status:403,headers:{"Cache-Control":"no-store"}})
   if(protocolPost||request.headers.has("authorization")&&request.headers.has("cookie")){const headers=new Headers(request.headers);headers.delete("cookie");request=new Request(request,{headers})}
   if(path==="/oauth2/register"){
    const reader=request.body?.getReader();let size=0;const chunks:ArrayBuffer[]=[]
    if(reader){while(true){const next=await reader.read();if(next.done)break;size+=next.value.byteLength;if(size>32768){await reader.cancel();return Response.json({error:"invalid_client_metadata",error_description:"Client metadata is too large"},{status:413,headers:{"Cache-Control":"no-store"}})}chunks.push(new Uint8Array(next.value).buffer)}}
    request=new Request(request,{body:new Blob(chunks)})
   }
   const response=path==="/oauth2/register"?await withAccountMutation(db,"mcp-client-registration",async()=>{
    if(await db.oauthClient.count()>=1000)return Response.json({error:"temporarily_unavailable",error_description:"Client registration capacity reached"},{status:429,headers:{"Cache-Control":"no-store"}})
    return auth.handler(request)
   }):accountRoute?await mcp.accountHandler(request):await auth.handler(request)
   if(!response)return Response.json({error:"not_found"},{status:404})
   response.headers.set("Cache-Control","no-store");response.headers.set("Referrer-Policy","no-referrer");return response
  },
  isTrustedOrigin(origin:string){return origins.includes(origin)},
  listUsers:(headers?:Headers)=>boundary(async()=>({users:(headers?(await auth.api.listUsers({headers,query:{limit:1000,sortBy:"createdAt",sortDirection:"asc"}})).users:await db.user.findMany({orderBy:{createdAt:"asc"},take:1000})).map(userDTO)})),
  createUser:(input:CreateUserInput,headers?:Headers)=>boundary(async()=>{
   try{const result=await auth.api.createUser({...(headers?{headers}:{}),body:{name:input.name.trim(),email:input.email.trim().toLowerCase(),password:input.password}});const user=await db.user.findUnique({where:{id:result.user.id}});if(!user)throw new Error("Account creation failed");await db.member.upsert({where:{organizationId_userId:{organizationId:DEFAULT_ORGANIZATION_ID,userId:user.id}},create:{id:crypto.randomUUID(),organizationId:DEFAULT_ORGANIZATION_ID,userId:user.id,role:"member"},update:{}});return userDTO(user)}catch(e){if(e instanceof AuthFailure)throw e;const status=(e as{statusCode?:number}).statusCode;if(status===400||status===422||status===409)throw fail("invalid_request","Account could not be created. Check the email and password requirements.");throw e}
  }),
  createAdministrator:(input:CreateUserInput,headers?:Headers)=>boundary(async()=>{
   if(headers){const session=await auth.api.getSession({headers,query:{disableCookieCache:true}});const actor=session?await db.user.findUnique({where:{id:session.user.id}}):null;if(actor?.role!=="owner")throw fail("forbidden","Only the owner can create administrators")}
   const password=await hashPassword(input.password),id=crypto.randomUUID()
   try{return await db.$transaction(async tx=>{
    const user=await tx.user.create({data:{id,name:input.name.trim(),email:input.email.trim().toLowerCase(),role:"admin"}})
    await tx.account.create({data:{id:crypto.randomUUID(),accountId:id,userId:id,providerId:"credential",password}})
    await tx.member.create({data:{id:crypto.randomUUID(),organizationId:DEFAULT_ORGANIZATION_ID,userId:id,role:"member"}})
    return userDTO(user)
   })}catch(error){if((error as{code?:string}).code==="P2002")throw fail("invalid_request","Email is already in use");throw error}
  }),
  updateUserRole:(id:string,role:"admin"|"user",headers?:Headers)=>boundary(()=>withAccountMutation(db,"instance-role:"+id,async()=>{const user=await db.user.findUnique({where:{id}});if(!user)throw fail("not_found","User not found");if(user.role==="owner")throw fail("forbidden","The instance owner role cannot be changed");if(headers){await auth.api.setRole({headers,body:{userId:id,role}});return userDTO(await db.user.findUniqueOrThrow({where:{id}}))}return userDTO(await db.user.update({where:{id},data:{role}}))})),
  setupOwner:(input:CreateUserInput)=>boundary(async()=>{const password=await hashPassword(input.password);return db.$transaction(async tx=>{await tx.$queryRawUnsafe('SELECT "id" FROM "User" WHERE "id"=$1::uuid FOR UPDATE',BOOTSTRAP_OWNER_ID);if(await tx.account.findFirst({where:{userId:BOOTSTRAP_OWNER_ID,providerId:"credential"}}))throw fail("conflict","The owner account has already been initialized");if(await tx.user.findFirst({where:{email:input.email.trim().toLowerCase(),id:{not:BOOTSTRAP_OWNER_ID}}}))throw fail("conflict","Email is already in use");const user=await tx.user.update({where:{id:BOOTSTRAP_OWNER_ID},data:{name:input.name.trim(),email:input.email.trim().toLowerCase()}});await tx.account.create({data:{id:crypto.randomUUID(),accountId:BOOTSTRAP_OWNER_ID,userId:BOOTSTRAP_OWNER_ID,providerId:"credential",password}});return userDTO(user)})}),
 }
}
export type AuthenticationService=ReturnType<typeof makeAuthenticationService>
