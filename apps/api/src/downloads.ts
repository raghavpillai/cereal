/**
 * The apps this server hands out, always for the version it is itself running
 * (GET /api/v1/downloads and GET /api/v1/downloads/files/<file>, both public).
 *
 * There is no manifest file. A release is published by putting two files, named for their version, where
 * the server keeps its files (`storage.ts`): the downloads directory (DOWNLOADS_DIR, /data/downloads in the
 * container), or `<prefix>downloads/` in the bucket when storage is S3-compatible. Both are looked in, the
 * selected one first, so a build on disk is offered under either setting. The bytes always pass through
 * this server: a build in a bucket is streamed, never handed out as a link to the bucket.
 *
 *   Cereal-<version>-mac-<arch>.dmg   the installer people download
 *   Cereal-<version>-mac-<arch>.zip   the archive the desktop app updates itself from
 *
 * The server looks only for the two names of its own version, so a build for another version in the same
 * place is neither listed nor served. Sizes are read from the files. A SHA-256 digest is the one recorded
 * with the file when it was published to a bucket (`scripts/publish-release.ts`), else it is computed by
 * reading the file in pieces, once, and remembered until the file changes.
 */
import { createHash } from "node:crypto"
import { constants } from "node:fs"
import { open } from "node:fs/promises"
import type { DownloadFileDTO, DownloadsDTO } from "@meeting-notes/contracts"
import { makeStorage, type StorageArea, type StoredFile } from "./storage"
import { APP_VERSION_PATTERN } from "./version"

/** The processor the Mac build is for. One build today. */
const MAC_ARCH = "arm64"
/** The oldest macOS the desktop app runs on (LSMinimumSystemVersion in its Info.plist). */
export const MAC_MINIMUM_SYSTEM = "14.2"
export const DOWNLOAD_FILES_PATH = "/api/v1/downloads/files/"
const CONTENT_TYPES: Record<string, string> = { dmg: "application/x-apple-diskimage", zip: "application/zip" }
const HASH_CHUNK = 1024 * 1024

export interface DownloadsOptions {
  /** DOWNLOADS_DIR: where published builds are on local disk. It may be missing or empty: then nothing is offered from it. */
  readonly directory: string
  /** The server's own version (`readAppVersion`). */
  readonly version: string
  /** The numeric App Store ID, once the iPhone app is published (set on the admin panel). A function is asked
   * for every answer, so a change applies at once. */
  readonly iosAppStoreId?: string | (() => Promise<string | null>) | undefined
  /** Where published builds are kept (`storage.ts`). Default: `directory` on local disk only. */
  readonly storage?: StorageArea
}
export interface DownloadsService {
  readonly manifest: () => Promise<DownloadsDTO>
  /** Answers a request for `/api/v1/downloads/files/<name>`; `name` is the path segment as sent. */
  readonly file: (request: Request, name: string) => Promise<Response>
}
/** The two file names of one version's Mac build. Shared with the script that publishes them. */
export const macBuildFiles = (version: string) => ({ installer: `Cereal-${version}-mac-${MAC_ARCH}.dmg`, update: `Cereal-${version}-mac-${MAC_ARCH}.zip` })

const jsonHeaders = { "Content-Type": "application/json", "Cache-Control": "no-store", "X-Content-Type-Options": "nosniff", "Referrer-Policy": "no-referrer" }
const notFound = () => new Response(JSON.stringify({ error: "not_found", message: "Download not found" }), { status: 404, headers: jsonHeaders })

/** One byte range of a file of `size` bytes, as `Range` asks for it. `null`: no usable range, send the
 * whole file (a missing or malformed header, another unit, several ranges). `"unsatisfiable"`: 416. */
export function parseRange(header: string | null, size: number): { start: number; end: number } | null | "unsatisfiable" {
  if (!header) return null
  const match = /^bytes=(\d*)-(\d*)$/.exec(header.trim())
  if (!match || (match[1] === "" && match[2] === "")) return null
  if (match[1]!.length > 15 || match[2]!.length > 15) return "unsatisfiable"
  if (match[1] === "") {
    // "-500": the last 500 bytes.
    const length = Number(match[2])
    if (length === 0 || size === 0) return "unsatisfiable"
    return { start: Math.max(0, size - length), end: size - 1 }
  }
  const start = Number(match[1]), end = match[2] === "" ? size - 1 : Math.min(Number(match[2]), size - 1)
  if (match[2] !== "" && Number(match[2]) < start) return null
  if (start >= size) return "unsatisfiable"
  return { start, end }
}

export function makeDownloads(options: DownloadsOptions): DownloadsService {
  if (!APP_VERSION_PATTERN.test(options.version)) throw new Error("Invalid product version")
  const storage = options.storage ?? makeStorage({ audioDir: options.directory, downloadsDir: options.directory }).downloads
  const { installer, update } = macBuildFiles(options.version)
  const appStoreId = async () => { const id = typeof options.iosAppStoreId === "function" ? await options.iosAppStoreId().catch(() => null) : options.iosAppStoreId; return /^\d{1,20}$/.test(id?.trim() ?? "") ? id!.trim() : null }
  /** One digest per file, kept until the file changes (its size and modification time on disk, its ETag in a bucket). A handful of entries at most. */
  const digests = new Map<string, { version: string; sha256: Promise<string> }>()

  /** The published file called `name`, if it is one of this version's two. On disk it must really be a file
   * inside the downloads directory: a link is followed, and refused when it leads out (`storage.ts`). */
  async function find(name: string): Promise<StoredFile | null> {
    if (name !== installer && name !== update) return null
    try { return await storage.stat(name) } catch { return null }
  }

  /** Reads the file a megabyte at a time, so a large installer never holds up other requests. On disk the file
   * must be the one that was measured, before and after: a file replaced meanwhile is not given a digest. */
  async function digest(name: string, found: StoredFile): Promise<string> {
    const hash = createHash("sha256")
    if (found.path === null) {
      // In a bucket, without a recorded digest: the object is streamed through the hash once.
      let read = 0
      for await (const chunk of new Response(await storage.body(name, found)).body!) { hash.update(chunk); read += chunk.byteLength }
      if (read !== found.size) throw new Error("Download changed while it was being read")
      return hash.digest("hex")
    }
    const handle = await open(found.path, constants.O_RDONLY | constants.O_NOFOLLOW)
    try {
      const before = await handle.stat()
      if (!before.isFile() || `${before.size}-${before.mtimeMs}` !== found.version) throw new Error("Download changed while it was being read")
      const buffer = Buffer.allocUnsafe(HASH_CHUNK)
      for (let position = 0; position < found.size;) {
        const { bytesRead } = await handle.read(buffer, 0, Math.min(HASH_CHUNK, found.size - position), position)
        if (bytesRead === 0) throw new Error("Download changed while it was being read")
        hash.update(bytesRead === HASH_CHUNK ? buffer : buffer.subarray(0, bytesRead))
        position += bytesRead
      }
      const after = await handle.stat()
      if (`${after.size}-${after.mtimeMs}` !== found.version) throw new Error("Download changed while it was being read")
      return hash.digest("hex")
    } finally { await handle.close().catch(() => {}) }
  }
  function sha256(name: string, found: StoredFile): Promise<string> {
    // Recorded when the build was published: nothing to compute.
    if (found.sha256) return Promise.resolve(found.sha256)
    const id = `${found.backend}:${name}`, known = digests.get(id)
    if (known && known.version === found.version) return known.sha256
    const entry = { version: found.version, sha256: digest(name, found) }
    digests.set(id, entry)
    // A digest that could not be computed is tried again by the next request, not remembered.
    entry.sha256.catch(() => { if (digests.get(id) === entry) digests.delete(id) })
    return entry.sha256
  }
  /** A file that changes under the reader is measured again; one that keeps changing is not offered. */
  async function describe(name: string): Promise<{ found: StoredFile; dto: DownloadFileDTO } | null> {
    for (let attempt = 0; attempt < 3; attempt++) {
      const found = await find(name)
      if (!found) return null
      try { return { found, dto: { file: name, url: DOWNLOAD_FILES_PATH + name, size: found.size, sha256: await sha256(name, found) } } }
      catch { /* replaced or still being written: look again */ }
    }
    return null
  }

  return {
    async manifest() {
      const [dmg, zip, storeId] = await Promise.all([describe(installer), describe(update), appStoreId()])
      return {
        version: options.version,
        mac: dmg ? { arch: MAC_ARCH, minimumSystem: MAC_MINIMUM_SYSTEM, installer: dmg.dto, update: zip?.dto ?? null } : null,
        iosAppStoreUrl: storeId ? `https://apps.apple.com/app/id${storeId}` : null,
      }
    },
    async file(request, raw) {
      if (request.method !== "GET" && request.method !== "HEAD")
        return new Response(JSON.stringify({ error: "method_not_allowed", message: "Method not allowed for this API route" }), { status: 405, headers: { ...jsonHeaders, Allow: "GET, HEAD" } })
      let name: string
      try { name = decodeURIComponent(raw) } catch { return notFound() }
      // Only the names advertised for this version exist. Everything else, including a real file for
      // another version and any path that climbs out of the directory, is the same 404.
      const described = await describe(name)
      if (!described) return notFound()
      const { found, dto } = described, etag = `"${dto.sha256}"`
      const headers: Record<string, string> = {
        "Content-Type": CONTENT_TYPES[name.slice(name.lastIndexOf(".") + 1)] ?? "application/octet-stream",
        "Content-Disposition": `attachment; filename="${name}"`,
        "Accept-Ranges": "bytes",
        "ETag": etag,
        ...(found.modifiedMs === null ? {} : { "Last-Modified": new Date(found.modifiedMs).toUTCString() }),
        "Cache-Control": "public, max-age=300",
        "X-Content-Type-Options": "nosniff",
        "Referrer-Policy": "no-referrer",
      }
      const sameEntity = (value: string) => value.split(",").map(tag => tag.trim().replace(/^W\//, "")).some(tag => tag === etag || tag === "*")
      const ifNoneMatch = request.headers.get("if-none-match")
      if (ifNoneMatch && sameEntity(ifNoneMatch)) return new Response(null, { status: 304, headers })
      // A range is honoured only for the file the client already has part of (If-Range), else the whole file is sent.
      const ifRange = request.headers.get("if-range")
      const range = ifRange && ifRange.trim() !== etag ? null : parseRange(request.headers.get("range"), found.size)
      if (range === "unsatisfiable")
        return new Response(null, { status: 416, headers: { ...headers, "Content-Range": `bytes */${found.size}`, "Content-Length": "0" } })
      const start = range?.start ?? 0, end = range?.end ?? found.size - 1, length = end - start + 1
      const sent = { ...headers, "Content-Length": String(length), ...(range ? { "Content-Range": `bytes ${start}-${end}/${found.size}` } : {}) }
      if (request.method === "HEAD") return new Response(null, { status: range ? 206 : 200, headers: sent })
      // The body is never held in memory. On disk it is the file by reference, which the runtime sends in
      // pieces; in a bucket it is the store's own response, passed on as it arrives.
      let body: BodyInit
      try { body = await storage.body(name, found, range ? { start, end } : undefined) }
      catch { return new Response(JSON.stringify({ error: "unavailable", message: "The download is temporarily unavailable" }), { status: 503, headers: jsonHeaders }) }
      // A file on disk goes out with its length. A stream from a bucket cannot: the runtime sends a streamed body
      // in chunks and drops a Content-Length set on it, so none is claimed (HEAD and the downloads answer give the size).
      if (found.path === null) delete (sent as Record<string, string>)["Content-Length"]
      return new Response(body, { status: range ? 206 : 200, headers: sent })
    },
  }
}
