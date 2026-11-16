import { describe, expect, test } from 'bun:test'
import type { GoogleConnectionDTO, GoogleStatusDTO } from '@meeting-notes/contracts'
import { syncedNotice } from '../components/account/google-calendars'

const connection = { id: '00000000-0000-4000-8000-000000000001', email: 'me@example.test', displayName: null, status: 'connected', contactsEnabled: false, lastSyncedAt: '2026-11-16T10:00:00.000Z', lastSyncError: null } as unknown as GoogleConnectionDTO
const status = (connections: GoogleConnectionDTO[]) => ({ configured: true, needsHttps: false, connections }) as unknown as GoogleStatusDTO

describe('"Sync now"', () => {
  test('says Synced. only when the returned connection synced cleanly', () => {
    expect(syncedNotice(status([connection]), connection.id)).toBe('Synced.')
  })
  test('says nothing when Google refused (the server still answers 200), so the card error is not contradicted', () => {
    expect(syncedNotice(status([{ ...connection, lastSyncError: 'provider_error' } as GoogleConnectionDTO]), connection.id)).toBeNull()
    expect(syncedNotice(status([{ ...connection, status: 'reconnect_required' } as GoogleConnectionDTO]), connection.id)).toBeNull()
    expect(syncedNotice(status([]), connection.id)).toBeNull()
  })
})
