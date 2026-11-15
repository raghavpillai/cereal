/**
 * A stand-in for an S3-compatible store, over real HTTP, for tests and local runs without a cloud account.
 *
 * It keeps objects in memory and speaks the five operations the server uses (PUT, GET with a byte range,
 * HEAD, DELETE, ListObjectsV2), path-style. Every request's Signature Version 4 is checked with a verifier
 * written from the specification, independently of the server's signer: it rebuilds the canonical request
 * from what actually arrived (the headers the Authorization header lists, in the order it lists them).
 * The same verifier accepts Bun's built-in S3 client, a second, unrelated signer (`storage.test.ts`).
 *
 *   bun apps/api/test/fake-s3.ts --port 9100 --bucket cereal-test --access-key <id> --secret <secret>
 */
import { createHash, createHmac } from "node:crypto"

export interface FakeS3Object { bytes: Uint8Array; contentType: string; metadata: Record<string, string>; etag: string; modified: Date }
export interface FakeS3 {
  readonly port: number
  readonly endpoint: string
  /** Bucket name → key → object. */
  readonly buckets: Map<string, Map<string, FakeS3Object>>
  /** Every request as it arrived: method, path with query, and whether it carried a Range. */
  readonly requests: Array<{ method: string; path: string; range: string | null }>
  /** The next request whose method matches is answered with this status and error code instead. */
  failNext(method: string, status: number, code: string): void
  stop(): void
}
const hash = (data: string | Uint8Array) => createHash("sha256").update(data).digest("hex")
const hmac = (key: string | Uint8Array, data: string) => createHmac("sha256", key).update(data).digest()
const xml = (status: number, code: string, message = code) => new Response(`<?xml version="1.0" encoding="UTF-8"?><Error><Code>${code}</Code><Message>${message}</Message></Error>`, { status, headers: { "Content-Type": "application/xml" } })
const escape = (text: string) => text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;")

/** Signature Version 4, checked from the request as received. Returns an S3 error code, or null when it is valid. */
export function verifySignature(request: Request, body: Uint8Array, credentials: { accessKeyId: string; secretAccessKey: string }): string | null {
  const authorization = /^AWS4-HMAC-SHA256 Credential=([^/]+)\/(\d{8})\/([^/]+)\/s3\/aws4_request,\s*SignedHeaders=([^,]+),\s*Signature=([0-9a-f]{64})$/.exec(request.headers.get("authorization") ?? "")
  if (!authorization) return "AccessDenied"
  const [, accessKeyId, day, region, signedHeaders, signature] = authorization as unknown as [string, string, string, string, string, string]
  if (accessKeyId !== credentials.accessKeyId) return "InvalidAccessKeyId"
  const url = new URL(request.url), stamp = request.headers.get("x-amz-date") ?? ""
  if (!stamp.startsWith(day) || Math.abs(Date.parse(stamp.replace(/^(\d{4})(\d{2})(\d{2})T(\d{2})(\d{2})(\d{2})Z$/, "$1-$2-$3T$4:$5:$6Z")) - Date.now()) > 15 * 60_000) return "RequestTimeTooSkewed"
  const names = signedHeaders.split(";")
  if (!names.includes("host") || [...request.headers.keys()].some(name => name.startsWith("x-amz-") && !names.includes(name))) return "AccessDenied"
  const payload = request.headers.get("x-amz-content-sha256") ?? ""
  if (payload !== "UNSIGNED-PAYLOAD" && payload !== hash(body)) return "XAmzContentSHA256Mismatch"
  // The query is canonical when each name and value is percent-encoded and the pairs are sorted by name.
  const encode = (text: string) => encodeURIComponent(text).replace(/[!'()*]/g, character => `%${character.charCodeAt(0).toString(16).toUpperCase()}`)
  const query = [...url.searchParams].map(([name, value]) => `${encode(name)}=${encode(value)}`).sort().join("&")
  const canonical = [request.method, url.pathname, query, ...names.map(name => `${name}:${(request.headers.get(name) ?? "").trim().replace(/\s+/g, " ")}`), "", signedHeaders, payload].join("\n")
  const key = hmac(hmac(hmac(hmac(`AWS4${credentials.secretAccessKey}`, day), region), "s3"), "aws4_request")
  const expected = createHmac("sha256", key).update(["AWS4-HMAC-SHA256", stamp, `${day}/${region}/s3/aws4_request`, hash(canonical)].join("\n")).digest("hex")
  return expected === signature ? null : "SignatureDoesNotMatch"
}

export function startFakeS3(options: { port?: number; buckets: ReadonlyArray<string>; accessKeyId: string; secretAccessKey: string; pageSize?: number }): FakeS3 {
  const buckets = new Map(options.buckets.map(name => [name, new Map<string, FakeS3Object>()]))
  const requests: FakeS3["requests"] = [], failures: Array<{ method: string; status: number; code: string }> = []
  const server = Bun.serve({ port: options.port ?? 0, hostname: "127.0.0.1", maxRequestBodySize: 1024 * 1024 * 1024, async fetch(request) {
    const url = new URL(request.url), body = new Uint8Array(await request.arrayBuffer())
    requests.push({ method: request.method, path: url.pathname + url.search, range: request.headers.get("range") })
    const failure = failures.findIndex(entry => entry.method === request.method)
    if (failure >= 0) { const [entry] = failures.splice(failure, 1); return xml(entry!.status, entry!.code) }
    const refused = verifySignature(request, body, options)
    if (refused) return xml(403, refused)
    const [, bucketName = "", ...rest] = url.pathname.split("/"), key = rest.map(decodeURIComponent).join("/")
    const bucket = buckets.get(bucketName)
    if (!bucket) return xml(404, "NoSuchBucket", "The specified bucket does not exist")
    if (!key) {
      if (request.method !== "GET" || url.searchParams.get("list-type") !== "2") return xml(400, "InvalidRequest")
      const prefix = url.searchParams.get("prefix") ?? "", after = url.searchParams.get("continuation-token") ?? "", size = options.pageSize ?? 1000
      const keys = [...bucket.keys()].filter(name => name.startsWith(prefix) && name > after).sort(), page = keys.slice(0, size)
      return new Response(`<?xml version="1.0" encoding="UTF-8"?><ListBucketResult><Name>${bucketName}</Name><Prefix>${escape(prefix)}</Prefix><KeyCount>${page.length}</KeyCount><IsTruncated>${keys.length > size}</IsTruncated>${keys.length > size ? `<NextContinuationToken>${escape(page.at(-1)!)}</NextContinuationToken>` : ""}${page.map(name => `<Contents><Key>${escape(name)}</Key><Size>${bucket.get(name)!.bytes.byteLength}</Size></Contents>`).join("")}</ListBucketResult>`, { headers: { "Content-Type": "application/xml" } })
    }
    if (request.method === "PUT") {
      const length = request.headers.get("content-length")
      if (length === null) return xml(411, "MissingContentLength")
      const metadata: Record<string, string> = {}
      for (const [name, value] of request.headers) if (name.startsWith("x-amz-meta-")) metadata[name.slice("x-amz-meta-".length)] = value
      const etag = createHash("md5").update(body).digest("hex")
      bucket.set(key, { bytes: body, contentType: request.headers.get("content-type") ?? "application/octet-stream", metadata, etag, modified: new Date() })
      return new Response(null, { headers: { ETag: `"${etag}"` } })
    }
    if (request.method === "DELETE") { bucket.delete(key); return new Response(null, { status: 204 }) }
    if (request.method !== "GET" && request.method !== "HEAD") return xml(405, "MethodNotAllowed")
    const object = bucket.get(key)
    if (!object) return request.method === "HEAD" ? new Response(null, { status: 404 }) : xml(404, "NoSuchKey", "The specified key does not exist.")
    const headers: Record<string, string> = { "Content-Type": object.contentType, ETag: `"${object.etag}"`, "Last-Modified": object.modified.toUTCString(), "Accept-Ranges": "bytes", ...Object.fromEntries(Object.entries(object.metadata).map(([name, value]) => [`x-amz-meta-${name}`, value])) }
    const range = /^bytes=(\d+)-(\d*)$/.exec(request.headers.get("range") ?? ""), size = object.bytes.byteLength
    if (range) {
      const start = Number(range[1]), end = range[2] === "" ? size - 1 : Math.min(Number(range[2]), size - 1)
      if (start >= size) return xml(416, "InvalidRange")
      return new Response(request.method === "HEAD" ? null : object.bytes.slice(start, end + 1), { status: 206, headers: { ...headers, "Content-Range": `bytes ${start}-${end}/${size}`, "Content-Length": String(end - start + 1) } })
    }
    return new Response(request.method === "HEAD" ? null : object.bytes.slice(), { headers: { ...headers, "Content-Length": String(size) } })
  } })
  return { port: server.port!, endpoint: `http://127.0.0.1:${server.port}`, buckets, requests, failNext(method, status, code) { failures.push({ method, status, code }) }, stop() { void server.stop(true) } }
}

if (import.meta.main) {
  const args = process.argv.slice(2), flag = (name: string) => { const index = args.indexOf(`--${name}`); return index >= 0 ? args[index + 1] : undefined }
  const store = startFakeS3({ port: Number(flag("port") ?? 9100), buckets: (flag("bucket") ?? "cereal-test").split(","), accessKeyId: flag("access-key") ?? process.env.FAKE_S3_ACCESS_KEY ?? "", secretAccessKey: flag("secret") ?? process.env.FAKE_S3_SECRET ?? "" })
  console.log(`Stand-in S3 store listening at ${store.endpoint} (buckets: ${[...store.buckets.keys()].join(", ")})`)
  // A line a second process can read to see what is in the store, without any secret.
  process.on("SIGUSR2", () => { for (const [name, bucket] of store.buckets) console.log(`${name}: ${[...bucket].map(([key, object]) => `${key} (${object.bytes.byteLength} bytes${object.metadata.sha256 ? `, sha256 ${object.metadata.sha256}` : ""})`).join("; ") || "empty"}`) })
}
