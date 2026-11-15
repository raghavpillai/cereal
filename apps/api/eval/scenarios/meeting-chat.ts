/** Single-meeting chat on fixtures written for it (`fixtures/meeting-chat/`):
 * chat on a finished note (`note-chat`) and chat while the meeting is still
 * being recorded (`live-chat`).
 *
 * These add what the shared fixtures do not cover: meetings of well over an
 * hour with facts planted early, in the middle and late; follow-ups that depend
 * on earlier turns; arithmetic over spoken numbers; a question in another
 * language; answers that exist only in the typed notes or only in the
 * transcript; transcription errors and disfluency; recency questions at several
 * points of a live timeline; an utterance that is still being spoken; and a
 * line that has not reached the server yet.
 *
 * Fixture IDs start `mc-t-` (tuned: prompts were iterated against these) or
 * `mc-h-` (held out: scored, never tuned against). Select a set with
 * `--only mc-t-` or `--only mc-h-`. */
import type {ChatInput,TranscriptSegmentDTO} from "@meeting-notes/contracts"
import {compile,fixtureNote,meetingContextFor,noteId} from "../lib/fixtures"
import {runChat} from "../lib/harness"
import {chatFormat,citationChecks,forbidden,grounding,matches,mustAll,mustNot} from "../lib/checks"
import {DELIVERIES,clock,deliveredAt,findTurn,pattern,renderTruth,supportSegments,visibleAtMs} from "../lib/transcript"
import {citedClaims,completed,goldText,resolver,sourceText,SPEAKER_NOTE,type Scenario} from "../lib/scenario"
import type {Check,CompiledFixture,Turn} from "../lib/types"
import {tidewater} from "../fixtures/meeting-chat/tidewater"
import {kiln} from "../fixtures/meeting-chat/kiln"
import {partnerDay,springSummit} from "../fixtures/meeting-chat/long"
import {ABSTAIN,type McFixture,type McLive} from "../fixtures/meeting-chat/types"

export const MEETING_CHAT_FIXTURES:ReadonlyArray<McFixture>=[tidewater,partnerDay,kiln,springSummit]

const words=(text:string)=>text.replace(/\[\[S\d+\]\]/g," ").split(/\s+/).filter(Boolean).length
const markers=(output:string)=>new Set([...output.matchAll(/\[\[(S\d+)\]\]/g)].map(match=>match[1]!))
/** The stream event that carries citations holds at most 40; an answer that
 * uses more distinct markers than that cannot be delivered. */
const markerLimit=(output:string):Check=>{const used=markers(output).size;return {id:"citation-markers-within-wire-limit",group:"citation",pass:used<=40,...(used<=40?{}:{detail:`${used} distinct markers; the citations event carries at most 40`})}}
const cited=(output:string):Check=>({id:"substantive-answer-cited",group:"citation",soft:true,pass:markers(output).size>0,...(markers(output).size?{}:{detail:"no citation marker in an answer that states facts from the meeting"})})
const brief=(output:string,limit:number):Check=>({id:"live-answer-brief",group:"format",soft:true,pass:words(output)<=limit,detail:`${words(output)} words (limit ${limit})`})
/** "What did I miss" as a format contract: no introduction, one to three bullets, no quotation marks. */
export function recipeChecks(output:string):Check[] {
  const lines=output.split("\n").map(line=>line.trim()).filter(Boolean),bullet=/^(?:[-*•]|\d+[.)])\s+\S/
  const bullets=lines.filter(line=>bullet.test(line)),quoted=/["“”«»]/.test(output.replace(/\[\[S\d+\]\]/g,""))
  return [
    {id:"recipe-no-introduction",group:"format",pass:bullet.test(lines[0]??""),...(bullet.test(lines[0]??"")?{}:{detail:`starts "${(lines[0]??"").slice(0,60)}"`})},
    {id:"recipe-one-to-three-bullets",group:"format",pass:bullets.length>=1&&bullets.length<=3&&bullets.length===lines.length,detail:`${bullets.length} bullets in ${lines.length} lines`},
    {id:"recipe-no-quotation-marks",group:"format",pass:!quoted},
    {id:"recipe-brief",group:"format",soft:true,pass:words(output)<=90,detail:`${words(output)} words`},
  ]
}
const bulletLines=(output:string)=>{const lines=output.split("\n").map(line=>line.trim()).filter(Boolean),bullet=/^(?:[-*•]|\d+[.)])\s+\S/;return {lines,bullets:lines.filter(line=>bullet.test(line))}}
/** "Make me sound smart" (exactly two first-person bullets) and "Suggest questions" (exactly three questions). */
export function suggestionChecks(output:string,kind:"smart"|"questions"):Check[] {
  const {lines,bullets}=bulletLines(output),wanted=kind==="smart"?2:3
  return [
    {id:`recipe-exactly-${wanted}-bullets`,group:"format",pass:bullets.length===wanted&&lines.length===wanted,detail:`${bullets.length} bullets in ${lines.length} lines`},
    kind==="smart"?{id:"recipe-first-person",group:"format",pass:bullets.length>0&&bullets.every(line=>/\b(?:I|I'd|I'm|I'll|I've|we|we'd|we're|my|our|us|me)\b/.test(line.replace(/[\u2018\u2019]/g,"'")))}
      :{id:"recipe-all-questions",group:"format",pass:bullets.length>0&&bullets.every(line=>line.includes("?"))},
    {id:"recipe-no-citations",group:"format",soft:true,pass:markers(output).size===0},
  ]
}
/** Ground truth for the judge. A two-hour meeting is shown as the passages that
 * matter plus its opening and close, so the judge is not paid to read it all. */
function truthFor(built:CompiledFixture,turns:ReadonlyArray<Turn>,quotes:ReadonlyArray<string>,tail=0):string {
  if(turns.length<=260)return renderTruth(turns)
  const keep=new Set<number>()
  const around=(index:number,radius:number)=>{for(let i=Math.max(0,index-radius);i<=Math.min(turns.length-1,index+radius);i++)keep.add(i)}
  for(const quote of quotes)around(findTurn(built,quote).index,9)
  for(let i=0;i<Math.min(8,turns.length);i++)keep.add(i)
  for(let i=Math.max(0,turns.length-Math.max(8,tail));i<turns.length;i++)keep.add(i)
  const out=[`(This meeting has ${turns.length} turns${tail?" so far":""}. Only the passages relevant to the question are shown; the omitted turns are unrelated sessions and say nothing that changes the answer.)`]
  let previous=-1
  for(const index of [...keep].filter(index=>index<turns.length).sort((a,b)=>a-b)){
    if(index!==previous+1)out.push(`[… ${index-previous-1} turns omitted …]`)
    out.push(renderTruth([turns[index]!]));previous=index
  }
  return out.join("\n")
}

function noteScenarios(fixture:McFixture,now:Date):Scenario[] {
  const built=compile(fixture),meeting=meetingContextFor(fixture,now),meetings=new Map(meeting?[[noteId(fixture),meeting]]:[])
  return fixture.questions.map((question):Scenario=>{
    const note=fixtureNote(fixture,{now,level:"sparse",summary:question.withSummary}),resolve=resolver([note])
    const input:ChatInput={message:question.question,history:question.history??[],includeSources:true,timeZone:"UTC"}
    return {
      feature:"note-chat",id:`note-chat/${fixture.id}/${question.id}`,fixture:fixture.id,variant:question.withSummary?`${question.tag}+summary`:question.tag,fast:!!question.fast,
      input:{question:question.question,...(question.history?{history:question.history}:{}),reference:question.reference},
      context:{segmentsAvailable:built.segments.length,segmentsTotal:built.segments.length,transcriptCharacters:built.segments.reduce((sum,segment)=>sum+segment.text.length,0)},
      execute:target=>runChat(target,[note],note.id,input,meetings),
      check:execution=>!execution.ok?[completed(execution)]:[
        completed(execution),
        question.kind==="absent"&&matches(execution.output,ABSTAIN)?{id:"answer-correct",group:"required",pass:true}:mustAll(execution.output,question.must,"answer-correct"),
        ...(question.forbid?.length?[mustNot(execution.output,question.forbid,"answer-not-wrong")]:[]),
        ...forbidden(execution.output,fixture.gold.notSaid),
        ...citationChecks(execution.output,{citations:execution.citations,resolve,supportSegmentIds:supportSegments(built,question.support),required:question.kind==="citation"}),
        markerLimit(execution.output),
        ...(question.kind==="absent"?[]:[cited(execution.output)]),
        ...chatFormat(execution.output,question.question),
        ...grounding(execution.output,sourceText(fixture,note.body+"\n"+note.enhanced,now,[question.question,...(question.history??[]).map(turn=>turn.content)].join("\n"))),
      ],
      judge:execution=>({feature:"note-chat",
        task:`Answer a question about the finished meeting "${fixture.title}". What the question stresses: ${question.tag}${question.kind==="absent"?" (the meeting does not contain the answer; the right response says so and adds only what the meeting does say)":question.kind==="citation"?" (the user asked for cited sources)":question.tag==="notes-only"?" (the answer is only in the author's typed notes, not in what was said; the right response gives it and may say where it comes from)":question.tag==="language"?" (the question is not in English; the answer should be in the language of the question)":question.tag==="stt"?" (the transcript contains speech-to-text errors; the typed notes carry the correct spellings)":""}.${question.history?`\nEarlier turns:\n${question.history.map(turn=>`${turn.role}: ${turn.content}`).join("\n")}`:""}\nQuestion: ${question.question}\nThe note owner (the person asking, "I") is ${fixture.attendees?.find(person=>person.self)?.name??"the person at the microphone"}.\n${SPEAKER_NOTE}`,
        source:truthFor(built,built.turns,question.support??[]),typedNotes:note.body,gold:goldText(fixture),reference:question.reference,output:execution.output,citations:citedClaims(execution,resolve)}),
    }
  })
}

function liveScenario(fixture:McFixture,checkpoint:McLive,now:Date):Scenario {
  const deliveryId=checkpoint.delivery??"stream",delivery=DELIVERIES[deliveryId]!,capture=delivery.capture
  const built=compile(fixture,capture),anchor=findTurn(built,checkpoint.after),lagMs=(delivery.lagSeconds??0)*1000
  let atMs=anchor.endMs+(checkpoint.delaySeconds??5)*1000,partial:{turn:Turn;text:string}|null=null
  if(checkpoint.partial){
    if(deliveryId!=="stream")throw new Error(`${fixture.id}/${checkpoint.id}: a partial utterance needs the stream delivery`)
    const next=built.turns[anchor.index+1]
    if(!next)throw new Error(`${fixture.id}/${checkpoint.id}: no line after the anchor`)
    const all=next.text.split(/\s+/),count=Math.max(1,Math.min(all.length-1,Math.round(all.length*checkpoint.partial.fraction)))
    partial={turn:next,text:all.slice(0,count).join(" ")}
    atMs=next.startMs+Math.round((next.endMs-next.startMs)*count/all.length)+lagMs
  }
  const delivered=delivery.lagSeconds===null?{segments:[] as TranscriptSegmentDTO[],deliveredUntilMs:0}:deliveredAt(built,delivery,atMs)
  let segments=delivered.segments
  if(partial){
    const whole=built.segments.find(segment=>built.segmentTurns.get(segment.id)?.includes(partial!.turn.index))!
    segments=[...segments.filter(segment=>segment.id!==whole.id),{...whole,text:partial.text,endMs:atMs-lagMs,isFinal:!!checkpoint.partial!.final}]
  }
  const anchorVisible=visibleAtMs(anchor,delivery),hasAnchor=anchorVisible!==null&&anchorVisible<=atMs
  const meeting=meetingContextFor(fixture,now)
  const note=fixtureNote(fixture,{now,level:checkpoint.notes??"none",capture,segments,recording:true}),resolve=resolver([note])
  const input:ChatInput={message:checkpoint.question,history:checkpoint.history??[],includeSources:true,timeZone:"UTC"}
  const spoken=built.turns.filter(turn=>turn.endMs<=atMs-(partial?lagMs:0)),unspoken=built.turns.filter(turn=>!spoken.includes(turn)&&turn!==partial?.turn)
  const said=[...spoken.map(turn=>turn.text),partial?.text??""].join("\n"),later=(checkpoint.later??[]).filter(source=>!matches(said,source))
  const expectsAnswer=checkpoint.kind!=="lag"
  const truth=[`--- Said before the question (${clock(atMs)}) ---`,truthFor(built,spoken,[],40)||"(nothing yet)",
    ...(partial?[`--- Being spoken as the question is asked; only these first words had reached the assistant ---\n${partial.turn.speaker}: ${partial.text} …`]:[]),
    "--- Not yet said at that moment (must not appear) ---",renderTruth(unspoken.slice(0,20))||"(nothing)"].join("\n")
  return {
    feature:"live-chat",id:`live-chat/${fixture.id}/${checkpoint.id}`,fixture:fixture.id,variant:`${checkpoint.kind}/${deliveryId}/${checkpoint.partial?"partial":`+${checkpoint.delaySeconds??5}s`}`,fast:!!checkpoint.fast,
    input:{question:checkpoint.question,...(checkpoint.history?{history:checkpoint.history}:{}),atMs,delivery:deliveryId,reference:checkpoint.reference},
    context:{segmentsAvailable:segments.length,segmentsTotal:built.segments.length,transcriptCharacters:segments.reduce((sum,segment)=>sum+segment.text.length,0),deliveredUntilMs:delivered.deliveredUntilMs},
    execute:target=>runChat(target,[note],note.id,input,new Map(meeting?[[note.id,meeting]]:[])),
    check:execution=>!execution.ok?[completed(execution)]:[
      completed(execution),
      expectsAnswer?mustAll(execution.output,checkpoint.must,"uses-what-was-said"):mustAll(execution.output,checkpoint.must,"admits-gap-and-says-why","temporal"),
      {id:"anchor-delivered",group:"temporal",soft:true,pass:hasAnchor,detail:hasAnchor?"the line was on the server when the question was asked":"the line had not reached the server when the question was asked"},
      ...(later.length?[mustNot(execution.output,later,"nothing-from-later")]:[]),
      ...(checkpoint.forbid?.length?[mustNot(execution.output,checkpoint.forbid,"nothing-stale-or-wrong")]:[]),
      ...forbidden(execution.output,fixture.gold.notSaid),
      ...(checkpoint.recipe===true?recipeChecks(execution.output):checkpoint.recipe?suggestionChecks(execution.output,checkpoint.recipe):[brief(execution.output,checkpoint.kind==="so-far"?220:checkpoint.kind==="recency"?140:110)]),
      ...citationChecks(execution.output,{citations:execution.citations,resolve,required:false}),
      markerLimit(execution.output),
      ...(expectsAnswer&&!["not-yet","not-said","unfinished"].includes(checkpoint.kind)&&(checkpoint.recipe===true||!checkpoint.recipe)?[cited(execution.output)]:[]),
      ...chatFormat(execution.output,checkpoint.question),
      ...grounding(execution.output,[fixture.title,said,note.body,checkpoint.question,...(checkpoint.history??[]).map(turn=>turn.content),JSON.stringify(meeting??"")].join("\n")),
    ],
    judge:execution=>({feature:"live-chat",
      task:`The meeting "${fixture.title}" is still being recorded. The user asks a question ${clock(atMs)} into it. The assistant can only know what the server had received by then: under the "${deliveryId}" delivery model that is ${segments.length} transcript segments${delivery.lagSeconds===null?" (none: this client uploads audio only after Stop)":partial?"":`, the transcript up to ${clock(delivered.deliveredUntilMs)}`}. The person asking ("you", "I") is the note owner, ${fixture.attendees?.find(person=>person.self)?.name??"the person who started the recording"}; the assistant was told so.${deliveryId==="stream"?" The assistant was not told the transcript is streaming, so a short remark that the transcript may be a little behind is acceptable, not an unsupported claim.":" The assistant was told that speech reaches the transcript in batches, usually 20 to 40 seconds late and sometimes later; repeating that is not an unsupported claim."} ${checkpoint.kind==="lag"?"The line the question is about had NOT reached the server, so the assistant could not know it. The right response says so plainly, explains that the transcript has not caught up (or does not exist yet), and offers the latest thing it does have if there is one; score coverage 3 or higher if it does that.":checkpoint.partial?"The last utterance was still being spoken: only its first words had arrived. The right response uses what has arrived, does not complete the sentence from imagination, and says so when the fact asked for has not been spoken yet.":"The line the question is about had been delivered."}${checkpoint.recipe===true?" The question is the built-in \"What did I miss\" recipe: answer directly with no introduction, one to three short bullets covering only the most recent beats (what the other participants said), with no quotation marks.":checkpoint.recipe==="smart"?" The question is the built-in \"Make me sound smart\" recipe: exactly two bullets, each a thoughtful first-person statement or question the user could say aloud right now, about what is being discussed at this moment and grounded in what was said. Judge coverage by whether both are relevant to the current discussion, not by the reference's wording.":checkpoint.recipe==="questions"?" The question is the built-in \"Suggest questions\" recipe: exactly three follow-up questions that would move the current discussion forward, grounded in what was said. Judge coverage by whether all three are relevant to the current discussion, not by the reference's wording.":" Answers during a meeting should lead with the answer and be short enough to read at a glance."} Using anything said after ${clock(atMs)} would be an error. Speaker names were not visible to the assistant except where spoken, typed or on the attendee list.${note.body?" The note owner's typed notes (shown below) were available to it as secondary context: a typed note may be used when the question is about the notes or the transcript lacks the fact, but must not override what was said and does not belong in a catch-up.":""}${checkpoint.history?`\nEarlier turns:\n${checkpoint.history.map(turn=>`${turn.role}: ${turn.content}`).join("\n")}`:""}\nQuestion: ${checkpoint.question}`,
      source:truth,...(note.body?{typedNotes:note.body}:{}),reference:`If the assistant had everything said so far: ${checkpoint.reference}`,output:execution.output,citations:citedClaims(execution,resolve)}),
  }
}

export const meetingChatScenarios=(now:Date):Scenario[]=>MEETING_CHAT_FIXTURES.flatMap(fixture=>[...noteScenarios(fixture,now),...fixture.live.map(checkpoint=>liveScenario(fixture,checkpoint,now))])

/** Fail fast on authoring mistakes. Mirrors `validateFixtures` for the shared set. */
export function validateMeetingChatFixtures():Array<{id:string;set:string;capture:string;turns:number;segments:number;words:number;duration:string;questions:number;live:number}> {
  return MEETING_CHAT_FIXTURES.map(fixture=>{
    const built=compile(fixture)
    for(const capture of ["desktop","ios","stream"] as const)compile(fixture,capture)
    const sources=[...fixture.gold.decisions,...fixture.gold.actionItems,...fixture.gold.facts].flatMap(item=>item.must).concat(fixture.gold.actionItems.map(item=>item.ownerMust),fixture.gold.notSaid.flatMap(item=>item.forbid))
    for(const source of sources)pattern(source)
    const ids=new Set<string>()
    for(const question of fixture.questions){
      if(ids.has(question.id))throw new Error(`${fixture.id}: duplicate question ${question.id}`)
      ids.add(question.id)
      supportSegments(built,question.support)
      const missing=question.must.filter(source=>!matches(question.reference,source))
      if(missing.length)throw new Error(`${fixture.id}/${question.id}: the reference answer does not satisfy /${missing.join("/, /")}/`)
      if((question.forbid??[]).some(source=>matches(question.reference,source)))throw new Error(`${fixture.id}/${question.id}: the reference answer matches a forbidden pattern`)
      const tripped=forbidden(question.reference,fixture.gold.notSaid).find(check=>!check.pass)
      if(tripped)throw new Error(`${fixture.id}/${question.id}: the reference answer trips ${tripped.id} (${tripped.detail})`)
    }
    for(const checkpoint of fixture.live){
      if(ids.has(checkpoint.id))throw new Error(`${fixture.id}: duplicate id ${checkpoint.id}`)
      ids.add(checkpoint.id)
      findTurn(compile(fixture,"stream"),checkpoint.after)
      const missing=checkpoint.must.filter(source=>!matches(checkpoint.reference,source))
      if(missing.length)throw new Error(`${fixture.id}/${checkpoint.id}: the reference answer does not satisfy /${missing.join("/, /")}/`)
      for(const source of [...(checkpoint.later??[]),...(checkpoint.forbid??[])]){
        if(matches(checkpoint.reference,source))throw new Error(`${fixture.id}/${checkpoint.id}: the reference answer matches the later/forbidden pattern /${source}/`)
      }
      const tripped=forbidden(checkpoint.reference,fixture.gold.notSaid).find(check=>!check.pass)
      if(tripped)throw new Error(`${fixture.id}/${checkpoint.id}: the reference answer trips ${tripped.id} (${tripped.detail})`)
      if((checkpoint.recipe===true?recipeChecks(checkpoint.reference):checkpoint.recipe?suggestionChecks(checkpoint.reference,checkpoint.recipe):[]).some(check=>!check.pass&&!check.soft))throw new Error(`${fixture.id}/${checkpoint.id}: the reference does not follow the recipe format`)
    }
    return {id:fixture.id,set:fixture.set,capture:fixture.capture,turns:built.turns.length,segments:built.segments.length,words:built.turns.reduce((sum,turn)=>sum+turn.text.split(/\s+/).length,0),duration:built.duration,questions:fixture.questions.length,live:fixture.live.length}
  })
}
