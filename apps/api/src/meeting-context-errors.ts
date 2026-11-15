/** A user must choose which note to keep; never merge or steal an existing note. */
export class MeetingBindingConflict extends Error {
  constructor(){super("This meeting is already associated with another note. Open that note or clear its association first.")}
}
export const mergeMeetingSnapshot = <T extends {participants:ReadonlyArray<{id:string;nameSource:string}>}>(next:T,previous:T|null,sameMeeting:boolean):T => {
 if(!sameMeeting||!previous)return next
 const ids=new Set(next.participants.map(p=>p.id))
 return{...next,participants:[...next.participants,...previous.participants.filter(p=>p.nameSource==="manual"&&!ids.has(p.id))]}
}

export const isDefaultNoteTitle=(title:string)=>/^(?:new note|untitled note)?$/i.test(title.trim())
