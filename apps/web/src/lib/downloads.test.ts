import { describe, expect, test } from 'bun:test'
import { DownloadsError, appStoreURL, archLabel, getDownloads, macDetails, parseDownloads, sizeLabel, systemLabel } from './downloads'

const sha = 'a'.repeat(64)
const manifest = {
  version: '0.1.0',
  mac: {
    arch: 'arm64', minimumSystem: '14.2',
    installer: { file: 'Cereal-0.1.0-mac-arm64.dmg', url: '/api/v1/downloads/files/Cereal-0.1.0-mac-arm64.dmg', size: 41_234_567, sha256: sha },
    update: { file: 'Cereal-0.1.0-mac-arm64.zip', url: '/api/v1/downloads/files/Cereal-0.1.0-mac-arm64.zip', size: 40_123_456, sha256: sha },
  },
  iosAppStoreUrl: 'https://apps.apple.com/app/id123',
}
const transport = (handler: () => Promise<Response> | Response): typeof fetch => Object.assign(async () => handler(), { preconnect: () => {} }) as typeof fetch

describe('the download page', () => {
  test('sizes are written as a Mac shows them', () => {
    expect([41_234_567, 40_000_000, 960_000, 1_000_000, 123_456_789, 999_400_000, 1_250_000_000, 12_300_000_000, 512_000, 900].map(sizeLabel)).toEqual(['41.2 MB', '40 MB', '1 MB', '1 MB', '123 MB', '999 MB', '1.3 GB', '12 GB', '512 KB', '1 KB'])
    expect([0, -5, Number.NaN, Number.POSITIVE_INFINITY].map(sizeLabel)).toEqual(['', '', '', ''])
  })
  test('the line under the button: version, size, processor, system', () => {
    expect(macDetails('0.1.0', parseDownloads(manifest)!.mac!)).toEqual(['Version 0.1.0', '41.2 MB', 'Apple silicon', 'macOS 14.2 or later'])
    expect([archLabel('arm64'), archLabel('x64'), archLabel('universal'), archLabel('riscv')]).toEqual(['Apple silicon', 'Intel', 'Apple silicon and Intel', 'riscv'])
    expect(systemLabel('15')).toBe('macOS 15 or later')
  })
  test('the manifest is read strictly: only this server\'s own files and the App Store', () => {
    expect(parseDownloads(manifest)).toEqual({ version: '0.1.0', mac: { arch: 'arm64', minimumSystem: '14.2', installer: { ...manifest.mac.installer } }, iosAppStoreUrl: 'https://apps.apple.com/app/id123' })
    expect(parseDownloads({ version: '0.1.0', mac: null, iosAppStoreUrl: null })).toEqual({ version: '0.1.0', mac: null, iosAppStoreUrl: null })
    const installer = (fields: object) => ({ ...manifest, mac: { ...manifest.mac, installer: { ...manifest.mac.installer, ...fields } } })
    for (const bad of [
      installer({ url: 'https://evil.example/Cereal.dmg' }), installer({ url: '//evil.example/Cereal-0.1.0-mac-arm64.dmg' }), installer({ url: 'javascript:alert(1)' }),
      installer({ url: '/api/v1/downloads/files/other.dmg' }), installer({ file: '../secret', url: '/api/v1/downloads/files/../secret' }), installer({ size: 0 }), installer({ size: '41' }),
      { ...manifest, version: '' }, { ...manifest, version: '<b>' }, { ...manifest, mac: { ...manifest.mac, minimumSystem: 'latest' } }, { ...manifest, mac: 'yes' }, null, 'text', [],
    ]) expect(parseDownloads(bad)).toBeNull()
    // An address that is not the App Store is left out; the Mac build is still offered.
    expect(parseDownloads({ ...manifest, iosAppStoreUrl: 'https://evil.example/app' })?.iosAppStoreUrl).toBeNull()
    expect([appStoreURL('http://apps.apple.com/app/id1'), appStoreURL('https://apps.apple.com.evil.example/app'), appStoreURL('javascript:alert(1)'), appStoreURL(undefined)]).toEqual([null, null, null, null])
  })
  test('a failed or malformed answer is one error, with nothing of the server in it', async () => {
    expect(await getDownloads(undefined, transport(() => Response.json(manifest)), 'https://notes.example')).toMatchObject({ version: '0.1.0' })
    for (const answer of [() => new Response('no', { status: 500 }), () => Response.json({ version: 5 }), () => new Response('<html>'), () => { throw new TypeError('offline') }]) {
      await expect(getDownloads(undefined, transport(answer), 'https://notes.example')).rejects.toBeInstanceOf(DownloadsError)
    }
  })
})
