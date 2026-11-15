/** Chat with one stopped note: factual, synthesis, "not in the meeting",
 * follow-up turns and citation-required questions, defined beside each fixture. */
import type {ChatInput} from "@meeting-notes/contracts"
import {FIXTURES,compile,fixtureNote,meetingContextFor,noteId} from "../lib/fixtures"
import {runChat} from "../lib/harness"
import {chatFormat,citationChecks,forbidden,grounding,matches,mustAll,mustNot,ABSTAIN} from "../lib/checks"
import {supportSegments} from "../lib/transcript"
import {citedClaims,completed,goldText,resolver,sourceText,truth,SPEAKER_NOTE,type Scenario} from "../lib/scenario"

export const noteChatScenarios=(now:Date):Scenario[]=>FIXTURES.flatMap(fixture=>{
  const built=compile(fixture),meeting=meetingContextFor(fixture,now),meetings=new Map(meeting?[[noteId(fixture),meeting]]:[])
  // Each fixture's synthesis question is also asked with a generated document
  // present, which is the usual state of a note after the meeting.
  const cases=fixture.questions.flatMap(question=>question.kind==="synthesis"&&question===fixture.questions.find(q=>q.kind==="synthesis")?[question,{...question,id:`${question.id}@summary`,withSummary:true,fast:false}]:[question])
  return cases.map((question):Scenario=>{
    const note=fixtureNote(fixture,{now,level:"sparse",summary:question.withSummary}),resolve=resolver([note])
    const input:ChatInput={message:question.question,history:question.history??[],includeSources:true,timeZone:"UTC"}
    return {
      feature:"note-chat",id:`note-chat/${fixture.id}/${question.id}`,fixture:fixture.id,variant:question.withSummary?`${question.kind}+summary`:question.kind,fast:!!fixture.fast&&!!question.fast,
      input:{question:question.question,...(question.history?{history:question.history}:{}),reference:question.reference},
      context:{segmentsAvailable:built.segments.length,segmentsTotal:built.segments.length,transcriptCharacters:built.segments.reduce((sum,segment)=>sum+segment.text.length,0)},
      execute:target=>runChat(target,[note],note.id,input,meetings),
      check:execution=>!execution.ok?[completed(execution)]:[
        completed(execution),
        // "Not in the meeting" can be said many ways; any clear statement of absence
        // counts, and `forbid` rejects an answer that goes on to invent one.
        question.kind==="absent"&&matches(execution.output,ABSTAIN)?{id:"answer-correct",group:"required",pass:true}:mustAll(execution.output,question.must,"answer-correct"),
        ...(question.forbid?.length?[mustNot(execution.output,question.forbid,"answer-not-wrong")]:[]),
        ...forbidden(execution.output,fixture.gold.notSaid),
        ...citationChecks(execution.output,{citations:execution.citations,resolve,supportSegmentIds:supportSegments(built,question.support),required:question.kind==="citation"}),
        ...chatFormat(execution.output,question.question),
        ...grounding(execution.output,sourceText(fixture,note.body+"\n"+note.enhanced,now,[question.question,...(question.history??[]).map(turn=>turn.content)].join("\n"))),
      ],
      judge:execution=>({feature:"note-chat",
        task:`Answer a question about the meeting "${fixture.title}". Question kind: ${question.kind}${question.kind==="absent"?" (the meeting does not contain the answer; the right response says so and adds only what the meeting does say)":question.kind==="citation"?" (the user asked for cited sources)":""}.${question.history?`\nEarlier turns:\n${question.history.map(turn=>`${turn.role}: ${turn.content}`).join("\n")}`:""}\nQuestion: ${question.question}\n${SPEAKER_NOTE}`,
        source:truth(fixture),typedNotes:note.body,gold:goldText(fixture),reference:question.reference,output:execution.output,citations:citedClaims(execution,resolve)}),
    }
  })
})
