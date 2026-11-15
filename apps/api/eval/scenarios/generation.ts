/** Meeting-note generation at each level of typed notes: none, sparse fragments, detailed. */
import {FIXTURES,fixtureNote,meetingContextFor,notesFor} from "../lib/fixtures"
import {runGeneration} from "../lib/harness"
import {forbidden,grounding,notesFidelity,owners,required} from "../lib/checks"
import {generationFormatLines,generationStyle,ownerChecks,typedNotesKept} from "../lib/generation-checks"
import {rosterOf} from "../lib/generation-fixtures"
import {hardGenerationScenarios,styleContext,unnotedChecks} from "./generation-hard"
import {completed,goldText,sourceText,truth,SPEAKER_NOTE,type Scenario} from "../lib/scenario"
import type {NotesLevel} from "../lib/types"

const LEVELS:ReadonlyArray<NotesLevel>=["none","sparse","detailed"]
/** The fast subset: one short meeting at every level, plus the numbers and
 * discovery meetings at one level each. */
const FAST=new Set(["standup/none","standup/sparse","standup/detailed","numbers/sparse","discovery/none"])

/** The shared meetings at three typed-notes levels, then the generation-only hard cases. */
export const generationScenarios=(now:Date):Scenario[]=>[...sharedGenerationScenarios(now),...hardGenerationScenarios(now)]
const sharedGenerationScenarios=(now:Date):Scenario[]=>FIXTURES.flatMap(fixture=>LEVELS.map((level):Scenario=>{
  const notes=notesFor(fixture,level)
  return {
    feature:"generation",id:`generation/${fixture.id}/${level}`,fixture:fixture.id,variant:`notes:${level}`,fast:FAST.has(`${fixture.id}/${level}`),input:{notesLevel:level},
    execute:target=>runGeneration(target,fixtureNote(fixture,{now,level}),meetingContextFor(fixture,now)),
    check:execution=>!execution.ok?[completed(execution)]:[
      completed(execution),
      ...required(execution.output,fixture.gold.decisions,"decision"),
      ...required(execution.output,fixture.gold.actionItems,"action"),
      ...required(execution.output,fixture.gold.facts,"fact"),
      ...owners(execution.output,fixture.gold.actionItems),
      ...forbidden(execution.output,fixture.gold.notSaid),
      // Soft: who each action is given to (named, and not wrongly named), and the document conventions.
      ...ownerChecks(execution.output,fixture.gold.actionItems,rosterOf(fixture)),
      ...generationFormatLines(execution.output,fixture.title),
      ...generationStyle(execution.output,styleContext(fixture,notes)),
      ...notesFidelity(execution.output,notes),
      ...typedNotesKept(execution.output,notes),
      ...unnotedChecks(execution.output,fixture.id,level,fixture.gold),
      ...grounding(execution.output,sourceText(fixture,notes,now)),
    ],
    judge:execution=>({feature:"generation",
      task:`Write the meeting notes for "${fixture.title}" (${fixture.kind}) from its transcript and the author's own typed notes. ${level==="none"?"The author typed nothing, so the outline must come from the transcript.":level==="sparse"?"The author typed a few fragments; they signal what the author cares about.":"The author typed detailed notes with their own headings; the result should be those notes, enriched."} The product shows title, date, duration and attendees separately, so the notes should not repeat them. ${SPEAKER_NOTE}`,
      source:truth(fixture),typedNotes:notes,gold:goldText(fixture),output:execution.output}),
  }
}))
