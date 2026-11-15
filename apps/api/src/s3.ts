/**
 * A small client for S3-compatible object storage (AWS S3, Cloudflare R2, MinIO, Backblaze B2), and the
 * rules for the settings that point at one.
 *
 * It signs plain `fetch` requests with AWS Signature Version 4 and uses five operations: PUT, GET (with
 * a byte range), HEAD, DELETE and ListObjectsV2. Bun's built-in client is not used because it cannot
 * write or read object metadata, and a published app file carries its SHA-256 there (`downloads.ts`).
 * A PUT is one request whose body is sent as it is read: bytes already in memory, or a file on disk
 * that is streamed (up to S3's 5 GiB single-request limit), never copied into memory first.
 */
import { createHash, createHmac } from "node:crypto"
import { isIP } from "node:net"

export interface S3Config {
  /** Null: Amazon S3 itself, at the region's address. */
  readonly endpoint: string | null
  readonly region: string
  readonly bucket: string
  /** Empty, or ends with "/". Every key this server writes starts with it. */
  readonly prefix: string
  /** Bucket in the path (`host/bucket/key`), as MinIO and most self-hosted stores need. */
  readonly forcePathStyle: boolean
  readonly accessKeyId: string
  readonly secretAccessKey: string
}
export class S3SettingsError extends Error {}
/** A request the store refused or that never reached it. `code` is the store's own error code when it sent one. */
export class S3Failure extends Error {
  constructor(readonly operation: string, readonly status: number | null, readonly code: string | null) { super(`S3 ${operation} failed${status ? ` (${status}${code ? ` ${code}` : ""})` : ""}`) }
}

const LOOPBACK = new Set(["localhost", "127.0.0.1", "[::1]"])
/** Addresses a server must never be pointed at: link-local (cloud metadata services live there), the
 * unspecified address, and the IPv6 forms that reach the same places. Private networks are allowed:
 * a self-hosted store on the same network is an ordinary deployment. */
export function forbiddenAddress(host: string): boolean {
  const bare = host.replace(/^\[|\]$/g, "").toLowerCase()
  if (isIP(bare) === 4) { const [a, b] = bare.split(".").map(Number); return a === 0 || (a === 169 && b === 254) }
  if (isIP(bare) === 6) {
    const mapped = /^(?:::ffff:|64:ff9b::)(\d+\.\d+\.\d+\.\d+)$/.exec(bare)?.[1]
    if (mapped) return forbiddenAddress(mapped)
    // ::ffff:a9fe:a9fe is 169.254.169.254 written in hexadecimal.
    const hex = /^::ffff:([0-9a-f]{1,4}):([0-9a-f]{1,4})$/.exec(bare)
    if (hex) return forbiddenAddress(`${parseInt(hex[1]!, 16) >> 8}.${parseInt(hex[1]!, 16) & 255}.${parseInt(hex[2]!, 16) >> 8}.${parseInt(hex[2]!, 16) & 255}`)
    return bare === "::" || /^fe[89ab]/.test(bare) || bare.startsWith("fd00:ec2:")
  }
  return /^(?:metadata\.google\.internal|metadata\.azure\.com|instance-data(?:\.|$)|metadata$)/.test(bare)
}
/** The endpoint as it is stored: an origin, with a path only if the store has one. HTTPS, except for a
 * store on this machine (localhost, 127.0.0.1, [::1]), which is how a local MinIO is reached. */
export function normalizeS3Endpoint(raw: string): string {
  let url: URL
  try { url = new URL(raw.trim()) } catch { throw new S3SettingsError("Enter the endpoint as a full address, such as https://s3.example.com") }
  if (url.username || url.password) throw new S3SettingsError("The endpoint must not contain a user name or password")
  if (url.search || url.hash) throw new S3SettingsError("The endpoint must not contain a query or fragment")
  if (forbiddenAddress(url.hostname)) throw new S3SettingsError("That endpoint address is not allowed")
  if (url.protocol !== "https:" && !(url.protocol === "http:" && LOOPBACK.has(url.hostname))) throw new S3SettingsError("The endpoint must use https (http is accepted only for a store on this machine)")
  return url.origin + url.pathname.replace(/\/+$/, "")
}
/** S3's own bucket naming rules: 3 to 63 lower-case letters, digits, hyphens and dots; not shaped like an IP address. */
export function validBucketName(name: string): boolean {
  return /^[a-z0-9](?:[a-z0-9.-]{1,61})[a-z0-9]$/.test(name) && !name.includes("..") && !/^\d+\.\d+\.\d+\.\d+$/.test(name) && !/\.-|-\./.test(name)
}
/** "" or "some/folder/": no leading slash, exactly one trailing slash, no empty or dot segments. */
export function normalizeS3Prefix(raw: string): string {
  const prefix = raw.trim().replace(/^\/+/, "").replace(/\/+$/, "")
  if (!prefix) return ""
  if (prefix.length > 200 || !/^[A-Za-z0-9!_.*'()\/-]+$/.test(prefix) || prefix.split("/").some(part => part === "" || part === "." || part === "..")) throw new S3SettingsError("The prefix may contain letters, digits, and - _ . / only, without empty or dot segments")
  return prefix + "/"
}
export const validS3Region = (region: string) => /^[a-z0-9-]{1,40}$/.test(region)
export const validAccessKeyId = (id: string) => /^[A-Za-z0-9_\-./+=]{3,128}$/.test(id)
export const validSecretAccessKey = (secret: string) => secret.length >= 8 && secret.length <= 512 && !/\s/.test(secret)

const hash = (data: string | Uint8Array) => createHash("sha256").update(data).digest("hex")
const hmac = (key: string | Uint8Array, data: string) => createHmac("sha256", key).update(data).digest()
/** RFC 3986 encoding, as Signature Version 4 requires: everything but letters, digits and - _ . ~ */
const encode = (text: string) => encodeURIComponent(text).replace(/[!'()*]/g, character => `%${character.charCodeAt(0).toString(16).toUpperCase()}`)

/** The address of `key` in the bucket and the Signature Version 4 headers for one request. Exported for the tests'
 * stand-in store, which checks a request's signature with the same function. */
export function signS3Request(config: S3Config, input: { method: string; key: string; query?: Record<string, string>; headers?: Record<string, string>; payloadHash?: string; now?: Date }): { url: string; headers: Record<string, string> } {
  const base = new URL(config.endpoint ?? `https://s3.${config.region}.amazonaws.com`)
  const virtual = !config.forcePathStyle
  const host = virtual ? `${config.bucket}.${base.host}` : base.host
  const path = `${base.pathname.replace(/\/+$/, "")}${virtual ? "" : `/${config.bucket}`}/${input.key.split("/").map(encode).join("/")}`.replace(/\/$/, input.key ? "/" : "") || "/"
  const query = Object.entries(input.query ?? {}).map(([name, value]) => [encode(name), encode(value)] as const).sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0).map(([name, value]) => `${name}=${value}`).join("&")
  const stamp = (input.now ?? new Date()).toISOString().replace(/[-:]|\.\d{3}/g, ""), day = stamp.slice(0, 8), payloadHash = input.payloadHash ?? "UNSIGNED-PAYLOAD"
  // Signed: the host, the two x-amz headers every request carries, and any x-amz-meta-* a write adds.
  const signed: Record<string, string> = { host, "x-amz-content-sha256": payloadHash, "x-amz-date": stamp }
  for (const [name, value] of Object.entries(input.headers ?? {})) if (name.toLowerCase().startsWith("x-amz-")) signed[name.toLowerCase()] = value.trim()
  const names = Object.keys(signed).sort()
  const canonical = [input.method, path, query, ...names.map(name => `${name}:${signed[name]}`), "", names.join(";"), payloadHash].join("\n")
  const scope = `${day}/${config.region}/s3/aws4_request`
  const key = hmac(hmac(hmac(hmac(`AWS4${config.secretAccessKey}`, day), config.region), "s3"), "aws4_request")
  const signature = createHmac("sha256", key).update(["AWS4-HMAC-SHA256", stamp, scope, hash(canonical)].join("\n")).digest("hex")
  const { host: _host, ...sent } = signed
  return {
    url: `${base.protocol}//${host}${path}${query ? `?${query}` : ""}`,
    headers: { ...input.headers, ...sent, Authorization: `AWS4-HMAC-SHA256 Credential=${config.accessKeyId}/${scope}, SignedHeaders=${names.join(";")}, Signature=${signature}` },
  }
}

export interface S3ObjectInfo { readonly size: number; readonly etag: string | null; readonly modifiedMs: number | null; readonly sha256: string | null }
export interface S3Client {
  readonly config: S3Config
  /** Null when there is no such object. */
  head(key: string): Promise<S3ObjectInfo | null>
  /** The object's bytes as a streamed response (200, or 206 for a range); null when there is no such object. */
  get(key: string, range?: { start: number; end: number }): Promise<Response | null>
  /** `sha256` is kept as the object's `sha256` metadata. A file on disk (a Blob) is streamed, not read into memory. */
  put(key: string, body: Uint8Array | Blob, options?: { sha256?: string; contentType?: string }): Promise<void>
  /** Succeeds whether or not the object existed. */
  delete(key: string): Promise<void>
  /** Every key under `prefix`, in pages of a thousand. */
  list(prefix: string): Promise<string[]>
}
const xmlText = (xml: string, tag: string) => new RegExp(`<${tag}>([^<]*)</${tag}>`).exec(xml)?.[1]?.replace(/&amp;/g, "&").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&quot;/g, "\"").replace(/&apos;/g, "'") ?? null

/** Keys given to the client are relative to the configured prefix. */
export function makeS3Client(config: S3Config, transport: typeof fetch = globalThis.fetch): S3Client {
  const full = (key: string) => config.prefix + key
  async function send(operation: string, input: Parameters<typeof signS3Request>[1], init: { body?: BodyInit; timeoutMs: number; accept?: ReadonlyArray<number> }): Promise<Response> {
    const { url, headers } = signS3Request(config, input)
    let response: Response
    try { response = await transport(url, { method: input.method, headers, ...(init.body === undefined ? {} : { body: init.body }), redirect: "error", signal: AbortSignal.timeout(init.timeoutMs) }) }
    catch { throw new S3Failure(operation, null, null) }
    if (response.ok || init.accept?.includes(response.status)) return response
    const text = input.method === "HEAD" ? "" : await response.text().catch(() => "")
    throw new S3Failure(operation, response.status, xmlText(text.slice(0, 4000), "Code") ?? response.headers.get("x-amz-error-code"))
  }
  const info = (headers: Headers): S3ObjectInfo => {
    const sha256 = headers.get("x-amz-meta-sha256")?.trim().toLowerCase() ?? null, modified = Date.parse(headers.get("last-modified") ?? "")
    return { size: Number(headers.get("content-length") ?? "0"), etag: headers.get("etag")?.replace(/^W\//, "").replace(/^"|"$/g, "") || null, modifiedMs: Number.isFinite(modified) ? modified : null, sha256: sha256 && /^[0-9a-f]{64}$/.test(sha256) ? sha256 : null }
  }
  return {
    config,
    async head(key) {
      const response = await send("head", { method: "HEAD", key: full(key) }, { timeoutMs: 30_000, accept: [404] })
      return response.status === 404 ? null : info(response.headers)
    },
    async get(key, range) {
      const response = await send("get", { method: "GET", key: full(key), ...(range ? { headers: { Range: `bytes=${range.start}-${range.end}` } } : {}) }, { timeoutMs: 600_000, accept: [404] })
      if (response.status === 404) { await response.body?.cancel().catch(() => {}); return null }
      return response
    },
    async put(key, body, options = {}) {
      const bytes = body instanceof Uint8Array ? body : null
      const headers: Record<string, string> = { "Content-Type": options.contentType ?? "application/octet-stream", "Content-Length": String(bytes ? bytes.byteLength : (body as Blob).size), ...(options.sha256 ? { "x-amz-meta-sha256": options.sha256 } : {}) }
      // Bytes in memory are signed with their digest; a file is streamed, so its digest is not in the signature.
      await send("put", { method: "PUT", key: full(key), headers, ...(bytes ? { payloadHash: hash(bytes) } : {}) }, { body: (bytes ? new Uint8Array(bytes) : body) as BodyInit, timeoutMs: 1_800_000 })
    },
    async delete(key) { await send("delete", { method: "DELETE", key: full(key) }, { timeoutMs: 30_000, accept: [404] }) },
    async list(prefix) {
      const keys: string[] = []
      for (let token: string | null = null, pages = 0; pages < 1000; pages++) {
        const response = await send("list", { method: "GET", key: "", query: { "list-type": "2", prefix: full(prefix), ...(token ? { "continuation-token": token } : {}) } }, { timeoutMs: 30_000 })
        const xml = await response.text()
        for (const match of xml.matchAll(/<Key>([^<]*)<\/Key>/g)) { const key = xmlText(match[0], "Key")!; if (key.startsWith(config.prefix)) keys.push(key.slice(config.prefix.length)) }
        token = xmlText(xml, "IsTruncated") === "true" ? xmlText(xml, "NextContinuationToken") : null
        if (!token) break
      }
      return keys
    },
  }
}
