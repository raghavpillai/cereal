/** Deterministic checks. Hard checks decide pass/fail; soft checks are review
 * aids whose false-positive rate is too high to fail a run on their own. */
import type {Check,GoldAction,GoldItem,NotSaid,Pattern} from "./types"
import {pattern} from "./transcript"

/** Markdown emphasis, typographic apostrophes and hyphenation must not decide a
 * check: "crash-reporting" and "crash reporting" are the same answer. */
const plain=(text:string)=>text.replace(/[*_`]/g,"").replace(/[\u2018\u2019]/g,"'").replace(/(?<=[A-Za-z0-9])[\u2010-\u2015-](?=[A-Za-z0-9])/g," ")
export const matches=(text:string,source:Pattern)=>pattern(source).test(plain(text))
/** A forbidden claim counts only when it is asserted: "not $30,000" or "no
 * launch date was set" must not fail an answer that got it right. */
export function asserted(text:string,source:Pattern):string|null {
  const body=plain(text),matcher=new RegExp(source,"gi")
  for(const match of body.matchAll(matcher)){
    // "the calendar and two-way sync are not in Q3" is itself a denial.
    if(/\b(?:not|no|never)\b|n't\b/i.test(match[0]))continue
    const before=body.slice(Math.max(0,match.index-70),match.index),sentence=before.slice(Math.max(before.lastIndexOf(". "),before.lastIndexOf("\n"))+1)
    if(/\b(?:not|no|never|without|rather than|instead of|isn't|wasn't|weren't|didn't|doesn't|don't|cannot|can't|nobody|none|neither|incorrect(?:ly)?|wrong(?:ly)?|mistaken(?:ly)?|misstated|misspoke|originally|initially|first said|corrected from|told|informed|messaged)\b[^.\n]*$/i.test(sentence))continue
    const after=body.slice(match.index+match[0].length,match.index+match[0].length+60)
    if(/^[^.\n]{0,50}\b(?:was (?:not|wrong|incorrect|rejected|corrected)|is (?:not|wrong|incorrect)|not (?:agreed|approved|decided))\b/i.test(after))continue
    return match[0]
  }
  return null
}
export const required=(output:string,items:ReadonlyArray<GoldItem>,prefix:string):Check[]=>items.map(item=>{
  const missing=item.must.filter(source=>!matches(output,source))
  return {id:`${prefix}:${item.id}`,group:"required",pass:missing.length===0,...(missing.length?{detail:`missing /${missing.join("/, /")}/ — ${item.text}`}:{})}
})
export const owners=(output:string,items:ReadonlyArray<GoldAction>):Check[]=>items.filter(item=>item.must.every(source=>matches(output,source))).map(item=>({id:`owner:${item.id}`,group:"owner",pass:matches(output,item.ownerMust),soft:true,...(matches(output,item.ownerMust)?{}:{detail:`task present without its owner (${item.owner})`})}))
export const forbidden=(output:string,items:ReadonlyArray<NotSaid>):Check[]=>items.map(item=>{
  const hit=item.forbid.map(source=>asserted(output,source)).find(Boolean)
  return {id:`not-said:${item.id}`,group:"hallucination",pass:!hit,...(hit?{detail:`asserts "${hit}" — ${item.claim}`}:{})}
})
export const mustAll=(output:string,sources:ReadonlyArray<Pattern>,id:string,group:Check["group"]="required"):Check=>{
  const missing=sources.filter(source=>!matches(output,source))
  return {id,group,pass:missing.length===0,...(missing.length?{detail:`missing /${missing.join("/, /")}/`}:{})}
}
export const mustNot=(output:string,sources:ReadonlyArray<Pattern>,id:string,group:Check["group"]="hallucination"):Check=>{
  const hit=sources.map(source=>asserted(output,source)).find(Boolean)
  return {id,group,pass:!hit,...(hit?{detail:`asserts "${hit}"`}:{})}
}

// ---------------------------------------------------------------- grounding
const UNITS:Record<string,number>={zero:0,one:1,two:2,three:3,four:4,five:5,six:6,seven:7,eight:8,nine:9,ten:10,eleven:11,twelve:12,thirteen:13,fourteen:14,fifteen:15,sixteen:16,seventeen:17,eighteen:18,nineteen:19}
const TENS:Record<string,number>={twenty:20,thirty:30,forty:40,fifty:50,sixty:60,seventy:70,eighty:80,ninety:90}
const SCALES:Record<string,number>={hundred:100,thousand:1e3,million:1e6,billion:1e9}
const ORDINALS:Record<string,number>={first:1,second:2,third:3,fourth:4,fifth:5,sixth:6,seventh:7,eighth:8,ninth:9,tenth:10,eleventh:11,twelfth:12,thirteenth:13,fourteenth:14,fifteenth:15,sixteenth:16,seventeenth:17,eighteenth:18,nineteenth:19,twentieth:20,thirtieth:30}
const round=(value:number)=>Number(value.toPrecision(6))
/** Every number a text states, as values: digits ("4.82M", "12,000", "31%")
 * and spoken forms ("four point eight two million", "one hundred forty eight"). */
export function numbersIn(text:string):Set<number> {
  const values=new Set<number>(),add=(value:number)=>{if(Number.isFinite(value))values.add(round(value))}
  const cleaned=text.replace(/\[\[S\d+\]\]/g," ")
  for(const match of cleaned.matchAll(/(?<![A-Za-z\d.])(\d{1,3}(?:,\d{3})+|\d+)(\.\d+)?\s?(k|m|bn|b|million|thousand|billion)?(?![A-Za-z\d])/gi)){
    const base=Number(match[1]!.replace(/,/g,"")+(match[2]??"")),unit=(match[3]??"").toLowerCase()
    add(base);if(unit)add(base*(unit==="k"||unit==="thousand"?1e3:unit==="m"||unit==="million"?1e6:1e9))
  }
  // Spoken forms, one clause at a time so a comma or full stop ends a number.
  for(const clause of cleaned.toLowerCase().replace(/\ba (?=hundred|thousand|million|billion)/g,"one ").split(/[,;:!?()\n]|\.(?=\s|$)/)){
    const words=clause.split(/[^a-z0-9]+/).filter(Boolean)
    for(let i=0;i<words.length;i++){
      if(!(words[i]! in UNITS)&&!(words[i]! in TENS)&&!(words[i]! in ORDINALS))continue
      let total=0,current=0,fraction="",j=i
      for(;j<words.length;j++){
        const word=words[j]!
        if(word in UNITS)current+=UNITS[word]!
        else if(word in TENS)current+=TENS[word]!
        else if(word in ORDINALS){current+=ORDINALS[word]!;j++;break}
        else if(word==="hundred")current*=100
        else if(word==="thousand"||word==="million"||word==="billion"){const mantissa=current+(fraction?Number(`0.${fraction}`):0);add(mantissa);total+=mantissa*SCALES[word]!;current=0;fraction=""}
        // "one hundred and forty", but not "sixty five and seventy five".
        else if(word==="and"&&words[j-1]==="hundred"&&words[j+1]!==undefined&&(words[j+1]! in UNITS||words[j+1]! in TENS))continue
        else if(word==="and"&&words[j+1]==="a"&&words[j+2]==="half"&&!fraction){fraction="5";j+=2}
        else if(word==="point"){
          let k=j+1
          // "four point eight two" reads digit by digit; "two point fourteen" reads one number.
          while(k<words.length&&words[k]! in UNITS){const digit=UNITS[words[k]!]!;if(digit>=10){if(!fraction){fraction=String(digit);k++}break}fraction+=String(digit);k++}
          if(!fraction)break
          j=k-1
        }else break
      }
      add(total+current+(fraction?Number(`0.${fraction}`):0))
      if(total&&current){add(total);add(current)}
      i=Math.max(i,j-1)
    }
  }
  return values
}
/** Numbers in the output that the source never states. Small counts and values
 * derivable by one addition, subtraction or percentage from stated numbers are
 * accepted, because correct answers legitimately compute them. */
export function ungroundedNumbers(output:string,source:string):number[] {
  const stated=[...numbersIn(source)],known=new Set(stated),result:number[]=[]
  const derived=(value:number,ratios:boolean)=>{
    for(const a of stated)for(const b of stated){
      if(round(a+b)===value||round(a-b)===value||round(a*b)===value)return true
      // Shares and changes ("5.5% under plan"), within rounding of what a person would write.
      if(ratios&&b!==0&&value>=0.1&&(Math.abs(a/b*100-value)<0.6||Math.abs(Math.abs((a-b)/b*100)-value)<0.6||round(a/b)===value))return true
    }
    return false
  }
  for(const value of numbersIn(output.replace(/^\s*\d+[.)]\s/gm," ").replace(/^#+.*$/gm,match=>match))){
    if(known.has(value)||Number.isInteger(value)&&value<=12)continue
    if([1e3,1e6,1e9].some(scale=>known.has(round(value/scale))||known.has(round(value*scale))))continue
    // "$280,000" from "5.1 million" minus "4.82 million": try the value at each scale.
    if(stated.length<=400&&[1,1e3,1e6].some(scale=>derived(round(value/scale),scale===1)))continue
    result.push(value)
  }
  return result
}
const COMMON=new Set("the a an and or but if so to of in on at by for from with without about as is are was were be been it its this that these those there here we you they he she i my our your their his her not no yes all any each every some most more less than then when while before after during until since because however also only just very still already yet first second third next last new final key main total overall current open action decision decisions items item notes note summary meeting discussion context risk risks status update updates today tomorrow yesterday monday tuesday wednesday thursday friday saturday sunday january february march april may june july august september october november december q1 q2 q3 q4 utc usd eur none unknown unassigned owner owners deadline deadlines source sources transcript speaker team customer customers company what who why how where which per vs via".split(" "))
/** Capitalised words used mid-sentence that occur nowhere in the source: a
 * cheap detector for invented people, companies and products. */
export function ungroundedNames(output:string,source:string):string[] {
  const vocabulary=new Set(source.toLowerCase().match(/[a-z][a-z'’-]+/g)??[]),found=new Set<string>()
  for(const line of plain(output).split("\n")){
    if(/^\s*#/.test(line))continue
    // Not after a colon: "Owner: Send the quote" starts a phrase, it does not name anyone.
    for(const match of line.matchAll(/(?<=[a-z,;] )([A-Z][a-z]{2,})(?:'s)?\b/g)){
      const word=match[1]!.toLowerCase()
      if(!COMMON.has(word)&&!vocabulary.has(word)&&!vocabulary.has(word.replace(/s$/,"")))found.add(match[1]!)
    }
  }
  return [...found]
}
export const grounding=(output:string,source:string):Check[]=>{
  const values=ungroundedNumbers(output,source),names=ungroundedNames(output,source)
  return [
    {id:"numbers-in-source",group:"grounding",soft:true,pass:values.length===0,...(values.length?{detail:`not stated in the source: ${values.slice(0,12).join(", ")}`}:{})},
    {id:"names-in-source",group:"grounding",soft:true,pass:names.length===0,...(names.length?{detail:`not in the source: ${names.slice(0,12).join(", ")}`}:{})},
  ]
}

// ------------------------------------------------------------------- format
const INTERNAL=/\b(?:mic|system):[0-9a-f-]{36}:(?:speaker_\d+|unknown)\b|\b[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}\b|\bspeakerId\b|\bsegmentId\b|\btranscriptRevision\b/i
export const noInternalIdentifiers=(output:string):Check=>{
  const hit=INTERNAL.exec(output)
  return {id:"no-internal-identifiers",group:"format",pass:!hit,...(hit?{detail:`exposes "${hit[0]}"`}:{})}
}
/** The generation prompt's own format contract. */
export function generationFormat(output:string,title:string):Check[] {
  const lines=output.split("\n"),headings=lines.filter(line=>/^#{1,6}\s/.test(line)),top=lines.filter(line=>/^#\s+\S/.test(line))
  const bullets=lines.filter(line=>/^\s*[-*]\s+\S/.test(line)),prose=lines.filter(line=>line.trim()&&!/^\s*([-*]|#{1,6}|\d+[.)])\s/.test(line))
  const generic=top.find(line=>/^#\s+(?:meeting )?(?:notes|summary|overview|recap|minutes)\s*$/i.test(line)),titled=top.find(line=>line.replace(/^#\s+/,"").trim().toLowerCase()===title.toLowerCase())
  const placeholder=/\b(?:TBD|TODO|N\/A)\b|\[(?:insert|placeholder|name|date)[^\]]*\]|<[a-z ]+>/i.exec(output)
  return [
    {id:"level-one-headings",group:"format",pass:top.length>=1&&headings.every(line=>/^#\s/.test(line)),...(top.length>=1&&headings.every(line=>/^#\s/.test(line))?{}:{detail:`${top.length} level-one of ${headings.length} headings`})},
    {id:"mostly-bullets",group:"format",pass:bullets.length>=3&&prose.length<=Math.max(2,bullets.length/3),...(bullets.length>=3&&prose.length<=Math.max(2,bullets.length/3)?{}:{detail:`${bullets.length} bullets, ${prose.length} prose lines`})},
    {id:"no-generic-or-title-heading",group:"format",pass:!generic&&!titled,...(generic||titled?{detail:`heading "${generic??titled}"`}:{})},
    {id:"no-placeholders",group:"format",pass:!placeholder,...(placeholder?{detail:`contains "${placeholder[0]}"`}:{})},
    {id:"no-code-fence-or-markers",group:"format",pass:!/^```/m.test(output)&&!/\[\[S\d+\]\]/.test(output)},
    noInternalIdentifiers(output),
  ]
}
/** Fidelity to the author's typed notes: their headings should organise the
 * result, in their order, and their own lines should survive. */
export function notesFidelity(output:string,notes:string):Check[] {
  if(!notes.trim())return []
  const tokens=(text:string)=>new Set((text.toLowerCase().match(/[a-z0-9][a-z0-9.%-]{2,}/g)??[]).filter(word=>!COMMON.has(word)))
  const outputTokens=tokens(output),lower=plain(output).toLowerCase()
  const noteHeadings=notes.split("\n").filter(line=>/^#+\s/.test(line)).map(line=>line.replace(/^#+\s+/,"").trim())
  const outputHeadings=output.split("\n").filter(line=>/^#+\s/.test(line)).map(line=>line.replace(/^#+\s+/,"").trim().toLowerCase())
  const normal=(text:string)=>plain(text).toLowerCase().replace(/[^a-z0-9 ]+/g," ").replace(/\s+/g," ").trim()
  const position=(heading:string)=>{
    const wanted=normal(heading),words=wanted.split(" ").filter(word=>word.length>2)
    return outputHeadings.findIndex(candidate=>{const have=normal(candidate);return !!wanted&&(have.includes(wanted)||words.length>0&&words.filter(word=>have.includes(word.replace(/s$/,""))).length>=Math.ceil(words.length/2))})
  }
  const positions=noteHeadings.map(position),kept=positions.filter(index=>index>=0)
  const ordered=kept.every((index,i)=>i===0||index>=kept[i-1]!)
  const lines=notes.split("\n").map(line=>line.replace(/^[#\s*-]+/,"").trim()).filter(line=>tokens(line).size>=2)
  const survived=lines.filter(line=>{const words=[...tokens(line)];return words.filter(word=>outputTokens.has(word)||lower.includes(word.replace(/s$/,""))).length/words.length>=0.5})
  const checks:Check[]=[{id:"typed-lines-kept",group:"notes-fidelity",soft:true,pass:lines.length===0||survived.length/lines.length>=0.7,detail:`${survived.length}/${lines.length} typed lines reflected`}]
  if(noteHeadings.length>=2)checks.push({id:"typed-headings-kept",group:"notes-fidelity",soft:true,pass:kept.length/noteHeadings.length>=0.6&&ordered,detail:`${kept.length}/${noteHeadings.length} typed headings used${ordered?"":", order changed"}`})
  return checks
}
export function chatFormat(output:string,question:string):Check[] {
  const checks:Check[]=[noInternalIdentifiers(output),{id:"no-raw-json",group:"format",pass:!/^\s*[[{]"/m.test(output)}]
  if(/\b(?:recent|latest)\b[\s\S]{0,60}\b(?:to-?dos?|action items?)\b|\blist (?:my )?(?:recent )?todos\b/i.test(question)&&!/\b(?:table|tabular|csv)\b/i.test(question))
    checks.push({id:"recent-todos-not-a-table",group:"format",pass:!/^\s*\|.*\|\s*$/m.test(output)})
  return checks
}

// ---------------------------------------------------------------- citations
const STOP=new Set([...COMMON,"said","says","will","would","have","has","had","can","could","should","them","they","been","being","into","over","under","than","that's","it's","i'll","we'll","i'm","we're"])
const content=(text:string)=>new Set((plain(text).toLowerCase().match(/[a-z0-9][a-z0-9.]{2,}/g)??[]).filter(word=>!STOP.has(word)).map(word=>word.replace(/(?:ing|ed|es|s)$/,"")))
/** The sentence or bullet a citation marker is attached to. Markers follow the
 * full stop, so trailing markers and punctuation are dropped before looking back. */
export function claimBefore(output:string,markerIndex:number):string {
  const before=output.slice(0,markerIndex).replace(/(?:\s|\[\[S\d+\]\])+$/,"").replace(/[.!?]+$/,"")
  return before.slice(Math.max(before.lastIndexOf("\n"),before.lastIndexOf(". "))+1).replace(/\[\[S\d+\]\]/g,"").trim()
}
export interface CitationContext {
  citations:ReadonlyArray<{id:string;source:string;noteId:string;segmentId:string|null;quote:string}>
  /** Looks up the stored text a citation points at; undefined when it does not exist. */
  resolve:(citation:{source:string;noteId:string;segmentId:string|null})=>string|undefined
  supportSegmentIds?:ReadonlyArray<string>;expectedNoteIds?:ReadonlyArray<string>;required:boolean
}
export function citationChecks(output:string,context:CitationContext):Check[] {
  const markers=[...output.matchAll(/\[\[(S\d+)\]\]/g)],byId=new Map(context.citations.map(citation=>[citation.id,citation]))
  const used=[...new Set(markers.map(match=>match[1]!))],checks:Check[]=[]
  if(context.required)checks.push({id:"citation-present",group:"citation",pass:used.length>0,...(used.length?{}:{detail:"a cited answer was requested and none was given"})})
  if(!used.length)return checks
  const unresolved=used.filter(id=>{const citation=byId.get(id),text=citation&&context.resolve(citation);return !citation||!text||!text.includes(citation.quote)})
  checks.push({id:"citations-resolve",group:"citation",pass:unresolved.length===0,...(unresolved.length?{detail:`${unresolved.join(", ")} do not point at a real stored passage`}:{})})
  // Does the cited passage plausibly support the sentence it is attached to?
  const numberWords=(text:string)=>numbersIn(text)
  const weak:string[]=[]
  for(const match of markers){
    const citation=byId.get(match[1]!);if(!citation)continue
    const claim=claimBefore(output,match.index),passage=context.resolve(citation)??citation.quote
    const claimTokens=content(claim),passageTokens=content(passage)
    const overlap=[...claimTokens].filter(token=>passageTokens.has(token)).length/Math.max(1,claimTokens.size)
    const claimNumbers=[...numberWords(claim)].filter(value=>value>12),passageNumbers=numberWords(passage)
    const numbersBacked=claimNumbers.length===0||claimNumbers.some(value=>passageNumbers.has(value))
    if(claimTokens.size>=3&&(overlap<0.2||!numbersBacked))weak.push(`${match[1]} (${Math.round(overlap*100)}% term overlap${numbersBacked?"":", number not in passage"})`)
  }
  checks.push({id:"citations-support-claim",group:"citation",soft:true,pass:weak.length===0,...(weak.length?{detail:`weak lexical support: ${[...new Set(weak)].slice(0,6).join("; ")}`}:{})})
  if(context.supportSegmentIds?.length){
    const cited=new Set(used.map(id=>byId.get(id)?.segmentId).filter(Boolean))
    const hit=context.supportSegmentIds.some(id=>cited.has(id))
    checks.push({id:"cites-gold-passage",group:"citation",soft:true,pass:hit,...(hit?{}:{detail:"none of the cited passages is the one that states the answer"})})
  }
  if(context.expectedNoteIds?.length){
    const cited=new Set(used.map(id=>byId.get(id)?.noteId))
    const missing=context.expectedNoteIds.filter(id=>!cited.has(id))
    checks.push({id:"cites-expected-meetings",group:"citation",soft:true,pass:missing.length===0,...(missing.length?{detail:`${missing.length} of ${context.expectedNoteIds.length} expected meetings not cited`}:{})})
  }
  return checks
}
/** The answer says the source does not contain something. Deliberately broad:
 * what the answer must not claim instead is each scenario's `forbid`/`later`. */
export const ABSTAIN:Pattern="\\b(?:does|do|did|is|are|was|were|has|have|had|can|could)(?: not|n't)\\b|\\bcannot\\b|\\bcan't\\b|\\bwon't\\b|\\bnot (?:yet|been|available|in the)\\b|\\bno\\b (?:[\\w-]+ ){0,3}(?:mention|discussion|record|reference|information|evidence|decision|commitment|transcript|indication|terms|answer|number|figure|date|agreement|plan|details?)|\\bnothing\\b|\\bnone\\b|unable to|undecided|unknown"
export const failed=(checks:ReadonlyArray<Check>)=>checks.filter(check=>!check.pass&&!check.soft)
