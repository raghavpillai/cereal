import type {MeetingContextDTO,NoteDTO} from "@meeting-notes/contracts"
import type {Capture,CompiledFixture,Fixture,NotesLevel} from "./types"
import {compileFixture,pattern,stableUuid,supportSegments,findTurn} from "./transcript"
import {standup} from "../fixtures/standup"
import {discovery} from "../fixtures/discovery"
import {oneOnOne} from "../fixtures/one-on-one"
import {interview} from "../fixtures/interview"
import {brainstorm} from "../fixtures/brainstorm"
import {planning} from "../fixtures/planning"
import {crosstalk} from "../fixtures/crosstalk"
import {numbers} from "../fixtures/numbers"

/** To add a meeting: write `fixtures/<name>.ts` and list it here. */
export const FIXTURES:ReadonlyArray<Fixture>=[standup,discovery,oneOnOne,interview,brainstorm,planning,crosstalk,numbers]
export const fixtureById=(id:string):Fixture=>{const found=FIXTURES.find(fixture=>fixture.id===id);if(!found)throw new Error(`Unknown fixture ${id}`);return found}
const compiled=new Map<string,CompiledFixture>()
export const compile=(fixture:Fixture,capture:Capture=fixture.capture):CompiledFixture=>{
  const key=`${fixture.id}:${capture}`
  if(!compiled.has(key))compiled.set(key,compileFixture(fixture,capture))
  return compiled.get(key)!
}
export const noteId=(fixture:Fixture)=>stableUuid("note",fixture.id)
export const isoDate=(now:Date,daysAgo:number)=>new Date(now.getTime()-daysAgo*86_400_000).toISOString().slice(0,10)
export const notesFor=(fixture:Fixture,level:NotesLevel)=>level==="none"?"":fixture.notes[level]

/** A reference summary built from the gold annotations. Used as the generated
 * document for scenarios that need one, so chat can be evaluated on a fixed
 * summary instead of whatever generation produced in that run. */
export const referenceSummary=(fixture:Fixture)=>[
  "# Decisions",...fixture.gold.decisions.map(item=>`- ${item.text}`),"",
  "# Action items",...fixture.gold.actionItems.map(item=>`- ${item.text}`),"",
  "# Key facts",...fixture.gold.facts.map(item=>`- ${item.text}`),
].join("\n")

export function fixtureNote(fixture:Fixture,options:{now:Date;level:NotesLevel;summary?:boolean;capture?:Capture;segments?:NoteDTO["transcriptSegments"];recording?:boolean}):NoteDTO {
  const built=compile(fixture,options.capture),date=isoDate(options.now,fixture.daysAgo),stamp=`${date}T16:00:00.000Z`
  const segments=options.segments??built.segments
  return {id:noteId(fixture),title:fixture.title,body:notesFor(fixture,options.level),enhanced:options.summary?referenceSummary(fixture):"",date,
    duration:options.segments?(segments.length?`${Math.floor(Math.max(...segments.map(s=>s.endMs??s.startMs))/60000)}:${String(Math.floor(Math.max(...segments.map(s=>s.endMs??s.startMs))/1000)%60).padStart(2,"0")}`:"0:00"):built.duration,
    createdAt:stamp,updatedAt:stamp,transcriptSegments:segments,recordingState:options.recording?"recording":"stopped",generationState:options.summary?"ready":"idle",
    transcriptRevision:segments.length?1:0,generatedRevision:options.summary?1:null}
}
export function meetingContextFor(fixture:Fixture,now:Date):MeetingContextDTO|null {
  if(!fixture.attendees)return null
  const date=isoDate(now,fixture.daysAgo)
  const people=fixture.attendees.map((person,index)=>{
    const [given,...rest]=person.name.split(" ")
    return {id:stableUuid("attendee",fixture.id,index),displayName:person.name,givenName:given??null,familyName:rest.join(" ")||null,email:person.email,nameSource:"calendar",isSelf:!!person.self,isOrganizer:!!person.self,isResource:false,responseStatus:"accepted"}
  })
  return {eventId:stableUuid("event",fixture.id),provider:"ics",title:fixture.title,calendarName:"Work",startAt:`${date}T16:00:00Z`,endAt:`${date}T17:30:00Z`,allDay:false,timeZone:"UTC",location:null,meetingUrl:null,eventUrl:null,
    organizer:people.find(person=>person.isSelf)??null,participants:people} as unknown as MeetingContextDTO
}

/** Short unrelated notes that make workspace search non-trivial. */
export function distractorNotes(now:Date):NoteDTO[] {
  const make=(key:string,title:string,daysAgo:number,body:string,enhanced:string):NoteDTO=>{
    const date=isoDate(now,daysAgo),stamp=`${date}T12:00:00.000Z`
    return {id:stableUuid("distractor",key),title,body,enhanced,date,duration:"",createdAt:stamp,updatedAt:stamp,transcriptSegments:[],recordingState:"idle",generationState:enhanced?"ready":"idle",transcriptRevision:0,generatedRevision:enhanced?0:null}
  }
  return [
    make("lunch-learn","Lunch and learn: spreadsheet shortcuts",2,"- pivot tables\n- keyboard shortcuts handout","# Topics\n- Pivot tables and lookup formulas.\n- A handout of keyboard shortcuts will be shared in the team folder."),
    make("office-move","Office move logistics",8,"movers booked\n- crates arrive the week before","# Plan\n- Movers are booked; crates arrive the week before the move.\n\n# Action items\n- Facilities to label desks."),
    make("book-club","Book club",18,"next book: the lighthouse one",""),
    make("vendor-intro","Intro call: Larkspur Catering",25,"- menus\n- minimum order","# Summary\n- Larkspur Catering presented seasonal menus and a minimum order of 20 lunches.\n- No decision was made."),
    make("old-retro","Ferry app retrospective",40,"- flaky tests again\n- release checklist","# What went well\n- Release checklist caught two issues.\n\n# Action items\n- Dario to quarantine flaky tests."),
  ]
}

/** Fail fast on authoring mistakes: unparsable scripts, unknown speakers,
 * invalid patterns, support quotes that do not match exactly one turn. */
export function validateFixtures():Array<{id:string;kind:string;capture:string;turns:number;segments:number;words:number;duration:string;questions:number}> {
  const ids=new Set<string>()
  return FIXTURES.map(fixture=>{
    if(ids.has(fixture.id))throw new Error(`Duplicate fixture ${fixture.id}`)
    ids.add(fixture.id)
    const built=compile(fixture)
    for(const capture of ["desktop","ios","stream"] as const)compile(fixture,capture)
    const patterns=[...fixture.gold.decisions,...fixture.gold.actionItems,...fixture.gold.facts].flatMap(item=>item.must)
      .concat(fixture.gold.actionItems.map(item=>item.ownerMust),fixture.gold.notSaid.flatMap(item=>item.forbid),fixture.questions.flatMap(question=>[...question.must,...(question.forbid??[])]))
    for(const source of patterns)pattern(source)
    const truth=built.turns.map(turn=>turn.text).join("\n")
    // A gold fact must be findable in what was actually said; a not-said claim must not be.
    for(const item of [...fixture.gold.decisions,...fixture.gold.facts])if(item.must.length===0)throw new Error(`${fixture.id}/${item.id}: no patterns`)
    const questionIds=new Set<string>()
    for(const question of fixture.questions){
      if(questionIds.has(question.id))throw new Error(`${fixture.id}: duplicate question ${question.id}`)
      questionIds.add(question.id)
      for(const quote of question.support??[])findTurn(built,quote)
      supportSegments(built,question.support)
      if(!question.must.every(source=>pattern(source).test(question.reference)))throw new Error(`${fixture.id}/${question.id}: the reference answer does not satisfy its own required patterns`)
      if((question.forbid??[]).some(source=>pattern(source).test(question.reference)))throw new Error(`${fixture.id}/${question.id}: the reference answer matches a forbidden pattern`)
    }
    void truth
    return {id:fixture.id,kind:fixture.kind,capture:fixture.capture,turns:built.turns.length,segments:built.segments.length,words:built.turns.reduce((sum,turn)=>sum+turn.text.split(/\s+/).length,0),duration:built.duration,questions:fixture.questions.length}
  })
}
