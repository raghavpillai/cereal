/** Generation on the hard-case fixtures (lib/generation-fixtures.ts), at each
 * level of typed notes. IDs are `generation/hard-…` for fixtures the prompt was
 * tuned against and `generation/held-…` for the held-out ones, so
 * `--only generation/held-` reports the held-out set on its own. */
import {compile,fixtureNote,meetingContextFor,notesFor} from "../lib/fixtures"
import {runGeneration} from "../lib/harness"
import {forbidden,grounding,notesFidelity,required} from "../lib/checks"
import {generationFormatLines,generationStyle,noManufacturedActions,ownerChecks,ownerVerdict,typedNotesKept} from "../lib/generation-checks"
import {GENERATION_FIXTURES,SPARSE_UNNOTED,rosterOf,type GenerationFixture} from "../lib/generation-fixtures"
import {completed,sourceText,truth,SPEAKER_NOTE,type Scenario} from "../lib/scenario"
import type {Check,Fixture,GoldItem,NotesLevel} from "../lib/types"

const LEVELS:ReadonlyArray<NotesLevel>=["none","sparse","detailed"]
const FAST=new Set(["hard-bakery/none","hard-bakery/sparse","hard-bakery/detailed","hard-owners/none","hard-reversal/sparse","hard-quiet/none","hard-short/none","hard-stt/detailed"])
const asFixture=(fixture:GenerationFixture)=>fixture as unknown as Fixture

export const styleContext=(fixture:Fixture,notes:string)=>{
  const built=compile(fixture)
  return {transcriptCharacters:built.segments.reduce((sum,segment)=>sum+segment.text.length,0),transcriptWords:built.turns.reduce((sum,turn)=>sum+turn.text.split(/\s+/).length,0),typedNotes:notes,title:fixture.title}
}
/** With sparse typed notes: is each gold item on a topic the author did not note still there?
 * Soft, because a notes-as-outline policy may leave un-noted facts out by design; the rate is the point. */
export const unnotedChecks=(output:string,fixtureId:string,level:NotesLevel,gold:{facts:GoldItem[];decisions:GoldItem[]}):Check[]=>{
  const unnoted=SPARSE_UNNOTED[fixtureId]
  if(level!=="sparse"||!unnoted)return []
  const present=(item:GoldItem)=>required(output,[item],"x")[0]!.pass
  return [
    ...gold.facts.filter(item=>unnoted.facts.includes(item.id)).map(item=>({id:`unnoted-fact:${item.id}`,group:"notes-fidelity" as const,soft:true,pass:present(item),detail:item.text.slice(0,100)})),
    ...gold.decisions.filter(item=>unnoted.decisions.includes(item.id)).map(item=>({id:`unnoted-decision:${item.id}`,group:"notes-fidelity" as const,soft:true,pass:present(item),detail:item.text.slice(0,100)})),
  ]
}
const goldText=(fixture:GenerationFixture)=>[
  "Decisions:",...(fixture.gold.decisions.length?fixture.gold.decisions.map(item=>`- ${item.text}`):["- (none: nothing was decided)"]),
  "Action items (owner, due):",...(fixture.gold.actionItems.length?fixture.gold.actionItems.map(item=>`- ${item.text} [owner: ${item.unassigned?"NONE IDENTIFIABLE from what the assistant could see; naming any person as the owner is an error, leaving it unowned is correct":item.owner}${item.due?`; due: ${item.due}`:""}]`):["- (none: nobody took a task; a list of action items would be invented)"]),
  "Key facts and numbers:",...fixture.gold.facts.map(item=>`- ${item.text}`),
  "NOT said or NOT decided (stating any of these as fact is a hallucination):",...fixture.gold.notSaid.map(item=>`- ${item.claim}`),
].join("\n")

export const hardGenerationScenarios=(now:Date):Scenario[]=>GENERATION_FIXTURES.flatMap(source=>LEVELS.map((level):Scenario=>{
  const fixture=asFixture(source),notes=notesFor(fixture,level),roster=rosterOf(source)
  return {
    feature:"generation",id:`generation/${source.id}/${level}`,fixture:source.id,variant:`notes:${level}`,fast:FAST.has(`${source.id}/${level}`),input:{notesLevel:level},
    execute:target=>runGeneration(target,fixtureNote(fixture,{now,level}),meetingContextFor(fixture,now)),
    check:execution=>!execution.ok?[completed(execution)]:[
      completed(execution),
      ...required(execution.output,source.gold.decisions,"decision"),
      ...required(execution.output,source.gold.actionItems,"action"),
      ...required(execution.output,source.gold.facts,"fact"),
      // Same id as the shared `owners` check, so the standard report's "Owner attached" column includes these.
      ...source.gold.actionItems.filter(item=>!item.unassigned).flatMap(item=>{const verdict=ownerVerdict(execution.output,item,roster).verdict;return verdict==="absent"?[]:[{id:`owner:${item.id}`,group:"owner" as const,soft:true,pass:verdict==="correct",...(verdict==="correct"?{}:{detail:`task present without its owner (${item.owner})`})}]}),
      ...ownerChecks(execution.output,source.gold.actionItems,roster),
      ...forbidden(execution.output,source.gold.notSaid),
      ...(source.expect?.noActionItems?[noManufacturedActions(execution.output,source.expect.noActionItems.allowed,roster,notes)]:[]),
      ...generationFormatLines(execution.output,source.title),
      ...generationStyle(execution.output,styleContext(fixture,notes)),
      ...notesFidelity(execution.output,notes),
      ...typedNotesKept(execution.output,notes),
      ...unnotedChecks(execution.output,source.id,level,source.gold),
      ...grounding(execution.output,sourceText(fixture,notes,now)),
    ],
    judge:execution=>({feature:"generation",
      task:`Write the meeting notes for "${source.title}" (${source.kind}) from its transcript and the author's own typed notes. ${level==="none"?"The author typed nothing, so the outline must come from the transcript.":level==="sparse"?"The author typed a few fragments; they signal what the author cares about.":"The author typed detailed notes with their own headings; the result should be those notes, enriched."} The product shows title, date, duration and attendees separately, so the notes should not repeat them. ${SPEAKER_NOTE}`,
      source:truth(fixture),typedNotes:notes,gold:goldText(source),output:execution.output}),
  }
}))
