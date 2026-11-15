import { afterAll, describe, expect, test } from 'bun:test'
import { randomBytes } from 'node:crypto'
import { makePrismaRepository } from '../src/prisma-repository'
import { makePrismaGoogleStore } from '../src/google-store'
import { normalizeGoogleEvent } from '../src/google'
import { secretBoxFromKey } from '../src/admin-secrets'

const enabled = process.env.RUN_AUTH_PG_TESTS === '1' && Boolean(process.env.DATABASE_URL)
/** Runs only in the disposable database created by test-postgres-isolated.ts. */
describe.skipIf(!enabled)('Google personal Prisma persistence', () => {
  const resources = enabled ? makePrismaRepository(process.env.DATABASE_URL!) : null
  afterAll(async () => { await resources?.close() })
  test('state consume races, personal calendars, cross-calendar note deduplication and saved context survive refresh/disconnect', async () => {
    const db = resources!.database, store = makePrismaGoogleStore(db), box = secretBoxFromKey(randomBytes(32))
    const alice = crypto.randomUUID(), bob = crypto.randomUUID(), ids = [alice, bob]
    try {
      for (const id of ids) await db.user.create({ data: { id, name: 'Google store test', email: `google-${id}@example.test`, role: id === bob ? 'admin' : 'user' } })
      const stateHash = 'a'.repeat(64), sessionHash = 'b'.repeat(64)
      await store.saveState({ stateHash, sessionHash, userId: alice, encryptedVerifier: box.seal('private-verifier'), contacts: true, expiresAt: '2099-01-01T00:00:00Z' })
      expect(await store.consumeState(stateHash, bob, sessionHash, new Date().toISOString())).toBeNull()
      expect(await store.consumeState(stateHash, alice, 'c'.repeat(64), new Date().toISOString())).toBeNull()
      const consumed = await Promise.all([1, 2, 3].map(() => store.consumeState(stateHash, alice, sessionHash, new Date().toISOString())))
      expect(consumed.filter(Boolean)).toHaveLength(1)
      expect((await db.googleOAuthState.count({ where: { stateHash } }))).toBe(0)
      const connect = (userId: string, subject: string) => store.saveConnection({ userId, subject, email: `${subject}@example.test`, displayName: null, contactsEnabled: false, status: 'connected', encryptedGrant: box.seal(JSON.stringify({ accessToken: 'private-google-access', refreshToken: 'private-google-refresh', expiresAt: Date.now() + 3600000, scopes: [] })) })
      const [one, two, foreign] = await Promise.all([connect(alice, 'one'), connect(alice, 'two'), connect(bob, 'foreign')])
      const remoteCalendar = (remoteId: string) => ({ remoteId, name: `Calendar ${remoteId}`, color: '#87e5e8', timeZone: 'America/New_York', primary: true, accessRole: 'owner' })
      for (const c of [one, two, foreign]) await store.saveCalendars(c.userId, c.id, [remoteCalendar(c.subject)])
      const a = (await store.listCalendars(alice, one.id))[0]!, b = (await store.listCalendars(alice, two.id))[0]!
      expect(await store.listCalendars(bob, one.id)).toEqual([])
      expect(await store.getConnection(bob, one.id)).toBeNull()
      expect(await store.selectCalendar(bob, a.id, false)).toBeNull()
      const raw = { id: 'copy-one', iCalUID: 'google-pg-same-occurrence',recurringEventId:'series-id',originalStartTime:{dateTime:'2026-11-13T15:00:00Z'}, summary: 'Scoped Google meeting', start: { dateTime: '2026-11-13T10:00:00-05:00' }, end: { dateTime: '2026-11-13T11:00:00-05:00' }, attendees: [{ email: 'person@example.test', displayName: 'An explicit calendar name' }] }
      const first = normalizeGoogleEvent(raw, a)!, second = normalizeGoogleEvent({ ...raw, id: 'copy-two', originalStartTime: { dateTime: '2026-11-13T15:00:00Z' } }, b)!
      await store.replaceEvents(alice, a.id, [first]); await store.replaceEvents(alice, b.id, [second])
      expect(first.meetingKey).toBe(second.meetingKey)
      const range = ['2026-11-12T00:00:00Z', '2026-11-20T00:00:00Z'] as const
      expect(await store.listEvents(bob, ...range)).toEqual([])
      expect(await store.createEventNote(bob, first.id)).toBeNull()
      const results = await Promise.all([first.id, second.id, first.id].map(id => store.createEventNote(alice, id)))
      expect(new Set(results.map(note => note!.id)).size).toBe(1)
      const note = results[0]!, context = await store.getContext(alice, note.id)
      expect(context!.meeting!.title).toBe('Scoped Google meeting')
      expect(context!.meeting!.participants[0]!.givenName).toBeNull()
      expect(await store.getContext(bob, note.id)).toBeNull()
      expect(await store.putContext(bob, note.id, null)).toBeNull()
      const listed = await store.listEvents(alice, ...range)
      expect(listed).toHaveLength(2); expect(listed.every(e => e.noteId === note.id)).toBe(true)
      expect(JSON.stringify(listed)).not.toContain('meetingKey')
      const manual=await db.note.create({data:{ownerId:alice,title:'New note',date:new Date('2026-11-13T00:00:00Z')}})
      await expect(store.putContext(alice,manual.id,first.context)).rejects.toThrow('already associated')
      expect((await store.getContext(alice,manual.id))!.meeting).toBeNull()
      await store.putContext(alice,note.id,null)
      expect((await store.listEvents(alice,...range)).every(e=>e.noteId===null)).toBe(true)
      await store.putContext(alice,manual.id,first.context)
      expect((await store.createEventNote(alice,second.id))!.id).toBe(manual.id)
      expect((await db.note.findUniqueOrThrow({where:{id:manual.id}})).title).toBe('Scoped Google meeting')
      expect((await db.note.findUniqueOrThrow({where:{id:note.id}})).title).toBe('Scoped Google meeting')
      await store.putContext(alice,manual.id,null)
      await store.putContext(alice,note.id,first.context)
      const previous=await store.getConnection(alice,one.id)
      await store.updateConnection(alice,one.id,{encryptedGrant:box.seal('fresh-reconnect')})
      expect(await store.updateConnection(alice,one.id,{encryptedGrant:'old-write',status:'reconnect_required'},previous!.encryptedGrant)).toBe(false)
      expect(await store.saveCalendars(alice,one.id,[remoteCalendar('must-not-appear')],previous!.encryptedGrant)).toBe(false)
      expect(await store.replaceEvents(alice,a.id,[],previous!.encryptedGrant)).toBe(false)
      expect((await store.listEvents(alice,...range)).find(e=>e.id===first.id)).toBeDefined()
      expect((await store.getConnection(alice,one.id))!.status).toBe('connected')
      await store.selectCalendar(alice, a.id, false)
      expect(await store.getEvent(alice, first.id)).toBeNull()
      expect(await store.listEvents(alice, ...range)).toHaveLength(1)
      await store.replaceEvents(alice, b.id, [])
      expect((await store.getContext(alice, note.id))!.meeting!.participants[0]!.displayName).toBe('An explicit calendar name')
      expect(await store.deleteConnection(bob, one.id)).toBe(false)
      await store.deleteConnection(alice, one.id); await store.deleteConnection(alice, two.id)
      expect(await store.listConnections(alice)).toEqual([])
      expect((await store.getContext(alice, note.id))!.meeting!.title).toBe('Scoped Google meeting')
      const recreated = makePrismaGoogleStore(db)
      expect((await recreated.getContext(alice, note.id))!.meeting!.title).toBe('Scoped Google meeting')
      expect((await store.listConnections(bob))[0]!.id).toBe(foreign.id)
    } finally {
      await db.user.deleteMany({ where: { id: { in: ids } } })
    }
  })
})
