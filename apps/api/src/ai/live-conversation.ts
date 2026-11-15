/**
 * What the server remembers about a chat thread on a meeting that is being recorded, so that the
 * next request of the thread can repeat the previous one byte for byte and only append to it
 * (`meeting-chat.ts`, "Chat during a recording").
 *
 * A thread is not identified by the client: `ChatInput` carries only the earlier turns as text. A
 * request is matched to a remembered thread by those turns: its `history` must be the tail of the
 * thread as the server knows it (every question, and the start of every answer). So:
 *
 *  - an unchanged client continues its thread with no new field on the wire;
 *  - a client that sends only its newest turns (iPhone sends 20 messages, desktop 15 exchanges) still
 *    matches, and the model keeps the whole thread the server holds;
 *  - a thread that lost its last answer (interrupted, failed, regenerated) matches at the turn before,
 *    and the turns after it are dropped;
 *  - anything else (the server restarted, the history was edited, the thread is new) matches nothing.
 *    The caller then starts a new thread, which costs one cache miss and nothing else.
 *
 * Kept in memory, bounded, per process. That is enough for a single-instance server because a miss is
 * harmless, and the provider's own cache lives for minutes anyway. Nothing here is persisted.
 */
import {createHash} from "node:crypto"
import type {ChatCitationDTO,ChatInput} from "@meeting-notes/contracts"
import type {LanguageMessage} from "./shared"

/** One stored segment as the model has been shown it. Never mutated: a change makes a new object. */
export interface ShownSegment {readonly id:string;readonly startMs:number;readonly text:string;readonly label:string;
  /** Whether the segment was final when shown. One that was not has no labels, and is shown again once it is. */
  readonly final:boolean
  /** The label of each line of the segment, in order; null where a line has none. */
  readonly lineIds:ReadonlyArray<string|null>}
/** Everything needed to work out what the next message must add. Copied, not shared, between turns. */
export interface LiveState {
  shown:Map<string,ShownSegment>
  /** Stored segments a thread left out when it started because the meeting was too long to show whole; they are not sent later either. */
  omitted:Set<string>
  /** Every citation issued in the thread that still points at a line the model may cite. */
  citations:ChatCitationDTO[]
  /** The number of the next label. Labels are never reused within a thread. */
  nextId:number
  /** Labels of notes lines by source and exact text, so an unchanged line keeps its label when the notes are sent again. */
  documentIds:Map<string,string>
  /** The typed notes and generated notes as last sent (rendered text); null before they were ever sent. */
  notes:string|null;summary:string|null
  /** The speaker legend as last stated. */
  legend:string
  /** Characters a transcript line is cut to; fixed for the thread so a segment always splits the same way. */
  lineTarget:number
}
export const copyState=(state:LiveState):LiveState=>({...state,shown:new Map(state.shown),omitted:new Set(state.omitted),citations:[...state.citations],documentIds:new Map(state.documentIds)})

export interface LiveTurn {question:string;answer:string|null;
  /** Index in `messages` of this turn's user message; its answer, once known, is the next one. */
  at:number
  /** State after this turn's user message. Dropped for old turns; a thread can only be continued from a turn that has it. */
  state:LiveState|null}
export interface LiveConversation {
  scope:string;noteId:string;citable:boolean
  /** The request without its system prompt: the transcript so far, any turns carried over from the client, then one user and one assistant message per turn. */
  messages:LanguageMessage[]
  /** Turns the thread already had when the server started remembering it, as the client sent them. */
  carried:ChatInput["history"]
  baseMessages:number;baseState:LiveState
  turns:LiveTurn[]
  touched:number
}

const normal=(text:string)=>text.replace(/\s*\[\[S\d+\]\]/g,"").replace(/\s+/g," ").trim()
/** Two turns are the same turn when the questions are equal and the answers start alike. Citation
 * markers and spacing are ignored, and only the start of an answer counts, so an answer the client
 * holds slightly differently (cut short, re-rendered) still matches. */
const key=(turn:{role:string;content:string})=>createHash("sha1").update(turn.role==="assistant"?`a\u0000${normal(turn.content).slice(0,160)}`:`u\u0000${normal(turn.content)}`).digest("base64")
const characters=(conversation:LiveConversation)=>conversation.messages.reduce((sum,message)=>sum+message.content.length,0)

export interface LiveConversationLimits {conversations:number;perNote:number;characters:number;idleMs:number;
  /** Turns, counted from the newest, that keep the state needed to continue from them. */
  resumableTurns:number}
export const LIVE_CONVERSATION_LIMITS:LiveConversationLimits={conversations:200,perNote:4,characters:48_000_000,idleMs:6*60*60_000,resumableTurns:6}
export interface LiveConversationMatch {conversation:LiveConversation;
  /** How many of the thread's turns the request's history covers; later ones are to be dropped. */
  turns:number
  /** The client's text of the last answer, when the server never saw that answer complete. */
  adoptedAnswer:string|null}
export interface LiveConversationStore {
  /** The remembered thread this history continues, or null. An empty history never matches: it is a new thread. */
  match(scope:string,noteId:string,citable:boolean,history:ChatInput["history"]):LiveConversationMatch|null
  /** Why `match` found nothing for this history, in words for the server log: the thread was dropped to make room
   * (and why), it was continued from a turn too old to branch from, or it is simply not known here. */
  explain(scope:string,noteId:string,citable:boolean,history:ChatInput["history"]):string
  /** Remember a thread, or note that it was used, and make room. */
  save(conversation:LiveConversation):void
  readonly size:number
  clear():void
}

export function makeLiveConversationStore(limits:LiveConversationLimits=LIVE_CONVERSATION_LIMITS,clock:()=>number=Date.now):LiveConversationStore {
  let conversations:LiveConversation[]=[]
  // Why threads were dropped, by their last exchange, so a later request for one can be explained. Bounded; oldest first out.
  const dropped=new Map<string,string>()
  const exchange=(scope:string,noteId:string,citable:boolean,question:string,answer:string)=>[scope,noteId,citable,key({role:"user",content:question}),key({role:"assistant",content:answer})].join("\u0000")
  const drop=(conversation:LiveConversation,why:string)=>{
    conversations=conversations.filter(other=>other!==conversation)
    for(const turn of conversation.turns)if(turn.answer!==null)dropped.set(exchange(conversation.scope,conversation.noteId,conversation.citable,turn.question,turn.answer),why)
    while(dropped.size>2000)dropped.delete(dropped.keys().next().value!)
  }
  const expire=()=>{const cutoff=clock()-limits.idleMs;for(const conversation of conversations.filter(other=>other.touched<cutoff))drop(conversation,`evicted: idle for more than ${Math.round(limits.idleMs/3_600_000)} hours`)}
  return {
    get size(){return conversations.length},
    clear(){conversations=[];dropped.clear()},
    match(scope,noteId,citable,history){
      if(!history.length)return null
      expire()
      const wanted=history.map(key)
      let best:LiveConversationMatch|null=null
      for(const conversation of conversations){
        if(conversation.scope!==scope||conversation.noteId!==noteId||conversation.citable!==citable)continue
        const carried=conversation.carried.map(key)
        // More turns matched is better; at the same length, a thread whose last answer the server saw complete
        // beats one that has only the same question waiting for an answer.
        for(let turns=conversation.turns.length;turns>=1&&(!best||turns>best.turns||turns===best.turns&&best.adoptedAnswer!==null);turns--){
          const last:LiveTurn=conversation.turns[turns-1]!
          if(!last.state)break
          // The thread up to and including that turn, as a client would send it back.
          const known=[...carried,...conversation.turns.slice(0,turns).flatMap(turn=>[key({role:"user",content:turn.question}),turn.answer===null?null:key({role:"assistant",content:turn.answer})])]
          if(wanted.length>known.length||conversation.turns.slice(0,turns-1).some(turn=>turn.answer===null))continue
          // A history that ends with the previous turn's answer belongs to the previous turn, even when the unanswered question repeats that turn's.
          if(last.answer===null&&turns>=2&&wanted.at(-1)===key({role:"assistant",content:conversation.turns[turns-2]!.answer!}))continue
          // The history must be the tail of it. An answer the server never completed is taken on the client's word.
          let same=true
          for(let offset=1;offset<=wanted.length&&same;offset++){
            const mine=known[known.length-offset],theirs=wanted[wanted.length-offset]
            same=mine===null?offset===1&&history.at(-1)!.role==="assistant":mine===theirs
          }
          if(same&&(!best||turns>best.turns||last.answer!==null))best={conversation,turns,adoptedAnswer:last.answer===null?history.at(-1)!.content:null}
        }
      }
      return best
    },
    explain(scope,noteId,citable,history){
      const question=history.at(-2),answer=history.at(-1)
      if(!question||!answer||question.role!=="user"||answer.role!=="assistant")return "not found: the history does not end with a question and its answer"
      const evicted=dropped.get(exchange(scope,noteId,citable,question.content,answer.content))
      if(evicted)return evicted
      const same=(turn:LiveTurn)=>turn.answer!==null&&key({role:"user",content:turn.question})===key(question)&&key({role:"assistant",content:turn.answer})===key(answer)
      for(const conversation of conversations){
        if(conversation.scope!==scope||conversation.noteId!==noteId||!conversation.turns.some(same))continue
        if(conversation.citable!==citable)return "not found: the thread was started with sources switched the other way"
        if(conversation.turns.some(turn=>same(turn)&&!turn.state))return `not found: continued from a turn older than the last ${limits.resumableTurns}`
        return "not found: the turns before the last one do not match what was sent"
      }
      return "not found: no thread with this history is remembered here (the server restarted, or the history was changed)"
    },
    save(conversation){
      conversation.touched=clock()
      // Old turns keep their text, which the request repeats, but not the state to branch from them.
      conversation.turns.slice(0,-limits.resumableTurns).forEach(turn=>{turn.state=null})
      conversations=[conversation,...conversations.filter(other=>other!==conversation)]
      expire()
      const sameNote=conversations.filter(other=>other.scope===conversation.scope&&other.noteId===conversation.noteId)
      for(const extra of sameNote.slice(limits.perNote))drop(extra,`evicted: more than ${limits.perNote} threads on this note`)
      // Most recently used first: drop from the end until the store fits.
      let total=conversations.reduce((sum,other)=>sum+characters(other),0)
      while(conversations.length>1&&(conversations.length>limits.conversations||total>limits.characters)){
        const last=conversations.at(-1)!
        total-=characters(last)
        drop(last,conversations.length>limits.conversations?`evicted: more than ${limits.conversations} threads remembered`:"evicted: the remembered threads passed their size limit")
      }
    },
  }
}
/** The store the API process uses unless a caller supplies its own (tests do). */
export const liveConversations=makeLiveConversationStore()
