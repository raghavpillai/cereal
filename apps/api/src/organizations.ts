import { Effect } from "effect"
import {withAccountMutation} from "./account-mutations"
import type { Database } from "@meeting-notes/database"
import type { AuthenticationEngine } from "./auth"
import type { PrincipalData, ApiError } from "@meeting-notes/contracts/api"
import type {
  OrganizationDTO, OrganizationsDTO, OrganizationDetailsDTO, OrganizationMemberDTO,
  OrganizationInvitationDTO, OrganizationInvitationsDTO, CreateOrganizationInput,
  UpdateOrganizationInput, InviteOrganizationMemberInput, UpdateOrganizationMemberRoleInput,
} from "@meeting-notes/contracts"

export const DEFAULT_ORGANIZATION_ID = "00000000-0000-4000-8000-000000000002"
type Role = OrganizationDTO["role"]
class OrganizationFailure extends Error {
  constructor(readonly payload: ApiError) { super(payload.message) }
}
const fail = (error: ApiError["error"], message: string) => new OrganizationFailure({ error, message } as ApiError)
const role = (value: string): Role => {
  if (value === "owner" || value === "admin" || value === "member") return value
  throw fail("unavailable", "Workspace role is invalid")
}
function errorDTO(error: unknown): ApiError {
  if (error instanceof OrganizationFailure) return error.payload
  const status = (error as {statusCode?: number})?.statusCode
  if (status === 401 || status === 403) return {error:"forbidden",message:"Workspace action is not permitted"}
  if (status === 404) return {error:"not_found",message:"Workspace or invitation not found"}
  if (status === 409) return {error:"conflict",message:"Workspace state changed; refresh and try again"}
  if (status === 400 || status === 422) return {error:"invalid_request",message:"Workspace action could not be completed. Check the details and refresh."}
  return {error:"unavailable",message:"Workspace service is temporarily unavailable"}
}
const boundary = <A>(f: () => Promise<A>) => Effect.tryPromise({try:f,catch:errorDTO})
const sessionHeaders = (headers: Headers) => {
  const result = new Headers(headers)
  if (result.has("authorization")) result.delete("cookie")
  return result
}
const organizationDTO = (org:{id:string;name:string;slug:string;createdAt:Date}, memberRole:string):OrganizationDTO =>
  ({id:org.id,name:org.name,slug:org.slug,role:role(memberRole),createdAt:org.createdAt.toISOString()})
const memberDTO = (member:{id:string;organizationId:string;userId:string;role:string;createdAt:Date;user:{name:string;email:string}}):OrganizationMemberDTO =>
  ({id:member.id,organizationId:member.organizationId,userId:member.userId,name:member.user.name,email:member.user.email,role:role(member.role),createdAt:member.createdAt.toISOString()})
function invitationDTO(invitation:{id:string;organizationId:string;email:string;role:string|null;status:string;inviterId:string;createdAt:Date;expiresAt:Date},name:string):OrganizationInvitationDTO {
  const status = invitation.status === "pending" && invitation.expiresAt <= new Date() ? "expired" : invitation.status
  if (!["pending","accepted","rejected","canceled","expired"].includes(status)) throw fail("unavailable","Invitation state is invalid")
  return {id:invitation.id,organizationId:invitation.organizationId,organizationName:name,email:invitation.email,
    role:role(invitation.role ?? "member"),status:status as OrganizationInvitationDTO["status"],inviterId:invitation.inviterId,
    createdAt:invitation.createdAt.toISOString(),expiresAt:invitation.expiresAt.toISOString()}
}

/** Typed façade around real Better Auth plugin APIs. Raw plugin HTTP routes are
 * not mounted. All mutation authorization is enforced here, independently of
 * instance roles. Membership never grants access to Note/Transcript/Share. */
export function makeOrganizationService(db:Database,auth:AuthenticationEngine) {
  async function signed(principal:PrincipalData,headers:Headers) {
    if (principal.source !== "session") throw fail("forbidden","Sign in with your account to manage workspaces")
    const safe = sessionHeaders(headers)
    const session = await auth.api.getSession({headers:safe,query:{disableCookieCache:true}})
    if (!session || session.user.id !== principal.id) throw fail("unauthorized","Sign in again to manage workspaces")
    return {headers:safe,session}
  }
  async function membership(userId:string,organizationId:string) {
    const member = await db.member.findUnique({where:{organizationId_userId:{organizationId,userId}},include:{organization:true}})
    if (!member) throw fail("not_found","Workspace not found")
    return member
  }
  // Every role/removal/invitation mutation in this façade takes the same database
  // advisory lock. Plugin writes use their own adapter transaction; advisory
  // ownership is deliberately separate, avoiding row-lock/self-deadlock.
  async function serialized<A>(organizationId:string,operation:()=>Promise<A>):Promise<A> {
    return withAccountMutation(db,"workspace:"+organizationId,operation)
  }
  async function list(principal:PrincipalData,headers:Headers):Promise<OrganizationsDTO> {
    if (principal.source === "bootstrap") {
      const memberships = await db.member.findMany({where:{userId:principal.id},include:{organization:true},orderBy:{createdAt:"asc"}})
      return {organizations:memberships.map(m=>organizationDTO(m.organization,m.role)),activeOrganizationId:memberships.find(m=>m.organizationId===DEFAULT_ORGANIZATION_ID)?.organizationId ?? memberships[0]?.organizationId ?? null}
    }
    const context = await signed(principal,headers)
    const organizations = await auth.api.listOrganizations({headers:context.headers})
    const memberships = await db.member.findMany({where:{userId:principal.id}})
    const visible = organizations.flatMap(org=>{
      const member = memberships.find(m=>m.organizationId===org.id)
      return member ? [organizationDTO(org,member.role)] : []
    })
    const active = context.session.session.activeOrganizationId
    return {organizations:visible,activeOrganizationId:visible.some(o=>o.id===active)?active!:
      visible.find(o=>o.id===DEFAULT_ORGANIZATION_ID)?.id ?? visible[0]?.id ?? null}
  }
  async function detail(principal:PrincipalData,headers:Headers,id:string):Promise<OrganizationDetailsDTO> {
    const context = await signed(principal,headers), actor = await membership(principal.id,id)
    const org = await auth.api.getFullOrganization({headers:context.headers,query:{organizationId:id,membersLimit:1000}})
    if (!org) throw fail("not_found","Workspace not found")
    const members = await auth.api.listMembers({headers:context.headers,query:{organizationId:id,limit:1000}})
    const invitations = actor.role === "member" ? [] : await auth.api.listInvitations({headers:context.headers,query:{organizationId:id}})
    return {organization:organizationDTO(org,actor.role),members:members.members.map(memberDTO),invitations:invitations.map(i=>invitationDTO(i,org.name))}
  }
  async function recipient(principal:PrincipalData,id:string) {
    const invitation = await db.invitation.findUnique({where:{id},include:{organization:true}})
    if (!invitation || invitation.email.toLowerCase() !== principal.email.toLowerCase()) throw fail("not_found","Invitation not found")
    if (invitation.status !== "pending") throw fail("conflict","Invitation has already been handled")
    if (invitation.expiresAt <= new Date()) throw fail("conflict","Invitation has expired")
    return invitation
  }
  return {
    list:(principal:PrincipalData,headers:Headers)=>boundary(()=>list(principal,headers)),
    get:(principal:PrincipalData,headers:Headers,id:string)=>boundary(()=>detail(principal,headers,id)),
    create:(principal:PrincipalData,headers:Headers,input:CreateOrganizationInput)=>boundary(async()=>{
      const context = await signed(principal,headers)
      if (context.session.user.role !== "owner") throw fail("forbidden","Only the instance owner can create workspaces")
      const name=input.name.trim()
      if (!name) throw fail("invalid_request","Workspace name is required")
      const created=await auth.api.createOrganization({headers:context.headers,body:{name,slug:input.slug,keepCurrentActiveOrganization:true}})
      if (!created) throw fail("unavailable","Workspace could not be created")
      return organizationDTO(created,"owner")
    }),
    select:(principal:PrincipalData,headers:Headers,id:string)=>boundary(()=>serialized(id,async()=>{
      const context=await signed(principal,headers)
      await membership(principal.id,id)
      await auth.api.setActiveOrganization({headers:context.headers,body:{organizationId:id}})
      // The selected workspace belongs to the account, not to one session. Keep
      // browser, native and MCP connections on one selected workspace for new notes.
      await db.$transaction(async tx=>{
        await tx.userWorkspaceState.upsert({where:{userId:principal.id},create:{userId:principal.id,activeOrganizationId:id},update:{activeOrganizationId:id}})
        await tx.session.updateMany({where:{userId:principal.id},data:{activeOrganizationId:id}})
      })
      return list(principal,context.headers)
    })),
    update:(principal:PrincipalData,headers:Headers,id:string,input:UpdateOrganizationInput)=>boundary(()=>serialized(id,async()=>{
      const context=await signed(principal,headers),actor=await membership(principal.id,id)
      if (actor.role!=="owner"&&actor.role!=="admin") throw fail("forbidden","Workspace administrator access required")
      const name=input.name.trim()
      if (!name) throw fail("invalid_request","Workspace name is required")
      const updated=await auth.api.updateOrganization({headers:context.headers,body:{organizationId:id,data:{name}}})
      if (!updated) throw fail("not_found","Workspace not found")
      return organizationDTO(updated,actor.role)
    })),
    invite:(principal:PrincipalData,headers:Headers,id:string,input:InviteOrganizationMemberInput)=>boundary(()=>serialized(id,async()=>{
      const context=await signed(principal,headers),actor=await membership(principal.id,id)
      if (actor.role!=="owner"&&actor.role!=="admin") throw fail("forbidden","Workspace administrator access required")
      if (input.role==="admin"&&actor.role!=="owner") throw fail("forbidden","Only the workspace owner can invite administrators")
      const email=input.email.trim().toLowerCase()
      if (!await db.user.findFirst({where:{email:{equals:email,mode:"insensitive"}}})) throw fail("invalid_request","An instance administrator must provision this account before it can be invited")
      const invitation=await auth.api.createInvitation({headers:context.headers,body:{organizationId:id,email,role:input.role}})
      return invitationDTO(invitation,actor.organization.name)
    })),
    cancel:(principal:PrincipalData,headers:Headers,id:string,invitationId:string)=>boundary(()=>serialized(id,async()=>{
      const context=await signed(principal,headers),actor=await membership(principal.id,id)
      const invitation=await db.invitation.findUnique({where:{id:invitationId}})
      if (!invitation||invitation.organizationId!==id) throw fail("not_found","Invitation not found")
      if (actor.role!=="owner"&&(actor.role!=="admin"||invitation.role!=="member")) throw fail("forbidden","Only the workspace owner can manage administrator invitations")
      if (invitation.status!=="pending") throw fail("conflict","Invitation has already been handled")
      await auth.api.cancelInvitation({headers:context.headers,body:{invitationId}})
      return {deleted:true as const}
    })),
    updateRole:(principal:PrincipalData,headers:Headers,id:string,memberId:string,input:UpdateOrganizationMemberRoleInput)=>boundary(()=>serialized(id,async()=>{
      const context=await signed(principal,headers),actor=await membership(principal.id,id)
      if (actor.role!=="owner") throw fail("forbidden","Only the workspace owner can change member roles")
      const target=await db.member.findUnique({where:{id:memberId}})
      if (!target||target.organizationId!==id) throw fail("not_found","Workspace member not found")
      if (target.role==="owner") throw fail("forbidden","The workspace owner role cannot be changed")
      await auth.api.updateMemberRole({headers:context.headers,body:{organizationId:id,memberId,role:input.role}})
      return memberDTO(await db.member.findUniqueOrThrow({where:{id:memberId},include:{user:true}}))
    })),
    remove:(principal:PrincipalData,headers:Headers,id:string,memberId:string)=>boundary(()=>serialized(id,async()=>{
      const context=await signed(principal,headers),actor=await membership(principal.id,id)
      const target=await db.member.findUnique({where:{id:memberId}})
      if (!target||target.organizationId!==id) throw fail("not_found","Workspace member not found")
      if (actor.role!=="owner"&&(actor.role!=="admin"||target.role!=="member")) throw fail("forbidden","Only the workspace owner can remove administrators or owners")
      if (target.role==="owner"&&await db.member.count({where:{organizationId:id,role:"owner"}})<=1) throw fail("conflict","The last workspace owner cannot be removed")
      await auth.api.removeMember({headers:context.headers,body:{organizationId:id,memberIdOrEmail:memberId}})
      await db.session.updateMany({where:{userId:target.userId,activeOrganizationId:id},data:{activeOrganizationId:null}})
      return {deleted:true as const}
    })),
    incoming:(principal:PrincipalData,headers:Headers)=>boundary(async():Promise<OrganizationInvitationsDTO>=>{
      await signed(principal,headers)
      // Server-only plugin API: authenticated principal supplies this exact email.
      // No fabricated emailVerified flag and no anonymous email lookup route.
      const invitations=await auth.api.listUserInvitations({query:{email:principal.email}})
      return {invitations:invitations.map(i=>invitationDTO(i,i.organizationName))}
    }),
    accept:(principal:PrincipalData,headers:Headers,id:string)=>boundary(async()=>{
      const context=await signed(principal,headers),invitation=await recipient(principal,id)
      return serialized(invitation.organizationId,async()=>{
        const current=await recipient(principal,id)
        await auth.api.acceptInvitation({headers:context.headers,body:{invitationId:id}})
        const member=await membership(principal.id,current.organizationId)
        return organizationDTO(current.organization,member.role)
      })
    }),
    reject:(principal:PrincipalData,headers:Headers,id:string)=>boundary(async()=>{
      const context=await signed(principal,headers),invitation=await recipient(principal,id)
      return serialized(invitation.organizationId,async()=>{
        await recipient(principal,id)
        await auth.api.rejectInvitation({headers:context.headers,body:{invitationId:id}})
        return {deleted:true as const}
      })
    }),
  }
}
