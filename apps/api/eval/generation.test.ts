/** Offline checks of the generation-only evaluation: the hard-case fixtures are
 * well-formed and the structure and owner checks behave. No network.
 *   bun test apps/api/eval/generation.test.ts */
import {expect,test} from "bun:test"
import {GENERATION_FIXTURES,HELD_OUT,SPARSE_UNNOTED,TUNED,rosterOf,validateGenerationFixtures} from "./lib/generation-fixtures"
import {FIXTURES,compile} from "./lib/fixtures"
import {generationFormatLines,generationStyle,isTitleCase,maxHeadingsFor,noManufacturedActions,ownerPosition,ownerVerdict,parseNotes,styleMetrics,typedNotesKept,wordBudget} from "./lib/generation-checks"
import {day} from "./lib/generation-patterns"
import {selectScenarios} from "./run"
import type {Fixture} from "./lib/types"
import {titleChecks} from "./lib/title-checks"
import {fixtureMeetings,runTitle} from "./titles"
import {stubTransport} from "./lib/instrument"

test("hard-case fixtures compile, and the long ones really are long",()=>{
  const rows=validateGenerationFixtures()
  expect(rows.length).toBe(TUNED.length+HELD_OUT.length)
  expect(rows.find(row=>row.id==="hard-long")!.words).toBeGreaterThan(12_000)
  expect(rows.find(row=>row.id==="held-long")!.words).toBeGreaterThan(12_000)
  expect(rows.find(row=>row.id==="hard-short")!.words).toBeLessThan(150)
  // Held-out fixtures are never in the fast (tuning) subset.
  const scenarios=selectScenarios({features:["generation"],fast:false,only:null},new Date("2026-11-13T12:00:00Z"))
  expect(scenarios.filter(scenario=>scenario.id.includes("/held-")).every(scenario=>!scenario.fast)).toBe(true)
  expect(scenarios.filter(scenario=>scenario.id.includes("/held-")).length).toBe(HELD_OUT.length*3)
  expect(scenarios.length).toBe((FIXTURES.length+GENERATION_FIXTURES.length)*3)
})
test("every un-noted annotation names a real gold item, and every scripted gold line is in its transcript",()=>{
  const all=[...FIXTURES,...GENERATION_FIXTURES as unknown as Fixture[]]
  for(const [id,unnoted] of Object.entries(SPARSE_UNNOTED)){
    const fixture=all.find(item=>item.id===id)
    expect(fixture,id).toBeDefined()
    for(const fact of unnoted.facts)expect(fixture!.gold.facts.some(item=>item.id===fact),`${id}/${fact}`).toBe(true)
    for(const decision of unnoted.decisions)expect(fixture!.gold.decisions.some(item=>item.id===decision),`${id}/${decision}`).toBe(true)
  }
  const long=compile(GENERATION_FIXTURES.find(fixture=>fixture.id==="hard-long") as unknown as Fixture).turns.map(turn=>turn.text).join("\n")
  for(const line of ["Then I'm reversing what I said earlier. We do not sign two years. One year only, and only if the fuel surcharge is capped.","The upgrade is one point eight million. Sorry, one point three."])expect(long.includes(line)).toBe(true)
  const held=compile(GENERATION_FIXTURES.find(fixture=>fixture.id==="held-long") as unknown as Fixture).turns.map(turn=>turn.text).join("\n")
  expect(held.includes("I'm reversing the earlier call. No double shifts. We move the opening to Friday, July seventeenth.")).toBe(true)
})
const REFERENCE_NOTES=`# Flour Supplier Switch
- Switching from Greystone Mills to Windrush Organics from 1st January
  - Greystone: £655/tonne vs. Windrush: £612/tonne
  - Saves £172/month at ~4 tonnes/month usage

# Deck Oven Replacement
- Approved: purchase Vulcan Mark 5 (£24,500, spread over 3 years)

# Next Steps
- **Sign Windrush Organics contract** (Yusuf)
  Deadline: 20th November.
- **Adjust sourdough recipe for extra water** (Caspian)
- **Notify Greystone Mills of supplier switch**`
test("document conventions: the reference layout passes, a wordy sentence layout does not",()=>{
  const context={transcriptCharacters:1500,transcriptWords:250,typedNotes:"",title:"Supplier review"}
  expect(generationStyle(REFERENCE_NOTES,context).filter(check=>!check.pass).map(check=>check.id)).toEqual([])
  expect(generationFormatLines(REFERENCE_NOTES,"Supplier review").filter(check=>!check.pass)).toEqual([])
  const metrics=styleMetrics(REFERENCE_NOTES,context)
  expect(metrics).toMatchObject({headings:3,maxHeadings:4,bullets:7,subBullets:2,actionBullets:3,actionBold:3,actionWithOwner:2,proseLines:0,maxDepth:1})
  const wordy="Here are the notes.\n\n# flour discussion\n- The team discussed the flour supplier at length and agreed, after hearing from everyone present, that the co-operative would switch from Graystone Mills to Windrush Organics at the start of January.\n- The seller will send the contract.\n\n# Next steps\n- Yusuf will sign the contract by 20 November.\n\n# Other\n- More.\n\n# Four\n- x\n\n# Five\n- y"
  expect(generationStyle(wordy,context).filter(check=>!check.pass).map(check=>check.id)).toEqual(expect.arrayContaining(["style:heading-count","style:title-case-headings","style:no-preamble-or-signoff","style:fragment-bullets","style:actions-last","style:action-format"]))
  expect([maxHeadingsFor(1500),maxHeadingsFor(8000),maxHeadingsFor(30_000),maxHeadingsFor(55_000),maxHeadingsFor(69_999),maxHeadingsFor(200_000)]).toEqual([4,5,5,6,7,8])
  expect(wordBudget(250,0)).toBeGreaterThan(125);expect(wordBudget(14_000,0)).toBe(1200);expect(wordBudget(100,0)).toBeLessThan(100)
  expect(isTitleCase("Budget and Vendors")).toBe(true);expect(isTitleCase("Go-Live Decision and Next Steps")).toBe(true);expect(isTitleCase("Open questions")).toBe(false)
  expect(parseNotes(REFERENCE_NOTES).find(line=>line.kind==="continuation")!.ancestors).toEqual(["Sign Windrush Organics contract (Yusuf)"])
})
test("owner verdicts: correct, missing, wrong, and unowned tasks that stay unowned",()=>{
  const roster=["Yusuf","Caspian","Ottoline"]
  const contract={id:"contract",owner:"Yusuf",text:"",must:["contract",day("November",20)],ownerMust:"(?:Yusuf)[^\\n]{0,100}(?:contract)|(?:contract)[^\\n]{0,100}(?:Yusuf)"}
  const tell={id:"tell",owner:"Ottoline",text:"",must:["Gr[ae]ystone[^\\n]{0,80}(?:notif|switch)|(?:notif|tell)[^\\n]{0,80}Gr[ae]ystone"],ownerMust:"Ottoline[^\\n]{0,120}Gr[ae]ystone|Gr[ae]ystone[^\\n]{0,120}Ottoline"}
  expect(ownerVerdict(REFERENCE_NOTES,contract,roster).verdict).toBe("correct")
  expect(ownerVerdict(REFERENCE_NOTES,tell,roster).verdict).toBe("missing")
  expect(ownerVerdict(REFERENCE_NOTES.replace("(Yusuf)","(Caspian)"),contract,roster)).toMatchObject({verdict:"wrong",named:"Caspian"})
  expect(ownerVerdict("# Next Steps\n- Yusuf\n  - Sign the Windrush contract by 20 November",contract,roster).verdict).toBe("correct")
  expect(ownerVerdict("# Flour\n- Nothing about it",contract,roster).verdict).toBe("absent")
  const unowned={...tell,unassigned:true,owner:"nobody",ownerMust:""}
  expect(ownerVerdict(REFERENCE_NOTES,unowned,roster).verdict).toBe("unassigned-ok")
  expect(ownerVerdict(REFERENCE_NOTES.replace("supplier switch**","supplier switch** (Caspian)"),unowned,roster).verdict).toBe("wrong")
  expect(ownerPosition("Marcus confirms with Renata whether she can join","Renata")).toBe(false)
  expect(ownerPosition("Marcus confirms with Renata whether she can join","Marcus")).toBe(true)
  expect(ownerPosition("Bring a test design to the February review (Mateus and Dominic)","Dominic")).toBe(true)
  expect(ownerPosition("Backup generator (Ostrander Dynamics) tested 6 hours","Ostrander")).toBe(true) // names-only parentheses: only roster names are ever asked
  expect(rosterOf(GENERATION_FIXTURES.find(fixture=>fixture.id==="hard-owners")!)).toEqual(expect.arrayContaining(["Halima","Annika","Teodor"]))
})
test("typed notes: every heading in order and no line dropped; a meeting with no tasks gets no to-do list",()=>{
  const notes="## Oven first\nvulcan mk5 24.5k - 3 yrs\n## Flour\nMY TAKE: push Windrush for a 2 yr price lock"
  const good="# Oven First\n- Vulcan Mark 5 approved: £24,500 spread over 3 years\n\n# Flour\n- My take: push Windrush for a 2-year price lock\n\n# Next Steps\n- **Sign contract** (Yusuf)"
  expect(typedNotesKept(good,notes).every(check=>check.pass)).toBe(true)
  const reordered=typedNotesKept("# Flour\n- Switching to Windrush\n\n# Oven First\n- Vulcan Mark 5 approved: £24,500 spread over 3 years",notes)
  expect(reordered.find(check=>check.id==="typed:headings-all-in-order")!.pass).toBe(false)
  expect(reordered.find(check=>check.id==="typed:no-line-dropped")!.pass).toBe(false)
  const allowed=["no (?:action items|owners?)","revisit|readout"]
  expect(noManufacturedActions("# Concepts\n- A is slower\n\n# Next Steps\n- Revisit after the usability readout",allowed,["Rafa","Lior"]).pass).toBe(true)
  expect(noManufacturedActions("# Concepts\n- A is slower\n\n# Next Steps\n- **Draft hybrid concept** (Lior)",allowed,["Rafa","Lior"]).pass).toBe(false)
  expect(noManufacturedActions("# Concepts\n- A is slower",allowed,["Rafa","Lior"]).pass).toBe(true)
})

// ------------------------------------------------- the generation module itself
import {analyseNote,maxSectionsFor,targetWords,voiceTags} from "../src/ai/notes-generation-analysis"
import {GENERATION_INSTRUCTIONS,GENERATION_PARAMETERS,generationMessages,generationSystemPrompt,serializeGenerationContext} from "../src/ai/notes-generation"
import {fixtureNote,meetingContextFor} from "./lib/fixtures"

const noteFor=(id:string,level:"none"|"sparse"|"detailed")=>{
  const fixture=GENERATION_FIXTURES.find(item=>item.id===id) as unknown as Fixture,now=new Date("2026-11-13T12:00:00Z")
  return {...fixtureNote(fixture,{now,level}),meetingContext:meetingContextFor(fixture,now)}
}
test("context: part-scoped voice tags, no segment or calendar identifiers, typed notes and transcript text verbatim",()=>{
  const note=noteFor("hard-owners","sparse"),context=JSON.parse(serializeGenerationContext(note))
  expect(context.userNotes).toBe(note.body)
  expect(context.transcript.map((line:{text:string})=>line.text)).toEqual(note.transcriptSegments.map(segment=>segment.text))
  expect(context).not.toHaveProperty("enhanced")
  // Segment and calendar identifiers are not sent; the speakerId is (test/diarization.test.ts requires it).
  const text=JSON.stringify(context)
  for(const segment of note.transcriptSegments)expect(text).not.toContain(segment.id)
  expect(text).not.toContain(note.meetingContext!.eventId)
  expect(context.transcript[0]).toEqual({at:"0:00",voice:"mic p1 v1",speakerId:note.transcriptSegments[0]!.speakerId,text:note.transcriptSegments[0]!.text})
  // Speaker numbers restart in every part, so the tag carries the part.
  const tags=voiceTags(note.transcriptSegments)
  expect(new Set(tags.filter(tag=>tag.startsWith("sys"))).size).toBeGreaterThan(3)
  expect(tags.every(tag=>/^(mic|sys) p\d+ v\d+$/.test(tag))).toBe(true)
  // A human label from upstream speaker naming passes through; unknown shapes stay honest.
  expect(voiceTags([{channel:"mic",speakerId:"fixture",speakerLabel:"Mira",text:"x",startMs:0},{channel:"system",speakerId:"fixture",speakerLabel:null,text:"y",startMs:1}])).toEqual(["mic Mira","sys unlabelled"])
  expect(context.calendarEvent.invited.find((person:{noteOwner?:boolean})=>person.noteOwner).name).toBe("Halima Yusufzai")
})
test("analysis: size limits follow transcript length; typed-notes mode and channel facts are worked out in code",()=>{
  expect([maxSectionsFor(3_000),maxSectionsFor(8_000),maxSectionsFor(45_000),maxSectionsFor(60_000),maxSectionsFor(90_000)]).toEqual([4,5,5,7,8])
  expect(targetWords(250,0)[1]).toBeGreaterThanOrEqual(125);expect(targetWords(250,0)[1]).toBeLessThanOrEqual(160)
  expect(targetWords(14_000,0)).toEqual([600,1000]);expect(targetWords(100,0)[1]).toBeLessThanOrEqual(70)
  const owners=analyseNote(noteFor("hard-owners","sparse"))
  expect(owners.notes.mode).toBe("fragments");expect(owners.people.owner).toEqual({name:"Halima Yusufzai",first:"Halima"})
  expect(owners.people.channels).toEqual({kind:"two-channel",oneMicVoice:true});expect(owners.size.maxSections).toBe(4)
  expect(analyseNote(noteFor("held-long","none")).people).toMatchObject({owner:null,invited:[],channels:{kind:"single-microphone"}})
  expect(analyseNote(noteFor("hard-long","detailed"))).toMatchObject({size:{maxSections:8},notes:{mode:"outline",actionHeading:"Mine"}})
  expect(analyseNote(noteFor("hard-bakery","sparse")).notes).toMatchObject({mode:"fragments",actionHeading:"actions"})
  expect(analyseNote(noteFor("hard-bakery","none"))).toMatchObject({notes:{mode:"none"},people:{channels:{kind:"system-only"}},size:{tiny:false}})
  expect(analyseNote(noteFor("hard-short","none")).size.tiny).toBe(true)
})
test("prompt: one typed-notes branch per note, limits stated, and no example lifted from an evaluation fixture",()=>{
  const none=generationSystemPrompt(noteFor("hard-bakery","none")),sparse=generationSystemPrompt(noteFor("hard-bakery","sparse")),detailed=generationSystemPrompt(noteFor("hard-bakery","detailed"))
  expect(none).toContain("The author typed nothing");expect(none).not.toContain("typed a few lines")
  expect(sparse).toContain("typed a few lines without headings");expect(sparse).toContain('call it "Actions"')
  expect(detailed).toContain('"Oven first", "Flour", "Money"');expect(detailed).toContain('add "Next Steps" as the last section')
  expect(none).toContain("author's original notes")
  expect(none).toContain("At most 4 sections");expect(generationSystemPrompt(noteFor("hard-long","none"))).toContain("At most 8 sections")
  expect(generationSystemPrompt(noteFor("hard-bakery","sparse"),"strict")).toContain("Leave out topics the author did not note")
  expect(generationSystemPrompt(noteFor("hard-bakery","sparse"),"keep")).toContain("cover what else mattered")
  // First names, companies and products from every fixture must not appear in the instructions.
  const instructions=generationSystemPrompt(noteFor("held-quiet","none")).toLowerCase()
  const fromFixtures=[...FIXTURES,...GENERATION_FIXTURES as unknown as Fixture[]].flatMap(fixture=>[...Object.keys(fixture.people),...(fixture.attendees??[]).map(person=>person.name.split(" ")[0]!)])
  const distinctive=["Windrush","Graystone","Vulcan","Kestrel","Corvid","Pallas","Halcyon","Tidewater","Hartwell","Stratus","Dockwise","Ferrymen","Brightpath","Tallyline"]
  expect([...new Set([...fromFixtures,...distinctive])].filter(word=>new RegExp(`\\b${word.toLowerCase()}\\b`).test(instructions))).toEqual([])
  const messages=generationMessages(noteFor("hard-short","none"))
  // The instructions are two system messages (sent as one): the core that is the same for every note, marked for
  // the provider's prompt cache, then the sections for this note. Together they are the system prompt, unchanged.
  expect(messages.map(message=>message.role)).toEqual(["system","system","user"])
  expect(messages[0]).toEqual({role:"system",content:GENERATION_INSTRUCTIONS,cache:true})
  expect(messages[0]!.content+messages[1]!.content).toBe(generationSystemPrompt(noteFor("hard-short","none")))
  // A one-off: the transcript is never sent again, so nothing but the marked core is written to a prompt cache.
  expect(GENERATION_PARAMETERS).toEqual({maxTokens:8192,temperature:0,cache:{ttl:"5m",tail:false}})
})

import {longA,longB} from "./fixtures/generation/long.beats"
test("the long scripts contain every hand-written line of their skeleton, verbatim and in order",()=>{
  for(const spec of [longA,longB]){
    const turns=compile(GENERATION_FIXTURES.find(fixture=>fixture.id===spec.id) as unknown as Fixture).turns
    let at=0
    for(const item of spec.items)for(const [speaker,line] of item.keys){
      const found=turns.findIndex((turn,index)=>index>=at&&turn.speaker===speaker&&turn.text.includes(line))
      expect(found,`${spec.id}: ${speaker}: ${line}`).toBeGreaterThanOrEqual(0)
      at=found
    }
    expect(new Set(turns.map(turn=>turn.speaker))).toEqual(new Set(Object.keys(spec.cast)))
  }
})

test("title checks: length, word count, cleanliness, dates, attendee lists, and capitals that are not names",()=>{
  const source="Thanks for joining. Let's walk the launch checklist and decide whether Thursday still holds. I talked to Priya about the iPhone build and the Northwind pilot."
  const failing=(title:string,people:string[]=[])=>titleChecks(title,{source,people}).filter(check=>!check.pass).map(check=>check.id)
  expect(failing("Launch checklist and iPhone build")).toEqual([])
  expect(failing("Northwind pilot launch checklist")).toEqual([])
  expect(failing("")).toEqual(["title-present"])
  expect(failing("Launch checklist")).toEqual(["title-words"])
  expect(failing("Launch checklist and the iPhone build for the Northwind pilot")).toEqual(["title-length","title-words","title-clean"])
  expect(failing("\"Launch checklist and iPhone build\"")).toEqual(["title-clean"])
  expect(failing("Launch checklist and iPhone build.")).toEqual(["title-clean"])
  expect(failing("Launch checklist build Thursday")).toEqual(["title-no-date"])
  expect(failing("Launch checklist 2026-11-19 build")).toEqual(["title-no-date","title-names-grounded"])
  expect(failing("Launch checklist with Priya and Sam",["Priya","Sam","Dana"])).toEqual(["title-not-attendees"])
  expect(failing("Launch checklist with Priya",["Priya","Sam"])).toEqual([])
  // Title Case is noticed (softly); names and acronyms the meeting itself capitalises are not.
  expect(failing("Launch Checklist And iPhone Build")).toEqual(["title-sentence-case"])
  expect(titleChecks("Launch Checklist And iPhone Build",{source}).find(check=>check.id==="title-sentence-case")!.soft).toBe(true)
  // A name or a number the meeting never mentioned is noticed; ordinary summary words are not.
  expect(failing("Kubernetes migration launch checklist")).toEqual([])
  expect(failing("Launch checklist for Kubernetes")).toEqual(["title-sentence-case","title-names-grounded"])
  expect(failing("Launch checklist for 40 seats")).toEqual(["title-names-grounded"])
  expect(failing("Self-hosted launch readiness review")).toEqual([])
  // A script without spaces has no word count to check.
  expect(titleChecks("发布清单与手机版本",{source:"发布清单与手机版本"}).every(check=>check.pass)).toBe(true)
})
test("the title run completes against the stub: an untitled note comes back with the stub's answer as its title",async()=>{
  const now=new Date("2026-11-13T12:00:00Z"),meetings=fixtureMeetings(false,now)
  expect(meetings.map(meeting=>meeting.id).sort()).toEqual(["discovery","numbers","standup"])
  expect(fixtureMeetings(true,now).length).toBe(FIXTURES.length)
  const result=await runTitle({provider:"openrouter",model:"openai/gpt-6-sol",transport:stubTransport,stub:true},meetings[0]!,now)
  expect(result.error).toBeUndefined()
  expect(result.title).toBe("Stub notes")
  expect(result.notes).toContain("offline stub")
  expect(result.titleMs).not.toBeNull()
  expect(result.checks.find(check=>check.id==="title-words")!.pass).toBe(false)
})
