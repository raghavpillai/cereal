import { describe, expect, test } from 'bun:test'
import { appOrigin, isHandheld, noteAppURL, sharedAppURL, shouldAttemptOpen, type TabMemory } from './open-in-app'

const noteId = '01136768-b41d-4ee8-ad19-e73976f18475', token = 'AbCdEfGhIjKlMnOpQrStUvWxYz0123456789-_AbCdE'
const mac = { userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/19.0 Safari/605.1.15', platform: 'MacIntel', maxTouchPoints: 0 }
const windows = { userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36', platform: 'Win32', maxTouchPoints: 0 }
const iphone = { userAgent: 'Mozilla/5.0 (iPhone; CPU iPhone OS 19_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/19.0 Mobile/15E148 Safari/604.1', platform: 'iPhone', maxTouchPoints: 5 }
// An iPad that asks for desktop sites is a Mac in everything but its touch screen.
const ipad = { ...mac, maxTouchPoints: 5 }
const android = { userAgent: 'Mozilla/5.0 (Linux; Android 15; Pixel 9) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Mobile Safari/537.36', platform: 'Linux armv81', maxTouchPoints: 5 }

function memory(): TabMemory & { kept: Map<string, string> } {
  const kept = new Map<string, string>()
  return { kept, getItem: key => kept.get(key) ?? null, setItem: (key, value) => { kept.set(key, value) } }
}

describe('the address handed to the app', () => {
  test('a note carries its server and id, percent-encoded', () => {
    expect(noteAppURL('https://notes.example.com', noteId)).toBe(`cereal://note?server=https%3A%2F%2Fnotes.example.com&id=${noteId}`)
    expect(noteAppURL('https://notes.example.com:8443/', noteId)).toBe(`cereal://note?server=https%3A%2F%2Fnotes.example.com%3A8443&id=${noteId}`)
    // A page address, with its path and query, still names only the origin.
    expect(noteAppURL(`https://notes.example.com/notes/${noteId}?from=mail#top`, noteId.toUpperCase())).toBe(`cereal://note?server=https%3A%2F%2Fnotes.example.com&id=${noteId}`)
  })
  test('a server on this machine may be plain http; any other must be https', () => {
    expect(noteAppURL('http://127.0.0.1:3000', noteId)).toBe(`cereal://note?server=http%3A%2F%2F127.0.0.1%3A3000&id=${noteId}`)
    expect(appOrigin('http://localhost:3000/')).toBe('http://localhost:3000')
    expect(appOrigin('http://[::1]:3000')).toBe('http://[::1]:3000')
    expect(appOrigin('http://192.168.1.20:3000')).toBeNull()
    expect(appOrigin('http://notes.example.com')).toBeNull()
    expect(appOrigin('https://user:secret@notes.example.com')).toBeNull()
    expect(appOrigin('cereal://note')).toBeNull()
    expect(appOrigin('not an address')).toBeNull()
    expect(noteAppURL('http://notes.example.com', noteId)).toBeNull()
  })
  test('a shared link is passed whole, as one encoded value', () => {
    expect(sharedAppURL('https://notes.example.com', token)).toBe(`cereal://shared?url=https%3A%2F%2Fnotes.example.com%2Fs%2F${token}`)
    expect(sharedAppURL('http://127.0.0.1:3000', token)).toBe(`cereal://shared?url=http%3A%2F%2F127.0.0.1%3A3000%2Fs%2F${token}`)
    const url = new URL(sharedAppURL('https://notes.example.com:8443', token)!)
    expect(url.searchParams.get('url')).toBe(`https://notes.example.com:8443/s/${token}`)
  })
  test('nothing that is not a note id or a share token becomes part of an app address', () => {
    for (const bad of ['', 'not-a-uuid', `${noteId}&server=https://evil.example`, '../admin', `${noteId} `]) expect(noteAppURL('https://notes.example.com', bad)).toBeNull()
    for (const bad of ['', 'short', `${token}x`, `${token.slice(0, 42)}/`, `${token.slice(0, 42)}&`, `${token.slice(0, 40)}%2F`]) expect(sharedAppURL('https://notes.example.com', bad)).toBeNull()
  })
})

describe('trying the app without being asked', () => {
  const appURL = noteAppURL('https://notes.example.com', noteId)!, path = `/notes/${noteId}`
  test('phones and tablets are never sent to a custom scheme', () => {
    for (const device of [iphone, ipad, android]) {
      expect(isHandheld(device)).toBe(true)
      const tab = memory()
      expect(shouldAttemptOpen({ appURL, device, memory: tab, entryPath: path, currentPath: path })).toBe(false)
      // Nothing was tried, so nothing is remembered.
      expect(tab.kept.size).toBe(0)
    }
    expect(isHandheld(mac)).toBe(false)
    expect(isHandheld(windows)).toBe(false)
    expect(isHandheld({ userAgent: mac.userAgent })).toBe(false)
  })
  test('a desktop browser tries once per tab; back and reload do not try again', () => {
    const tab = memory()
    expect(shouldAttemptOpen({ appURL, device: mac, memory: tab, entryPath: path, currentPath: path })).toBe(true)
    expect(shouldAttemptOpen({ appURL, device: mac, memory: tab, entryPath: path, currentPath: path })).toBe(false)
    expect(shouldAttemptOpen({ appURL, device: windows, memory: tab, entryPath: path, currentPath: path })).toBe(false)
    // Another link in the same tab is its own first visit; another tab starts clean.
    const other = sharedAppURL('https://notes.example.com', token)!
    expect(shouldAttemptOpen({ appURL: other, device: mac, memory: tab, entryPath: `/s/${token}`, currentPath: `/s/${token}` })).toBe(true)
    expect(shouldAttemptOpen({ appURL, device: mac, memory: memory(), entryPath: path, currentPath: path })).toBe(true)
  })
  test('a note opened from the list was chosen on the web and stays there', () => {
    const tab = memory()
    expect(shouldAttemptOpen({ appURL, device: mac, memory: tab, entryPath: '/notes', currentPath: path })).toBe(false)
    expect(tab.kept.size).toBe(0)
  })
  test('with no address, or no working tab storage, the page does not try', () => {
    expect(shouldAttemptOpen({ appURL: null, device: mac, memory: memory(), entryPath: path, currentPath: path })).toBe(false)
    expect(shouldAttemptOpen({ appURL, device: mac, memory: null, entryPath: path, currentPath: path })).toBe(false)
    const refusing: TabMemory = { getItem: () => { throw new Error('blocked') }, setItem: () => { throw new Error('blocked') } }
    expect(shouldAttemptOpen({ appURL, device: mac, memory: refusing, entryPath: path, currentPath: path })).toBe(false)
    // Storage that accepts a write and forgets it (a full or disabled store) cannot guard either.
    const forgetting: TabMemory = { getItem: () => null, setItem: () => {} }
    expect(shouldAttemptOpen({ appURL, device: mac, memory: forgetting, entryPath: path, currentPath: path })).toBe(false)
  })
})
