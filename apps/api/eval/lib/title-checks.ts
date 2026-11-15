/** What must be true of a title written for an untitled note (`src/ai/note-title.ts`), restated as
 * checks. They look only at the title and at the meeting's own words; no model is asked. */
import {cleanNoteTitle,TITLE_MAX_CHARACTERS,TITLE_MAX_WORDS} from "../../src/ai/note-title"
import type {Check} from "./types"

export interface TitleContext {
  /** Everything the title may draw on: the transcript and the generated notes. */
  source:string
  /** First names of the people in the meeting, when known. */
  people?:ReadonlyArray<string>
}
const SPACED=/^[\p{Script=Latin}\p{Script=Cyrillic}\p{Script=Greek}\p{N}\p{P}\p{S}\s]+$/u
const MONTH="jan(?:uary)?|feb(?:ruary)?|mar(?:ch)?|apr(?:il)?|may|jun(?:e)?|jul(?:y)?|aug(?:ust)?|sep(?:t(?:ember)?)?|oct(?:ober)?|nov(?:ember)?|dec(?:ember)?"
const DATE=new RegExp(`\\b\\d{4}-\\d{2}-\\d{2}\\b|\\b\\d{1,2}[/.]\\d{1,2}[/.]\\d{2,4}\\b|\\b(?:${MONTH})\\s+\\d{1,2}(?:st|nd|rd|th)?\\b|\\b\\d{1,2}(?:st|nd|rd|th)?\\s+(?:${MONTH})\\b|\\b(?:mon|tues|wednes|thurs|fri|satur|sun)day\\b`,"i")
const fold=(text:string)=>text.normalize("NFKD").replace(/\p{M}/gu,"").toLowerCase()
const content=(word:string)=>fold(word).replace(/[^\p{L}\p{N}]/gu,"")

/** A word that is capitalised because it is a name or an acronym, not because the title is in Title Case:
 * it has a capital or digit past its first letter (iPhone, Q3, MCP, PostgreSQL), or the meeting itself
 * writes it with a capital in the middle of a sentence. */
function properNoun(word:string,source:string):boolean {
  const bare=word.replace(/^[^\p{L}\p{N}]+|[^\p{L}\p{N}]+$/gu,"").replace(/['’]s$/,"")
  if(!bare||/[\p{Lu}\p{N}]/u.test(bare.slice(1)))return true
  return new RegExp(`[\\p{Ll}\\p{N},;:)]\\s+${bare.replace(/[.*+?^${}()|[\]\\]/g,"\\$&")}(?![\\p{L}\\p{N}])`,"u").test(source)
}

export function titleChecks(title:string,context:TitleContext):Check[] {
  const present=title.trim().length>0,words=title.trim().split(/\s+/).filter(Boolean),spaced=SPACED.test(title)
  const check=(id:string,pass:boolean,detail?:string,soft=false):Check=>({id,group:"format",pass,...(soft?{soft:true}:{}),...(!pass&&detail?{detail}:{})})
  if(!present)return [check("title-present",false,"no title was saved")]
  const source=fold(context.source)
  const people=(context.people??[]).map(fold)
  // Names and numbers are where a title can say something the meeting did not: a word with a capital or a
  // digit (after the first word, whose capital is only the start of the title) has to be in the meeting.
  // Ordinary words are the model's summary and need not be anyone's exact words.
  const pieces=words.flatMap((word,index)=>word.split(/[-/]/).map(piece=>({piece,first:index===0})))
  const ungrounded=pieces.filter(({piece,first})=>first?/[\p{Lu}\p{N}]/u.test(piece.replace(/^[^\p{L}\p{N}]*./u,"")):/[\p{Lu}\p{N}]/u.test(piece))
    .map(({piece})=>content(piece.replace(/['’]s$/,""))).filter(word=>word.length>=2&&!source.includes(word)&&!people.includes(word))
  const capitalised=words.slice(1).filter(word=>/^\p{Lu}/u.test(word)&&!people.includes(content(word.replace(/['’]s$/,"")))&&!properNoun(word,context.source))
  const named=(context.people??[]).filter(person=>new RegExp(`(?<![\\p{L}])${person}(?![\\p{L}])`,"iu").test(title))
  return [
    check("title-present",true),
    check("title-length",title.length<=TITLE_MAX_CHARACTERS,`${title.length} characters`),
    check("title-words",!spaced||words.length>=3&&words.length<=TITLE_MAX_WORDS,`${words.length} words`),
    check("title-clean",cleanNoteTitle(title)===title,"quotation marks, closing punctuation, a label or a generic name"),
    check("title-single-line",!/[\r\n]/.test(title)),
    check("title-no-date",!DATE.test(title),"contains a date or a weekday"),
    check("title-not-attendees",named.length<2,`names ${named.join(", ")}`),
    check("title-sentence-case",capitalised.length===0,`capitalised without being names: ${capitalised.join(", ")}`,true),
    check("title-names-grounded",ungrounded.length===0,`names or numbers that are not in the meeting: ${ungrounded.join(", ")}`,true),
  ]
}
