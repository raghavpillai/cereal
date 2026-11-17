import { afterEach, describe, expect, test } from 'bun:test'
import { createHash, randomBytes } from 'node:crypto'
import { Effect } from 'effect'
import type { PrincipalData } from '@meeting-notes/contracts/api'
import { makeGoogleService, exactContactName, normalizeGoogleEvent } from '../src/google'
import { makeMemoryGoogleStore } from '../src/google-store'
import { makeMemoryRepository } from '../src/repository'
import { secretBoxFromKey } from '../src/admin-secrets'

const sha = (v: string) => createHash('sha256').update(v).digest('hex')
const alice = { id: '31bb7a9f-fbd5-411d-8fa2-17390ad35a76', sessionId: 'alice-browser-session', name: 'Alice', email: 'alice@example.test', role: 'user', source: 'session', expiresAt: '2099-01-01T00:00:00Z', createdAt: '2026-11-12T00:00:00Z' } satisfies PrincipalData
const bob = { ...alice, id: '9f862984-b35f-4349-81a4-5acf93bc7317', sessionId: 'bob-browser-session', role: 'owner' } satisfies PrincipalData
const scopes = ['openid', 'email', 'profile', 'https://www.googleapis.com/auth/calendar.calendarlist.readonly', 'https://www.googleapis.com/auth/calendar.events.readonly', 'https://www.googleapis.com/auth/contacts.readonly'].join(' ')
const cleanup: (() => Promise<void>)[] = []
afterEach(async () => { for (const close of cleanup.splice(0)) await close() })

function fixture(configured = true) {
  const notes = makeMemoryRepository(), store = makeMemoryGoogleStore(notes), box = secretBoxFromKey(randomBytes(32))
  let clock = new Date('2026-11-12T12:00:00Z'), failRefresh = false, failEvents = false, refreshes = 0, lastVerifier = '', tokenCode = ''
  const seen: string[] = []
  const transport: typeof fetch = Object.assign(async (...[input, init]: Parameters<typeof fetch>) => {
    const req = new Request(input, init), url = new URL(req.url); seen.push(url.pathname)
    if (url.href === 'https://oauth2.googleapis.com/token') {
      const form = new URLSearchParams(await req.text())
      if (form.get('grant_type') === 'refresh_token') { refreshes++; return failRefresh ? Response.json({ error: 'invalid_grant', error_description: 'PRIVATE_PROVIDER_SECRET' }, { status: 400 }) : Response.json({ access_token: 'refreshed-private-access', expires_in: 3600 }) }
      lastVerifier = form.get('code_verifier') ?? ''; tokenCode = form.get('code') ?? ''
      return Response.json({ access_token: 'private-access-token', refresh_token: 'private-refresh-token', expires_in: 3600, scope: scopes })
    }
    if (url.host === 'openidconnect.googleapis.com') return Response.json({ sub: 'google-subject-alice', email: 'alice-google@example.test', email_verified: true, name: 'Alice Calendar' })
    if (url.pathname.endsWith('/calendarList')) return Response.json({ items: [
      { id: 'primary-private-id', summary: 'Alice private calendar', primary: true, timeZone: 'America/New_York', backgroundColor: '#87e5e8', accessRole: 'owner' },
      { id: 'second-private-id', summary: 'Alice second calendar', timeZone: 'UTC', backgroundColor: '#91a84d', accessRole: 'reader' },
    ] })
    if (url.pathname.endsWith('/events')) {
      if (failEvents) return Response.json({ error: { message: 'PRIVATE_FEED_CONTENT' } }, { status: 503 })
      return Response.json({ items: [{ id: `event-${url.pathname.includes('second-private-id') ? 'second' : 'primary'}`, summary: 'Private roadmap meeting', start: { dateTime: '2026-11-13T10:00:00-05:00', timeZone: 'America/New_York' }, end: { dateTime: '2026-11-13T11:00:00-05:00' }, attendees: [
        { email: 'person@example.test', responseStatus: 'accepted' },
        { email: 'display@example.test', displayName: 'Single Display Name', responseStatus: 'tentative' },
        { email: 'unknown@example.test' },
      ], organizer: { email: 'host@example.test', displayName: 'Meeting Host' }, conferenceData: { entryPoints: [{ entryPointType: 'video', uri: 'https://meet.google.com/fixture-room' }] } }] })
    }
    if (url.pathname === '/v1/people:searchContacts') {
      if (url.searchParams.get('query') !== 'person@example.test') return Response.json({ results: [] })
      return Response.json({ results: [
        { person: { emailAddresses: [{ value: 'person@example.test.attacker.invalid' }], names: [{ displayName: 'Wrong Prefix Match' }] } },
        { person: { emailAddresses: [{ value: 'PERSON@example.test' }], names: [{ displayName: 'Renée van der Meer', givenName: 'Renée', familyName: 'van der Meer', metadata: { primary: true } }] } },
      ] })
    }
    if (url.pathname === '/revoke') return new Response(null, { status: 200 })
    throw new Error(`Unexpected test endpoint: ${url.host}${url.pathname}`)
  }, { preconnect: () => {} }) as typeof fetch
  const service = makeGoogleService(store, box, { baseURL: 'http://localhost:3000', ...(configured ? { clientId: 'fixture-client', clientSecret: 'fixture-client-secret' } : {}), fetch: transport, clock: () => clock })
  cleanup.push(service.dispose)
  async function begin(contacts = true) { return new URL((await Effect.runPromise(service.beginConnect(alice.id, sha(alice.sessionId), contacts))).authorizationUrl) }
  async function callback(auth: URL, principal: PrincipalData = alice) { return service.callback(new Request(`http://localhost:3000/api/integrations/google/callback?state=${auth.searchParams.get('state')}&code=fixture-code`), principal) }
  async function connected(contacts = true) { const auth = await begin(contacts); const result = await callback(auth); expect(result.headers.get('location')).toBe('/account?google=connected'); return (await Effect.runPromise(service.getStatus(alice.id))).connections[0]! }
  return { service, store, notes, box, begin, callback, connected, seen, get refreshes() { return refreshes }, get lastVerifier() { return lastVerifier }, get tokenCode() { return tokenCode }, advance: (hours: number) => { clock = new Date(clock.getTime() + hours * 3600_000) }, failRefresh: () => { failRefresh = true }, failEvents: () => { failEvents = true } }
}

describe('personal Google OAuth and context', () => {
  test('unconfigured instances expose an honest status and no fake authorization URL', async () => {
    const f = fixture(false)
    expect(await Effect.runPromise(f.service.getStatus(alice.id))).toEqual({ configured: false, redirectUri: null, connections: [], needsHttps: false })
    await expect(f.begin()).rejects.toThrow()
    expect(await Effect.runPromise(f.service.listEvents(alice.id, { from: '2026-11-12T00:00:00Z', to: '2026-11-20T00:00:00Z' }))).toEqual([])
    expect(f.seen).toHaveLength(0)
  })
  test('connecting asks for contacts by default, so attendee names fill in without an opt-in', async () => {
    const f = fixture(), auth = new URL((await Effect.runPromise(f.service.beginConnect(alice.id, sha(alice.sessionId)))).authorizationUrl)
    expect(auth.searchParams.get('scope')?.split(' ')).toContain('https://www.googleapis.com/auth/contacts.readonly')
  })
  test('OAuth binds a single-use state to the current browser and verifies the S256 PKCE exchange', async () => {
    const f = fixture(), auth = await f.begin()
    expect(auth.origin).toBe('https://accounts.google.com')
    expect(auth.searchParams.get('redirect_uri')).toBe('http://localhost:3000/api/integrations/google/callback')
    expect(auth.searchParams.get('access_type')).toBe('offline')
    expect(auth.searchParams.get('code_challenge_method')).toBe('S256')
    expect((await f.callback(auth, bob)).headers.get('location')).toBe('/account?google=failed')
    expect((await f.callback(auth, { ...alice, sessionId: 'different-browser' })).headers.get('location')).toBe('/account?google=failed')
    expect(f.seen).toHaveLength(0)
    expect((await f.callback(auth)).headers.get('location')).toBe('/account?google=connected')
    expect(createHash('sha256').update(f.lastVerifier).digest('base64url')).toBe(auth.searchParams.get('code_challenge')!)
    const before = f.seen.length
    expect((await f.callback(auth)).headers.get('location')).toBe('/account?google=failed')
    expect(f.seen.length).toBe(before)
    const second = await f.begin(); f.advance(1)
    expect((await f.callback(second)).headers.get('location')).toBe('/account?google=failed')
  })
  test('encrypted grants and calendar selection remain private even from another owner-role account', async () => {
    const f = fixture(), c = await f.connected()
    const raw = (await f.store.listConnections(alice.id))[0]!
    expect(raw.encryptedGrant).not.toContain('private-refresh-token')
    expect(f.box.open(raw.encryptedGrant)).toContain('private-refresh-token')
    const status = await Effect.runPromise(f.service.getStatus(alice.id))
    expect(JSON.stringify(status)).not.toContain('private-access-token')
    expect(await Effect.runPromise(f.service.getStatus(bob.id))).toEqual({ configured: true, redirectUri: 'http://localhost:3000/api/integrations/google/callback', connections: [], needsHttps: false })
    await expect(Effect.runPromise(f.service.listCalendars(bob.id, c.id))).rejects.toThrow()
    const calendars = await Effect.runPromise(f.service.listCalendars(alice.id, c.id))
    expect(calendars.filter(v => v.selected)).toHaveLength(1)
    await expect(Effect.runPromise(f.service.selectCalendar(bob.id, calendars[1]!.id, true))).rejects.toThrow()
    await Effect.runPromise(f.service.selectCalendar(alice.id, calendars[1]!.id, true))
    expect((await Effect.runPromise(f.service.listCalendars(alice.id, c.id))).filter(v => v.selected)).toHaveLength(2)
  })
  test('event-note preparation is idempotent and context attach/detach checks both owned note and event', async () => {
    const f = fixture(); await f.connected()
    const range = { from: '2026-11-12T00:00:00Z', to: '2026-11-20T00:00:00Z' }
    const events = await Effect.runPromise(f.service.listEvents(alice.id, range)), event = events[0]!
    expect(events).toHaveLength(1)
    expect(await Effect.runPromise(f.service.listEvents(bob.id, range))).toEqual([])
    expect(await Effect.runPromise(f.service.createEventNote(bob.id, event.id))).toBeNull()
    const notes = await Promise.all([1, 2, 3].map(() => Effect.runPromise(f.service.createEventNote(alice.id, event.id))))
    expect(new Set(notes.map(n => n!.id)).size).toBe(1)
    const noteId = notes[0]!.id, context = await Effect.runPromise(f.service.getNoteContext(alice.id, noteId))
    expect(context.meeting!.participants[0]).toMatchObject({ displayName: 'Renée van der Meer', givenName: 'Renée', familyName: 'van der Meer', nameSource: 'contacts' })
    expect(context.meeting!.participants[1]).toMatchObject({ displayName: 'Single Display Name', givenName: null, familyName: null, nameSource: 'calendar' })
    expect(context.meeting!.participants[2]).toMatchObject({ displayName: null, givenName: null, familyName: null, nameSource: 'unknown' })
    await expect(Effect.runPromise(f.service.getNoteContext(bob.id, noteId))).rejects.toThrow()
    const other = await Effect.runPromise(f.notes.forUser(bob.id).createNote({ title: 'Bob note' }))
    await expect(Effect.runPromise(f.service.setNoteContext(bob.id, other.id, event.id))).rejects.toThrow()
    expect((await Effect.runPromise(f.service.setNoteContext(alice.id, noteId, null))).meeting).toBeNull()
    expect((await Effect.runPromise(f.service.setNoteContext(alice.id, noteId, event.id))).meeting!.eventId).toBe(event.id)
  })
  test('refresh/revocation and temporary sync errors retain owned note snapshots without leaking provider errors', async () => {
    const f = fixture(), c = await f.connected(false), events = await Effect.runPromise(f.service.listEvents(alice.id, { from: '2026-11-12T00:00:00Z', to: '2026-11-20T00:00:00Z' }))
    const note = await Effect.runPromise(f.service.createEventNote(alice.id, events[0]!.id))
    f.advance(2); await Effect.runPromise(f.service.syncConnection(alice.id, c.id)); expect(f.refreshes).toBe(1)
    f.failEvents(); const failed = await Effect.runPromise(f.service.syncConnection(alice.id, c.id))
    expect(failed.connections[0]!.lastSyncError).not.toContain('PRIVATE_FEED_CONTENT')
    expect(await Effect.runPromise(f.service.listEvents(alice.id, { from: '2026-11-12T00:00:00Z', to: '2026-11-20T00:00:00Z' }))).toHaveLength(1)
    f.advance(2); f.failRefresh(); const revoked = await Effect.runPromise(f.service.syncConnection(alice.id, c.id))
    expect(revoked.connections[0]!.status).toBe('reconnect_required')
    expect(JSON.stringify(revoked)).not.toContain('PRIVATE_PROVIDER_SECRET')
    await expect(Effect.runPromise(f.service.disconnect(bob.id, c.id))).rejects.toThrow()
    await Effect.runPromise(f.service.disconnect(alice.id, c.id))
    expect((await Effect.runPromise(f.service.getStatus(alice.id))).connections).toEqual([])
    expect((await Effect.runPromise(f.service.getNoteContext(alice.id, note!.id))).meeting!.title).toBe('Private roadmap meeting')
  })
  test('People name enrichment rejects prefix collisions and ambiguity without splitting display names', () => {
    expect(exactContactName([{ person: { emailAddresses: [{ value: 'alice@example.test.evil' }], names: [{ displayName: 'Wrong' }] } }], 'alice@example.test')).toBeNull()
    const match = (displayName: string) => ({ person: { emailAddresses: [{ value: 'alice@example.test' }], names: [{ displayName }] } })
    expect(exactContactName([match('One'), match('Two')], 'alice@example.test')).toBeNull()
    expect(exactContactName([match('A compound family name')], 'alice@example.test')).toEqual({ displayName: 'A compound family name', givenName: null, familyName: null })
  })
  test('same meeting copies deduplicate by canonical occurrence without collapsing recurring instances or calendar-native IDs', () => {
    const calendar = { id: crypto.randomUUID(), connectionId: crypto.randomUUID(), remoteId: 'calendar-one', name: 'Work', color: '#87e5e8', timeZone: 'UTC', primary: true, selected: true, accessRole: 'owner' }
    const raw = { id: 'provider-event-one', iCalUID: 'shared-meeting-icaluid', recurringEventId:'series-id',originalStartTime:{dateTime:'2026-11-13T15:00:00Z'}, start: { dateTime: '2026-11-13T10:00:00-05:00' }, end: { dateTime: '2026-11-13T11:00:00-05:00' }, htmlLink: 'https://calendar.google.com/calendar/event?eid=fixture' }
    const one = normalizeGoogleEvent(raw, calendar)!
    const two = normalizeGoogleEvent({ ...raw, id: 'another-copy-id', originalStartTime: { dateTime: '2026-11-13T15:00:00Z' } }, { ...calendar, id: crypto.randomUUID(), remoteId: 'calendar-two' })!
    expect(one.id).not.toBe(two.id)
    expect(one.meetingKey).toBe(two.meetingKey)
    expect(one.context.eventUrl).toBe('https://calendar.google.com/calendar/event?eid=fixture')
    expect(normalizeGoogleEvent({ ...raw, originalStartTime: { dateTime: '2026-11-20T15:00:00Z' } }, calendar)!.meetingKey).not.toBe(one.meetingKey)
    expect(normalizeGoogleEvent({ ...raw, iCalUID: null, htmlLink: 'javascript:alert(1)' }, calendar)!.context.eventUrl).toBeNull()
    expect(normalizeGoogleEvent({ ...raw, iCalUID: null }, calendar)!.meetingKey).not.toBe(normalizeGoogleEvent({ ...raw, iCalUID: null }, { ...calendar, remoteId: 'other-calendar' })!.meetingKey)
    const single={...raw,recurringEventId:undefined,originalStartTime:undefined}
    expect(normalizeGoogleEvent(single,calendar)!.meetingKey).toBe(normalizeGoogleEvent({...single,start:{dateTime:'2026-11-20T17:00:00Z'},end:{dateTime:'2026-11-20T18:00:00Z'}},calendar)!.meetingKey)
    const day = normalizeGoogleEvent({ id: 'all-day', start: { date: '2026-11-13' }, end: { date: '2026-11-14' } }, calendar)!
    expect(day.allDay).toBe(true); expect(day.startDate).toBe('2026-11-13'); expect(day.endDate).toBe('2026-11-14')
  })
  test('connecting a calendar reports the picture Google has for that identity, and a failure to save it changes nothing', async () => {
    for (const fails of [false, true]) {
      const notes = makeMemoryRepository(), store = makeMemoryGoogleStore(notes), told: unknown[][] = []
      const transport = Object.assign(async (...[input, init]: Parameters<typeof fetch>) => {
        const url = new URL(new Request(input, init).url)
        if (url.pathname === '/token') return Response.json({ access_token: 'fixture-access', refresh_token: 'fixture-refresh', expires_in: 3600, scope: scopes })
        if (url.pathname.endsWith('/userinfo')) return Response.json({ sub: 'fixture-subject', email: 'fixture@example.test', email_verified: true, picture: 'https://pictures.example.test/fixture=s96-c' })
        return Response.json({ items: [] })
      }, { preconnect: () => {} }) as typeof fetch
      const service = makeGoogleService(store, secretBoxFromKey(randomBytes(32)), { clientId: 'fixture-client', clientSecret: 'fixture-secret', baseURL: 'http://localhost:3000', fetch: transport,
        picture: async (...call) => { told.push(call); if (fails) throw new Error('database away') } })
      cleanup.push(service.dispose)
      const begin = new URL((await Effect.runPromise(service.beginConnect(alice.id, sha(alice.sessionId)))).authorizationUrl)
      const answer = await service.callback(new Request(`http://localhost:3000/api/integrations/google/callback?state=${begin.searchParams.get('state')}&code=fixture`), alice)
      expect(answer.headers.get('location')).toBe('/account?google=connected')
      expect(told).toEqual([[alice.id, 'fixture-subject', 'https://pictures.example.test/fixture=s96-c']])
    }
  })
  test('provider body streaming and repeated calendar pagination stop at finite limits', async () => {
    for (const mode of ['stream', 'pages'] as const) {
      let canceled = false, pages = 0
      const notes = makeMemoryRepository(), store = makeMemoryGoogleStore(notes)
      const transport = Object.assign(async (...[input, init]: Parameters<typeof fetch>) => {
        const url = new URL(new Request(input, init).url)
        if (url.pathname === '/token') return Response.json({ access_token: 'fixture-access', refresh_token: 'fixture-refresh', expires_in: 3600, scope: scopes })
        if (url.pathname.endsWith('/userinfo')) return Response.json({ sub: 'fixture-subject', email: 'fixture@example.test', email_verified: true })
        pages++
        return mode === 'pages' ? Response.json({ items: [], nextPageToken: 'repeated-provider-page' }) : new Response(new ReadableStream<Uint8Array>({ pull(controller) { controller.enqueue(new Uint8Array(1_000_000)) }, cancel() { canceled = true } }))
      }, { preconnect: () => {} }) as typeof fetch
      const service = makeGoogleService(store, secretBoxFromKey(randomBytes(32)), { clientId: 'fixture-client', clientSecret: 'fixture-secret', baseURL: 'http://localhost:3000', fetch: transport })
      cleanup.push(service.dispose)
      const begin = new URL((await Effect.runPromise(service.beginConnect(alice.id, sha(alice.sessionId)))).authorizationUrl)
      await service.callback(new Request(`http://localhost:3000/api/integrations/google/callback?state=${begin.searchParams.get('state')}&code=fixture`), alice)
      const result = await Effect.runPromise(service.getStatus(alice.id))
      expect(result.connections[0]!.lastSyncError).toBeTruthy()
      if (mode === 'stream') expect(canceled).toBe(true)
      else expect(pages).toBeLessThanOrEqual(21)
    }
  })
})

test('an old refresh cannot overwrite a fresh reconnect grant, events or connection status',async()=>{
 for(const rejected of[false,true]){
  const notes=makeMemoryRepository(),store=makeMemoryGoogleStore(notes),box=secretBoxFromKey(randomBytes(32));let started:()=>void=()=>{},release:(r:Response)=>void=()=>{};const waiting=new Promise<void>(r=>{started=r})
  const transport:typeof fetch=Object.assign(async(input:Parameters<typeof fetch>[0],init?:RequestInit)=>{const req=new Request(input,init),url=new URL(req.url);if(url.host==='oauth2.googleapis.com'){started();return new Promise<Response>(r=>{release=r})}if(url.pathname.endsWith('/calendarList'))return Response.json({items:[{id:'primary',summary:'Work',primary:true,timeZone:'UTC',accessRole:'owner'}]});if(url.pathname.endsWith('/events'))return Response.json({items:[{id:'fresh',iCalUID:'fresh',summary:'NEW CONNECTION EVENT',start:{dateTime:'2026-11-13T12:00:00Z'},end:{dateTime:'2026-11-13T13:00:00Z'}}]});throw new Error('Unexpected provider request')},{preconnect:()=>{}})as typeof fetch
  const service=makeGoogleService(store,box,{baseURL:'http://localhost:3000',clientId:'test-client',clientSecret:'test-secret',fetch:transport,clock:()=>new Date('2026-11-12T12:00:00Z')});cleanup.push(service.dispose)
  const old=await store.saveConnection({userId:alice.id,subject:'same-subject',email:'calendar@example.test',displayName:null,contactsEnabled:false,status:'connected',encryptedGrant:box.seal(JSON.stringify({accessToken:'old-access',refreshToken:'old-refresh',expiresAt:0,scopes:[]}))})
  const refresh=Effect.runPromise(service.syncConnection(alice.id,old.id));await waiting
  const fresh=await store.saveConnection({...old,encryptedGrant:box.seal(JSON.stringify({accessToken:'new-consent-access',refreshToken:'new-consent-refresh',expiresAt:Date.parse('2026-11-13T12:00:00Z'),scopes:[]}))});await Effect.runPromise(service.syncConnection(alice.id,fresh.id))
  release(rejected?Response.json({error:'invalid_grant'},{status:400}):Response.json({access_token:'obsolete-refreshed-access',refresh_token:'obsolete-refresh',expires_in:3600}));await refresh
  const actual=(await store.getConnection(alice.id,fresh.id))!;expect(JSON.parse(box.open(actual.encryptedGrant)).accessToken).toBe('new-consent-access');expect(actual.status).toBe('connected');expect(actual.lastSyncError).toBeNull();expect((await store.listEvents(alice.id,'2026-11-12T00:00:00Z','2026-11-20T00:00:00Z'))[0]!.title).toBe('NEW CONNECTION EVENT')
 }
})
test('a held old event fetch cannot replace the fresh reconnect event cache',async()=>{
 const notes=makeMemoryRepository(),store=makeMemoryGoogleStore(notes),box=secretBoxFromKey(randomBytes(32));let started:()=>void=()=>{},release:(r:Response)=>void=()=>{};const waiting=new Promise<void>(r=>{started=r})
 const event=(title:string)=>Response.json({items:[{id:'same-event',iCalUID:'canonical-event',summary:title,start:{dateTime:'2026-11-13T12:00:00Z'},end:{dateTime:'2026-11-13T13:00:00Z'}}]})
 const transport:typeof fetch=Object.assign(async(input:Parameters<typeof fetch>[0],init?:RequestInit)=>{const req=new Request(input,init),url=new URL(req.url);if(url.pathname.endsWith('/calendarList'))return Response.json({items:[{id:'primary',summary:'Work',primary:true,timeZone:'UTC',accessRole:'owner'}]});if(url.pathname.endsWith('/events')){if(req.headers.get('authorization')==='Bearer old-access'){started();return new Promise<Response>(r=>{release=r})}return event('NEW EVENT')}throw new Error('Unexpected request')},{preconnect:()=>{}})as typeof fetch
 const service=makeGoogleService(store,box,{baseURL:'http://localhost:3000',clientId:'test-client',clientSecret:'test-secret',fetch:transport,clock:()=>new Date('2026-11-12T12:00:00Z')});cleanup.push(service.dispose)
 const old=await store.saveConnection({userId:alice.id,subject:'same-subject',email:'calendar@example.test',displayName:null,contactsEnabled:false,status:'connected',encryptedGrant:box.seal(JSON.stringify({accessToken:'old-access',refreshToken:'old-refresh',expiresAt:Date.parse('2026-11-13T12:00:00Z'),scopes:[]}))});const previous=Effect.runPromise(service.syncConnection(alice.id,old.id));await waiting
 const fresh=await store.saveConnection({...old,encryptedGrant:box.seal(JSON.stringify({accessToken:'fresh-access',refreshToken:'fresh-refresh',expiresAt:Date.parse('2026-11-13T12:00:00Z'),scopes:[]}))});await Effect.runPromise(service.syncConnection(alice.id,fresh.id));release(event('OBSOLETE EVENT'));await previous
 expect((await store.listEvents(alice.id,'2026-11-12T00:00:00Z','2026-11-20T00:00:00Z'))[0]!.title).toBe('NEW EVENT');expect((await store.getConnection(alice.id,fresh.id))!.lastSyncError).toBeNull()
})
test('upcoming excludes self-declined meetings and focus blocks while preserving another attendee decline',()=>{
 const calendar={id:crypto.randomUUID(),connectionId:crypto.randomUUID(),remoteId:'primary',name:'Work',color:'#87e5e8',timeZone:'UTC',primary:true,selected:true,accessRole:'owner'},raw={id:'event',start:{dateTime:'2026-11-13T12:00:00Z'},end:{dateTime:'2026-11-13T13:00:00Z'}}
 expect(normalizeGoogleEvent({...raw,eventType:'focusTime'},calendar)).toBeNull();expect(normalizeGoogleEvent({...raw,attendees:[{self:true,responseStatus:'declined'}]},calendar)).toBeNull();expect(normalizeGoogleEvent({...raw,attendees:[{self:false,responseStatus:'declined'}]},calendar)).not.toBeNull()
})
