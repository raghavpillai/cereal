import type {NoteDTO} from "@meeting-notes/contracts"
import type {Check,Feature,Fixture,RunResult} from "./types"
import type {Execution,Target} from "./harness"
import type {JudgeInput} from "./judge"
import {compile,meetingContextFor} from "./fixtures"
import {renderTruth} from "./transcript"
import {claimBefore} from "./checks"

/** One evaluated case. To add one, append to the list a scenario file returns:
 * say how to run it (`execute`), what must be true of the output (`check`) and
 * what the judge needs to grade it (`judge`). */
export interface Scenario {
  feature:Feature
  /** Stable, unique: `<feature>/<fixture>/<case>`. Used by `--only`. */
  id:string
  fixture:string;variant:string
  /** Part of the `--fast` subset. */
  fast:boolean
  input:RunResult["input"]
  context?:RunResult["context"]
  execute:(target:Target)=>Promise<Execution>
  check:(execution:Execution)=>Check[]
  judge:(execution:Execution)=>JudgeInput
}
export const completed=(execution:Execution):Check=>({id:"completed",group:"format",pass:execution.ok&&execution.output.trim().length>0,...(execution.ok&&execution.output.trim()?{}:{detail:execution.error??"empty output"})})

export const goldText=(fixture:Fixture)=>[
  "Decisions:",...fixture.gold.decisions.map(item=>`- ${item.text}`),
  "Action items (owner, due):",...fixture.gold.actionItems.map(item=>`- ${item.text} [owner: ${item.owner}${item.due?`; due: ${item.due}`:""}]`),
  "Key facts and numbers:",...fixture.gold.facts.map(item=>`- ${item.text}`),
  "NOT said or NOT decided (stating any of these as fact is a hallucination):",...fixture.gold.notSaid.map(item=>`- ${item.claim}`),
].join("\n")
/** Everything a faithful output may draw on, for the grounding checks. */
export const sourceText=(fixture:Fixture,notes:string,now:Date,extra="")=>{
  const meeting=meetingContextFor(fixture,now)
  return [fixture.title,notes,compile(fixture).turns.map(turn=>turn.text).join("\n"),Object.keys(fixture.people).join(" "),meeting?JSON.stringify(meeting):"",extra].join("\n")
}
export const truth=(fixture:Fixture)=>renderTruth(compile(fixture).turns)
export const SPEAKER_NOTE="The assistant did not see speaker names. It saw the transcript as the product stores it: anonymous labels such as \"Speaker 1\" that restart in every uploaded audio part, on microphone/system channels, with one voice's words within a part merged into one segment. Names in its output can only come from what people said, the typed notes or the attendee list. Do not penalise it for leaving a speaker unidentified; do penalise a wrong attribution."
/** Citation claim/passage pairs for the judge. */
export function citedClaims(execution:Execution,passage:(citation:Execution["citations"][number])=>string|undefined) {
  const byId=new Map(execution.citations.map(citation=>[citation.id,citation])),seen=new Set<string>(),result:Array<{marker:string;claim:string;passage:string}>=[]
  for(const match of execution.output.matchAll(/\[\[(S\d+)\]\]/g)){
    const citation=byId.get(match[1]!)
    if(!citation||seen.has(match[1]!)||result.length>=6)continue
    seen.add(match[1]!)
    result.push({marker:match[1]!,claim:claimBefore(execution.output,match.index).slice(0,400),passage:(passage(citation)??citation.quote).slice(0,700)})
  }
  return result
}
export const resolver=(notes:ReadonlyArray<NoteDTO>)=>(citation:{source:string;noteId:string;segmentId:string|null}):string|undefined=>{
  const note=notes.find(candidate=>candidate.id===citation.noteId)
  if(!note)return undefined
  return citation.source==="transcript"?note.transcriptSegments.find(segment=>segment.id===citation.segmentId)?.text:citation.source==="notes"?note.body:note.enhanced
}
