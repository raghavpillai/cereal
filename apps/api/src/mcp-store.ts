import {Schema} from "effect"
import type {Database} from "@meeting-notes/database"
import {MeetingContextDTO,type NoteDTO} from "@meeting-notes/contracts"
import {noteDTO} from "./prisma-repository"
import {SUMMARY_TEXT_SQL,withoutTranscriptLink} from "./transcript-link"
import {meetingSearchTerms,transcriptPassage} from "./meeting-search"

const DEFAULT_WORKSPACE="00000000-0000-4000-8000-000000000002"
export interface McpOwnerScope {userId:string;workspaceId:string}
export class McpStoreError extends Error {
  constructor(readonly code:"not_found"|"forbidden"|"conflict"|"invalid_arguments"|"unavailable",message:string){super(message);this.name="McpStoreError"}
}
const missing=()=>new McpStoreError("not_found","Meeting or attachment not found in this private workspace.")
const order=[{startMs:"asc" as const},{id:"asc" as const}]
const scoped=(scope:McpOwnerScope)=>({ownerId:scope.userId,organizationId:scope.workspaceId})
const cursor=(value:unknown)=>Buffer.from(JSON.stringify(value)).toString("base64url")
function decodeCursor(value:string|undefined):Record<string,unknown>|null {
  if(!value)return null
  try{if(value.length>1000)throw Error();const decoded:unknown=JSON.parse(Buffer.from(value,"base64url").toString());if(!decoded||typeof decoded!=="object"||Array.isArray(decoded))throw Error();return decoded as Record<string,unknown>}
  catch{throw new McpStoreError("invalid_arguments","Invalid or expired pagination cursor.")}
}
const pageOffset=(value:string|undefined,scope:McpOwnerScope,kind:string)=>{
  const parsed=decodeCursor(value)
  if(!parsed)return 0
  if(parsed.kind!==kind||parsed.userId!==scope.userId||parsed.workspaceId!==scope.workspaceId||!Number.isInteger(parsed.offset)||Number(parsed.offset)<0||Number(parsed.offset)>100_000)throw new McpStoreError("invalid_arguments","Invalid or expired pagination cursor.")
  return Number(parsed.offset)
}
const nextPage=(scope:McpOwnerScope,kind:string,offset:number)=>cursor({...scope,kind,offset})
function meetingContext(value:unknown) {
  try{
    const meeting=Schema.decodeUnknownSync(MeetingContextDTO)(value)
    const person=(p:typeof meeting.participants[number])=>({id:p.id,name:p.displayName,givenName:p.givenName,familyName:p.familyName,email:p.email,nameSource:p.nameSource,isSelf:p.isSelf,isOrganizer:p.isOrganizer,isResource:p.isResource,responseStatus:p.responseStatus})
    return {title:meeting.title,startAt:meeting.startAt,endAt:meeting.endAt,timeZone:meeting.timeZone,allDay:meeting.allDay,location:meeting.location,organizer:meeting.organizer?person(meeting.organizer):null,attendees:meeting.participants.map(person),attribution:"explicit_calendar_association"}
  }catch{return null}
}
const dateString=(value:Date)=>value.toISOString().slice(0,10)
export interface McpMeetingSearch {query?:string;from?:string;to?:string;attendee?:string;company?:string;limit:number;cursor?:string;includeTranscriptMatch?:boolean}

/** Creation helper also used by native API writes. Existing API reads remain
 * owner-only; only MCP additionally narrows to this explicit organization. */
export async function resolveNoteOrganization(db:Database,userId:string):Promise<string|null> {
  const saved=await db.userWorkspaceState.findUnique({where:{userId}})
  if(saved?.activeOrganizationId&&await db.member.findUnique({where:{organizationId_userId:{organizationId:saved.activeOrganizationId,userId}}}))return saved.activeOrganizationId
  const memberships=await db.member.findMany({where:{userId},orderBy:[{createdAt:"asc"},{id:"asc"}]})
  return memberships.find(member=>member.organizationId===DEFAULT_WORKSPACE)?.organizationId??memberships[0]?.organizationId??null
}

export function createMcpStore(db:Database) {
  async function assertScope(scope:McpOwnerScope) {
    if(!await db.member.findUnique({where:{organizationId_userId:{organizationId:scope.workspaceId,userId:scope.userId}}}))throw new McpStoreError("forbidden","Workspace membership is required.")
  }
  async function ownedNote(scope:McpOwnerScope,id:string) {
    await assertScope(scope)
    const note=await db.note.findFirst({where:{id,...scoped(scope)},include:{transcriptSegments:{orderBy:order},meetingContext:true,attachments:{select:{id:true,name:true,mimeType:true,createdAt:true}}}})
    if(!note)throw missing()
    return note
  }
  const metadata=(note:{id:string;title:string;date:Date;duration:string;transcriptRevision:number;meetingContext:{meeting:unknown}|null})=>({id:note.id,title:note.title,date:dateString(note.date),duration:note.duration,transcriptRevision:note.transcriptRevision,meeting:meetingContext(note.meetingContext?.meeting)})
  async function searchIds(scope:McpOwnerScope,input:McpMeetingSearch,transcripts=false) {
    await assertScope(scope)
    const values:unknown[]=[]
    const bind=(value:unknown)=>{values.push(value);return `$${values.length}`}
    const clauses=[`n."ownerId"=${bind(scope.userId)}::uuid`,`n."organizationId"=${bind(scope.workspaceId)}::uuid`]
    if(input.from)clauses.push(`n."date">=${bind(input.from)}::date`)
    if(input.to)clauses.push(`n."date"<=${bind(input.to)}::date`)
    const pattern=(value:string)=>`%${value.replace(/[\\%_]/g,"\\$&")}%`
    for(const value of [input.attendee,input.company].filter((v):v is string=>!!v)){
      const parameter=bind(pattern(value))
      clauses.push(`EXISTS(SELECT 1 FROM "NoteMeetingContext" mc, LATERAL jsonb_array_elements(CASE WHEN jsonb_typeof(mc.meeting->'participants')='array' THEN mc.meeting->'participants' ELSE '[]'::jsonb END) person WHERE mc."noteId"=n.id AND mc."userId"=n."ownerId" AND concat_ws(' ',person->>'displayName',person->>'givenName',person->>'familyName',person->>'email') ILIKE ${parameter} ESCAPE '\\')`)
    }
    const terms=meetingSearchTerms(input.query??"")
    if(terms.length){
      const alternatives=terms.map(term=>{
        const p=bind(pattern(term))
        return transcripts?`s.text ILIKE ${p} ESCAPE '\\'`:`(n.title ILIKE ${p} ESCAPE '\\' OR n.body ILIKE ${p} ESCAPE '\\' OR ${SUMMARY_TEXT_SQL("n")} ILIKE ${p} ESCAPE '\\'${input.includeTranscriptMatch?` OR EXISTS(SELECT 1 FROM "TranscriptSegment" t WHERE t."noteId"=n.id AND t.text ILIKE ${p} ESCAPE '\\')`:""})`
      })
      clauses.push(`(${alternatives.join(" OR ")})`)
    }else if(transcripts)return {ids:[] as string[],offset:0}
    const kind=transcripts?"transcripts":"meetings",offset=pageOffset(input.cursor,scope,kind)
    const query=transcripts?`SELECT s.id FROM "TranscriptSegment" s JOIN "Note" n ON n.id=s."noteId" WHERE ${clauses.join(" AND ")} ORDER BY n.date DESC,s."startMs",s.id LIMIT ${bind(input.limit+1)} OFFSET ${bind(offset)}`:`SELECT n.id FROM "Note" n WHERE ${clauses.join(" AND ")} ORDER BY n.date DESC,n.id DESC LIMIT ${bind(input.limit+1)} OFFSET ${bind(offset)}`
    // Only static SQL fragments and positional parameters are assembled above.
    const rows=await db.$queryRawUnsafe<Array<{id:string}>>(query,...values)
    return {ids:rows.map(row=>row.id),offset}
  }
  return {
    assertScope,
    async resolveWorkspace(userId:string) {
      const memberships=await db.member.findMany({where:{userId},include:{organization:true},orderBy:[{createdAt:"asc"},{id:"asc"}]})
      const activeId=await resolveNoteOrganization(db,userId)
      const workspaces=memberships.map(member=>({id:member.organizationId,name:member.organization.name,slug:member.organization.slug,role:member.role}))
      if(activeId)await db.userWorkspaceState.upsert({where:{userId},create:{userId,activeOrganizationId:activeId},update:{activeOrganizationId:activeId}})
      return {workspaces,activeWorkspace:workspaces.find(workspace=>workspace.id===activeId)??null}
    },
    async accountInfo(userId:string) {
      const user=await db.user.findUnique({where:{id:userId},select:{id:true,name:true,email:true}})
      if(!user)throw new McpStoreError("forbidden","Account unavailable.")
      return {user,...await this.resolveWorkspace(userId),ownershipPolicy:"private-owned-only",nativeBrowsePolicy:"Native clients may browse all of their own notes; MCP reads only the active workspace. Workspace membership never grants another person's private notes."}
    },
    async switchWorkspace(userId:string,workspaceId:string) {
      await db.$transaction(async tx=>{
        if(!await tx.member.findUnique({where:{organizationId_userId:{organizationId:workspaceId,userId}}}))throw new McpStoreError("forbidden","Workspace membership is required.")
        await tx.userWorkspaceState.upsert({where:{userId},create:{userId,activeOrganizationId:workspaceId},update:{activeOrganizationId:workspaceId}})
        await tx.session.updateMany({where:{userId},data:{activeOrganizationId:workspaceId}})
      })
      return this.resolveWorkspace(userId)
    },
    async listMeetings(scope:McpOwnerScope,input:McpMeetingSearch) {
      const {ids,offset}=await searchIds(scope,input),selected=ids.slice(0,input.limit)
      const rows=await db.note.findMany({where:{id:{in:selected},...scoped(scope)},select:{id:true,title:true,date:true,duration:true,transcriptRevision:true,meetingContext:{select:{meeting:true}}}})
      const byId=new Map(rows.map(note=>[note.id,metadata(note)]))
      return {meetings:selected.flatMap(id=>byId.get(id)?[byId.get(id)!]:[]),nextCursor:ids.length>input.limit?nextPage(scope,"meetings",offset+input.limit):null,workspaceId:scope.workspaceId,access:"private-owned-only"}
    },
    async getMeetings(scope:McpOwnerScope,ids:ReadonlyArray<string>) {
      const meetings=[]
      for(const id of ids){const note=await ownedNote(scope,id);meetings.push({...metadata(note),privateNotes:note.body,summary:withoutTranscriptLink(note.id,note.enhanced),generationState:note.generationState,generatedRevision:note.generatedRevision,attachments:note.attachments.map(file=>({...file,createdAt:file.createdAt.toISOString()}))})}
      return {meetings,workspaceId:scope.workspaceId,access:"private-owned-only"}
    },
    async searchTranscripts(scope:McpOwnerScope,input:McpMeetingSearch) {
      const {ids,offset}=await searchIds(scope,input,true),selected=ids.slice(0,input.limit)
      const rows=await db.transcriptSegment.findMany({where:{id:{in:selected},note:scoped(scope)},include:{note:{select:{id:true,title:true,date:true,transcriptRevision:true,meetingContext:{select:{meeting:true}}}}}})
      const byId=new Map(rows.map(segment=>[segment.id,{noteId:segment.note.id,title:segment.note.title,date:dateString(segment.note.date),meeting:meetingContext(segment.note.meetingContext?.meeting),segmentId:segment.id,channel:segment.channel,speakerId:segment.speakerId,speakerLabel:segment.speakerLabel,startMs:segment.startMs,endMs:segment.endMs,isFinal:segment.isFinal,quote:transcriptPassage(segment.text,meetingSearchTerms(input.query??""),1600),transcriptRevision:segment.note.transcriptRevision}]))
      return {passages:selected.flatMap(id=>byId.get(id)?[byId.get(id)!]:[]),nextCursor:ids.length>input.limit?nextPage(scope,"transcripts",offset+input.limit):null,workspaceId:scope.workspaceId}
    },
    async transcript(scope:McpOwnerScope,id:string,input:{limit:number;cursor?:string;startMs?:number;maxCharacters:number}) {
      await assertScope(scope)
      const note=await db.note.findFirst({where:{id,...scoped(scope)},select:{id:true,title:true,transcriptRevision:true,updatedAt:true}})
      if(!note)throw missing()
      const snapshot=note.updatedAt.toISOString(),parsed=decodeCursor(input.cursor)
      if(parsed&&(parsed.kind!=="full-transcript"||parsed.noteId!==id||parsed.userId!==scope.userId||parsed.workspaceId!==scope.workspaceId||parsed.snapshot!==snapshot||parsed.revision!==note.transcriptRevision||!Number.isInteger(parsed.index)||Number(parsed.index)<0||Number(parsed.index)>100_000||!Number.isInteger(parsed.offset)||Number(parsed.offset)<0||Number(parsed.offset)>1_000_000||!Number.isInteger(parsed.from)||Number(parsed.from)<0||Number(parsed.from)>2147483647))throw new McpStoreError("conflict","The transcript changed or this pagination cursor is invalid. Start reading again.")
      const index=Number(parsed?.index??0),initialOffset=Number(parsed?.offset??0),from=Number(parsed?.from??Math.max(0,(input.startMs??0)-30_000))
      const rows=await db.transcriptSegment.findMany({where:{noteId:id,...(from?{startMs:{gte:from}}:{})},orderBy:order,skip:index,take:input.limit+1})
      const segments=[];let budget=input.maxCharacters,nextCursor:string|null=null
      for(let row=0;row<rows.length;row++){
        const segment=rows[row]!,offset=row===0?initialOffset:0
        if(offset>segment.text.length||offset>0&&/[\uDC00-\uDFFF]/.test(segment.text[offset]!))throw new McpStoreError("invalid_arguments","Invalid transcript character cursor.")
        if(segments.length>=input.limit||budget<=0){nextCursor=cursor({...scope,kind:"full-transcript",noteId:id,snapshot,revision:note.transcriptRevision,index:index+row,offset,from});break}
        let end=Math.min(segment.text.length,offset+budget)
        if(end<segment.text.length&&/[\uD800-\uDBFF]/.test(segment.text[end-1]!))end--
        const text=segment.text.slice(offset,end);budget-=text.length
        segments.push({...segment,text,textOffset:offset,continued:end<segment.text.length})
        if(end<segment.text.length){nextCursor=cursor({...scope,kind:"full-transcript",noteId:id,snapshot,revision:note.transcriptRevision,index:index+row,offset:end,from});break}
      }
      return {noteId:id,title:note.title,transcriptRevision:note.transcriptRevision,segments,nextCursor,startsAroundMs:input.startMs??null,workspaceId:scope.workspaceId,speakerIdentityPolicy:"Speaker IDs are local to source channel and recording part; matching labels do not identify the same person."}
    },
    async updatePrivateNotes(scope:McpOwnerScope,id:string,text:string,position:"append"|"prepend") {
      await assertScope(scope)
      return db.$transaction(async tx=>{
        const rows=await tx.$queryRawUnsafe<Array<{id:string;body:string;generationState:string}>>('SELECT id,body,"generationState" FROM "Note" WHERE id=$1::uuid AND "ownerId"=$2::uuid AND "organizationId"=$3::uuid FOR UPDATE',id,scope.userId,scope.workspaceId)
        const current=rows[0];if(!current)throw missing()
        const body=current.body?(position==="prepend"?text+"\n\n"+current.body:current.body+"\n\n"+text):text
        if(body.length>1_000_000)throw new McpStoreError("conflict","The updated private notes would exceed their size limit.")
        const note=await tx.note.update({where:{id},data:{body,generationRun:null,...(current.generationState==="writing_notes"?{generationState:"idle"}:{})},select:{id:true,body:true,updatedAt:true}})
        return {noteId:id,privateNotes:note.body,updatedAt:note.updatedAt.toISOString(),summaryRegenerated:false}
      })
    },
    async noteForProcessing(scope:McpOwnerScope,id:string):Promise<NoteDTO> {return noteDTO(await ownedNote(scope,id))},
    async getAttachment(scope:McpOwnerScope,id:string) {
      await assertScope(scope)
      const file=await db.noteAttachment.findFirst({where:{id,note:scoped(scope)}})
      if(!file)throw missing()
      return file
    },
    /** Owner-authenticated REST upload hooks can call this; MIME signatures and
     * byte limits are checked before any bytes are persisted or returned. */
    async storeAttachment(scope:McpOwnerScope,noteId:string,name:string,mimeType:string,data:Uint8Array) {
      await assertScope(scope)
      const valid=data.length>0&&data.length<=5*1024*1024&&name.length>0&&name.length<=255&&(
        mimeType==="image/png"&&Buffer.from(data.subarray(0,8)).equals(Buffer.from([137,80,78,71,13,10,26,10]))||
        mimeType==="image/jpeg"&&data[0]===255&&data[1]===216&&data[2]===255||
        mimeType==="image/webp"&&Buffer.from(data.subarray(0,4)).toString()==="RIFF"&&Buffer.from(data.subarray(8,12)).toString()==="WEBP")
      if(!valid)throw new McpStoreError("invalid_arguments","Use a PNG, JPEG or WebP image of at most 5 MiB.")
      if(!await db.note.findFirst({where:{id:noteId,...scoped(scope)},select:{id:true}}))throw missing()
      return db.noteAttachment.create({data:{noteId,name,mimeType,data:Buffer.from(data)},select:{id:true,noteId:true,name:true,mimeType:true,createdAt:true}})
    },
    async agentBrief(scope:McpOwnerScope) {
      await assertScope(scope)
      const brief=await db.agentBrief.findUnique({where:{userId_organizationId:{userId:scope.userId,organizationId:scope.workspaceId}}})
      return {body:brief?.body??"",updatedAt:brief?.updatedAt.toISOString()??null,source:"explicit_user_profile",automaticallyInferred:false,workspaceId:scope.workspaceId}
    },
    async updateAgentBrief(scope:McpOwnerScope,body:string) {
      await assertScope(scope)
      if(body.length>20_000)throw new McpStoreError("invalid_arguments","Agent brief must be at most 20,000 characters.")
      const brief=await db.agentBrief.upsert({where:{userId_organizationId:{userId:scope.userId,organizationId:scope.workspaceId}},create:{userId:scope.userId,organizationId:scope.workspaceId,body},update:{body}})
      return {body:brief.body,updatedAt:brief.updatedAt.toISOString(),source:"explicit_user_profile",automaticallyInferred:false}
    },
  }
}
export type McpStore=ReturnType<typeof createMcpStore>
