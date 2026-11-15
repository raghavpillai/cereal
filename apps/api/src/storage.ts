/**
 * Where the server keeps its files: recorded audio and the app builds it hands out.
 *
 * Two backends behind one interface. Local disk is the default and is exactly what the server did before
 * this existed: audio parts at `<AUDIO_STORAGE_DIR>/<note>/<hash>.wav`, builds in `DOWNLOADS_DIR`. The
 * other is an S3-compatible bucket (`s3.ts`), where the same files are `<prefix>audio/<note>/<hash>.wav`
 * and `<prefix>downloads/<file>`. Which one is used is chosen on the admin panel and read for every
 * operation, so a change applies at once.
 *
 * Switching never hides a file. New files go to the selected backend. A read tries the selected backend
 * and then the other (local disk is always there; the bucket only while it is still configured), and a
 * delete removes the file wherever it is. Nothing is copied from one to the other.
 *
 * Keys are built only from identifiers the server made itself (a note's UUID, a part's SHA-256, a build's
 * file name for the server's own version): `audioKey` and the downloads code refuse anything else.
 */
import { createHash, randomBytes } from "node:crypto"
import { constants } from "node:fs"
import { link, mkdir, open, readFile, realpath, rm, stat, unlink } from "node:fs/promises"
import { dirname, join, resolve, sep } from "node:path"
import { makeS3Client, S3Failure, type S3Client, type S3Config } from "./s3"

export type StorageBackend = "local" | "s3"
/** What is configured now. `s3` is present whenever a complete bucket configuration exists, selected or not. */
export interface StorageConfiguration { readonly backend: StorageBackend; readonly s3: S3Config | null }
export interface StoredFile {
  readonly backend: StorageBackend
  readonly size: number
  /** Changes when the file's content does: the store's ETag, or size and modification time on disk. */
  readonly version: string
  readonly modifiedMs: number | null
  /** The digest recorded with the file when it was stored, if any. */
  readonly sha256: string | null
  /** The file itself, on local disk only: a real path inside the area's directory. */
  readonly path: string | null
}
export class StorageFailure extends Error {
  constructor(readonly backend: StorageBackend, readonly operation: string, readonly detail: string | null = null) { super(`Storage ${operation} failed on ${backend}`) }
}
/** One kind of file (audio, downloads) on whatever the storage settings say. */
export interface StorageArea {
  /** Writes to the selected backend. Writing the same bytes under the same key again is not an error. */
  put(key: string, data: Uint8Array | Blob, options?: { sha256?: string; contentType?: string }): Promise<StorageBackend>
  /** The selected backend first, then the other. Null when neither has it. */
  stat(key: string): Promise<StoredFile | null>
  /** The whole file. For audio parts, which are at most 20 MiB. */
  bytes(key: string): Promise<Uint8Array | null>
  /** A response body for `file` (as `stat` returned it), whole or one inclusive byte range; sent in pieces, never held in memory. */
  body(key: string, file: StoredFile, range?: { start: number; end: number }): Promise<BodyInit>
  /** Removes the file from every backend that has it. */
  delete(key: string): Promise<void>
  /** Removes everything under `prefix` (a note's audio) from every backend. */
  deletePrefix(prefix: string): Promise<void>
}
export interface StorageTestResult { readonly ok: boolean; readonly backend: StorageBackend; readonly step: "write" | "read" | "delete" | null; readonly message: string }
export interface StorageService {
  readonly audio: StorageArea
  readonly downloads: StorageArea
  /** The backend new files go to right now. */
  current(): Promise<StorageConfiguration>
  /** Writes a small file, reads it back and deletes it, on the given configuration, and says which step failed. */
  test(configuration: StorageConfiguration): Promise<StorageTestResult>
}

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i, SHA256 = /^[0-9a-f]{64}$/
/** The key of one stored audio part: the note's UUID and the SHA-256 of the audio, both made by the server. */
export function audioKey(noteId: string, hash: string): string {
  if (!UUID.test(noteId) || !SHA256.test(hash)) throw new StorageFailure("local", "key")
  return `${noteId.toLowerCase()}/${hash}.wav`
}
/** Everything stored for one note. */
export function audioPrefix(noteId: string): string {
  if (!UUID.test(noteId)) throw new StorageFailure("local", "key")
  return `${noteId.toLowerCase()}/`
}
/** A key is one or more plain segments. Nothing in it can climb out of its area. */
const safeKey = (key: string) => key.length > 0 && key.length <= 512 && key.split("/").every(part => /^[A-Za-z0-9][A-Za-z0-9._+-]*$/.test(part))

interface Backend {
  put(key: string, data: Uint8Array | Blob, options?: { sha256?: string; contentType?: string }): Promise<void>
  stat(key: string): Promise<StoredFile | null>
  bytes(key: string): Promise<Uint8Array | null>
  body(key: string, file: StoredFile, range?: { start: number; end: number }): Promise<BodyInit>
  delete(key: string): Promise<void>
  deletePrefix(prefix: string): Promise<void>
}

/** Files in one directory. A write is a private temporary file, synced, then linked into place: readers
 * never see half a file, and the same content arriving twice is recognised instead of rewritten. */
function localBackend(directory: string): Backend {
  const root = resolve(directory), target = (key: string) => join(root, ...key.split("/"))
  /** The file's real location, if it is a regular, non-empty file inside the directory. A link is followed
   * and refused when it leads out. */
  async function locate(key: string) {
    try {
      const base = await realpath(root), path = await realpath(target(key))
      if (!path.startsWith(base.endsWith(sep) ? base : base + sep)) return null
      const info = await stat(path)
      return info.isFile() && info.size > 0 ? { path, info } : null
    } catch { return null }
  }
  return {
    async put(key, data) {
      const bytes = data instanceof Uint8Array ? data : new Uint8Array(await data.arrayBuffer())
      const path = target(key), folder = dirname(path)
      await mkdir(folder, { recursive: true, mode: 0o700 })
      const temporary = join(folder, `.upload-${crypto.randomUUID()}`)
      const file = await open(temporary, constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | constants.O_NOFOLLOW, 0o600)
      try { await file.writeFile(bytes); await file.sync() } finally { await file.close() }
      try {
        try { await link(temporary, path) }
        catch (error) {
          if ((error as NodeJS.ErrnoException).code !== "EEXIST") throw error
          // Already there: it must be the same content, or the store is damaged.
          if (createHash("sha256").update(await readFile(path)).digest("hex") !== createHash("sha256").update(bytes).digest("hex")) throw new Error("Corrupt storage")
        }
        const handle = await open(folder, constants.O_RDONLY)
        try { await handle.sync() } finally { await handle.close() }
      } finally { await unlink(temporary).catch(() => {}) }
    },
    async stat(key) {
      const found = await locate(key)
      return found ? { backend: "local", size: found.info.size, version: `${found.info.size}-${found.info.mtimeMs}`, modifiedMs: found.info.mtimeMs, sha256: null, path: found.path } : null
    },
    async bytes(key) { const found = await locate(key); return found ? new Uint8Array(await readFile(found.path)) : null },
    // The file by reference: the runtime sends it from disk in pieces. The slice pins the length to the size that was measured.
    async body(_key, file, range) { return Bun.file(file.path!).slice(range?.start ?? 0, (range?.end ?? file.size - 1) + 1) },
    async delete(key) { await rm(target(key), { force: true }) },
    async deletePrefix(prefix) { await rm(target(prefix.replace(/\/+$/, "")), { recursive: true, force: true }) },
  }
}

function s3Backend(client: S3Client, area: string): Backend {
  const at = (key: string) => `${area}/${key}`
  const fail = (operation: string) => (error: unknown): never => { throw new StorageFailure("s3", operation, error instanceof S3Failure ? [error.status, error.code].filter(Boolean).join(" ") || "no answer" : null) }
  return {
    put: (key, data, options) => client.put(at(key), data, options).catch(fail("write")),
    async stat(key) {
      const info = await client.head(at(key)).catch(fail("read"))
      return info && info.size > 0 ? { backend: "s3", size: info.size, version: info.etag ?? `${info.size}-${info.modifiedMs}`, modifiedMs: info.modifiedMs, sha256: info.sha256, path: null } : null
    },
    async bytes(key) { const response = await client.get(at(key)).catch(fail("read")); return response ? new Uint8Array(await response.arrayBuffer().catch(fail("read"))) : null },
    async body(key, file, range) {
      const response = await client.get(at(key), range).catch(fail("read"))
      if (!response?.body) throw new StorageFailure("s3", "read", "the object is gone")
      // The bucket is read after the headers were decided: an object replaced in between is not passed off as the one described.
      const etag = response.headers.get("etag")?.replace(/^W\//, "").replace(/^"|"$/g, "")
      if (etag && file.version !== etag && !file.version.includes("-null")) { await response.body.cancel().catch(() => {}); throw new StorageFailure("s3", "read", "the object changed") }
      return response.body
    },
    delete: key => client.delete(at(key)).catch(fail("delete")),
    async deletePrefix(prefix) { for (const key of await client.list(at(prefix)).catch(fail("delete"))) await client.delete(key).catch(fail("delete")) },
  }
}

export function makeStorage(options: {
  /** AUDIO_STORAGE_DIR and DOWNLOADS_DIR: the local-disk homes of the two areas. */
  audioDir: string
  downloadsDir: string
  /** Read for every operation; absent means local disk only. */
  configuration?: () => Promise<StorageConfiguration>
  fetch?: typeof fetch
}): StorageService {
  const transport = options.fetch ?? globalThis.fetch
  const clients = new Map<string, S3Client>()
  /** One client per distinct configuration; a changed setting makes a new one. */
  const clientFor = (config: S3Config) => {
    const id = JSON.stringify([config.endpoint, config.region, config.bucket, config.prefix, config.forcePathStyle, config.accessKeyId, createHash("sha256").update(config.secretAccessKey).digest("hex")])
    let client = clients.get(id)
    if (!client) { if (clients.size >= 8) clients.clear(); client = makeS3Client(config, transport); clients.set(id, client) }
    return client
  }
  const current = async (): Promise<StorageConfiguration> => options.configuration ? await options.configuration() : { backend: "local", s3: null }
  function area(name: "audio" | "downloads", directory: string): StorageArea {
    const local = localBackend(directory)
    /** The selected backend, then the other one. */
    async function backends(configuration?: StorageConfiguration): Promise<Backend[]> {
      const { backend, s3 } = configuration ?? await current()
      const bucket = s3 ? s3Backend(clientFor(s3), name) : null
      return backend === "s3" && bucket ? [bucket, local] : bucket ? [local, bucket] : [local]
    }
    const checked = (key: string, operation: string) => { if (!safeKey(key.replace(/\/$/, ""))) throw new StorageFailure("local", operation, "invalid key") }
    return {
      async put(key, data, putOptions) {
        checked(key, "write")
        const configuration = await current(), [selected] = await backends(configuration)
        if (configuration.backend === "s3" && !configuration.s3) throw new StorageFailure("s3", "write", "not configured")
        await selected!.put(key, data, putOptions).catch(error => { throw error instanceof StorageFailure ? error : new StorageFailure(configuration.backend, "write") })
        return configuration.backend
      },
      async stat(key) {
        checked(key, "read")
        let failure: unknown
        for (const backend of await backends()) {
          // A bucket that cannot be reached must not hide a file that is on disk, and the other way round.
          try { const found = await backend.stat(key); if (found) return found } catch (error) { failure ??= error }
        }
        if (failure) throw failure
        return null
      },
      async bytes(key) {
        checked(key, "read")
        let failure: unknown
        for (const backend of await backends()) {
          try { const found = await backend.bytes(key); if (found) return found } catch (error) { failure ??= error }
        }
        if (failure) throw failure
        return null
      },
      async body(key, file, range) {
        checked(key, "read")
        const configuration = await current()
        const backend = file.backend === "local" ? local : configuration.s3 ? s3Backend(clientFor(configuration.s3), name) : null
        if (!backend) throw new StorageFailure("s3", "read", "not configured")
        return backend.body(key, file, range)
      },
      async delete(key) {
        checked(key, "delete")
        const results = await Promise.allSettled((await backends()).map(backend => backend.delete(key)))
        const failed = results.find(result => result.status === "rejected")
        if (failed) throw (failed as PromiseRejectedResult).reason
      },
      async deletePrefix(prefix) {
        checked(prefix, "delete")
        const results = await Promise.allSettled((await backends()).map(backend => backend.deletePrefix(prefix)))
        const failed = results.find(result => result.status === "rejected")
        if (failed) throw (failed as PromiseRejectedResult).reason
      },
    }
  }
  const REASONS: Record<string, string> = {
    NoSuchBucket: "the bucket does not exist", InvalidAccessKeyId: "the access key ID is not known to the store", SignatureDoesNotMatch: "the secret access key is wrong",
    AccessDenied: "the key is not allowed to do this in the bucket", InvalidBucketName: "the bucket name is not valid for this store", PermanentRedirect: "the bucket is in another region or at another address",
    AuthorizationHeaderMalformed: "the region does not match the bucket", "no answer": "the endpoint did not answer",
  }
  const explain = (error: unknown) => {
    const detail = error instanceof StorageFailure ? error.detail : null
    if (!detail) return "the store could not be used"
    const code = detail.split(" ").at(-1)!
    return `${REASONS[code] ?? REASONS[detail] ?? "the store refused the request"} (${detail})`
  }
  return {
    audio: area("audio", options.audioDir),
    downloads: area("downloads", options.downloadsDir),
    current,
    async test(configuration) {
      const backend = configuration.backend, where = backend === "s3" ? "the bucket" : "the audio directory"
      if (backend === "s3" && !configuration.s3) return { ok: false, backend, step: "write", message: "Enter the bucket, access key ID and secret access key first" }
      // Audio is what the server writes most: the test object goes where audio would.
      const target = backend === "s3" ? s3Backend(clientFor(configuration.s3!), "audio") : localBackend(options.audioDir)
      const key = `connection-test/${crypto.randomUUID()}.txt`, content = new Uint8Array(randomBytes(64))
      const failed = (step: StorageTestResult["step"], message: string): StorageTestResult => ({ ok: false, backend, step, message })
      try { await target.put(key, content, { contentType: "text/plain" }) }
      catch (error) { return failed("write", `Writing a test file to ${where} failed: ${explain(error)}`) }
      try {
        const read = await target.bytes(key)
        if (!read || Buffer.compare(Buffer.from(read), Buffer.from(content)) !== 0) { await target.delete(key).catch(() => {}); return failed("read", `The test file was written to ${where} but did not read back the same`) }
      } catch (error) { await target.delete(key).catch(() => {}); return failed("read", `The test file was written to ${where} but could not be read back: ${explain(error)}`) }
      try { await target.delete(key); if (backend === "local") await target.deletePrefix("connection-test") }
      catch (error) { return failed("delete", `The test file was written and read, but could not be deleted from ${where}: ${explain(error)}`) }
      return { ok: true, backend, step: null, message: backend === "s3" ? `Wrote, read and deleted a test file in ${configuration.s3!.bucket}` : "Wrote, read and deleted a test file on local disk" }
    },
  }
}
