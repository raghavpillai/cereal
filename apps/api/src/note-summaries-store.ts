/**
 * The notes list and notes search in PostgreSQL. Every statement starts from the
 * caller's own notes (`ownerId`, and the workspace when the repository is scoped
 * to one), exactly like the existing scoped reads; no statement can name
 * another account's note. Values reach SQL only as bound parameters.
 *
 * The rules (order, filters, matching, weights) are the ones in
 * `note-summaries.ts`; a PostgreSQL test runs both over the same notes.
 */
import {Schema} from "effect"
import {SUMMARY_TEXT_SQL} from "./transcript-link"
import type {Database} from "@meeting-notes/database"
import {NoteSearchPageDTO,NoteSummariesPageDTO} from "@meeting-notes/contracts"
import type {NoteSearchField,NoteSummaryDTO} from "@meeting-notes/contracts"
import {EXCERPT_SOURCE_CHARACTERS,FIELD_WEIGHT,PHRASE_WEIGHT,SEARCH_FIELDS,attendeeNames,decodeMeeting,listCursor,noteSummary,searchCursor,searchResult,type ListCursor,type NoteListInput,type NoteSearchInput} from "./note-summaries"

interface SummaryRow {
  id:string;title:string;date:Date;duration:string;createdAt:Date;updatedAt:Date;recordingState:string;generationState:string
  transcriptRevision:number;generatedRevision:number|null;organizationId:string|null
  enhancedHead:string;bodyHead:string;hasSummary:boolean;hasBody:boolean;speakerCount:number;meeting:unknown
}
/** Numbered parameters, each with its cast, so a statement can be assembled from fragments. */
function statement() {
  const values:unknown[]=[]
  return{values,bind:(value:unknown,type:string)=>{values.push(value);return`$${values.length}::${type}`}}
}
/** The generated notes as previews and search read them: without the chat link line (transcript-link.ts). */
const SUMMARY=SUMMARY_TEXT_SQL("n")
const like=(text:string)=>`%${text.replace(/[\\%_]/g,"\\$&")}%`
const HAS_TEXT=String.raw`~ '\S'`
const SPOKEN=`EXISTS (SELECT 1 FROM "TranscriptSegment" spoken WHERE spoken."noteId"=n."id" AND spoken."text" ${HAS_TEXT})`
const SUMMARY_COLUMNS=`n."id",n."title",n."date",n."duration",n."createdAt",n."updatedAt",n."recordingState",n."generationState",n."transcriptRevision",n."generatedRevision",n."organizationId",
  left(${SUMMARY},${EXCERPT_SOURCE_CHARACTERS}) AS "enhancedHead",left(n."body",${EXCERPT_SOURCE_CHARACTERS}) AS "bodyHead",
  (n."enhanced" ${HAS_TEXT}) AS "hasSummary",(n."body" ${HAS_TEXT}) AS "hasBody",
  (SELECT count(DISTINCT (voice."channel",voice."speakerId"))::int FROM "TranscriptSegment" voice WHERE voice."noteId"=n."id" AND voice."text" ${HAS_TEXT}) AS "speakerCount",
  context."meeting"`
const CONTEXT_JOIN=`LEFT JOIN "NoteMeetingContext" context ON context."noteId"=n."id" AND context."userId"=n."ownerId"`
const ORDER=`"date" DESC,"createdAt" DESC,"id" DESC`
const dayOf=(date:Date)=>date.toISOString().slice(0,10)

export function makePrismaNoteSummaries(db:Database) {
  const scope=(sql:ReturnType<typeof statement>,userId:string,organizationId:string|undefined,filter:NoteListInput["filter"])=>
    `n."ownerId"=${sql.bind(userId,"uuid")}${organizationId?` AND n."organizationId"=${sql.bind(organizationId,"uuid")}`:""}${filter==="summarized"?` AND n."enhanced" ${HAS_TEXT}`:filter==="transcribed"?` AND ${SPOKEN}`:""}`
  const summaries=(rows:ReadonlyArray<SummaryRow>):Array<{summary:NoteSummaryDTO;people:string[]}>=>{
    return rows.map(row=>{
      const meeting=decodeMeeting(row.meeting)
      return{people:attendeeNames(meeting),summary:noteSummary({id:row.id,title:row.title,date:dayOf(row.date),duration:row.duration,createdAt:row.createdAt.toISOString(),updatedAt:row.updatedAt.toISOString(),
        recordingState:row.recordingState,generationState:row.generationState,transcriptRevision:row.transcriptRevision,generatedRevision:row.generatedRevision,
        hasBody:row.hasBody,hasSummary:row.hasSummary,speakerCount:row.speakerCount,enhanced:row.enhancedHead,body:row.bodyHead,complete:false,
        meeting,organizationId:row.organizationId})}
    })
  }
  // The instant is bound as a Date, the way Prisma binds the ones it writes. Bound as text
  // with a zone it would be read in the session's time zone and miss rows on any server not set to UTC.
  const after=(sql:ReturnType<typeof statement>,cursor:ListCursor,prefix:string)=>
    `(${prefix}"date",${prefix}"createdAt",${prefix}"id") < (${sql.bind(cursor.date,"date")},${sql.bind(new Date(cursor.createdAt),"timestamptz")},${sql.bind(cursor.id,"uuid")})`

  return{
    async list(userId:string,organizationId:string|undefined,input:NoteListInput) {
      const page=statement(),count=statement()
      const [rows,total]=await Promise.all([
        db.$queryRawUnsafe<SummaryRow[]>(`SELECT ${SUMMARY_COLUMNS} FROM "Note" n ${CONTEXT_JOIN} WHERE ${scope(page,userId,organizationId,input.filter)}${input.cursor?` AND ${after(page,input.cursor,"n.")}`:""} ORDER BY n."date" DESC,n."createdAt" DESC,n."id" DESC LIMIT ${input.limit+1}`,...page.values),
        db.$queryRawUnsafe<Array<{total:number}>>(`SELECT count(*)::int AS "total" FROM "Note" n WHERE ${scope(count,userId,organizationId,input.filter)}`,...count.values),
      ])
      const shown=summaries(rows.slice(0,input.limit)),last=shown[shown.length-1]?.summary
      return Schema.decodeUnknownSync(NoteSummariesPageDTO)({notes:shown.map(row=>row.summary),nextCursor:rows.length>input.limit&&last?listCursor(last,input.filter):null,total:total[0]?.total??0})
    },
    async search(userId:string,organizationId:string|undefined,input:NoteSearchInput) {
      if(!input.terms.length)return{results:[],nextCursor:null,total:0,terms:[]} satisfies typeof NoteSearchPageDTO.Type
      const sql=statement(),where=scope(sql,userId,organizationId,input.filter)
      const patterns=input.terms.map(term=>sql.bind(like(term),"text"))
      const invited=`jsonb_array_elements(CASE WHEN jsonb_typeof(n."meeting"->'participants')='array' THEN n."meeting"->'participants' ELSE '[]'::jsonb END)`
      const inField:Record<NoteSearchField,(pattern:string)=>string>={
        title:pattern=>`n."title" ILIKE ${pattern}`,summary:pattern=>`${SUMMARY} ILIKE ${pattern}`,notes:pattern=>`n."body" ILIKE ${pattern}`,
        transcript:pattern=>`EXISTS (SELECT 1 FROM "TranscriptSegment" said WHERE said."noteId"=n."id" AND said."text" ILIKE ${pattern})`,
        people:pattern=>`EXISTS (SELECT 1 FROM ${invited} person WHERE coalesce(person->>'isResource','false')<>'true' AND (btrim(person->>'displayName') ILIKE ${pattern} OR btrim(person->>'email') ILIKE ${pattern}))`,
      }
      // Cheapest test first: a transcript is read only when the written fields and the invitees do not already hold the word.
      const anywhere:ReadonlyArray<NoteSearchField>=["title","summary","notes","people","transcript"]
      const every=patterns.map(pattern=>`(${anywhere.map(field=>inField[field](pattern)).join(" OR ")})`).join(" AND ")
      const flag=(field:NoteSearchField,term:number)=>`"${field}${term}"`
      const flags=patterns.flatMap((pattern,term)=>SEARCH_FIELDS.map(field=>`(${inField[field](pattern)}) AS ${flag(field,term)}`))
      const weights=patterns.flatMap((_,term)=>SEARCH_FIELDS.map(field=>`${FIELD_WEIGHT[field]}*${flag(field,term)}::int`))
      let together=""
      if(input.terms.length>1){
        const whole=sql.bind(like(input.query),"text")
        flags.push(`(n."title" ILIKE ${whole}) AS "titlePhrase"`,`(${SUMMARY} ILIKE ${whole} OR n."body" ILIKE ${whole}) AS "documentPhrase"`)
        together=`+${PHRASE_WEIGHT.title}*"titlePhrase"::int+${PHRASE_WEIGHT.document}*"documentPhrase"::int`
      }
      const matched=SEARCH_FIELDS.map(field=>`(${patterns.map((_,term)=>flag(field,term)).join(" OR ")}) AS "in_${field}"`)
      const cursor=input.cursor?`WHERE "score"<${sql.bind(input.cursor.score,"int")} OR ("score"=${sql.bind(input.cursor.score,"int")} AND ${after(sql,input.cursor,"")})`:""
      const found=await db.$queryRawUnsafe<Array<{id:string;score:number;total:number}&Record<`in_${NoteSearchField}`,boolean>>>(
        // A note is a candidate when every word is somewhere in it. The conditions are
        // tested in turn, so a note that lacks one word is not read for the next, and the
        // per-field flags that make the score are worked out for candidates alone.
        `WITH flagged AS (
           SELECT n."id",n."date",n."createdAt",${flags.join(",")}
           FROM (SELECT note.*,context."meeting" FROM "Note" note LEFT JOIN "NoteMeetingContext" context ON context."noteId"=note."id" AND context."userId"=note."ownerId") n
           WHERE ${where} AND ${every}
         ), scored AS (
           SELECT "id","date","createdAt",(${weights.join("+")}${together})::int AS "score",${matched.join(",")} FROM flagged
         )
         SELECT counted."total",page.* FROM (SELECT count(*)::int AS "total" FROM scored) counted
         LEFT JOIN LATERAL (SELECT * FROM scored ${cursor} ORDER BY "score" DESC,${ORDER} LIMIT ${input.limit+1}) page ON true
         ORDER BY page."score" DESC,page."date" DESC,page."createdAt" DESC,page."id" DESC`,...sql.values)
      // The count comes back even when the page is empty, as one row with no note in it.
      const matches=found.filter(row=>row.id!==null),page=matches.slice(0,input.limit),ids=page.map(row=>row.id)
      if(!ids.length)return Schema.decodeUnknownSync(NoteSearchPageDTO)({results:[],nextCursor:null,total:found[0]?.total??0,terms:[...input.terms]})
      // Only the rows on this page are read further, and a transcript only where
      // neither the summary nor the typed notes can show the match.
      const text=statement()
      const [rows,documents]=await Promise.all([
        db.$queryRawUnsafe<SummaryRow[]>(`SELECT ${SUMMARY_COLUMNS} FROM "Note" n ${CONTEXT_JOIN} WHERE n."ownerId"=$1::uuid AND n."id"=ANY($2::uuid[])`,userId,ids),
        db.$queryRawUnsafe<Array<{id:string;enhanced:string|null;body:string|null}>>(
          `SELECT n."id",CASE WHEN ${input.terms.map(term=>`${SUMMARY} ILIKE ${text.bind(like(term),"text")}`).join(" OR ")} THEN ${SUMMARY} END AS "enhanced",
             CASE WHEN ${input.terms.map(term=>`n."body" ILIKE ${text.bind(like(term),"text")}`).join(" OR ")} THEN n."body" END AS "body"
           FROM "Note" n WHERE n."ownerId"=${text.bind(userId,"uuid")} AND n."id"=ANY(${text.bind(ids,"uuid[]")})`,...text.values),
      ])
      const heard=page.filter(row=>row.in_transcript&&!row.in_summary&&!row.in_notes).map(row=>row.id),speech=statement()
      const segments=heard.length?await db.$queryRawUnsafe<Array<{noteId:string;id:string;text:string;startMs:number}>>(
        `SELECT DISTINCT ON (said."noteId") said."noteId",said."id",said."text",said."startMs" FROM "TranscriptSegment" said
         WHERE said."noteId"=ANY(${speech.bind(heard,"uuid[]")}) AND (${input.terms.map(term=>`said."text" ILIKE ${speech.bind(like(term),"text")}`).join(" OR ")})
         ORDER BY said."noteId",said."startMs",said."id"`,...speech.values):[]
      const summarized=summaries(rows),byId=new Map(summarized.map(row=>[row.summary.id,row]))
      const written=new Map(documents.map(row=>[row.id,row])),segmentFor=new Map(segments.map(row=>[row.noteId,row]))
      const results=page.flatMap(row=>{
        const note=byId.get(row.id)
        if(!note)return[]
        const segment=segmentFor.get(row.id)
        return[searchResult(note.summary,SEARCH_FIELDS.filter(field=>row[`in_${field}`]),{title:note.summary.title,enhanced:written.get(row.id)?.enhanced??"",body:written.get(row.id)?.body??"",people:note.people,
          segment:segment?{id:segment.id,text:segment.text,startMs:segment.startMs}:null},input.terms)]
      })
      const last=page[page.length-1]!,position=byId.get(last.id)?.summary
      return Schema.decodeUnknownSync(NoteSearchPageDTO)({results,nextCursor:matches.length>input.limit&&position?searchCursor({score:last.score,date:position.date,createdAt:position.createdAt,id:position.id},input):null,total:found[0]?.total??0,terms:[...input.terms]})
    },
  }
}
