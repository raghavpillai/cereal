/**
 * Deterministic checks specific to generated meeting notes: the document
 * conventions (heading count and case, fragment bullets, the actions section),
 * fidelity to the author's typed notes, and who an action item is given to.
 *
 * These state the conventions independently of `src/ai/notes-generation.ts`
 * on purpose: the checks are the contract, the prompt is one way to meet it.
 * Structure checks are soft (reported, never failing a run) so "all hard checks
 * pass" stays comparable with runs from before they existed; inventing an
 * action item in a meeting that assigned none is hard.
 */
import type {Check,GoldAction,Pattern} from "./types"
import {generationFormat,matches,numbersIn} from "./checks"

const plain=(text:string)=>text.replace(/[*_`]/g,"").replace(/[‘’]/g,"'")
export const wordCount=(text:string)=>(text.match(/[A-Za-z0-9$£€%~][A-Za-z0-9$£€%.,'’/:-]*/g)??[]).length

// ------------------------------------------------------------ document shape
/** How many section headings a note may have, by transcript length in
 * characters: 4 below 8,000, 5 below 50,000, then one more for every further
 * 10,000, up to 8. The count includes the final actions section. */
export const maxHeadingsFor=(transcriptCharacters:number)=>transcriptCharacters<8_000?4:transcriptCharacters<50_000?5:transcriptCharacters<80_000?Math.floor(transcriptCharacters/10_000)+1:8
/** Upper bound on the length of the notes, in words: a share of what was said
 * that shrinks as the meeting grows, plus room for every word the author typed.
 * A ceiling, not a target: notes of about 125 words for a 250-word meeting, or
 * about 200 for a 500-word one, fit with room to spare. */
export const wordBudget=(transcriptWords:number,typedWords:number)=>Math.round(Math.min(140+0.18*transcriptWords,0.6*transcriptWords+30,1200))+typedWords

export interface Line {raw:string;text:string;kind:"heading"|"bullet"|"continuation"|"prose"|"blank";depth:number;
  /** The heading this line sits under, and the bullets it is nested in. */
  heading:string;ancestors:string[]
  /** For a bullet: its own text plus any plain lines that continue it. */
  full:string}
export function parseNotes(output:string):Line[] {
  const lines:Line[]=[]
  let heading=""
  const stack:Array<{indent:number;line:Line}>=[]
  const indents=output.split("\n").filter(line=>/^\s*[-*•]\s+\S/.test(line)).map(line=>/^\s*/.exec(line)![0].replace(/\t/g,"    ").length).filter(indent=>indent>0)
  const unit=Math.max(1,Math.min(4,...(indents.length?indents:[2])))
  for(const raw of output.split("\n")){
    const trimmed=raw.trim()
    if(!trimmed){lines.push({raw,text:"",kind:"blank",depth:0,heading,ancestors:[],full:""});continue}
    const head=/^(#{1,6})\s+(.+)$/.exec(trimmed)
    if(head){heading=plain(head[2]!).trim();stack.length=0;lines.push({raw,text:heading,kind:"heading",depth:0,heading,ancestors:[],full:heading});continue}
    const bullet=/^(\s*)(?:[-*•]|\d+[.)])\s+(.+)$/.exec(raw)
    if(bullet){
      const indent=bullet[1]!.replace(/\t/g,"    ").length
      while(stack.length&&stack.at(-1)!.indent>=indent)stack.pop()
      const text=plain(bullet[2]!).trim()
      const line:Line={raw,text,kind:"bullet",depth:Math.round(indent/unit),heading,ancestors:stack.map(item=>item.line.text),full:text}
      lines.push(line);stack.push({indent,line})
      continue
    }
    // An indented plain line directly under a bullet continues that bullet
    // (an action's deadline is written this way).
    const parent=stack.at(-1),text=plain(trimmed)
    if(parent&&/^\s+/.test(raw)){parent.line.full+=` ${text}`;lines.push({raw,text,kind:"continuation",depth:parent.line.depth,heading,ancestors:[...parent.line.ancestors,parent.line.text],full:text});continue}
    stack.length=0
    lines.push({raw,text,kind:"prose",depth:0,heading,ancestors:[],full:text})
  }
  return lines
}
const ACTION_HEADING=/^(?:action items?|next steps?|to.?dos?|follow.?ups?|actions?|tasks?|owners?|jobs|mine|for me|next)\b/i
export const isActionHeading=(heading:string)=>ACTION_HEADING.test(heading.trim())||/\bnext steps\b|\baction items\b/i.test(heading)
const MINOR=new Set("a an and as at but by for from in into of on or the to vs via with per".split(" "))
/** "Flour Supplier Switch", "Budget and Vendors": major words capitalised. */
export const isTitleCase=(heading:string)=>heading.split(/\s+/).filter(word=>/^[a-z]/i.test(word)).every((word,index)=>index>0&&MINOR.has(word.toLowerCase())||/^[A-Z0-9]/.test(word)||/^[a-z]+[A-Z]/.test(word))

export interface StyleContext {transcriptCharacters:number;transcriptWords:number;typedNotes:string;title:string}
export interface StyleMetrics {words:number;budget:number;headings:number;maxHeadings:number;bullets:number;subBullets:number;proseLines:number;maxDepth:number;medianBulletWords:number;longestBulletWords:number;
  sentenceBullets:number;actionHeading:string|null;actionBullets:number;actionBold:number;actionWithOwner:number;outputCharacters:number}
export function styleMetrics(output:string,context:StyleContext):StyleMetrics {
  const lines=parseNotes(output),bullets=lines.filter(line=>line.kind==="bullet"),headings=lines.filter(line=>line.kind==="heading")
  const lengths=bullets.map(line=>wordCount(line.text)).sort((a,b)=>a-b)
  const typedHeadings=context.typedNotes.split("\n").filter(line=>/^#+\s/.test(line)).length
  const cap=maxHeadingsFor(context.transcriptCharacters)
  const actions=bullets.filter(line=>line.depth===0&&isActionHeading(line.heading))
  return {words:wordCount(plain(output).replace(/^#+\s+/gm,"")),budget:wordBudget(context.transcriptWords,wordCount(context.typedNotes)),headings:headings.length,
    // The author's own headings are always kept, and the notes may add an actions section and one leftover section.
    maxHeadings:typedHeadings?Math.max(cap,typedHeadings+2):cap,bullets:bullets.length,subBullets:bullets.filter(line=>line.depth>0).length,proseLines:lines.filter(line=>line.kind==="prose").length,maxDepth:Math.max(0,...bullets.map(line=>line.depth)),
    medianBulletWords:lengths.length?lengths[Math.floor((lengths.length-1)/2)]!:0,longestBulletWords:lengths.at(-1)??0,
    sentenceBullets:bullets.filter(line=>/[a-z0-9)]\.$/.test(line.text)).length,
    actionHeading:headings.map(line=>line.text).find(isActionHeading)??null,actionBullets:actions.length,
    actionBold:actions.filter(line=>/^\s*[-*•]\s+\*\*[^*]+\*\*/.test(line.raw)).length,actionWithOwner:actions.filter(line=>/\*\*\s*\(([^()]+)\)\s*$/.test(line.raw.trim())).length,outputCharacters:output.length}
}
/** The document conventions, one soft check each, plus the raw metrics. */
export function generationStyle(output:string,context:StyleContext):Check[] {
  const metrics=styleMetrics(output,context),lines=parseNotes(output).filter(line=>line.kind!=="blank")
  const first=lines[0],last=lines.at(-1),headings=lines.filter(line=>line.kind==="heading").map(line=>line.text)
  const preamble=first&&first.kind==="prose"&&/^(?:here (?:are|is)|below (?:are|is)|these are|the following|meeting notes|summary of|notes (?:for|from)|sure|certainly|i(?:'ve| have))/i.test(first.text)
  const signoff=last&&last.kind==="prose"&&/(?:let me know|hope this helps|feel free|if you(?:'d| would) like|anything else|end of (?:notes|summary))/i.test(last.text)
  const typedActionHeading=context.typedNotes.split("\n").some(line=>/^#+\s/.test(line)&&isActionHeading(line.replace(/^#+\s+/,"")))
  const actionIndex=headings.findIndex(isActionHeading),lowerCased=headings.filter(heading=>!isTitleCase(heading))
  const actionLines=parseNotes(output).filter(line=>line.kind==="bullet"&&line.depth===0&&isActionHeading(line.heading))
  const actionPlaceholder=actionLines.some(line=>/\((?:no owner|unassigned|tbd|owner tbd)\)|^(?:unassigned|tbd|someone|team)\s*:/i.test(line.text))
  const actionSentence=actionLines.some(line=>/[a-z0-9)]\.$/.test(line.text))
  const check=(id:string,pass:boolean,detail:string):Check=>({id,group:"format",soft:true,pass,...(pass?{}:{detail})})
  return [
    check("style:heading-count",metrics.headings<=metrics.maxHeadings&&(metrics.headings>=1||context.transcriptWords<60),`${metrics.headings} headings, at most ${metrics.maxHeadings} for ${context.transcriptCharacters} transcript characters`),
    check("style:title-case-headings",lowerCased.length===0,`not title case: ${lowerCased.slice(0,3).map(heading=>`"${heading}"`).join(", ")}`),
    check("style:length-budget",metrics.words<=metrics.budget,`${metrics.words} words, budget ${metrics.budget} for a ${context.transcriptWords}-word meeting`),
    check("style:no-preamble-or-signoff",!preamble&&!signoff&&first?.kind!=="prose",`starts with "${first?.text.slice(0,50)}", ends with "${last?.text.slice(0,50)}"`),
    check("style:one-sub-level",metrics.maxDepth<=1,`nesting depth ${metrics.maxDepth}`),
    check("style:fragment-bullets",metrics.medianBulletWords<=12&&metrics.longestBulletWords<=30&&metrics.sentenceBullets<=Math.max(1,metrics.bullets*0.1),`median bullet ${metrics.medianBulletWords} words, longest ${metrics.longestBulletWords}, ${metrics.sentenceBullets} of ${metrics.bullets} end with a full stop`),
    check("style:actions-last",typedActionHeading||actionIndex<0||actionIndex===headings.length-1,`"${headings[actionIndex]}" is section ${actionIndex+1} of ${headings.length}`),
    // An action item is "Owner: task" in plain text (the earlier form, a bold task with the owner in parentheses, is
    // still accepted for stored outputs). Never a placeholder where an owner would go, and never a sentence.
    check("style:action-format",metrics.actionBullets===0||(metrics.actionBold===metrics.actionBullets||metrics.actionBold===0)&&!actionPlaceholder&&!actionSentence,`${metrics.actionBold} of ${metrics.actionBullets} action items are bold${actionPlaceholder?"; a placeholder stands in for an owner":""}${actionSentence?"; an action item is a sentence":""}`),
    {id:"style:metrics",group:"format",soft:true,pass:true,detail:JSON.stringify(metrics)},
  ]
}
/** `generationFormat` from checks.ts with one correction: a plain line that
 * continues a bullet (an action item's deadline) is part of that bullet, not
 * prose. Outputs without such lines are scored exactly as before. */
export function generationFormatLines(output:string,title:string):Check[] {
  const lines=parseNotes(output),bullets=lines.filter(line=>line.kind==="bullet").length,prose=lines.filter(line=>line.kind==="prose").length
  const pass=bullets>=3&&prose<=Math.max(2,bullets/3)
  return generationFormat(output,title).map(check=>check.id==="mostly-bullets"?{id:check.id,group:check.group,pass,...(pass?{}:{detail:`${bullets} bullets, ${prose} prose lines`})}:check)
}

// ------------------------------------------------------- typed-notes fidelity
const STOP=new Set("the a an and or but if so to of in on at by for from with without about as is are was were be been it its this that these those there here we you they he she i my our your their his her not no yes all any each every some most more less than then when while before after during until since also only just very still vs per via re w".split(" "))
const tokens=(text:string)=>(plain(text).toLowerCase().match(/[a-z0-9][a-z0-9.%-]*/g)??[]).map(word=>word.replace(/[.-]+$/,"")).filter(word=>word.length>=2&&!STOP.has(word))
const normal=(text:string)=>plain(text).toLowerCase().replace(/[^a-z0-9 ]+/g," ").replace(/\s+/g," ").trim()
/** The author's headings must all survive, in the author's order, and none of
 * the lines they typed may vanish. Stricter than `notesFidelity` in checks.ts,
 * which accepts 60% of headings and 70% of lines. */
export function typedNotesKept(output:string,notes:string):Check[] {
  if(!notes.trim())return []
  const outputHeadings=output.split("\n").filter(line=>/^#+\s/.test(line)).map(line=>normal(line.replace(/^#+\s+/,"")))
  const noteHeadings=notes.split("\n").filter(line=>/^#+\s/.test(line)).map(line=>line.replace(/^#+\s+/,"").trim())
  const position=(heading:string)=>{
    const wanted=normal(heading),words=wanted.split(" ").filter(word=>word.length>2)
    return outputHeadings.findIndex(candidate=>candidate===wanted||candidate.includes(wanted)||words.length>0&&words.every(word=>candidate.includes(word.replace(/s$/,""))))
  }
  const positions=noteHeadings.map(position),missing=noteHeadings.filter((_,index)=>positions[index]!<0)
  const kept=positions.filter(index=>index>=0),ordered=kept.every((index,i)=>i===0||index>kept[i-1]!)
  const outputTokens=new Set(tokens(output)),lower=normal(output),outputWords=[...new Set(lower.split(" "))],outputNumbers=numbersIn(output)
  // "yrs" is "years", "mktg" is "marketing", "mk5" is "Mark 5": good notes expand what the author abbreviated.
  const abbreviates=(word:string)=>{const letters=word.replace(/[^a-z]/g,"");if(letters.length<2)return false
    return outputWords.some(candidate=>{if(candidate[0]!==letters[0]||candidate.length<=letters.length)return false;let at=0;for(const character of candidate)if(character===letters[at])at++;return at===letters.length})}
  const lines=notes.split("\n").filter(line=>!/^#+\s/.test(line)).map(line=>line.replace(/^[\s*-]+/,"").trim()).filter(line=>tokens(line).length>=2)
  const dropped=lines.filter(line=>{
    const words=tokens(line),hit=words.filter(word=>outputTokens.has(word)||lower.includes(word.replace(/s$/,""))||abbreviates(word)).length
    const numbers=[...numbersIn(line)].filter(value=>value>12||!Number.isInteger(value))
    // A line also counts when every figure in it survives ("24.5k" as "£24,500").
    return hit/words.length<0.5&&!(numbers.length>0&&numbers.every(value=>outputNumbers.has(value)))
  })
  const checks:Check[]=[{id:"typed:no-line-dropped",group:"notes-fidelity",soft:true,pass:dropped.length===0,detail:dropped.length?`${dropped.length} of ${lines.length} typed lines not reflected: ${dropped.slice(0,4).map(line=>`"${line.slice(0,60)}"`).join("; ")}`:`${lines.length}/${lines.length} typed lines reflected`}]
  if(noteHeadings.length)checks.push({id:"typed:headings-all-in-order",group:"notes-fidelity",soft:true,pass:missing.length===0&&ordered,detail:missing.length?`missing typed headings: ${missing.map(heading=>`"${heading}"`).join(", ")}`:ordered?`${noteHeadings.length}/${noteHeadings.length} typed headings kept in order`:"typed headings kept but reordered"})
  return checks
}

// --------------------------------------------------------------------- owners
export type OwnerVerdict="correct"|"missing"|"wrong"|"unassigned-ok"|"unassigned-named"|"absent"
export interface OwnerResult {verdict:OwnerVerdict;named?:string;line?:string}
const escape=(text:string)=>text.replace(/[.*+?^${}()|[\]\\]/g,"\\$&")
/** A person named where an owner goes: in the parentheses that close an action
 * item, at the start of the bullet, before a verb of commitment, or in an
 * explicit owner slot. "Marcus confirms with Renata" names Renata, but not as
 * the owner. */
export function ownerPosition(text:string,name:string):boolean {
  const n=`${escape(name)}(?:\\s+[A-Z][\\w'-]+)?`,word=new RegExp(`\\b${escape(name)}\\b`,"i")
  // "(Yusuf)", "(Mateus and Dominic)", "(Tessa Lindgren, Joon)": parentheses holding only names.
  for(const group of text.matchAll(/\(([^()]{1,80})\)/g))if(/^(?:owners?:?\s*)?[A-Z][\w.'-]+(?:\s+[A-Z][\w.'-]+)?(?:\s*(?:,|&|\/|\band\b)\s*[A-Z][\w.'-]+(?:\s+[A-Z][\w.'-]+)?)*$/.test(group[1]!.trim())&&word.test(group[1]!))return true
  return new RegExp(`^(?:\\[[ x]\\]\\s*)?${n}(?:'s)?(?=\\s*(?:[:–—-]|,|and\\b|&|/|will\\b|to\\b|is\\b|has\\b|agreed\\b|owns\\b|should\\b|needs\\b|[a-z]+s\\b))|(?:^|[\\s(,;/&])${n}(?: will| to| is to| is going to| agreed to| should| needs to| owns| has to|'ll| volunteered| took| takes| offered)\\b|\\b(?:owner|owned by|assigned to|assignee|lead)[:\\s]+${n}\\b|[–—-]\\s*${n}\\s*$|@${n}\\b|\\band ${n}(?::| will| to)`,"i").test(text)
}
type Owned=GoldAction&{unassigned?:boolean;ownerNames?:string[]}
/** Who the notes give a gold action item to. Looks at the bullet that states the
 * task, the plain lines continuing it and the bullets it is nested under, so
 * "**Contract draft from Pallas** (Keiko)" with "Deadline: Wednesday." beneath,
 * "Keiko: contract draft by Wednesday", and a "Keiko" bullet with the task
 * under it all count. */
export function ownerVerdict(output:string,action:Owned,roster:ReadonlyArray<string>):OwnerResult {
  const items=parseNotes(output).filter(line=>line.kind==="bullet"||line.kind==="prose")
  const contexts=items.map(line=>({line,context:[...line.ancestors,line.full].join(" — ")}))
  let candidates=contexts.filter(item=>action.must.every(source=>matches(item.context,source)))
  // The task's parts are spread over several bullets: any bullet carrying one of them may hold the owner,
  // but such a loose match is never enough to call the owner wrong.
  let loose=false
  if(!candidates.length&&action.must.every(source=>matches(output,source))){candidates=contexts.filter(item=>action.must.some(source=>matches(item.context,source)));loose=true}
  if(!candidates.length)return {verdict:"absent"}
  // Prefer where the task is listed as an action over where it is merely mentioned.
  const listed=candidates.filter(item=>isActionHeading(item.line.heading));if(listed.length)candidates=listed
  const own=action.ownerNames??action.owner.split(/\s+and\s+|,\s*/).filter(name=>/^[A-Z]/.test(name))
  const texts=(item:{line:Line})=>[item.line.full,...item.line.ancestors]
  const names=(item:{line:Line})=>roster.filter(name=>texts(item).some(text=>ownerPosition(text,name)))
  if(action.unassigned){
    const clean=candidates.find(item=>!names(item).length),named=[...new Set(candidates.flatMap(names))]
    if(clean)return {verdict:"unassigned-ok",line:clean.line.text}
    return named.every(name=>own.includes(name))?{verdict:"unassigned-named",named:named.join(", "),line:candidates[0]!.line.text}:{verdict:"wrong",named:named.join(", "),line:candidates[0]!.line.text}
  }
  const correct=candidates.find(item=>(action.ownerMust&&matches(item.context,action.ownerMust))||own.some(name=>texts(item).some(text=>ownerPosition(text,name))))
  if(correct)return {verdict:"correct",line:correct.line.text}
  // Several bullets can touch the same task ("Confirm the demo with Renata" / "Hold the 19th (Tessa)"):
  // it is given to the wrong person only if every one of them names somebody else.
  const perCandidate=candidates.map(item=>names(item).filter(name=>!own.includes(name))),unnamed=candidates.find((_,index)=>!perCandidate[index]!.length)
  return unnamed||loose?{verdict:"missing",line:(unnamed??candidates[0]!).line.text}:{verdict:"wrong",named:[...new Set(perCandidate.flat())].join(", "),line:candidates[0]!.line.text}
}
/** Two soft checks per action item that is present: is the owner named, and is
 * the named owner the right one. Unowned tasks pass when they stay unowned. */
export function ownerChecks(output:string,actions:ReadonlyArray<Owned>,roster:ReadonlyArray<string>):Check[] {
  return actions.flatMap(action=>{
    const result=ownerVerdict(output,action,roster)
    if(result.verdict==="absent")return []
    const detail=`${result.verdict}${result.named?` (${result.named})`:""}: ${action.unassigned?"no identifiable owner":action.owner} — "${(result.line??"").slice(0,110)}"`
    return [
      {id:`owner-named:${action.id}`,group:"owner" as const,soft:true,pass:result.verdict==="correct"||result.verdict==="unassigned-ok",detail},
      {id:`owner-wrong:${action.id}`,group:"owner" as const,soft:true,pass:result.verdict!=="wrong",detail},
    ]
  })
}
/** In a meeting that assigned nothing, a to-do list is an invention. A to-do the
 * author typed for themselves is theirs, not an invention, and is not counted. */
export function noManufacturedActions(output:string,allowed:ReadonlyArray<Pattern>,roster:ReadonlyArray<string>,typedNotes=""):Check {
  const typed=typedNotes.split("\n").map(line=>tokens(line)).filter(words=>words.length>=2)
  const fromNotes=(text:string)=>{const have=tokens(text);return typed.some(words=>words.filter(word=>have.some(other=>other===word||word.length>=5&&other.startsWith(word.slice(0,5)))).length/words.length>=0.5)}
  const lines=parseNotes(output).filter(line=>line.kind==="bullet"&&isActionHeading(line.heading)&&!fromNotes(line.full))
  const invented=lines.filter(line=>roster.some(name=>ownerPosition(line.full,name))||!allowed.some(source=>matches(line.full,source)))
  return {id:"no-manufactured-action-items",group:"hallucination",pass:invented.length===0,...(invented.length?{detail:`lists ${invented.length} action item(s) in a meeting that assigned none: "${invented[0]!.text.slice(0,100)}"`}:{})}
}
