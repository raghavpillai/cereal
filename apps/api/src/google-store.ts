import { Effect, Schema } from 'effect'
import type { Database } from '@meeting-notes/database'
import { MeetingContextDTO, type CalendarEventDTO, type GoogleCalendarDTO, type GoogleConnectionDTO, type NoteContextDTO, type NoteDTO } from '@meeting-notes/contracts'
import type { NotesRepositoryService } from './repository'
import type {CalendarStore} from './calendar-store'
import {MeetingBindingConflict,mergeMeetingSnapshot,isDefaultNoteTitle} from './meeting-context-errors'
import { noteDTO } from './prisma-repository'

export type GoogleConnection = GoogleConnectionDTO & { userId: string; subject: string; encryptedGrant: string }
export type GoogleCalendar = GoogleCalendarDTO & { remoteId: string }
export type GoogleEvent = Omit<CalendarEventDTO, 'calendarName' | 'color' | 'noteId'> & { remoteId: string; meetingKey: string; context: typeof MeetingContextDTO.Type }
export interface OAuthState { stateHash: string; userId: string; sessionHash: string; encryptedVerifier: string; contacts: boolean; expiresAt: string }
export interface GoogleStore {
  saveState(state: OAuthState): Promise<void>
  consumeState(hash: string, userId: string, sessionHash: string, now: string): Promise<OAuthState | null>
  listConnections(userId?: string): Promise<GoogleConnection[]>
  getConnection(userId: string, id: string): Promise<GoogleConnection | null>
  saveConnection(input: Omit<GoogleConnection, 'id' | 'createdAt' | 'lastSyncedAt' | 'lastSyncError'>): Promise<GoogleConnection>
  updateConnection(userId: string, id: string, patch: Partial<Pick<GoogleConnection, 'encryptedGrant' | 'status' | 'lastSyncedAt' | 'lastSyncError'>>, expectedGrant?:string): Promise<boolean>
  deleteConnection(userId: string, id: string): Promise<boolean>
  saveCalendars(userId: string, id: string, calendars: Omit<GoogleCalendar, 'id' | 'connectionId' | 'selected'>[], expectedGrant?:string): Promise<boolean>
  listCalendars(userId: string, connectionId: string): Promise<GoogleCalendar[]>
  getCalendar(userId: string, id: string): Promise<GoogleCalendar | null>
  selectCalendar(userId: string, id: string, selected: boolean): Promise<GoogleCalendar | null>
  replaceEvents(userId: string, calendarId: string, events: GoogleEvent[],expectedGrant?:string): Promise<boolean>
  listEvents(userId: string, from: string, to: string): Promise<CalendarEventDTO[]>
  getEvent(userId: string, eventId: string): Promise<GoogleEvent | null>
  createEventNote(userId: string, eventId: string): Promise<NoteDTO | null>
  getContext(userId: string, noteId: string): Promise<NoteContextDTO | null>
  putContext(userId: string, noteId: string, context: typeof MeetingContextDTO.Type | null, ifAbsent?: boolean): Promise<NoteContextDTO | null>
}

const decodeContext = Schema.decodeUnknownSync(MeetingContextDTO)
export function makePrismaGoogleStore(db: Database): GoogleStore {
  const connection = (row: Awaited<ReturnType<Database['googleConnection']['findMany']>>[number]): GoogleConnection => ({
    ...row, status: row.status === 'connected' ? 'connected' : 'reconnect_required',
    lastSyncedAt: row.lastSyncedAt?.toISOString() ?? null, createdAt: row.createdAt.toISOString(),
  })
  const calendar = (row: Awaited<ReturnType<Database['googleCalendar']['findMany']>>[number]): GoogleCalendar => ({ ...row })
  const event = (row: Awaited<ReturnType<Database['googlePersonalEvent']['findMany']>>[number]): GoogleEvent => ({ ...row, startAt: row.startAt.toISOString(), endAt: row.endAt.toISOString(), context: decodeContext(row.context) })
  return {
    async saveState(state) {
      await db.googleOAuthState.deleteMany({ where: { expiresAt: { lt: new Date() } } })
      await db.googleOAuthState.create({ data: { ...state, expiresAt: new Date(state.expiresAt) } })
    },
    async consumeState(stateHash, userId, sessionHash, now) {
      return db.$transaction(async tx => {
        const row = await tx.googleOAuthState.findFirst({ where: { stateHash, userId, sessionHash, expiresAt: { gt: new Date(now) } } })
        if (!row || !(await tx.googleOAuthState.deleteMany({ where: { stateHash, userId, sessionHash } })).count) return null
        return { ...row, expiresAt: row.expiresAt.toISOString() }
      })
    },
    async listConnections(userId) { return (await db.googleConnection.findMany({ where: userId ? { userId } : {}, orderBy: { createdAt: 'asc' } })).map(connection) },
    async getConnection(userId, id) { const row = await db.googleConnection.findFirst({ where: { userId, id } }); return row ? connection(row) : null },
    async saveConnection(input) {
      return db.$transaction(async tx => {
        await tx.$queryRawUnsafe('SELECT 1 FROM pg_advisory_xact_lock(hashtextextended($1,0))', `google:${input.userId}`)
        const old = await tx.googleConnection.findUnique({ where: { userId_subject: { userId: input.userId, subject: input.subject } } })
        if (!old && await tx.googleConnection.count({ where: { userId: input.userId } }) >= 5) throw new Error('Connection limit')
        return connection(await tx.googleConnection.upsert({ where: { userId_subject: { userId: input.userId, subject: input.subject } }, create: input, update: input }))
      })
    },
    async updateConnection(userId,id,patch,expectedGrant){return(await db.googleConnection.updateMany({where:{userId,id,...(expectedGrant?{encryptedGrant:expectedGrant}:{})},data:{...patch,...(patch.lastSyncedAt?{lastSyncedAt:new Date(patch.lastSyncedAt)}:{})}})).count===1},
    async deleteConnection(userId, id) { return (await db.googleConnection.deleteMany({ where: { userId, id } })).count > 0 },
    async saveCalendars(userId, id, calendars,expectedGrant) {
      return db.$transaction(async tx => {
        await tx.$queryRawUnsafe('SELECT "id" FROM "GoogleConnection" WHERE "id"=$1::uuid AND "userId"=$2::uuid FOR UPDATE',id,userId)
        if (!await tx.googleConnection.findFirst({ where: { userId, id,...(expectedGrant?{encryptedGrant:expectedGrant}:{}) } })) return false
        await tx.googleCalendar.deleteMany({ where: { connectionId: id, remoteId: { notIn: calendars.map(c => c.remoteId) } } })
        for (const item of calendars) await tx.googleCalendar.upsert({ where: { connectionId_remoteId: { connectionId: id, remoteId: item.remoteId } }, create: { ...item, connectionId: id, selected: item.primary }, update: item })
        return true
      })
    },
    async listCalendars(userId, connectionId) { return (await db.googleCalendar.findMany({ where: { connectionId, connection: { userId } }, orderBy: { name: 'asc' } })).map(calendar) },
    async getCalendar(userId, id) { const row = await db.googleCalendar.findFirst({ where: { id, connection: { userId } } }); return row ? calendar(row) : null },
    async selectCalendar(userId, id, selected) { if (!(await db.googleCalendar.updateMany({ where: { id, connection: { userId } }, data: { selected } })).count) return null; return calendar(await db.googleCalendar.findUniqueOrThrow({ where: { id } })) },
    async replaceEvents(userId, calendarId, events,expectedGrant) {
      return db.$transaction(async tx => {
        await tx.$queryRawUnsafe('SELECT c."id" FROM "GoogleConnection" c JOIN "GoogleCalendar" g ON g."connectionId"=c."id" WHERE g."id"=$1::uuid AND c."userId"=$2::uuid FOR UPDATE OF c',calendarId,userId)
        if (!await tx.googleCalendar.findFirst({ where: { id: calendarId, connection: { userId,...(expectedGrant?{encryptedGrant:expectedGrant}:{}) } } })) return false
        await tx.googlePersonalEvent.deleteMany({ where: { calendarId } })
        if (events.length) await tx.googlePersonalEvent.createMany({ data: events.map(e => ({ ...e, startAt: new Date(e.startAt), endAt: new Date(e.endAt), context: JSON.parse(JSON.stringify(e.context)) })) })
        return true
      })
    },
    async listEvents(userId, from, to) {
      const rows = await db.googlePersonalEvent.findMany({ where: { calendar: { selected: true, connection: { userId } }, startAt: { lt: new Date(to) }, endAt: { gt: new Date(from) } }, include: { calendar: true }, orderBy: { startAt: 'asc' }, take: 10001 })
      const bindings = new Map((await db.googleMeeting.findMany({ where: { userId, meetingKey: { in: rows.map(r => r.meetingKey) }, note: { ownerId: userId } } })).map(r => [r.meetingKey, r.noteId]))
      return rows.map(row => { const { remoteId, meetingKey, context, calendar: cal, ...dto } = row; return { ...dto, startAt: row.startAt.toISOString(), endAt: row.endAt.toISOString(), calendarName: cal.name, color: cal.color, noteId: bindings.get(row.meetingKey) ?? null } })
    },
    async getEvent(userId, id) { const row = await db.googlePersonalEvent.findFirst({ where: { id, calendar: { selected: true, connection: { userId } } } }); return row ? event(row) : null },
    async createEventNote(userId, eventId) {
      return db.$transaction(async tx => {
        const row = await tx.googlePersonalEvent.findFirst({ where: { id: eventId, calendar: { selected: true, connection: { userId } } } })
        if (!row) return null
        await tx.$queryRawUnsafe('SELECT 1 FROM pg_advisory_xact_lock(hashtextextended($1,0))', `google-note:${userId}:${row.meetingKey}`)
        const existing = await tx.googleMeeting.findFirst({ where: { userId, meetingKey: row.meetingKey, note: { ownerId: userId } }, include: { note: { include: { transcriptSegments: true } } } })
        if (existing) return noteDTO(existing.note)
        const note = await tx.note.create({ data: { ownerId: userId, title: row.title, date: new Date(`${row.startDate}T00:00:00Z`) }, include: { transcriptSegments: true } })
        await tx.googleMeeting.create({ data: { userId, meetingKey: row.meetingKey, noteId: note.id } })
        await tx.noteMeetingContext.create({ data: { noteId: note.id, userId, meeting: row.context! } })
        return noteDTO(note)
      })
    },
    async getContext(userId, noteId) {
      if (!await db.note.findFirst({ where: { id: noteId, ownerId: userId }, select: { id: true } })) return null
      const row = await db.noteMeetingContext.findFirst({ where: { noteId, userId } })
      return { noteId, meeting: row ? decodeContext(row.meeting) : null }
    },
    async putContext(userId, noteId, context, ifAbsent = false) {
      return db.$transaction(async tx => {
        await tx.$queryRawUnsafe('SELECT 1 FROM pg_advisory_xact_lock(hashtextextended($1,0))', `meeting-association:${userId}`)
        await tx.$queryRawUnsafe('SELECT "id" FROM "Note" WHERE "id"=$1::uuid AND "ownerId"=$2::uuid FOR UPDATE', noteId, userId)
        const note=await tx.note.findFirst({where:{id:noteId,ownerId:userId},select:{id:true,title:true}});if(!note)return null
        const old = await tx.noteMeetingContext.findFirst({where:{noteId,userId}})
        if (ifAbsent && old) return {noteId,meeting:decodeContext(old.meeting)}
        const previous=old?decodeContext(old.meeting):null
        let sameMeeting=false
        if(context?.provider==='ics'){
          const event=await tx.calendarEvent.findFirst({where:{id:context.eventId,calendar:{enabled:true}}})
          if(!event)return null
          await tx.$queryRawUnsafe('SELECT 1 FROM pg_advisory_xact_lock(hashtextextended($1,0))',userId+':'+event.meetingKey)
          const target=await tx.calendarMeeting.findUnique({where:{userId_meetingKey:{userId,meetingKey:event.meetingKey}}})
          if(target&&target.noteId!==noteId)throw new MeetingBindingConflict()
          sameMeeting=!!await tx.calendarMeeting.findFirst({where:{userId,noteId,meetingKey:event.meetingKey}})
          await tx.calendarMeeting.deleteMany({where:{userId,noteId}})
          await tx.googleMeeting.deleteMany({where:{userId,noteId}})
          await tx.calendarMeeting.upsert({where:{userId_meetingKey:{userId,meetingKey:event.meetingKey}},create:{userId,noteId,meetingKey:event.meetingKey},update:{noteId}})
        }else if(context?.provider==='google'){
          const event=await tx.googlePersonalEvent.findFirst({where:{id:context.eventId,calendar:{selected:true,connection:{userId}}}})
          if(!event)return null
          await tx.$queryRawUnsafe('SELECT 1 FROM pg_advisory_xact_lock(hashtextextended($1,0))',`google-note:${userId}:${event.meetingKey}`)
          const target=await tx.googleMeeting.findUnique({where:{userId_meetingKey:{userId,meetingKey:event.meetingKey}}})
          if(target&&target.noteId!==noteId)throw new MeetingBindingConflict()
          sameMeeting=!!await tx.googleMeeting.findFirst({where:{userId,noteId,meetingKey:event.meetingKey}})
          await tx.calendarMeeting.deleteMany({where:{userId,noteId}})
          await tx.googleMeeting.deleteMany({where:{userId,noteId}})
          await tx.googleMeeting.upsert({where:{userId_meetingKey:{userId,meetingKey:event.meetingKey}},create:{userId,noteId,meetingKey:event.meetingKey},update:{noteId}})
        }else{
          await tx.calendarMeeting.deleteMany({where:{userId,noteId}})
          await tx.googleMeeting.deleteMany({where:{userId,noteId}})
        }
        const meeting=context?mergeMeetingSnapshot(context,previous,sameMeeting||previous?.provider===context.provider&&previous.eventId===context.eventId):null
        if(meeting&&!ifAbsent&&isDefaultNoteTitle(note.title))await tx.note.update({where:{id:noteId},data:{title:meeting.title}})
        if(meeting)await tx.noteMeetingContext.upsert({where:{noteId},create:{noteId,userId,meeting:JSON.parse(JSON.stringify(meeting))},update:{meeting:JSON.parse(JSON.stringify(meeting))}})
        else await tx.noteMeetingContext.deleteMany({where:{noteId,userId}})
        return{noteId,meeting}
      })
    },
  }
}

/** Test adapter uses the same per-user repository boundary as the production API. */
export function makeMemoryGoogleStore(notes: NotesRepositoryService,sharedCalendars?:CalendarStore): GoogleStore {
  const states = new Map<string, OAuthState>(), connections = new Map<string, GoogleConnection>(), calendars = new Map<string, GoogleCalendar>(), events = new Map<string, GoogleEvent>()
  const bindings = new Map<string, string>(), contexts = new Map<string, NoteContextDTO>(); let queue = Promise.resolve()
  const clone = <T,>(v: T): T => structuredClone(v)
  const ownCalendar = (userId: string, id: string) => { const c = calendars.get(id); return c && connections.get(c.connectionId)?.userId === userId ? c : null }
  const ownEvent = (userId: string, id: string) => { const e = events.get(id); return e && ownCalendar(userId, e.calendarId)?.selected ? e : null }
  const getNote = (userId: string, id: string) => Effect.runPromise(notes.forUser(userId).getNote(id))
  return {
    async saveState(state) { states.set(state.stateHash, clone(state)) },
    async consumeState(hash, userId, sessionHash, now) { const s = states.get(hash); if (!s || s.userId !== userId || s.sessionHash !== sessionHash || s.expiresAt <= now) return null; states.delete(hash); return clone(s) },
    async listConnections(userId) { return clone([...connections.values()].filter(c => !userId || c.userId === userId)) },
    async getConnection(userId, id) { const c = connections.get(id); return c?.userId === userId ? clone(c) : null },
    async saveConnection(input) { const old = [...connections.values()].find(c => c.userId === input.userId && c.subject === input.subject); if (!old && [...connections.values()].filter(c => c.userId === input.userId).length >= 5) throw new Error('Connection limit'); const c = { id: old?.id ?? crypto.randomUUID(), createdAt: old?.createdAt ?? new Date().toISOString(), lastSyncedAt: old?.lastSyncedAt ?? null, lastSyncError: null, ...input }; connections.set(c.id, c); return clone(c) },
    async updateConnection(userId,id,patch,expectedGrant){const c=connections.get(id);if(c?.userId!==userId||expectedGrant&&c.encryptedGrant!==expectedGrant)return false;Object.assign(c,patch);return true},
    async deleteConnection(userId, id) { if (connections.get(id)?.userId !== userId) return false; for (const [cid, c] of calendars) if (c.connectionId === id) { for (const [eid, e] of events) if (e.calendarId === cid) events.delete(eid); calendars.delete(cid) } return connections.delete(id) },
    async saveCalendars(userId,id,rows,expectedGrant){const connection=connections.get(id);if(connection?.userId!==userId||expectedGrant&&connection.encryptedGrant!==expectedGrant)return false; for (const [cid, c] of calendars) if (c.connectionId === id && !rows.some(r => r.remoteId === c.remoteId)) { calendars.delete(cid); for (const [eid, e] of events) if (e.calendarId === cid) events.delete(eid) } for (const row of rows) { const old = [...calendars.values()].find(c => c.connectionId === id && c.remoteId === row.remoteId); const c = { ...row, id: old?.id ?? crypto.randomUUID(), connectionId: id, selected: old?.selected ?? row.primary }; calendars.set(c.id, c) } return true },
    async listCalendars(userId, id) { return clone([...calendars.values()].filter(c => c.connectionId === id && connections.get(id)?.userId === userId)) },
    async getCalendar(userId, id) { return clone(ownCalendar(userId, id)) },
    async selectCalendar(userId, id, selected) { const c = ownCalendar(userId, id); if (!c) return null; const updated = { ...c, selected }; calendars.set(id, updated); return clone(updated) },
    async replaceEvents(userId,id,rows,expectedGrant){const cal=ownCalendar(userId,id);if(!cal||expectedGrant&&connections.get(cal.connectionId)?.encryptedGrant!==expectedGrant)return false; for (const [eid, e] of events) if (e.calendarId === id) events.delete(eid); for (const e of rows) events.set(e.id, clone(e));return true },
    async listEvents(userId, from, to) { return clone([...events.values()].filter(e => ownCalendar(userId, e.calendarId)?.selected && e.startAt < to && e.endAt > from).map(e => { const { remoteId, meetingKey, context, ...dto } = e; const c = calendars.get(e.calendarId)!; return { ...dto, calendarName: c.name, color: c.color, noteId: bindings.get(`${userId}:${e.meetingKey}`) ?? null } })) },
    async getEvent(userId, id) { return clone(ownEvent(userId, id)) },
    createEventNote(userId, id) { const operation = queue.then(async () => { const e = ownEvent(userId, id); if (!e) return null; const key = `${userId}:${e.meetingKey}`, existing = bindings.get(key); if (existing) { const note = await getNote(userId, existing); if (note) return note } const note = await Effect.runPromise(notes.forUser(userId).createNote({ title: e.title, date: e.startDate })); bindings.set(key, note.id); contexts.set(note.id, { noteId: note.id, meeting: clone(e.context) }); return note }); queue = operation.then(() => {}, () => {}); return operation },
    async getContext(userId, noteId) { return await getNote(userId, noteId) ? clone(contexts.get(noteId) ?? { noteId, meeting: null }) : null },
    putContext(userId,noteId,meeting,ifAbsent=false){const operation=queue.then(async()=>{
      const note=await getNote(userId,noteId);if(!note)return null
      const previous=contexts.get(noteId)?.meeting??null
      if(ifAbsent&&contexts.has(noteId))return clone(contexts.get(noteId)!)
      let sameMeeting=false
      if(meeting?.provider==='google'){
        const event=ownEvent(userId,meeting.eventId);if(!event)return null
        const key=`${userId}:${event.meetingKey}`,bound=bindings.get(key)
        if(bound&&bound!==noteId&&await getNote(userId,bound))throw new MeetingBindingConflict()
        sameMeeting=bound===noteId
        for(const[k,value]of bindings)if(value===noteId&&k.startsWith(userId+':'))bindings.delete(k)
        await sharedCalendars?.unlinkNote(userId,noteId)
        bindings.set(key,noteId)
      }else if(meeting?.provider==='ics'){
        if(!sharedCalendars||!await sharedCalendars.associateNote(userId,noteId,meeting.eventId))return null
        for(const[k,value]of bindings)if(value===noteId&&k.startsWith(userId+':'))bindings.delete(k)
      }else{
        for(const[k,value]of bindings)if(value===noteId&&k.startsWith(userId+':'))bindings.delete(k)
        await sharedCalendars?.unlinkNote(userId,noteId)
      }
      const current=meeting?mergeMeetingSnapshot(meeting,previous,sameMeeting||previous?.provider===meeting.provider&&previous.eventId===meeting.eventId):null
      if(current&&!ifAbsent&&isDefaultNoteTitle(note.title))await Effect.runPromise(notes.forUser(userId).updateNote(noteId,{title:current.title}))
      const context={noteId,meeting:clone(current)};contexts.set(noteId,context);return clone(context)
    });queue=operation.then(()=>{},()=>{});return operation},
  }
}
