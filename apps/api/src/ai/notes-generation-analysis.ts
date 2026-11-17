/**
 * What note generation works out in code before the model is asked anything:
 * voice tags that say exactly as much as the stored speaker data supports, who
 * the note owner is, what the audio channels imply about who is speaking, what
 * kind of typed notes the author left, and how long and how divided the
 * document may be. Pure functions of the note; no model, no I/O.
 */
import type {TranscriptSegmentDTO} from "@meeting-notes/contracts"
import type {ProviderNote} from "../providers"

type Segment=Pick<TranscriptSegmentDTO,"channel"|"speakerId"|"speakerLabel"|"text"|"startMs">

/** Native elapsed labels are m:ss or h:mm:ss, not an ambiguous clock time. */
export const elapsed=(display:string)=>{
  const match=/^(\d+):([0-5]\d)(?::([0-5]\d))?$/.exec(display.trim())
  if(!match)return display
  const seconds=match[3]===undefined?Number(match[1])*60+Number(match[2]):Number(match[1])*3600+Number(match[2])*60+Number(match[3])
  const minutes=Math.round(seconds/60)
  return seconds<90?`${seconds} seconds`:`about ${minutes} minutes (elapsed ${display})`
}

const DIARIZED=/^(mic|system):(.+):(speaker_(\d+)|unknown)$/
/** Live transcription: one number per voice on a channel for the whole recording. */
const LIVE=/^(mic|system):n:(\d+)$/
export const hasLiveVoices=(segments:ReadonlyArray<Pick<Segment,"speakerId">>)=>segments.some(segment=>LIVE.test(segment.speakerId))
/** One short tag per segment, in order: `mic p3 v1` is the microphone channel,
 * the third uploaded audio part on that channel, the first voice the diarizer
 * found in that part. Speaker numbers restart in every part, so the part is in
 * the tag: equal tags are the same voice, different parts are never comparable.
 * A stored label that is not an anonymous "Speaker N" (a name, once speakers can
 * be named upstream) is passed through as it is. */
export function voiceTags(segments:ReadonlyArray<Segment>):string[] {
  const parts:Record<string,Map<string,number>>={mic:new Map(),system:new Map()}
  return segments.map(segment=>{
    const channel=segment.channel==="system"?"sys":"mic",label=segment.speakerLabel?.trim()
    if(label&&!/^speaker[ _]?\d+$/i.test(label)&&!/^unknown$/i.test(label))return `${channel} ${label.replace(/[\u0000-\u001f"\\]/g," ")}`
    const live=LIVE.exec(segment.speakerId)
    if(live)return `${channel} live v${live[2]}`
    const match=DIARIZED.exec(segment.speakerId)
    if(!match)return `${channel} unlabelled`
    const known=parts[match[1]!]!
    if(!known.has(match[2]!))known.set(match[2]!,known.size+1)
    return `${channel} p${known.get(match[2]!)} ${match[4]?`v${match[4]}`:"v?"}`
  })
}

/** The most sections a note may have, by transcript length in characters:
 * 4 below 8,000, 5 below 50,000, then one more for every further 10,000, up
 * to 8. The count includes the final actions section. */
export const maxSectionsFor=(transcriptCharacters:number)=>transcriptCharacters<8_000?4:transcriptCharacters<50_000?5:transcriptCharacters<80_000?Math.floor(transcriptCharacters/10_000)+1:8
/** Target length in words. Notes should grow much more slowly than the
 * meeting: about 150 words for 250 spoken and about 270 for 500. The target is
 * a square-root curve through those points, extended to long meetings. It is a
 * fifth above the first version's, for bullets as full as the reference's
 * (usually 6 to 15 words, against 12 at most before), which squeezed figures
 * out of notes that also had to keep the author's lines.
 * Typed notes add room, because none of the author's lines is dropped. */
export function targetWords(transcriptWords:number,typedWords:number):[number,number] {
  const upper=Math.min(12*Math.sqrt(transcriptWords),0.6*transcriptWords+24,1200)+0.6*typedWords
  const round=(value:number)=>value<100?Math.max(10,Math.round(value/5)*5):Math.round(value/10)*10
  return [round(upper*0.6),round(upper)]
}

const ACTION_HEADING=/^(?:action items?|next steps?|to.?dos?|follow.?ups?|actions?|tasks?|owners?|jobs|mine|for me|next)\b/i
export interface NoteAnalysis {
  size:{characters:number;words:number;maxSections:number;targetWords:[number,number];tiny:boolean}
  notes:{mode:"none"|"fragments"|"outline";headings:string[];actionHeading:string|null;lines:number}
  people:{owner:{name:string;first:string}|null;invited:string[];
    channels:{kind:"two-channel"|"single-microphone"|"system-only";oneMicVoice:boolean;
      /** Some voices were numbered by live transcription (tags such as "sys live v2"). */
      liveVoices?:boolean}}
}
export function analyseNote(note:ProviderNote):NoteAnalysis {
  const spoken=note.transcriptSegments.filter(segment=>segment.text.trim())
  const characters=spoken.reduce((sum,segment)=>sum+segment.text.length,0),words=spoken.reduce((sum,segment)=>sum+segment.text.trim().split(/\s+/).length,0)
  const typed=note.body.split("\n").map(line=>line.trim()).filter(Boolean)
  const headings=typed.filter(line=>/^#{1,6}\s+\S/.test(line)).map(line=>line.replace(/^#+\s+/,"").trim())
  const typedWords=note.body.trim()?note.body.trim().split(/\s+/).length:0
  const self=note.meetingContext?.participants.find(person=>person.isSelf&&!person.isResource&&person.displayName?.trim())
  const name=self?.displayName?.trim()
  const mic=spoken.filter(segment=>segment.channel!=="system"),system=spoken.filter(segment=>segment.channel==="system")
  // One microphone voice in every part: nobody else was audibly in the author's room.
  const voicesByPart=new Map<string,Set<string>>()
  for(const segment of mic){const match=DIARIZED.exec(segment.speakerId),part=match?.[2]??"";voicesByPart.set(part,(voicesByPart.get(part)??new Set()).add(segment.speakerId))}
  return {
    size:{characters,words,maxSections:maxSectionsFor(characters),targetWords:targetWords(words,typedWords),tiny:words<180},
    notes:{mode:!typed.length?"none":headings.length?"outline":"fragments",headings,lines:typed.length,
      // With headings: the author's own heading for tasks. Without: a line that is only such a word ("actions").
      actionHeading:(headings.length?headings.find(heading=>ACTION_HEADING.test(heading)):typed.map(line=>line.replace(/^[-*•\s]+/,"").replace(/[:!?.\s]+$/,"")).find(line=>ACTION_HEADING.test(line)&&line.split(/\s+/).length<=2))??null},
    people:{owner:name?{name,first:(self?.givenName?.trim()||name.split(/\s+/)[0])!}:null,
      invited:(note.meetingContext?.participants??[]).filter(person=>!person.isSelf&&!person.isResource&&person.displayName?.trim()&&person.responseStatus!=="declined").map(person=>person.displayName!.trim()),
      channels:{kind:mic.length&&system.length?"two-channel":system.length?"system-only":"single-microphone",oneMicVoice:[...voicesByPart.values()].every(voices=>voices.size<=1),...(hasLiveVoices(spoken)?{liveVoices:true}:{})}},
  }
}
