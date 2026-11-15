/**
 * Fixtures used only by note generation: the hard cases (long and dense
 * meetings, reversals, speech-to-text errors, meetings where nothing was
 * decided, very short meetings, first-person ownership).
 *
 * They are deliberately NOT listed in `lib/fixtures.ts`, so they do not create
 * note-chat, workspace-chat or live-chat scenarios. `split` separates the
 * fixtures the generation prompt was tuned against from the held-out ones that
 * were only ever run to report a number.
 */
import type {Fixture,GoldAction,Pattern} from "./types"
import {compile} from "./fixtures"
import {pattern} from "./transcript"
import {reversal} from "../fixtures/generation/reversal"
import {owners} from "../fixtures/generation/owners"
import {stt} from "../fixtures/generation/stt"
import {quiet} from "../fixtures/generation/quiet"
import {short} from "../fixtures/generation/short"
import {long} from "../fixtures/generation/long"
import {bakery} from "../fixtures/generation/bakery"
import {heldReversal} from "../fixtures/generation/held-reversal"
import {heldOwners} from "../fixtures/generation/held-owners"
import {heldQuiet} from "../fixtures/generation/held-quiet"
import {heldStt} from "../fixtures/generation/held-stt"
import {heldLong} from "../fixtures/generation/held-long"

export interface GenerationAction extends GoldAction {
  /** Names that count as this owner, when a transcription variant is acceptable. Defaults to `owner`. */
  ownerNames?:string[]
  /** Nobody identifiable committed to this: the honest owner is "unassigned".
   * `owner` then records who it really was (for the judge) and `ownerMust` is unused. */
  unassigned?:boolean
}
export interface GenerationFixture extends Omit<Fixture,"gold"|"questions"> {
  split:"tuned"|"held-out"
  /** One line on what the fixture stresses, for the report. */
  stress:string
  gold:Omit<Fixture["gold"],"actionItems">&{actionItems:GenerationAction[]}
  questions:[]
  /** First names (and transcription variants) that count as people when the
   * checks look for who an action item was given to. Defaults to the speakers
   * plus attendee first names. */
  roster?:string[]
  expect?:{
    /** Nothing was assigned in this meeting: any bullet under an action-items or
     * next-steps heading must match one of these patterns (or the list must be absent). */
    noActionItems?:{allowed:Pattern[]}
  }
}
export const TUNED:ReadonlyArray<GenerationFixture>=[bakery,reversal,owners,stt,quiet,short,long]
export const HELD_OUT:ReadonlyArray<GenerationFixture>=[heldReversal,heldOwners,heldQuiet,heldStt,heldLong]
export const GENERATION_FIXTURES:ReadonlyArray<GenerationFixture>=[...TUNED,...HELD_OUT]

/** Gold facts and decisions whose topic the SPARSE typed notes do not touch, per
 * fixture (shared fixtures included). Used to measure what an outline-following
 * policy costs: these are exactly the items a strict "typed notes are the
 * filter" reading may drop. Annotated by hand from each fixture's sparse notes. */
export const SPARSE_UNNOTED:Readonly<Record<string,{facts:string[];decisions:string[]}>>={
  standup:{facts:[],decisions:[]},
  discovery:{facts:["reconcile","timing","pricing"],decisions:["pilot-shape"]},
  "one-on-one":{facts:["leave","feedback"],decisions:["review-before-leave"]},
  interview:{facts:["dedup"],decisions:[]},
  brainstorm:{facts:["budget"],decisions:["no-date"]},
  planning:{facts:["sms-late","tickets","nps","estimate-overrun","support-headcount"],decisions:["csv-export","reschedule-rules"]},
  crosstalk:{facts:["revenue","fix"],decisions:["blameless"]},
  numbers:{facts:["arr","margin","churn","pipeline","cac"],decisions:["clean-pipeline"]},
  "hard-bakery":{facts:["sales"],decisions:[]},
  "hard-reversal":{facts:["cost","artwork-date","booth"],decisions:[]},
  "hard-owners":{facts:["analytics-gap"],decisions:[]},
  "hard-stt":{facts:["reminders"],decisions:[]},
  "hard-quiet":{facts:[],decisions:[]},
  "hard-short":{facts:[],decisions:[]},
  "hard-long":{facts:["return-rate","concentration","response-time","loyalty","inventory"],decisions:["returns-45","porto","support-hires","paid-tier"]},
  "held-reversal":{facts:["marquee-quote","next-meeting"],decisions:[]},
  "held-owners":{facts:["stores","signoff","no-cost","timing"],decisions:[]},
  "held-quiet":{facts:["premium","deductible","brightpath","commuter","rates-start"],decisions:[]},
  "held-stt":{facts:["length","release-form"],decisions:["shape"]},
  "held-long":{facts:["tank","ozone","contingency","waitlist","progress"],decisions:["change-orders","school-bookings"]},
}

export const rosterOf=(fixture:Pick<GenerationFixture,"people"|"attendees"|"roster">|Fixture):string[]=>[...new Set([
  ...Object.keys(fixture.people),...(fixture.attendees??[]).map(person=>person.name.split(" ")[0]!),...(("roster" in fixture&&fixture.roster)||[]),
])]

/** Authoring checks, run offline by `eval/generation.test.ts`. */
export function validateGenerationFixtures() {
  const ids=new Set<string>()
  return GENERATION_FIXTURES.map(fixture=>{
    if(ids.has(fixture.id))throw new Error(`Duplicate fixture ${fixture.id}`)
    ids.add(fixture.id)
    if(!fixture.id.startsWith(fixture.split==="tuned"?"hard-":"held-"))throw new Error(`${fixture.id}: id must start with hard- (tuned) or held- (held-out)`)
    const built=compile(fixture as unknown as Fixture),truth=built.turns.map(turn=>turn.text).join("\n")
    for(const capture of ["desktop","ios"] as const)compile(fixture as unknown as Fixture,capture)
    const items=[...fixture.gold.decisions,...fixture.gold.actionItems,...fixture.gold.facts]
    for(const item of items){
      if(!item.must.length)throw new Error(`${fixture.id}/${item.id}: no patterns`)
      for(const source of item.must)pattern(source)
    }
    for(const item of fixture.gold.actionItems)if(!item.unassigned)pattern(item.ownerMust)
    for(const item of fixture.gold.notSaid)for(const source of item.forbid)pattern(source)
    for(const source of fixture.expect?.noActionItems?.allowed??[])pattern(source)
    return {id:fixture.id,split:fixture.split,kind:fixture.kind,capture:fixture.capture,turns:built.turns.length,segments:built.segments.length,
      words:built.turns.reduce((sum,turn)=>sum+turn.text.split(/\s+/).length,0),characters:truth.length,duration:built.duration,
      decisions:fixture.gold.decisions.length,actions:fixture.gold.actionItems.length,facts:fixture.gold.facts.length,notSaid:fixture.gold.notSaid.length,stress:fixture.stress}
  })
}
