import {afterAll,beforeAll,describe,expect,test} from "bun:test"
import {randomBytes} from "node:crypto"
import {Effect} from "effect"
import {hashPassword} from "better-auth/crypto"
import {makeAuthenticationService,BOOTSTRAP_OWNER_ID} from "../src/auth"
import {DEFAULT_ORGANIZATION_ID} from "../src/organizations"
import {makePrismaRepository} from "../src/prisma-repository"
import {makeApiHandler} from "../src/api"
import {withAccountMutation} from "../src/account-mutations"

const enabled=process.env.RUN_AUTH_PG_TESTS==="1"&&!!process.env.DATABASE_URL&&/^\/mn_test_[a-f0-9]+$/.test(new URL(process.env.DATABASE_URL).pathname)
/** Uses only the dedicated database created by scripts/test-postgres-isolated.ts. */
describe.skipIf(!enabled)("real Better Auth Organization/Admin plugin façade",()=>{
  const resources=enabled?makePrismaRepository(process.env.DATABASE_URL!):null,db=resources?.database
  const operator="organization-test-operator-token-at-least-32-characters",base="http://localhost:3000"
  const authentication=db?makeAuthenticationService(db,operator,{secret:randomBytes(32).toString("base64url"),baseURL:base}):null
  const api=resources?makeApiHandler(resources.repository,operator,{authentication:authentication!}):null
  const password="org-test-password-"+randomBytes(16).toString("hex")
  const suffix=randomBytes(5).toString("hex")
  type FixtureName="owner"|"admin"|"member"|"other"|"decline"
  const users={} as Record<FixtureName,{id:string;email:string;token:string;cookie:string;sessionId:string}>
  let orgId="",secondId="",adminMember="",memberId="",adminInvitation="",memberInvitation="",privateNote=""
  const request=(path:string,method="GET",body?:unknown,token=users.owner?.token,extra:Record<string,string>={})=>api!.handler(new Request(base+path,{
    method,headers:{...(token?{Authorization:"Bearer "+token}:{}),...(body===undefined?{}:{"Content-Type":"application/json"}),...extra},
    ...(body===undefined?{}:{body:JSON.stringify(body)}),
  }))
  const org=(suffix="")=>"/api/v1/organizations/"+orgId+suffix
  beforeAll(async()=>{
    for(const name of ["owner","admin","member","other","decline"] as const){
      const email=`org-${name}-${suffix}@example.test`
      // The instance has exactly one owner. Reuse its reserved fixture instead of
      // weakening the single-owner database constraint. This suite runs only in
      // the disposable integration-test DB; these are test-only credentials.
      const user=name==="owner"
        ? await db!.user.update({where:{id:BOOTSTRAP_OWNER_ID},data:{name:"Org owner",email}})
        : await Effect.runPromise(authentication!.createUser({name:"Org "+name,email,password}))
      if(name==="owner")await db!.account.upsert({where:{providerId_accountId:{providerId:"credential",accountId:BOOTSTRAP_OWNER_ID}},create:{id:crypto.randomUUID(),providerId:"credential",accountId:BOOTSTRAP_OWNER_ID,userId:BOOTSTRAP_OWNER_ID,password:await hashPassword(password)},update:{password:await hashPassword(password)}})
      const login=await request("/api/auth/sign-in/email","POST",{email,password},"")
      expect(login.status).toBe(200)
      const token=login.headers.get("set-auth-token")!
      expect(token).toBeTruthy()
      const principal=await authentication!.resolve(new Headers({Authorization:"Bearer "+token}))
      expect(principal?.sessionId).toBeTruthy()
      users[name]={id:user.id,email,token,sessionId:principal!.sessionId!,cookie:login.headers.getSetCookie().map(c=>c.split(";")[0]).join("; ")}
    }
  })
  afterAll(async()=>{await api?.dispose();await resources?.close()})

  test("migration/default provisioning adds memberships without moving existing private notes",async()=>{
    const defaultWorkspace=await db!.organization.findUniqueOrThrow({where:{id:DEFAULT_ORGANIZATION_ID}})
    expect(defaultWorkspace.slug).toBe("default")
    for(const user of Object.values(users))expect(await db!.member.count({where:{userId:user.id,organizationId:DEFAULT_ORGANIZATION_ID}})).toBe(1)
    const response=await request("/api/v1/notes","POST",{title:"Organization membership does not share this",body:"private fixture"})
    expect(response.status).toBe(200);privateNote=(await response.json()).id
    expect((await db!.note.findUniqueOrThrow({where:{id:privateNote}})).ownerId).toBe(users.owner.id)
    const listing=await(await request("/api/v1/organizations")).json()
    expect(listing.activeOrganizationId).toBe(DEFAULT_ORGANIZATION_ID)
    expect(listing.organizations.find((o:any)=>o.id===DEFAULT_ORGANIZATION_ID).role).toBe("owner")
  })
  test("raw plugin routes/signup/teams are denied and only signed instance owners create workspaces",async()=>{
    for(const [path,method] of [["/api/auth/sign-up/email","POST"],["/api/auth/admin/create-user","POST"],["/api/auth/admin/set-role","POST"],["/api/auth/organization/create","POST"],["/api/auth/organization/invite-member","POST"],["/api/auth/organization/list","GET"],["/api/auth/organization/create-team","POST"]])
      expect((await request(path!,method!,method==="GET"?undefined:{email:users.other.email,role:"owner"})).status).toBe(404)
    expect((await request("/api/v1/organizations","POST",{name:"Unauthorized",slug:"blocked-"+suffix},users.member.token)).status).toBe(403)
    expect((await request("/api/v1/organizations","POST",{name:"Bootstrap is not a session",slug:"bootstrap-"+suffix},operator)).status).toBe(403)
    const created=await request("/api/v1/organizations","POST",{name:"Engineering "+suffix,slug:"engineering-"+suffix})
    expect(created.status).toBe(200);const payload=await created.json();orgId=payload.id;expect(payload.role).toBe("owner")
    const second=await request("/api/v1/organizations","POST",{name:"Other workspace",slug:"other-"+suffix})
    expect(second.status).toBe(200);secondId=(await second.json()).id
    expect((await request("/api/v1/organizations","POST",{name:"Duplicate",slug:"engineering-"+suffix})).status).toBe(400)
  })
  test("workspace selection persists in the actual session, never in a bootstrap fiction",async()=>{
    const result=await request("/api/v1/organizations/active","POST",{organizationId:orgId})
    expect(result.status).toBe(200);expect((await result.json()).activeOrganizationId).toBe(orgId)
    expect((await db!.session.findUniqueOrThrow({where:{id:users.owner.sessionId}})).activeOrganizationId).toBe(orgId)
    expect((await(await request("/api/v1/organizations")).json()).activeOrganizationId).toBe(orgId)
    expect((await request("/api/v1/organizations/active","POST",{organizationId:orgId},users.other.token)).status).toBe(404)
    expect((await request("/api/v1/organizations/active","POST",{organizationId:orgId},operator)).status).toBe(403)
    expect((await request("/api/v1/organizations/active","POST",{organizationId:orgId},"",{Cookie:users.owner.cookie})).status).toBe(403)
    expect((await request("/api/v1/organizations/active","POST",{organizationId:orgId},"",{Cookie:users.owner.cookie,Origin:base})).status).toBe(200)
  })
  test("real pending invitations require preprovisioned accounts and exact authenticated recipients",async()=>{
    expect((await request(org("/invitations"),"POST",{email:"unprovisioned@example.test",role:"member"})).status).toBe(400)
    const admin=await request(org("/invitations"),"POST",{email:users.admin.email,role:"admin"})
    expect(admin.status).toBe(200);adminInvitation=(await admin.json()).id
    const member=await request(org("/invitations"),"POST",{email:users.member.email,role:"member"})
    expect(member.status).toBe(200);memberInvitation=(await member.json()).id
    const incoming=await(await request("/api/v1/organization-invitations","GET",undefined,users.admin.token)).json()
    expect(incoming.invitations.map((i:any)=>i.id)).toEqual([adminInvitation])
    expect((await request("/api/v1/organization-invitations/"+adminInvitation+"/accept","POST",undefined,users.other.token)).status).toBe(404)
    expect((await request("/api/v1/organization-invitations/"+adminInvitation+"/accept","POST",undefined,"")).status).toBe(401)
    for(const [name,id]of[["admin",adminInvitation],["member",memberInvitation]]){
      const response=await request("/api/v1/organization-invitations/"+id+"/accept","POST",undefined,users[name as FixtureName].token)
      expect(response.status).toBe(200);expect((await response.json()).role).toBe(name)
    }
    expect((await db!.user.findUniqueOrThrow({where:{id:users.admin.id}})).role).toBe("user")
    expect((await db!.user.findUniqueOrThrow({where:{id:users.admin.id}})).emailVerified).toBe(false)
    expect((await request("/api/v1/organization-invitations/"+adminInvitation+"/accept","POST",undefined,users.admin.token)).status).toBe(409)
    adminMember=(await db!.member.findUniqueOrThrow({where:{organizationId_userId:{organizationId:orgId,userId:users.admin.id}}})).id
    memberId=(await db!.member.findUniqueOrThrow({where:{organizationId_userId:{organizationId:orgId,userId:users.member.id}}})).id
  })
  test("members read only their own workspace; org admins cannot mint peers, owners, or instance admins",async()=>{
    const details=await request(org(),"GET",undefined,users.member.token)
    expect(details.status).toBe(200);expect((await details.json()).members).toHaveLength(3)
    expect((await request("/api/v1/organizations/"+secondId,"GET",undefined,users.member.token)).status).toBe(404)
    expect((await request(org(),"PATCH",{name:"Forbidden rename"},users.member.token)).status).toBe(403)
    expect((await request(org(),"PATCH",{name:"Engineering renamed"},users.admin.token)).status).toBe(200)
    expect((await request(org("/invitations"),"POST",{email:users.other.email,role:"admin"},users.admin.token)).status).toBe(403)
    expect((await request(org("/members/"+memberId),"PATCH",{role:"admin"},users.admin.token)).status).toBe(403)
    expect((await request(org("/members/"+memberId),"PATCH",{role:"owner"})).status).toBe(400)
    expect((await request(org("/members/"+adminMember),"DELETE",undefined,users.admin.token)).status).toBe(403)
    expect((await request("/api/v1/admin/users/"+users.member.id+"/role","PATCH",{role:"admin"},users.admin.token)).status).toBe(403)
    const owner=(await db!.member.findUniqueOrThrow({where:{organizationId_userId:{organizationId:orgId,userId:users.owner.id}}})).id
    expect((await request(org("/members/"+owner),"PATCH",{role:"member"})).status).toBe(403)
    expect((await request(org("/members/"+owner),"DELETE")).status).toBe(409)
    const concurrent=await Promise.all([1,2].map(()=>request(org("/members/"+owner),"DELETE")))
    expect(concurrent.every(r=>r.status===409)).toBe(true)
    expect(await db!.member.count({where:{organizationId:orgId,role:"owner"}})).toBe(1)
  })
  test("bursts of management requests preserve pool capacity and finish without deadlock",async()=>{
    const responses=await Promise.all(Array.from({length:12},(_,index)=>request(org(),"PATCH",{name:"Engineering "+index})))
    expect(responses.every(response=>response.status===200)).toBe(true)
    expect(await db!.member.count({where:{organizationId:orgId,role:"owner"}})).toBe(1)
  })
  test("owner promotes/demotes org admin without changing instance role or private content access",async()=>{
    expect((await request(org("/members/"+memberId),"PATCH",{role:"admin"})).status).toBe(200)
    expect((await db!.user.findUniqueOrThrow({where:{id:users.member.id}})).role).toBe("user")
    for(const token of[users.admin.token,users.member.token]){
      expect((await request("/api/v1/notes/"+privateNote,"GET",undefined,token)).status).toBe(404)
      expect((await request("/api/v1/notes/"+privateNote+"/shares","GET",undefined,token)).status).toBe(404)
      expect((await request("/api/v1/notes/"+privateNote+"/chat","POST",{message:"Read private note",history:[]},token)).status).toBe(404)
      expect((await request("/api/v1/notes/"+privateNote+"/audio/chunks","GET",undefined,token)).status).toBe(404)
    }
    expect((await request(org("/members/"+memberId),"PATCH",{role:"member"})).status).toBe(200)
  })
  test("cancel/reject/expired invitations are real plugin state and cannot be accepted or rebound",async()=>{
    const create=async(email:string,token=users.owner.token)=>{
      const response=await request(org("/invitations"),"POST",{email,role:"member"},token)
      expect(response.status).toBe(200);return(await response.json()).id as string
    }
    const canceled=await create(users.other.email,users.admin.token)
    expect((await request("/api/v1/organizations/"+secondId+"/invitations/"+canceled,"DELETE")).status).toBe(404)
    expect((await request(org("/invitations/"+canceled),"DELETE",undefined,users.admin.token)).status).toBe(200)
    expect((await db!.invitation.findUniqueOrThrow({where:{id:canceled}})).status).toBe("canceled")
    expect((await request("/api/v1/organization-invitations/"+canceled+"/accept","POST",undefined,users.other.token)).status).toBe(409)
    const rejected=await create(users.decline.email)
    expect((await request("/api/v1/organization-invitations/"+rejected+"/reject","POST",undefined,users.decline.token)).status).toBe(200)
    expect((await db!.invitation.findUniqueOrThrow({where:{id:rejected}})).status).toBe("rejected")
    const expired=await create(users.other.email)
    await db!.invitation.update({where:{id:expired},data:{expiresAt:new Date(Date.now()-1000)}})
    expect((await request("/api/v1/organization-invitations/"+expired+"/accept","POST",undefined,users.other.token)).status).toBe(409)
    const incoming=await(await request("/api/v1/organization-invitations","GET",undefined,users.other.token)).json()
    expect(incoming.invitations.find((i:any)=>i.id===expired).status).toBe("expired")
  })
  test("selection and removal serialize so a racing selection cannot restore a removed workspace",async()=>{
    let unlock!:()=>void,locked!:()=>void
    const gate=new Promise<void>(resolve=>{unlock=resolve}),entered=new Promise<void>(resolve=>{locked=resolve})
    const blocker=withAccountMutation(db!,"workspace:"+orgId,async()=>{locked();await gate})
    await entered
    let selectionFinished=false
    const selecting=request("/api/v1/organizations/active","POST",{organizationId:orgId},users.member.token).then(response=>{selectionFinished=true;return response})
    // An ordinary request already finishes well within this window in this
    // isolated fixture. A selection must instead wait behind the held management
    // lock, rather than passing the plugin membership check independently.
    await Bun.sleep(100)
    const removing=request(org("/members/"+memberId),"DELETE",undefined,users.admin.token)
    try { expect(selectionFinished).toBe(false) } finally { unlock() }
    await blocker
    expect((await selecting).status).toBe(200)
    expect((await removing).status).toBe(200)
    expect(await db!.member.findUnique({where:{id:memberId}})).toBeNull()
    expect((await db!.session.findFirstOrThrow({where:{userId:users.member.id}})).activeOrganizationId).toBeNull()
    expect((await request(org(),"GET",undefined,users.member.token)).status).toBe(404)
    expect((await request("/api/v1/organizations/active","POST",{organizationId:orgId},users.member.token)).status).toBe(404)
    expect((await db!.note.findUniqueOrThrow({where:{id:privateNote}})).ownerId).toBe(users.owner.id)
  })
  test("instance Admin plugin uses explicit permissions with session-based list/create and owner-only role grants",async()=>{
    expect((await request("/api/v1/admin/users/"+users.admin.id+"/role","PATCH",{role:"admin"})).status).toBe(200)
    expect((await request("/api/v1/admin/users","GET",undefined,users.admin.token)).status).toBe(200)
    const created=await request("/api/v1/admin/users","POST",{name:"Admin provisioned",email:`new-${suffix}@example.test`,password},users.admin.token)
    expect(created.status).toBe(200);expect((await created.json()).role).toBe("user")
    expect((await request("/api/v1/admin/users/"+users.other.id+"/role","PATCH",{role:"admin"},users.admin.token)).status).toBe(403)
    expect((await request("/api/v1/admin/users/"+users.admin.id+"/role","PATCH",{role:"user"})).status).toBe(200)
    expect((await request("/api/v1/admin/users","GET",undefined,users.admin.token)).status).toBe(403)
  })
})
