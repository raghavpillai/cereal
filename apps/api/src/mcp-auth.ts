import {mcp,requireMcpAuth} from "@better-auth/mcp"
import {cimd} from "@better-auth/cimd"
import {fetchClientMetadataResource} from "@better-auth/cimd/node"
import {verifyOAuthQueryParams} from "@better-auth/oauth-provider"
import {jwt} from "better-auth/plugins"
import type {Database} from "@meeting-notes/database"
import type {PrincipalData} from "@meeting-notes/contracts/api"
import type {AuthenticationEngine} from "./auth"
import {MCP_SCOPES} from "./mcp-tools"
import {resolveNoteOrganization} from "./mcp-store"

export const MCP_OAUTH_SCOPES=[...MCP_SCOPES,"offline_access"]
export interface McpGrant {principal:PrincipalData;workspaceId:string;scopes:ReadonlySet<string>;connectionId:string;clientId:string}
const noStore={"Cache-Control":"no-store","Referrer-Policy":"no-referrer"}
const invalid=(message="The connection is no longer authorized")=>Response.json({error:"invalid_token",error_description:message},{status:401,headers:noStore})
const deny=(message:string,status=403)=>Response.json({error:"access_denied",error_description:message},{status,headers:noStore})
const role=(value:string|undefined|null):PrincipalData["role"]=>value==="owner"?"owner":value==="admin"?"admin":"user"

/** Protocol, PKCE, signatures, refresh rotation, CIMD and replay protection are
 * implemented by the official Better Auth plugins. The additional grant-id claim
 * enforces immediate disconnect and prevents an old JWT reviving on re-consent. */
export function makeMcpAuthPlugins(db:Database,baseURL:string){
 const resource=baseURL+"/mcp"
 return [jwt({disableSettingJwtHeader:true}),mcp({
  resource,loginPage:"/mcp/connect",consentPage:"/mcp/consent",scopes:MCP_OAUTH_SCOPES,
  resources:[{identifier:resource,name:"Cereal MCP",allowedScopes:MCP_OAUTH_SCOPES,accessTokenTtl:900,refreshTokenTtl:30*86400}],
  grantTypes:["authorization_code","refresh_token"],allowDynamicClientRegistration:true,allowUnauthenticatedClientRegistration:true,
  allowPublicClientPrelogin:true,refreshTokenReuseInterval:0,codeExpiresIn:300,
  rateLimit:{register:{window:60,max:5}},
  postLogin:{page:"/mcp/consent",shouldRedirect:async()=>false,consentReferenceId:async({user})=>user.id},
  extensions:[{claims:{accessToken:async({user,client,referenceId,scopes})=>{
   if(!user||referenceId!==user.id)throw new Error("MCP requires an account authorization")
   const consent=await db.oauthConsent.findFirst({where:{clientId:client.clientId,userId:user.id,referenceId:user.id}})
   if(!consent||!scopes.every(scope=>consent.scopes.includes(scope)))throw new Error("MCP authorization has been revoked")
   return {mcp_grant_id:consent.id,account_id:user.id}
  }}}],
 }),cimd({metadataProfile:"mcp-2026-07-28",fetchClientMetadataResource,
  metadataFetchPolicy:{maximumConcurrentFetches:8,maximumConcurrentFetchesPerOrigin:2,maximumFetchesPerMinute:60,maximumFetchesPerOriginPerMinute:15}})] as const
}

export function makeMcpAuthentication(db:Database,auth:AuthenticationEngine,baseURL:string){
 const resource=baseURL+"/mcp"
 const invalidGrant=()=>{const response=invalid();response.headers.set("WWW-Authenticate",`Bearer resource_metadata="${baseURL}/.well-known/oauth-protected-resource/mcp", error="invalid_token", scope="${MCP_OAUTH_SCOPES.join(" ")}"`);return response}
 async function selectedWorkspace(userId:string,sessionOrganizationId?:string|null){
  const state=await db.userWorkspaceState.findUnique({where:{userId}})
  if(!state?.activeOrganizationId){
   const sessionMembership=sessionOrganizationId?await db.member.findUnique({where:{organizationId_userId:{organizationId:sessionOrganizationId,userId}}}):null
   const initial=sessionMembership?.organizationId??await resolveNoteOrganization(db,userId)
   if(initial){
    // Initialize older sessions once. An explicit concurrent app/MCP switch
    // wins: neither upsert nor the null-only write replaces a selected value.
    await db.userWorkspaceState.upsert({where:{userId},create:{userId,activeOrganizationId:initial},update:{}})
    await db.userWorkspaceState.updateMany({where:{userId,activeOrganizationId:null},data:{activeOrganizationId:initial}})
   }
  }
  const organizationId=await resolveNoteOrganization(db,userId)
  return organizationId?db.member.findUnique({where:{organizationId_userId:{organizationId,userId}},include:{organization:true}}):null
 }
 async function signed(headers:Headers){
  const safe=new Headers(headers);if(safe.has("authorization"))safe.delete("cookie")
  return auth.api.getSession({headers:safe,query:{disableCookieCache:true}})
 }
 async function revokeId(userId:string,id:string){
  return db.$transaction(async tx=>{
   const consent=await tx.oauthConsent.findFirst({where:{id,userId}})
   if(!consent)return false
   await tx.oauthConsent.delete({where:{id}})
   const where={clientId:consent.clientId,userId,referenceId:consent.referenceId}
   // Remove the revoked family, rather than leaving replay tombstones that the
   // provider would interpret as rotation reuse and use to invalidate a later
   // re-consented refresh family for this same client/account.
   await tx.oauthAccessToken.deleteMany({where})
   await tx.oauthRefreshToken.deleteMany({where})
   return true
  })
 }
 return {
  resource,
  /** Called only for public well-known discovery by the HTTP transport. */
  metadata:(request:Request)=>auth.handler(request),
  async protect(request:Request,handler:(request:Request,grant:McpGrant)=>Promise<Response>){
   return requireMcpAuth(auth,async(req,claims)=>{
    const userId=typeof claims.sub==="string"?claims.sub:null
    const clientId=typeof claims.azp==="string"?claims.azp:null
    const sessionId=typeof claims.sid==="string"?claims.sid:null
    const connectionId=typeof claims.mcp_grant_id==="string"?claims.mcp_grant_id:null
    if(!userId||!clientId||!connectionId||claims.account_id!==userId||typeof claims.exp!=="number"||typeof claims.iat!=="number")return invalidGrant()
    const [user,session,client,consent]=await Promise.all([
     db.user.findUnique({where:{id:userId}}),sessionId?db.session.findUnique({where:{id:sessionId}}):Promise.resolve(null),
     db.oauthClient.findUnique({where:{clientId}}),db.oauthConsent.findUnique({where:{id:connectionId}}),
    ])
    const scopes=new Set(typeof claims.scope==="string"?claims.scope.split(/\s+/).filter(Boolean):[])
    if(!user||user.banned||session&&session.userId!==userId||!client||client.disabled||!consent||consent.userId!==userId||consent.clientId!==clientId||consent.referenceId!==userId||![...scopes].every(scope=>MCP_OAUTH_SCOPES.includes(scope)&&consent.scopes.includes(scope)))return invalidGrant()
    const member=await selectedWorkspace(userId,session?.activeOrganizationId)
    if(!member)return deny("Select an accessible workspace before using this connection")
    const principal:PrincipalData={id:user.id,name:user.name,email:user.email,role:role(user.role),createdAt:user.createdAt.toISOString(),source:"session",...(sessionId?{sessionId}:{}),expiresAt:typeof claims.exp==="number"?new Date(claims.exp*1000).toISOString():null}
    return handler(req,{principal,workspaceId:member.organizationId,scopes,connectionId,clientId})
   },{resource,challengeScopes:MCP_OAUTH_SCOPES})(request)
  },
  revoke:(grant:McpGrant)=>revokeId(grant.principal.id,grant.connectionId).then(()=>undefined),
  /** Same-origin account settings APIs. Never accepts an operator bootstrap token
   * or treats the MCP OAuth JWT as a browser/native session. */
  async accountHandler(request:Request):Promise<Response|null>{
   const url=new URL(request.url),path=url.pathname
   if(!path.startsWith("/api/auth/mcp/"))return null
   const session=await signed(request.headers)
   if(!session)return invalid("Sign in with your account")
   if(path==="/api/auth/mcp/authorize-info"&&request.method==="GET"){
    const query=url.searchParams.get("oauth_query")??""
    if(query.length>16384||!await verifyOAuthQueryParams(query,(await auth.$context).secret))return deny("Authorization request is invalid or expired",400)
    const params=new URLSearchParams(query),clientId=params.get("client_id")??"",redirectUri=params.get("redirect_uri")??""
    const scopes=(params.get("scope")??"").split(/\s+/).filter(Boolean)
    if(params.getAll("resource").length!==1||params.get("resource")!==resource||!scopes.every(scope=>MCP_OAUTH_SCOPES.includes(scope)))return deny("Authorization resource or scopes are invalid",400)
    // Signed query proves the provider already validated exact/native-loopback
    // redirect matching. Public client API rechecks registration and disabled state.
    const client=await auth.api.getOAuthClientPublic({headers:request.headers,query:{client_id:clientId}})
    if(!client||client.disabled)return deny("Client is unavailable",400)
    const member=await selectedWorkspace(session.user.id,session.session.activeOrganizationId)
    return Response.json({clientId,clientName:client.client_name??clientId,redirectUri,scopes,resource,workspace:member?{id:member.organizationId,name:member.organization.name}:null,globalWorkspaceSwitch:true},{headers:noStore})
   }
   if(path==="/api/auth/mcp/connections"&&request.method==="GET"){
    const consents=await db.oauthConsent.findMany({where:{userId:session.user.id,referenceId:session.user.id},orderBy:{createdAt:"desc"},take:100,include:{client:true}})
    return Response.json({connections:consents.map(c=>({id:c.id,clientId:c.clientId,clientName:c.client.name??c.clientId,scopes:c.scopes,createdAt:c.createdAt.toISOString(),updatedAt:c.updatedAt.toISOString()}))},{headers:noStore})
   }
   const match=/^\/api\/auth\/mcp\/connections\/([^/]+)\/revoke$/.exec(path)
   if(match&&request.method==="POST")return await revokeId(session.user.id,match[1]!)?Response.json({success:true},{headers:noStore}):deny("Connection not found",404)
   return deny("Connection route is unavailable",404)
  },
 }
}
