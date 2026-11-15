import { expect, test } from 'bun:test'
import { googleAuthorizationURL } from './google-state'
import { createAdminClient } from './admin-client'

test('Google navigation only accepts the configured OAuth endpoint with PKCE and a same-instance callback', () => {
  const url = new URL('https://accounts.google.com/o/oauth2/v2/auth')
  for (const [k, v] of Object.entries({ redirect_uri: 'https://instance.example/api/integrations/google/callback', response_type: 'code', code_challenge_method: 'S256', state: 'a'.repeat(43), code_challenge: 'b'.repeat(43) })) url.searchParams.set(k, v)
  expect(googleAuthorizationURL(url.href, 'https://instance.example')).toBe(url.href)
  for (const [key, value] of [['redirect_uri', 'https://evil.example/callback'], ['response_type', 'token'], ['code_challenge_method', 'plain'], ['state', 'short']]) { const invalid = new URL(url); invalid.searchParams.set(key!, value!); expect(googleAuthorizationURL(invalid.href, 'https://instance.example')).toBeNull() }
  for (const value of ['javascript:alert(1)', url.href.replace('accounts.google.com', 'accounts.google.com.attacker.example')]) expect(googleAuthorizationURL(value, 'https://instance.example')).toBeNull()
})

test('personal Google connection DTOs never admit encrypted grant fields from a server response', async () => {
  const transport = Object.assign(async () => Response.json({ configured: true, redirectUri: 'https://instance.example/api/integrations/google/callback', connections: [{ id: crypto.randomUUID(), email: 'fixture@example.test', displayName: null, status: 'connected', contactsEnabled: false, lastSyncedAt: null, lastSyncError: null, createdAt: '2026-11-12T12:00:00Z', encryptedGrant: 'must-not-enter-browser-state', userId: crypto.randomUUID() }] }), { preconnect: () => {} }) as typeof fetch
  const status = await createAdminClient('https://instance.example', transport).getGoogleStatus()
  expect(status.connections).toHaveLength(1)
  expect(JSON.stringify(status)).not.toContain('encryptedGrant')
  expect(JSON.stringify(status)).not.toContain('userId')
})
