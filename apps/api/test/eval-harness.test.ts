/** Offline checks of the evaluation harness in apps/api/eval: fixtures are
 * well-formed, the scoring functions behave, and a whole run completes against
 * the stub provider. No network and no credentials. */
import {expect,test} from "bun:test"
import {mkdtemp,readFile,rm} from "node:fs/promises"
import {tmpdir} from "node:os"
import {resolve} from "node:path"
import {FIXTURES,compile,validateFixtures} from "../eval/lib/fixtures"
import {DELIVERIES,deliveredAt,findTurn,visibleAtMs} from "../eval/lib/transcript"
import {asserted,citationChecks,generationFormat,notesFidelity,numbersIn,ungroundedNumbers} from "../eval/lib/checks"
import {parseOptions,rescore,runEvaluation,selectScenarios} from "../eval/run"

test("fixtures compile into the stored segment shape for every capture mode",()=>{
  const rows=validateFixtures()
  expect(rows.length).toBe(FIXTURES.length)
  expect(rows.find(row=>row.id==="planning")!.duration.startsWith("6")).toBe(true) // an hour or more
  const desktop=compile(FIXTURES.find(fixture=>fixture.id==="standup")!)
  // Speaker numbers restart per uploaded part and IDs carry channel and part.
  expect(desktop.segments.every(segment=>/^(mic|system):[0-9a-f-]{36}:speaker_\d+$/.test(segment.speakerId)&&/^Speaker \d+$/.test(segment.speakerLabel!))).toBe(true)
  expect(new Set(desktop.segments.map(segment=>segment.channel)).size).toBe(2)
  // A lone microphone speaker's turns within one part are a single segment.
  expect(desktop.segments.filter(segment=>segment.channel==="mic"&&segment.startMs<60_000).length).toBe(1)
  expect(desktop.segments.length).toBeLessThan(desktop.turns.length)
  const phone=compile(FIXTURES.find(fixture=>fixture.id==="crosstalk")!)
  expect(phone.segments.every(segment=>segment.channel==="mic")).toBe(true)
  expect(new Set(phone.segments.map(segment=>segment.speakerId.split(":")[1])).size).toBeGreaterThan(1)
})

test("live delivery models expose only what the server would hold",()=>{
  const fixture=FIXTURES.find(item=>item.id==="discovery")!,stream=compile(fixture,"stream"),phone=compile(fixture,"ios")
  const anchor=findTurn(stream,"we spend roughly twelve hours a week")
  const soon=deliveredAt(stream,DELIVERIES.stream!,anchor.endMs+5000)
  expect(soon.segments.some(segment=>segment.text.includes("twelve hours"))).toBe(true)
  expect(soon.segments.every(segment=>(segment.endMs??0)<=anchor.endMs+3000)).toBe(true)
  const phoneAnchor=findTurn(phone,"we spend roughly twelve hours a week")
  expect(deliveredAt(phone,DELIVERIES.ios!,phoneAnchor.endMs+5000).segments.some(segment=>segment.text.includes("twelve hours"))).toBe(false)
  expect(deliveredAt(phone,DELIVERIES.ios!,visibleAtMs(phoneAnchor,DELIVERIES.ios!)!).segments.some(segment=>segment.text.includes("twelve hours"))).toBe(true)
  expect(deliveredAt(phone,DELIVERIES.desktop!,10*60_000).segments).toEqual([])
})

test("deterministic checks: negation, spoken numbers, format, notes and citations",()=>{
  expect(asserted("The budget is approved.","budget (?:is|was) approved")).toBe("budget is approved")
  expect(asserted("No, the budget is approved only in principle? It was not: the budget is not approved.","budget is not approved")).toBeNull()
  expect(asserted("They did not say the budget is approved.","budget (?:is|was) approved")).toBeNull()
  const spoken=numbersIn("four point eight two million against five point one, a hundred and forty workers, two point fourteen, 31%, $4,500. A base between one hundred sixty five and one hundred seventy five thousand. Nine thousand four hundred euros, five and a half percent, seventy one point four, August twenty ninth, one thousand one hundred eighty tickets.")
  for(const value of [4.82,4_820_000,5.1,140,2.14,31,4500,165,175_000,9400,5.5,71.4,29,1180])expect(spoken.has(value)).toBe(true)
  for(const value of [540,5.11,340_000])expect(spoken.has(value)).toBe(false)
  expect(ungroundedNumbers("Revenue was $4.82M, a miss of $280,000 (5.5%).","four point eight two million against a plan of five point one")).toEqual([])
  expect(ungroundedNumbers("Revenue was $9.7M.","four point eight two million")).toEqual([9.7,9_700_000])
  const format=generationFormat("# Meeting Notes\n\nSome prose.\n\n## Detail\n- mic:00000000-0000-4000-8000-000000000001:speaker_1 said hi","Title")
  expect(format.filter(check=>!check.pass).map(check=>check.id)).toEqual(expect.arrayContaining(["level-one-headings","mostly-bullets","no-generic-or-title-heading","no-internal-identifiers"]))
  expect(notesFidelity("# Updates\n- retry fix\n\n# Release\n- Thursday\n\n# To do\n- tell Priyanka","# Updates\n- retry fix\n# Release\n- Thursday\n# To do\n- tell Priyanka").every(check=>check.pass)).toBe(true)
  const resolve=(citation:{segmentId:string|null})=>citation.segmentId==="a"?"Kofi will rotate the staging certificate before noon.":undefined
  const good=citationChecks("Kofi rotates the certificate before noon. [[S1]]",{citations:[{id:"S1",source:"transcript",noteId:"n",segmentId:"a",quote:"rotate the staging certificate"}],resolve,supportSegmentIds:["a"],required:true})
  expect(good.every(check=>check.pass)).toBe(true)
  const bad=citationChecks("Revenue was 4.82 million. [[S1]] [[S2]]",{citations:[{id:"S1",source:"transcript",noteId:"n",segmentId:"a",quote:"rotate the staging certificate"},{id:"S2",source:"transcript",noteId:"n",segmentId:"missing",quote:"x"}],resolve,required:true})
  expect(bad.find(check=>check.id==="citations-resolve")!.pass).toBe(false)
  expect(bad.find(check=>check.id==="citations-support-claim")!.pass).toBe(false)
  expect(citationChecks("No sources.",{citations:[],resolve,required:true})[0]!.pass).toBe(false)
})

test("scenario selection and an end-to-end stub run with report and rescore",async()=>{
  const now=new Date("2026-11-13T12:00:00Z"),all=selectScenarios({features:["generation","note-chat","workspace-chat","live-chat"],fast:false,only:null},now)
  expect(new Set(all.map(scenario=>scenario.feature)).size).toBe(4)
  expect(all.length).toBeGreaterThan(100)
  const out=await mkdtemp(resolve(tmpdir(),"cereal-eval-"))
  try{
    const options=parseOptions(["all","--fast","--stub","--reps","2","--agreement","2","--out",out,"--label","smoke"],{})
    const lines:string[]=[]
    const result=await runEvaluation(options,line=>lines.push(line))
    expect(result.runs.length).toBe(selectScenarios({...options},now).length*2)
    expect(result.runs.every(run=>run.ok&&run.output.length>0&&run.perf.calls.length>0)).toBe(true)
    // The stub is free and labelled as such; it must never look like a baseline.
    expect(result.meta.spendUsd.total).toBe(0)
    expect(result.report).toContain("STUB PROVIDER")
    for(const heading of ["## Summary","## Latency, tokens and cost","## Judge noise","## generation","## note-chat","## workspace-chat","## live-chat","Answerability over time","### Worst examples"])expect(result.report).toContain(heading)
    const workspace=result.runs.find(run=>run.feature==="workspace-chat")!
    expect(workspace.perf.calls.some(call=>call.purpose==="planner")).toBe(true)
    expect(workspace.perf.toolCalls.get_meetings).toBe(1)
    expect(result.runs.find(run=>run.feature==="note-chat")!.perf.ttftMs).not.toBeNull()
    const stored=JSON.parse(await readFile(resolve(out,"smoke/results.json"),"utf8"))
    expect(stored.runs.length).toBe(result.runs.length)
    expect(JSON.stringify(stored)).not.toContain("Bearer ")
    await rescore(resolve(out,"smoke"),()=>{})
    expect(await readFile(resolve(out,"smoke/report.md"),"utf8")).toContain("## Summary")
  }finally{await rm(out,{recursive:true,force:true})}
// Runs every stub scenario and writes reports to disk; needs more than the 5 s default on a busy machine.
},60_000)
