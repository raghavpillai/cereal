/** What this server offers for download: the apps for the version it is itself running.
 * Pure helpers and one public request; no session is involved. */

export interface DownloadFile { file: string; url: string; size: number; sha256: string }
export interface MacDownload { arch: string; minimumSystem: string; installer: DownloadFile }
export interface Downloads { version: string; mac: MacDownload | null; iosAppStoreUrl: string | null }

const FILES = '/api/v1/downloads/files/'

/** A file this server serves itself: a plain path under its downloads, never another host or a script address. */
function downloadFile(value: unknown): DownloadFile | null {
  if (typeof value !== 'object' || value === null) return null
  const { file, url, size, sha256 } = value as Record<string, unknown>
  if (typeof file !== 'string' || !/^[A-Za-z0-9][A-Za-z0-9._-]{0,199}$/.test(file)) return null
  if (url !== `${FILES}${file}`) return null
  if (typeof size !== 'number' || !Number.isSafeInteger(size) || size <= 0) return null
  return { file, url, size, sha256: typeof sha256 === 'string' && /^[0-9a-f]{64}$/i.test(sha256) ? sha256.toLowerCase() : '' }
}

/** The App Store, and nowhere else. */
export function appStoreURL(value: unknown): string | null {
  if (typeof value !== 'string') return null
  try {
    const url = new URL(value)
    return url.protocol === 'https:' && (url.hostname === 'apps.apple.com' || url.hostname === 'itunes.apple.com') && !url.username && !url.password ? url.href : null
  } catch { return null }
}

/** The manifest as the page uses it, or null when the answer is not one. */
export function parseDownloads(value: unknown): Downloads | null {
  if (typeof value !== 'object' || value === null) return null
  const { version, mac, iosAppStoreUrl } = value as Record<string, unknown>
  if (typeof version !== 'string' || !/^[0-9A-Za-z][0-9A-Za-z.+-]{0,39}$/.test(version)) return null
  let build: MacDownload | null = null
  if (mac !== null && mac !== undefined) {
    if (typeof mac !== 'object') return null
    const { arch, minimumSystem, installer } = mac as Record<string, unknown>, file = downloadFile(installer)
    if (typeof arch !== 'string' || typeof minimumSystem !== 'string' || !/^\d{1,3}(\.\d{1,3}){0,2}$/.test(minimumSystem) || !file) return null
    build = { arch, minimumSystem, installer: file }
  }
  return { version, mac: build, iosAppStoreUrl: appStoreURL(iosAppStoreUrl) }
}

/** A file's size as people read it: "41.2 MB", in the decimal megabytes a Mac shows. */
export function sizeLabel(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return ''
  const megabytes = bytes / 1_000_000
  if (megabytes >= 1000) return `${(megabytes / 1000).toFixed(megabytes >= 10_000 ? 0 : 1).replace(/\.0$/, '')} GB`
  if (megabytes >= 100) return `${Math.round(megabytes)} MB`
  if (megabytes >= 0.95) return `${megabytes.toFixed(1).replace(/\.0$/, '')} MB`
  return `${Math.max(1, Math.round(bytes / 1000))} KB`
}
export function archLabel(arch: string): string {
  return arch === 'arm64' ? 'Apple silicon' : arch === 'x64' || arch === 'x86_64' ? 'Intel' : arch === 'universal' ? 'Apple silicon and Intel' : arch
}
export const systemLabel = (minimumSystem: string) => `macOS ${minimumSystem} or later`
/** The line under the Mac button: version, size, processor, system. */
export function macDetails(version: string, mac: MacDownload): string[] {
  return [`Version ${version}`, sizeLabel(mac.installer.size), archLabel(mac.arch), systemLabel(mac.minimumSystem)].filter(Boolean)
}

export class DownloadsError extends Error {
  constructor() { super('The downloads could not be loaded') }
}
export async function getDownloads(signal?: AbortSignal, transport: typeof fetch = globalThis.fetch, origin: string = window.location.origin): Promise<Downloads> {
  let response: Response
  try { response = await transport(`${new URL(origin).origin}/api/v1/downloads`, { credentials: 'omit', cache: 'no-store', redirect: 'error', referrerPolicy: 'no-referrer', headers: { accept: 'application/json' }, signal }) }
  catch (error) { if (signal?.aborted) throw error; throw new DownloadsError() }
  const downloads = response.ok ? parseDownloads(await response.json().catch(() => null)) : null
  if (!downloads) throw new DownloadsError()
  return downloads
}
