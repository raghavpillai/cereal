import { googleNeedsHttps } from '@meeting-notes/contracts/transport'
import { createHash, randomBytes } from 'node:crypto'
import { Effect } from 'effect'
import type { GoogleCalendarDTO, GoogleStatusDTO, MeetingContextDTO, MeetingParticipantDTO } from '@meeting-notes/contracts'
import type { ApiError, PrincipalData } from '@meeting-notes/contracts/api'
import type { SecretBox } from './admin-secrets'
import type { GoogleCalendar, GoogleConnection, GoogleEvent, GoogleStore } from './google-store'
import {MeetingBindingConflict} from './meeting-context-errors'
import type { GoogleIntegration } from './google-integration'
import { wellFormedOccurrence } from './calendar-feed'

const CALENDAR_LIST = 'https://www.googleapis.com/auth/calendar.calendarlist.readonly'
const CALENDAR_EVENTS = 'https://www.googleapis.com/auth/calendar.events.readonly'
const CONTACTS = 'https://www.googleapis.com/auth/contacts.readonly'
const TOKEN_URL = 'https://oauth2.googleapis.com/token'
const sha = (value: string) => createHash('sha256').update(value).digest('hex')
const uuid = (value: string) => { const h = sha(value); return `${h.slice(0, 8)}-${h.slice(8, 12)}-4${h.slice(13, 16)}-a${h.slice(17, 20)}-${h.slice(20, 32)}` }
const object = (v: unknown): Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v) ? v as Record<string, unknown> : {}
const text = (v: unknown, max = 500): string | null => typeof v === 'string' && v.trim() ? v.trim().slice(0, max) : null
const email = (v: unknown) => { const s = text(v, 254); return s && /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(s) ? s : null }
const cleanEmail = (v: string) => v.trim().toLowerCase()
const safeURL = (v: unknown) => { try { const u = new URL(String(v)); return u.protocol === 'https:' && !u.username && !u.password && u.href.length <= 4096 ? u.href : null } catch { return null } }
const safeCalendar = ({ remoteId, ...dto }: GoogleCalendar): GoogleCalendarDTO => dto
const safeConnection = ({ userId, subject, encryptedGrant, ...dto }: GoogleConnection) => dto
class Failure extends Error { constructor(readonly payload: ApiError) { super(payload.message) } }
class StaleGoogleSync extends Error {}
class ProviderFailure extends Error { constructor(readonly reconnect = false) { super('Google request failed') } }
const missing = () => new Failure({ error: 'not_found', message: 'Calendar, connection or note not found' })
const disabled = () => new Failure({ error: 'unavailable', message: 'Google Calendar is not configured for this instance' })
interface Grant { accessToken: string; refreshToken: string; expiresAt: number; scopes: string[] }

/** Calendar names and voice identities are separate. People search is prefix
 * matching, so only an exact email match may enrich an invited participant. */
export function exactContactName(people: unknown, attendeeEmail: string): Pick<MeetingParticipantDTO, 'displayName' | 'givenName' | 'familyName'> | null {
  if (!Array.isArray(people)) return null
  const matches = people.flatMap(result => {
    const p = object(object(result).person)
    if (!Array.isArray(p.emailAddresses) || !p.emailAddresses.some(v => cleanEmail(String(object(v).value ?? '')) === cleanEmail(attendeeEmail))) return []
    const names = Array.isArray(p.names) ? p.names : []
    const primary = names.find(n => object(object(n).metadata).primary === true) ?? names[0]
    const n = object(primary), displayName = text(n.displayName)
    return displayName ? [{ displayName, givenName: text(n.givenName, 200), familyName: text(n.familyName, 200) }] : []
  })
  const unique = new Map(matches.map(n => [JSON.stringify(n), n]))
  return unique.size === 1 ? [...unique.values()][0]! : null
}

function participant(raw: unknown, seed: string): MeetingParticipantDTO {
  const p = object(raw), name = text(p.displayName), address = email(p.email)
  const response = p.responseStatus
  return { id: uuid(`${seed}:${address ?? text(p.id) ?? ''}`), email: address, displayName: name,
    givenName: null, familyName: null, nameSource: name ? 'calendar' : 'unknown', isSelf: p.self === true,
    isOrganizer: p.organizer === true, isResource: p.resource === true,
    responseStatus: response === 'needsAction' || response === 'declined' || response === 'tentative' || response === 'accepted' ? response : 'unknown' }
}

export function normalizeGoogleEvent(raw: unknown, calendar: GoogleCalendar): GoogleEvent | null {
  const e = object(raw), id = text(e.id, 1024), start = object(e.start), end = object(e.end)
  if (!id || e.status === 'cancelled' || e.eventType === 'focusTime' || (Array.isArray(e.attendees)&&e.attendees.some(a=>object(a).self===true&&object(a).responseStatus==='declined'))) return null
  const allDay = Boolean(text(start.date)), startDate = text(start.date, 10) ?? text(start.dateTime, 100)?.slice(0, 10), endDate = text(end.date, 10) ?? text(end.dateTime, 100)?.slice(0, 10)
  const startAt = new Date(allDay ? `${startDate}T00:00:00Z` : String(start.dateTime)), endAt = new Date(allDay ? `${endDate}T00:00:00Z` : String(end.dateTime))
  if (!startDate || !endDate || !Number.isFinite(startAt.getTime()) || !Number.isFinite(endAt.getTime()) || endAt < startAt) return null
  const eventId = uuid(`google:${calendar.id}:${id}`), entries = object(e.conferenceData).entryPoints
  const video = Array.isArray(entries) ? entries.find(v => object(v).entryPointType === 'video') : null
  const meetingUrl = safeURL(object(video).uri) ?? safeURL(e.hangoutLink)
  const participants = (Array.isArray(e.attendees) ? e.attendees.slice(0, 500) : []).map((p, i) => participant(p, `${eventId}:${i}`))
  const organizerRaw = object(e.organizer)
  const organizer = email(organizerRaw.email) || text(organizerRaw.displayName) ? { ...participant(organizerRaw, `${eventId}:organizer`), isOrganizer: true } : null
  const context: MeetingContextDTO = { eventId, provider: 'google', title: text(e.summary) ?? 'Untitled event', calendarName: calendar.name,
    startAt: startAt.toISOString(), endAt: endAt.toISOString(), allDay, timeZone: text(start.timeZone, 200) ?? calendar.timeZone,
    location: text(e.location, 1000), meetingUrl, eventUrl: safeURL(e.htmlLink), organizer, participants }
  const original = object(e.originalStartTime), originalTime = text(original.dateTime, 100), originalDate = text(original.date, 10)
  const recurring=!!(text(e.recurringEventId,1024)||originalDate||originalTime)
  const occurrence = !recurring ? "single" : allDay ? `date:${originalDate ?? startDate}` : `instant:${originalTime && Number.isFinite(Date.parse(originalTime)) ? new Date(originalTime).toISOString() : context.startAt}`
  const meetingKey = sha(`google-meeting:${text(e.iCalUID, 2048) ?? `remote:${calendar.remoteId}:${id}`}:${occurrence}`)
  // The identity sent to the apps: the iCalendar UID as Google gives it (never the `remote:` fallback, which
  // means nothing on another server, and never a shortened one) with the occurrence in the same three forms as
  // calendar-feed.ts.
  const rawUid = typeof e.iCalUID === 'string' ? e.iCalUID.trim() : '', uid = rawUid && rawUid.length <= 2048 ? rawUid : null
  return { id: eventId, remoteId: id, meetingKey, meetingUid: uid, occurrence: wellFormedOccurrence(occurrence), calendarId: calendar.id, title: context.title, startAt: context.startAt, endAt: context.endAt, allDay, startDate, endDate, timeZone: context.timeZone, location: context.location, meetingUrl, context }
}

/** All Google grants, calendars, events and contact enrichments are account-scoped.
 * OAuth codes and provider errors never appear in logs, application DTOs or URLs. */
export function makeGoogleService(store: GoogleStore, box: SecretBox, options: {
  clientId?: string; clientSecret?: string; baseURL: string; fetch?: typeof fetch; clock?: () => Date; worker?: boolean
  /** The OAuth client as it is right now (set on the admin panel, else the environment). Read on every use, so a change needs no restart. */
  credentials?: () => Promise<{ clientId: string; clientSecret: string } | null>
  /** Told the picture Google has for an identity each time one connects (the value is Google's, unchecked). */
  picture?: (userId: string, subject: string, picture: unknown) => Promise<void>
}) {
  const base = new URL(options.baseURL), fixed = options.clientId && options.clientSecret ? { clientId: options.clientId, clientSecret: options.clientSecret } : null
  const credentials = async () => options.credentials ? await options.credentials() : fixed
  if (!['https:', 'http:'].includes(base.protocol)) throw new Error('The public address must be an http or https address')
  // Google redirects only to HTTPS addresses and to this machine. A server on plain HTTP elsewhere starts and
  // works; connecting a calendar is refused here, with the reason, instead of failing on Google's own page.
  const needsHttps = googleNeedsHttps(base.origin)
  const redirectUri = `${base.origin}/api/integrations/google/callback`, transport = options.fetch ?? globalThis.fetch, now = options.clock ?? (() => new Date())
  const shutdown = new AbortController(), running = new Map<string, Promise<void>>()
  let disposed = false, ticking = false
  const boundary = <A,>(f: () => Promise<A>) => Effect.tryPromise({ try: f, catch: (e): ApiError => e instanceof Failure ? e.payload : e instanceof MeetingBindingConflict ? {error:'conflict',message:e.message} : { error: 'unavailable', message: 'Google Calendar could not finish this request. Please retry or reconnect.' } })
  const requireConfigured = async () => { const client = await credentials(); if (!client) throw disabled(); return client }
  async function json(url: string, init?: RequestInit): Promise<Record<string, unknown>> {
    const response = await transport(url, { ...init, redirect: 'error', cache: 'no-store', signal: AbortSignal.any([shutdown.signal, AbortSignal.timeout(15_000)]) })
    const reader = response.body?.getReader(), chunks: Uint8Array[] = []; let size = 0
    if (!reader) throw new ProviderFailure()
    try { while (true) { const chunk = await reader.read(); if (chunk.done) break; size += chunk.value.byteLength; if (size > 4_000_000) { await reader.cancel(); throw new ProviderFailure() } chunks.push(chunk.value) } } finally { reader.releaseLock() }
    const bytes = new Uint8Array(size); let offset = 0; for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.byteLength }
    const body = new TextDecoder().decode(bytes)
    let data: Record<string, unknown>; try { data = object(JSON.parse(body)) } catch { throw new ProviderFailure() }
    if (!response.ok) throw new ProviderFailure(response.status === 401 || data.error === 'invalid_grant')
    return data
  }
  const grant = (c: GoogleConnection): Grant => JSON.parse(box.open(c.encryptedGrant))
  async function access(state:{connection:GoogleConnection}): Promise<string> {
    const c=state.connection,current=await store.getConnection(c.userId,c.id);if(!current||current.encryptedGrant!==c.encryptedGrant)throw new StaleGoogleSync()
    const g = grant(c)
    if (g.accessToken && g.expiresAt > now().getTime() + 60_000) return g.accessToken
    const client = await requireConfigured()
    try {
      const response = await json(TOKEN_URL, { method: 'POST', headers: { 'Content-Type': 'application/x-www-form-urlencoded' }, body: new URLSearchParams({ client_id: client.clientId, client_secret: client.clientSecret, refresh_token: g.refreshToken, grant_type: 'refresh_token' }) })
      const token = text(response.access_token, 10_000)
      if (!token) throw new ProviderFailure(true)
      const updated: Grant = { ...g, accessToken: token, refreshToken: text(response.refresh_token, 10_000) ?? g.refreshToken, expiresAt: now().getTime() + Math.min(Number(response.expires_in) || 3600, 86400) * 1000 }
      const encryptedGrant=box.seal(JSON.stringify(updated));if(!await store.updateConnection(c.userId,c.id,{encryptedGrant,status:'connected'},c.encryptedGrant))throw new StaleGoogleSync();state.connection={...c,encryptedGrant,status:'connected'}
      return token
    } catch (e) { if (e instanceof ProviderFailure && e.reconnect) await store.updateConnection(c.userId, c.id, { status: 'reconnect_required' },c.encryptedGrant); throw e }
  }
  const googleGet=async(url:string,token:string,c?:GoogleConnection)=>{if(c){const current=await store.getConnection(c.userId,c.id);if(!current||current.encryptedGrant!==c.encryptedGrant)throw new StaleGoogleSync()}return json(url,{headers:{Authorization:`Bearer ${token}`}})}
  async function calendars(c: GoogleConnection, token: string) {
    const rows: Omit<GoogleCalendar, 'id' | 'connectionId' | 'selected'>[] = []; let page: string | null = null, pageCount = 0
    do {
      const u = new URL('https://www.googleapis.com/calendar/v3/users/me/calendarList'); u.searchParams.set('maxResults', '250'); if (page) u.searchParams.set('pageToken', page)
      const data = await googleGet(u.href, token,c)
      for (const raw of Array.isArray(data.items) ? data.items : []) {
        const item = object(raw), remoteId = text(item.id, 1024)
        if (!remoteId || item.deleted === true || item.accessRole === 'freeBusyReader') continue
        rows.push({ remoteId, name: text(item.summaryOverride, 200) ?? text(item.summary, 200) ?? 'Calendar', color: /^#[a-f\d]{6}$/i.test(String(item.backgroundColor)) ? String(item.backgroundColor) : '#87e5e8', timeZone: text(item.timeZone, 200) ?? 'UTC', primary: item.primary === true, accessRole: text(item.accessRole, 64) ?? 'reader' })
      }
      page = text(data.nextPageToken, 4096); if (rows.length > 1000 || ++pageCount > 20) throw new ProviderFailure()
    } while (page && !disposed)
    if(!await store.saveCalendars(c.userId,c.id,rows,c.encryptedGrant))throw new StaleGoogleSync()
  }
  async function performSync(state:{connection:GoogleConnection}) {
    await requireConfigured()
    const token = await access(state),c=state.connection
    await calendars(c, token)
    const names = new Map<string, ReturnType<typeof exactContactName>>(); let contactsReady = false
    if (c.contactsEnabled) { try { await googleGet('https://people.googleapis.com/v1/people:searchContacts?query=&readMask=names,emailAddresses&pageSize=30', token,c); contactsReady = true } catch(e) { if(e instanceof StaleGoogleSync)throw e; /* Optional names never block calendar ingestion. */ } }
    async function resolveParticipant(person: MeetingParticipantDTO): Promise<MeetingParticipantDTO> {
      if (!contactsReady || !person.email || person.isResource || (person.displayName && person.givenName && person.familyName)) return person
      const key = cleanEmail(person.email)
      if (!names.has(key) && names.size < 100) {
        try { const u = new URL('https://people.googleapis.com/v1/people:searchContacts'); u.searchParams.set('query', person.email); u.searchParams.set('readMask', 'names,emailAddresses'); u.searchParams.set('pageSize', '30'); const result = await googleGet(u.href, token,c); names.set(key, exactContactName(result.results, person.email)) } catch(e) { if(e instanceof StaleGoogleSync)throw e; names.set(key, null) }
      }
      const found = names.get(key)
      return found ? { ...person, displayName: person.displayName ?? found.displayName, givenName: person.givenName ?? found.givenName, familyName: person.familyName ?? found.familyName, nameSource: 'contacts' } : person
    }
    for (const calendar of await store.listCalendars(c.userId, c.id)) {
      if (!calendar.selected || disposed) continue
      const events: GoogleEvent[] = []; let page: string | null = null, pageCount = 0
      do {
        const u = new URL(`https://www.googleapis.com/calendar/v3/calendars/${encodeURIComponent(calendar.remoteId)}/events`)
        u.searchParams.set('singleEvents', 'true'); u.searchParams.set('orderBy', 'startTime'); u.searchParams.set('maxResults', '2500')
        u.searchParams.set('timeMin', new Date(now().getTime() - 30 * 86400_000).toISOString()); u.searchParams.set('timeMax', new Date(now().getTime() + 90 * 86400_000).toISOString())
        if (page) u.searchParams.set('pageToken', page)
        const data = await googleGet(u.href, token,c)
        for (const raw of Array.isArray(data.items) ? data.items : []) { const event = normalizeGoogleEvent(raw, calendar); if (event) events.push(event) }
        page = text(data.nextPageToken, 4096); if (events.length > 10000 || ++pageCount > 20) throw new ProviderFailure()
      } while (page && !disposed)
      if (contactsReady) for (const event of events) {
        const enriched: MeetingParticipantDTO[] = []
        for (const person of event.context.participants) enriched.push(await resolveParticipant(person))
        event.context = { ...event.context, participants: enriched, organizer: event.context.organizer ? await resolveParticipant(event.context.organizer) : null }
      }
      if(!await store.replaceEvents(c.userId,calendar.id,events,c.encryptedGrant))throw new StaleGoogleSync()
    }
    if(!await store.updateConnection(c.userId,c.id,{lastSyncedAt:now().toISOString(),lastSyncError:null,status:'connected'},c.encryptedGrant))throw new StaleGoogleSync()
  }
  async function sync(c: GoogleConnection) {
    const key=c.id+':'+c.encryptedGrant,old=running.get(key);if(old)return old
    const state={connection:c}
    const job=performSync(state).catch(async e=>{
      if(e instanceof StaleGoogleSync)return
      const current=state.connection
      await store.updateConnection(current.userId,current.id,{lastSyncError:'Google Calendar could not be refreshed. Retry or reconnect your account.',...(e instanceof ProviderFailure&&e.reconnect?{status:'reconnect_required' as const}:{})},current.encryptedGrant)
    })
    running.set(key,job);try{await job}finally{if(running.get(key)===job)running.delete(key)}
  }
  async function status(userId: string): Promise<GoogleStatusDTO> { const configured = !!await credentials(); return { configured, redirectUri: configured ? redirectUri : null, connections: (await store.listConnections(userId)).map(safeConnection), needsHttps } }
  let timer: ReturnType<typeof setInterval> | undefined
  // The client can be added on the admin panel after start, so the worker always runs and checks each time.
  if (options.worker) {
    timer = setInterval(() => { if (ticking || disposed) return; ticking = true; void (async () => { if (!await credentials()) return; for (const c of await store.listConnections()) { if (disposed) break; if (c.status === 'connected' && (!c.lastSyncedAt || Date.parse(c.lastSyncedAt) < now().getTime() - 15 * 60_000)) await sync(c) } })().catch(() => {}).finally(() => { ticking = false }) }, 60_000); timer.unref()
  }
  const service = {
    getStatus: (userId: string) => boundary(() => status(userId)),
    beginConnect: (userId: string, sessionHash: string, contacts = true) => boundary(async () => {
      if (needsHttps) throw new Failure({ error: 'invalid_request', message: 'Google Calendar needs an HTTPS address. This server is reached over plain HTTP.' })
      const client = await requireConfigured(); if (!/^[a-f\d]{64}$/.test(sessionHash)) throw new Failure({ error: 'forbidden', message: 'A signed-in browser session is required' })
      const state = randomBytes(32).toString('base64url'), verifier = randomBytes(48).toString('base64url')
      await store.saveState({ stateHash: sha(state), userId, sessionHash, encryptedVerifier: box.seal(verifier), contacts, expiresAt: new Date(now().getTime() + 10 * 60_000).toISOString() })
      const u = new URL('https://accounts.google.com/o/oauth2/v2/auth')
      const scopes = ['openid', 'email', 'profile', CALENDAR_LIST, CALENDAR_EVENTS, ...(contacts ? [CONTACTS] : [])]
      for (const [k, v] of Object.entries({ client_id: client.clientId, redirect_uri: redirectUri, response_type: 'code', scope: scopes.join(' '), access_type: 'offline', include_granted_scopes: 'true', prompt: 'consent select_account', state, code_challenge: createHash('sha256').update(verifier).digest('base64url'), code_challenge_method: 'S256' })) u.searchParams.set(k, v)
      return { authorizationUrl: u.href }
    }),
    async callback(request: Request, principal: PrincipalData): Promise<Response> {
      let success = false
      try {
        const client = await requireConfigured()
        if (principal.source !== 'session' || !principal.sessionId || request.headers.has('authorization')) throw new ProviderFailure()
        const u = new URL(request.url), state = u.searchParams.get('state'), code = u.searchParams.get('code')
        if (!state || !/^[A-Za-z\d_-]{43}$/.test(state)) throw new ProviderFailure()
        const attempt = await store.consumeState(sha(state), principal.id, sha(principal.sessionId), now().toISOString())
        if (!attempt || u.searchParams.has('error') || !code || code.length > 4096) throw new ProviderFailure()
        const data = await json(TOKEN_URL, { method: 'POST', headers: { 'Content-Type': 'application/x-www-form-urlencoded' }, body: new URLSearchParams({ code, client_id: client.clientId, client_secret: client.clientSecret, redirect_uri: redirectUri, grant_type: 'authorization_code', code_verifier: box.open(attempt.encryptedVerifier) }) })
        const accessToken = text(data.access_token, 10000), scopes = String(data.scope ?? '').split(' ')
        // Google may consolidate a previously granted broader scope when
        // include_granted_scopes is enabled. Require actual read capability,
        // while still requesting only the two granular read-only scopes.
        const calendarRead = scopes.includes('https://www.googleapis.com/auth/calendar.readonly') || scopes.includes('https://www.googleapis.com/auth/calendar') || (scopes.includes(CALENDAR_LIST) && scopes.includes(CALENDAR_EVENTS))
        if (!accessToken || !calendarRead) throw new ProviderFailure()
        const identity = await googleGet('https://openidconnect.googleapis.com/v1/userinfo', accessToken), subject = text(identity.sub, 255), address = email(identity.email)
        if (!subject || !address || identity.email_verified !== true) throw new ProviderFailure()
        await options.picture?.(principal.id, subject, identity.picture).catch(() => {})
        const old = (await store.listConnections(principal.id)).find(c => c.subject === subject)
        const refreshToken = text(data.refresh_token, 10000) ?? (old ? grant(old).refreshToken : null)
        if (!refreshToken) throw new ProviderFailure(true)
        const c = await store.saveConnection({ userId: principal.id, subject, email: address, displayName: text(identity.name), contactsEnabled: attempt.contacts && (scopes.includes(CONTACTS) || scopes.includes('https://www.googleapis.com/auth/contacts')), status: 'connected', encryptedGrant: box.seal(JSON.stringify({ accessToken, refreshToken, scopes, expiresAt: now().getTime() + Math.min(Number(data.expires_in) || 3600, 86400) * 1000 } satisfies Grant)) })
        await sync(c); success = true
      } catch { /* Fixed redirect never reflects Google codes, state or error text. */ }
      return new Response(null, { status: 303, headers: { Location: `/account?google=${success ? 'connected' : 'failed'}`, 'Cache-Control': 'no-store', 'Referrer-Policy': 'no-referrer' } })
    },
    listCalendars: (userId: string, id: string) => boundary(async () => { if (!await store.getConnection(userId, id)) throw missing(); return (await store.listCalendars(userId, id)).map(safeCalendar) }),
    selectCalendar: (userId: string, id: string, selected: boolean) => boundary(async () => { const c = await store.selectCalendar(userId, id, selected); if (!c) throw missing(); const connection = await store.getConnection(userId, c.connectionId); if (selected && connection && await credentials()) await sync(connection); return safeCalendar(c) }),
    syncConnection: (userId: string, id: string) => boundary(async () => { await requireConfigured(); const c = await store.getConnection(userId, id); if (!c) throw missing(); await sync(c); return status(userId) }),
    disconnect: (userId: string, id: string) => boundary(async () => { const c = await store.getConnection(userId, id); if (!c) throw missing(); try { await transport('https://oauth2.googleapis.com/revoke', { method: 'POST', headers: { 'Content-Type': 'application/x-www-form-urlencoded' }, body: new URLSearchParams({ token: grant(c).refreshToken }), signal: AbortSignal.timeout(10_000), redirect: 'error' }) } catch { /* Local revocation is still final if Google is offline. */ } await store.deleteConnection(userId, id); return { deleted: true } }),
    listEvents: (userId: string, query: { from: string; to: string }) => boundary(async () => { if (!await credentials()) return []; const rows = await store.listEvents(userId, query.from, query.to); if (rows.length > 10000) throw new Failure({ error: 'invalid_request', message: 'Request a shorter event range' }); return rows }),
    createEventNote: (userId: string, id: string) => boundary(() => store.createEventNote(userId, id)),
    getNoteContext: (userId: string, noteId: string) => boundary(async () => { const context = await store.getContext(userId, noteId); if (!context) throw missing(); return context }),
    putExternalContext: (userId: string, noteId: string, meeting: MeetingContextDTO | null) => boundary(async () => { const context = await store.putContext(userId, noteId, meeting); if (!context) throw missing(); return context }),
    initializeExternalContext: (userId: string, noteId: string, meeting: MeetingContextDTO) => boundary(async () => { const context = await store.putContext(userId, noteId, meeting, true); if (!context) throw missing(); return context }),
    setNoteContext: (userId: string, noteId: string, eventId: string | null) => boundary(async () => { const existing = await store.getContext(userId, noteId); if (!existing) throw missing(); const event = eventId ? await store.getEvent(userId, eventId) : null; if (eventId && !event) throw missing(); const context = await store.putContext(userId, noteId, event?.context ?? null); if (!context) throw missing(); return context }),
    async dispose() { disposed = true; if (timer) clearInterval(timer); shutdown.abort(); await Promise.allSettled([...running.values()]) },
  } satisfies GoogleIntegration
  return service
}
export type GoogleService = ReturnType<typeof makeGoogleService>
