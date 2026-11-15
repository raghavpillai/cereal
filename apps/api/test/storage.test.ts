/** Local disk or an S3-compatible bucket (`src/storage.ts`, `src/s3.ts`), against a stand-in store over real HTTP. */
import { afterAll, afterEach, beforeAll, expect, test } from "bun:test"
import { createHash, randomBytes } from "node:crypto"
import { mkdir, mkdtemp, readdir, readFile, realpath, rm, stat, symlink, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { Effect } from "effect"
import { makeApiHandler } from "../src/api"
import { makeAdministrationService } from "../src/administration"
import { secretBoxFromKey } from "../src/admin-secrets"
import { makeMemoryCalendarStore } from "../src/calendar-store"
import { makeDownloads } from "../src/downloads"
import { makeProcessingService } from "../src/processing"
import type { ProviderServices } from "../src/providers"
import { makeMemoryRepository } from "../src/repository"
import { forbiddenAddress, makeS3Client, normalizeS3Endpoint, normalizeS3Prefix, S3Failure, signS3Request, validBucketName, type S3Config } from "../src/s3"
import { audioKey, audioPrefix, makeStorage, StorageFailure, type StorageConfiguration } from "../src/storage"
import { startFakeS3, type FakeS3 } from "./fake-s3"

const token = "storage-test-owner-token-with-at-least-32-characters"
const sha256 = (bytes: Uint8Array) => createHash("sha256").update(bytes).digest("hex")
const ACCESS = "AKIAFAKEFAKEFAKE0001", SECRET = "fake/Secret+Access=Key-0123456789abcdef"
let scratch = "", fake: FakeS3
beforeAll(async () => { scratch = await realpath(await mkdtemp(join(tmpdir(), "cereal-storage-"))); fake = startFakeS3({ buckets: ["cereal-test", "second-bucket"], accessKeyId: ACCESS, secretAccessKey: SECRET, pageSize: 2 }) })
afterAll(async () => { fake.stop(); await rm(scratch, { recursive: true, force: true }) })
const cleanup: Array<() => Promise<unknown> | unknown> = []
afterEach(async () => { for (const dispose of cleanup.splice(0).reverse()) await dispose(); for (const bucket of fake.buckets.values()) bucket.clear(); fake.requests.length = 0 })
const bucketConfig = (overrides: Partial<S3Config> = {}): S3Config => ({ endpoint: fake.endpoint, region: "us-east-1", bucket: "cereal-test", prefix: "", forcePathStyle: true, accessKeyId: ACCESS, secretAccessKey: SECRET, ...overrides })
let counter = 0
const directory = async (name: string) => { const path = join(scratch, `${name}-${counter++}`); await mkdir(path); return path }
const failure = async (work: Promise<unknown>) => { try { await work } catch (error) { return error as any } throw new Error("expected a failure") }
const NOTE = "3f0a1b7e-6f0e-4a55-9f7c-0d2c7f1f6a10"

test("the client writes, reads (whole and a range), describes, lists and deletes; errors carry the store's own code", async () => {
  const client = makeS3Client(bucketConfig({ prefix: "team a/" })), bytes = randomBytes(5000)
  await client.put("audio/one.wav", bytes, { sha256: sha256(bytes), contentType: "audio/wav" })
  // The prefix is part of every key, and what arrived is what was sent.
  const stored = fake.buckets.get("cereal-test")!.get("team a/audio/one.wav")!
  expect(Buffer.from(stored.bytes).equals(bytes)).toBe(true)
  expect(stored.metadata).toEqual({ sha256: sha256(bytes) })
  expect(stored.contentType).toBe("audio/wav")
  expect(await client.head("audio/one.wav")).toMatchObject({ size: 5000, etag: stored.etag, sha256: sha256(bytes) })
  expect(await client.head("audio/missing.wav")).toBeNull()
  expect(await client.get("audio/missing.wav")).toBeNull()
  expect(Buffer.from(await (await client.get("audio/one.wav"))!.arrayBuffer()).equals(bytes)).toBe(true)
  const part = (await client.get("audio/one.wav", { start: 100, end: 199 }))!
  expect(part.status).toBe(206)
  expect(Buffer.from(await part.arrayBuffer()).equals(bytes.subarray(100, 200))).toBe(true)
  expect(fake.requests.at(-1)).toMatchObject({ method: "GET", range: "bytes=100-199" })
  // Keys with characters that need encoding are signed as they are sent; a file on disk is sent without being read into memory by the caller.
  const onDisk = join(scratch, "upload.bin"); await writeFile(onDisk, bytes)
  await client.put("downloads/Cereal-1.0.0+build.7-mac-arm64 (1).dmg", Bun.file(onDisk), { sha256: sha256(bytes) })
  expect(Buffer.from(fake.buckets.get("cereal-test")!.get("team a/downloads/Cereal-1.0.0+build.7-mac-arm64 (1).dmg")!.bytes).equals(bytes)).toBe(true)
  for (const name of ["b", "c", "d"]) await client.put(`audio/${name}.wav`, new Uint8Array([1]))
  // Listing follows the store's pages (two keys a page here) and returns keys without the prefix.
  expect((await client.list("audio/")).sort()).toEqual(["audio/b.wav", "audio/c.wav", "audio/d.wav", "audio/one.wav"])
  expect(fake.requests.filter(request => request.path.includes("list-type=2")).length).toBe(2)
  await client.delete("audio/one.wav"); await client.delete("audio/one.wav")
  expect(await client.head("audio/one.wav")).toBeNull()
  expect(await failure(makeS3Client(bucketConfig({ secretAccessKey: "not-the-secret-000" })).put("x", new Uint8Array([1])))).toMatchObject({ status: 403, code: "SignatureDoesNotMatch" })
  expect(await failure(makeS3Client(bucketConfig({ accessKeyId: "AKIAUNKNOWN000000001" })).head("x"))).toBeInstanceOf(S3Failure)
  expect(await failure(makeS3Client(bucketConfig({ bucket: "no-such-bucket" })).put("x", new Uint8Array([1])))).toMatchObject({ status: 404, code: "NoSuchBucket" })
  expect(await failure(makeS3Client(bucketConfig({ endpoint: "http://127.0.0.1:9" })).put("x", new Uint8Array([1])))).toMatchObject({ status: null })
})

test("the stand-in's signature check also accepts a second, unrelated signer, and the two address styles are formed as S3 defines them", async () => {
  // Bun's built-in client signs on its own. If the verifier accepts it and the server's signer, the two signers agree.
  const independent = new Bun.S3Client({ endpoint: fake.endpoint, bucket: "cereal-test", region: "us-east-1", accessKeyId: ACCESS, secretAccessKey: SECRET })
  await independent.write("independent/hello world.txt", "from another signer")
  expect(new TextDecoder().decode(fake.buckets.get("cereal-test")!.get("independent/hello world.txt")!.bytes)).toBe("from another signer")
  expect(await (await makeS3Client(bucketConfig()).get("independent/hello world.txt"))!.text()).toBe("from another signer")
  expect(await independent.file("independent/hello world.txt").text()).toBe("from another signer")
  await makeS3Client(bucketConfig()).put("mine/é ü.txt", new TextEncoder().encode("from the server's signer"))
  expect(await independent.file("mine/é ü.txt").text()).toBe("from the server's signer")
  expect(await failure(new Bun.S3Client({ endpoint: fake.endpoint, bucket: "cereal-test", region: "us-east-1", accessKeyId: ACCESS, secretAccessKey: "wrong-secret-000" }).write("x", "y"))).toBeDefined()
  const now = new Date("2026-03-09T10:11:12Z")
  const path = signS3Request(bucketConfig({ endpoint: "https://store.example.com", forcePathStyle: true }), { method: "GET", key: "a b/c+d.wav", now })
  expect(path.url).toBe("https://store.example.com/cereal-test/a%20b/c%2Bd.wav")
  expect(path.headers.Authorization).toMatch(/^AWS4-HMAC-SHA256 Credential=AKIAFAKEFAKEFAKE0001\/20260309\/us-east-1\/s3\/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature=[0-9a-f]{64}$/)
  expect(path.headers["x-amz-date"]).toBe("20260309T101112Z")
  const virtual = signS3Request(bucketConfig({ endpoint: null, region: "eu-west-1", forcePathStyle: false }), { method: "PUT", key: "k", headers: { "x-amz-meta-sha256": "ab" }, now })
  expect(virtual.url).toBe("https://cereal-test.s3.eu-west-1.amazonaws.com/k")
  expect(virtual.headers.Authorization).toContain("SignedHeaders=host;x-amz-content-sha256;x-amz-date;x-amz-meta-sha256,")
  expect(signS3Request(bucketConfig({ endpoint: "https://account.r2.example.com/base/", forcePathStyle: true }), { method: "GET", key: "", query: { "list-type": "2", prefix: "a/b c" }, now }).url).toBe("https://account.r2.example.com/base/cereal-test?list-type=2&prefix=a%2Fb%20c")
})

test("settings are checked: https except on this machine, no metadata or link-local addresses, S3's bucket names, a tidy prefix", () => {
  expect(normalizeS3Endpoint(" https://s3.example.com/ ")).toBe("https://s3.example.com")
  expect(normalizeS3Endpoint("https://ACCOUNT.r2.cloudflarestorage.com")).toBe("https://account.r2.cloudflarestorage.com")
  expect(normalizeS3Endpoint("https://host.example/storage/v1/s3/")).toBe("https://host.example/storage/v1/s3")
  expect(normalizeS3Endpoint("https://10.0.0.5:9000")).toBe("https://10.0.0.5:9000")
  for (const local of ["http://localhost:9000", "http://127.0.0.1:9100", "http://[::1]:9000"]) expect(normalizeS3Endpoint(local)).toBe(local)
  for (const bad of ["", "s3.example.com", "ftp://s3.example.com", "http://s3.example.com", "http://10.0.0.5:9000", "http://minio:9000", "https://user:pass@s3.example.com", "https://s3.example.com/?x=1", "https://s3.example.com/#a",
    "http://169.254.169.254", "https://169.254.169.254/latest/meta-data", "https://169.254.10.1", "https://[fe80::1]", "https://[::ffff:169.254.169.254]", "https://[::ffff:a9fe:a9fe]", "https://[fd00:ec2::254]", "https://0.0.0.0", "https://metadata.google.internal", "https://[::]"])
    expect(() => normalizeS3Endpoint(bad), bad).toThrow()
  for (const host of ["169.254.169.254", "169.254.0.1", "0.0.0.0", "fe80::1", "[fe80::1]", "::ffff:169.254.169.254", "metadata.google.internal"]) expect(forbiddenAddress(host), host).toBe(true)
  for (const host of ["127.0.0.1", "10.0.0.1", "192.168.1.5", "s3.amazonaws.com", "::1", "2001:db8::1"]) expect(forbiddenAddress(host), host).toBe(false)
  for (const name of ["cereal", "my-bucket.eu", "a1b", "x".repeat(63)]) expect(validBucketName(name), name).toBe(true)
  for (const name of ["", "ab", "Cereal", "my_bucket", "-cereal", "cereal-", "a..b", "192.168.1.1", "x".repeat(64), "a/b", "a b", "a.-b"]) expect(validBucketName(name), name).toBe(false)
  expect(normalizeS3Prefix("")).toBe(""); expect(normalizeS3Prefix(" / ")).toBe("")
  expect(normalizeS3Prefix("cereal")).toBe("cereal/"); expect(normalizeS3Prefix("/team/cereal//")).toBe("team/cereal/")
  for (const bad of ["a/../b", "../a", "a//b", "a\\b", "a b", "a?b", "./a", "x".repeat(201)]) expect(() => normalizeS3Prefix(bad), bad).toThrow()
})

test("keys are made only of a note's UUID and the audio's SHA-256", () => {
  const hash = "a".repeat(64)
  expect(audioKey(NOTE.toUpperCase(), hash)).toBe(`${NOTE}/${hash}.wav`)
  expect(audioPrefix(NOTE)).toBe(`${NOTE}/`)
  for (const [note, digest] of [["../etc", hash], [NOTE, "../../passwd"], [NOTE + "/..", hash], [NOTE, hash.toUpperCase()], [NOTE, hash + "0"], ["", hash], [NOTE, ""], [`${NOTE}%2F..`, hash], ["my recording", hash]] as const) expect(() => audioKey(note, digest), `${note} ${digest}`).toThrow()
  expect(() => audioPrefix("..")).toThrow()
})

test("local disk is the default and keeps the layout it always had; a bucket gets the same files under audio/ and downloads/", async () => {
  const audioDir = await directory("audio"), downloadsDir = await directory("downloads"), bytes = randomBytes(3000), key = audioKey(NOTE, sha256(bytes))
  const local = makeStorage({ audioDir, downloadsDir })
  expect(await local.current()).toEqual({ backend: "local", s3: null })
  expect(await local.audio.put(key, bytes, { sha256: sha256(bytes) })).toBe("local")
  // Byte for byte where the server has always put it, private to the server's user, with no temporary file left behind.
  expect(Buffer.from(await readFile(join(audioDir, NOTE, `${sha256(bytes)}.wav`))).equals(bytes)).toBe(true)
  expect(await readdir(join(audioDir, NOTE))).toEqual([`${sha256(bytes)}.wav`])
  expect((await stat(join(audioDir, NOTE, `${sha256(bytes)}.wav`))).mode & 0o777).toBe(0o600)
  // The same audio again is one file; other bytes under the same name are refused.
  await local.audio.put(key, bytes, { sha256: sha256(bytes) })
  expect(await failure(local.audio.put(key, randomBytes(3000), { sha256: sha256(bytes) }))).toBeInstanceOf(StorageFailure)
  expect(Buffer.from((await local.audio.bytes(key))!).equals(bytes)).toBe(true)
  expect(await local.audio.stat(key)).toMatchObject({ backend: "local", size: 3000, sha256: null })
  expect(await local.audio.bytes(audioKey(NOTE, "b".repeat(64)))).toBeNull()
  // Keys cannot leave their area, and a link that does is not followed.
  for (const bad of ["../x", "a/../../x", "/etc/passwd", "a//b", ".hidden", "a/.b", ""]) expect(await failure(local.audio.bytes(bad)), bad).toBeInstanceOf(StorageFailure)
  await writeFile(join(scratch, "outside.wav"), "outside"); await mkdir(join(audioDir, "linked"))
  await symlink(join(scratch, "outside.wav"), join(audioDir, "linked", "x.wav"))
  expect(await local.audio.bytes("linked/x.wav")).toBeNull()
  await local.audio.deletePrefix(audioPrefix(NOTE))
  expect(await readdir(audioDir)).toEqual(["linked"])
  // The bucket.
  let configuration: StorageConfiguration = { backend: "s3", s3: bucketConfig({ prefix: "cereal/" }) }
  const storage = makeStorage({ audioDir, downloadsDir, configuration: async () => configuration })
  expect(await storage.audio.put(key, bytes, { sha256: sha256(bytes), contentType: "audio/wav" })).toBe("s3")
  const object = fake.buckets.get("cereal-test")!.get(`cereal/audio/${NOTE}/${sha256(bytes)}.wav`)!
  expect(Buffer.from(object.bytes).equals(bytes)).toBe(true)
  expect(object.metadata.sha256).toBe(sha256(bytes))
  expect(await readdir(audioDir)).toEqual(["linked"])
  expect(await storage.audio.stat(key)).toMatchObject({ backend: "s3", size: 3000, sha256: sha256(bytes), path: null })
  await storage.downloads.put("Cereal-1.2.3-mac-arm64.dmg", bytes)
  expect([...fake.buckets.get("cereal-test")!.keys()].sort()).toEqual([`cereal/audio/${NOTE}/${sha256(bytes)}.wav`, "cereal/downloads/Cereal-1.2.3-mac-arm64.dmg"])
  // S3 selected without a complete configuration: a write is refused rather than quietly put somewhere else.
  configuration = { backend: "s3", s3: null }
  expect(await failure(storage.audio.put(key, bytes))).toMatchObject({ backend: "s3", operation: "write" })
})

test("switching never hides a file: reads try the selected backend then the other, deletes remove it wherever it is", async () => {
  const audioDir = await directory("audio"), downloadsDir = await directory("downloads")
  let configuration: StorageConfiguration = { backend: "local", s3: null }
  const storage = makeStorage({ audioDir, downloadsDir, configuration: async () => configuration })
  const before = randomBytes(2000), after = randomBytes(2100), keyBefore = audioKey(NOTE, sha256(before)), keyAfter = audioKey(NOTE, sha256(after))
  await storage.audio.put(keyBefore, before)
  // Switch to the bucket: the old part is still read from disk, the new one goes to the bucket, nothing is copied.
  configuration = { backend: "s3", s3: bucketConfig() }
  expect(Buffer.from((await storage.audio.bytes(keyBefore))!).equals(before)).toBe(true)
  expect((await storage.audio.stat(keyBefore))!.backend).toBe("local")
  await storage.audio.put(keyAfter, after)
  expect(fake.buckets.get("cereal-test")!.size).toBe(1)
  expect(await readdir(join(audioDir, NOTE))).toEqual([`${sha256(before)}.wav`])
  // Switch back while the bucket is still configured: the part in the bucket is still read.
  configuration = { backend: "local", s3: bucketConfig() }
  expect(Buffer.from((await storage.audio.bytes(keyAfter))!).equals(after)).toBe(true)
  expect((await storage.audio.stat(keyAfter))!.backend).toBe("s3")
  // A bucket that cannot be reached does not hide what is on disk, but a file only it has is an error, not "missing".
  configuration = { backend: "s3", s3: bucketConfig({ endpoint: "http://127.0.0.1:9" }) }
  expect(Buffer.from((await storage.audio.bytes(keyBefore))!).equals(before)).toBe(true)
  expect(await failure(storage.audio.bytes(keyAfter))).toBeInstanceOf(StorageFailure)
  // With the bucket no longer configured its files are simply not there.
  configuration = { backend: "local", s3: null }
  expect(await storage.audio.bytes(keyAfter)).toBeNull()
  // Deleting a note's audio removes both halves, under either setting.
  configuration = { backend: "s3", s3: bucketConfig() }
  await storage.audio.put(audioKey("11111111-2222-4333-8444-555555555555", sha256(after)), after)
  await storage.audio.deletePrefix(audioPrefix(NOTE))
  expect(await readdir(audioDir)).toEqual([])
  expect([...fake.buckets.get("cereal-test")!.keys()]).toEqual([`audio/11111111-2222-4333-8444-555555555555/${sha256(after)}.wav`])
  await storage.audio.delete(audioKey("11111111-2222-4333-8444-555555555555", sha256(after)))
  expect(fake.buckets.get("cereal-test")!.size).toBe(0)
})

test("test connection writes, reads back and deletes, and names the step that failed", async () => {
  const audioDir = await directory("audio"), storage = makeStorage({ audioDir, downloadsDir: await directory("downloads") })
  expect(await storage.test({ backend: "s3", s3: bucketConfig() })).toEqual({ ok: true, backend: "s3", step: null, message: "Wrote, read and deleted a test file in cereal-test" })
  expect(fake.buckets.get("cereal-test")!.size).toBe(0)
  expect(fake.requests.map(request => request.method)).toEqual(["PUT", "GET", "DELETE"])
  expect(await storage.test({ backend: "s3", s3: bucketConfig({ secretAccessKey: "wrong-secret-0000" }) })).toEqual({ ok: false, backend: "s3", step: "write", message: "Writing a test file to the bucket failed: the secret access key is wrong (403 SignatureDoesNotMatch)" })
  expect(await storage.test({ backend: "s3", s3: bucketConfig({ bucket: "missing-bucket" }) })).toEqual({ ok: false, backend: "s3", step: "write", message: "Writing a test file to the bucket failed: the bucket does not exist (404 NoSuchBucket)" })
  expect(await storage.test({ backend: "s3", s3: bucketConfig({ accessKeyId: "AKIAUNKNOWN000000001" }) })).toMatchObject({ ok: false, step: "write", message: "Writing a test file to the bucket failed: the access key ID is not known to the store (403 InvalidAccessKeyId)" })
  expect(await storage.test({ backend: "s3", s3: bucketConfig({ endpoint: "http://127.0.0.1:9" }) })).toMatchObject({ ok: false, step: "write", message: "Writing a test file to the bucket failed: the endpoint did not answer (no answer)" })
  fake.failNext("GET", 403, "AccessDenied")
  expect(await storage.test({ backend: "s3", s3: bucketConfig() })).toMatchObject({ ok: false, step: "read", message: "The test file was written to the bucket but could not be read back: the key is not allowed to do this in the bucket (403 AccessDenied)" })
  expect(fake.buckets.get("cereal-test")!.size).toBe(0)
  fake.failNext("DELETE", 403, "AccessDenied")
  expect(await storage.test({ backend: "s3", s3: bucketConfig() })).toMatchObject({ ok: false, step: "delete" })
  expect(await storage.test({ backend: "s3", s3: null })).toMatchObject({ ok: false, step: "write" })
  expect(await storage.test({ backend: "local", s3: null })).toEqual({ ok: true, backend: "local", step: null, message: "Wrote, read and deleted a test file on local disk" })
  expect(await readdir(audioDir)).toEqual([])
  // No message ever carries the secret.
  for (const config of [bucketConfig({ secretAccessKey: "wrong-secret-0000" }), bucketConfig({ bucket: "missing-bucket" })]) expect(JSON.stringify(await storage.test({ backend: "s3", s3: config }))).not.toContain(config.secretAccessKey)
})

/** The real HTTP layer with admin settings in memory, storage following them, and a worker the test runs by hand. */
async function server(environment: NodeJS.ProcessEnv = {}) {
  const repository = makeMemoryRepository(), audioDir = await directory("audio"), downloadsDir = await directory("downloads")
  const administration = makeAdministrationService(makeMemoryCalendarStore(repository), secretBoxFromKey(randomBytes(32)), { environment })
  const storage = makeStorage({ audioDir, downloadsDir, configuration: () => administration.storageConfiguration() })
  const heard: number[] = []
  const providers: ProviderServices = { transcribe: input => Effect.sync(() => { heard.push(input.audio.byteLength); return [{ id: crypto.randomUUID(), channel: input.channel, speakerId: `${input.chunkId}:speaker_0`, speakerLabel: "Speaker 0", text: "Stored and read back.", isFinal: true, startMs: input.startMs, endMs: input.startMs + 100 }] }), generate: () => Effect.succeed("Notes"), chat: () => Effect.succeed("") }
  const processing = makeProcessingService(repository, { providers, worker: false, storage, storageDir: audioDir })
  const downloads = makeDownloads({ directory: downloadsDir, version: "3.4.5", storage: storage.downloads })
  const api = makeApiHandler(repository, token, { processing, administration, storage, downloads })
  cleanup.push(api.dispose)
  const request = (path: string, method = "GET", body?: unknown, headers: Record<string, string> = {}) => api.handler(new Request(`http://localhost/api/v1${path}`, { method, headers: { Authorization: `Bearer ${token}`, ...(body === undefined ? {} : { "Content-Type": "application/json" }), ...headers }, ...(body === undefined ? {} : { body: JSON.stringify(body) }) }))
  const s3Settings = { backend: "s3", endpoint: fake.endpoint, region: "us-east-1", bucket: "cereal-test", prefix: "", forcePathStyle: true, accessKeyId: ACCESS, secretAccessKey: SECRET }
  return { repository, administration, storage, processing, api, request, audioDir, downloadsDir, heard, s3Settings }
}
function wav(seed = 0) {
  const b = Buffer.alloc(44 + 3200, seed)
  b.write("RIFF"); b.writeUInt32LE(b.length - 8, 4); b.write("WAVEfmt ", 8); b.writeUInt32LE(16, 16); b.writeUInt16LE(1, 20); b.writeUInt16LE(1, 22)
  b.writeUInt32LE(16000, 24); b.writeUInt32LE(32000, 28); b.writeUInt16LE(2, 32); b.writeUInt16LE(16, 34); b.write("data", 36); b.writeUInt32LE(3200, 40)
  return b
}

test("storage settings: defaults, the environment as a fallback, validation, and a secret that is sealed and never returned", async () => {
  const f = await server()
  const initial = await (await f.request("/admin/settings")).json()
  expect(initial.storage).toEqual({ backend: "local", source: "default", endpoint: null, region: "us-east-1", bucket: null, prefix: "", forcePathStyle: false, accessKeyId: null, secretAccessKeySet: false })
  const patch = async (storage: Record<string, unknown>) => { const response = await f.request("/admin/settings", "PATCH", { storage }); return { status: response.status, body: await response.json() } }
  // S3 needs a bucket, a key and a secret; every value is checked.
  expect(await patch({ backend: "s3" })).toMatchObject({ status: 400, body: { message: "Enter the bucket, access key ID and secret access key before choosing S3-compatible storage" } })
  for (const bad of [{ endpoint: "http://s3.example.com" }, { endpoint: "https://169.254.169.254" }, { endpoint: "not a url" }, { bucket: "My_Bucket" }, { bucket: "ab" }, { prefix: "a/../b" }, { region: "US EAST" }, { accessKeyId: "has space" }, { secretAccessKey: "short" }])
    expect((await patch({ ...f.s3Settings, ...bad })).status, JSON.stringify(bad)).toBe(400)
  expect((await (await f.request("/admin/settings")).json()).storage.source).toBe("default")
  const saved = await patch({ ...f.s3Settings, prefix: "/cereal/" })
  expect(saved.status).toBe(200)
  expect(saved.body.storage).toEqual({ backend: "s3", source: "stored", endpoint: fake.endpoint, region: "us-east-1", bucket: "cereal-test", prefix: "cereal/", forcePathStyle: true, accessKeyId: ACCESS, secretAccessKeySet: true })
  expect(JSON.stringify(saved.body)).not.toContain(SECRET)
  expect(JSON.stringify(await (await f.request("/admin/settings")).json())).not.toContain(SECRET)
  // In force at once, without a restart, and sealed at rest.
  expect(await f.storage.current()).toEqual({ backend: "s3", s3: { endpoint: fake.endpoint, region: "us-east-1", bucket: "cereal-test", prefix: "cereal/", forcePathStyle: true, accessKeyId: ACCESS, secretAccessKey: SECRET } })
  // A partial change keeps the rest, the secret included.
  expect((await patch({ backend: "local" })).body.storage).toMatchObject({ backend: "local", bucket: "cereal-test", secretAccessKeySet: true })
  expect(await f.storage.current()).toMatchObject({ backend: "local", s3: { bucket: "cereal-test" } })
  // Removing the secret un-configures the bucket; S3 cannot then be selected.
  expect((await patch({ secretAccessKey: null })).body.storage).toMatchObject({ backend: "local", secretAccessKeySet: false })
  expect(await f.storage.current()).toEqual({ backend: "local", s3: null })
  expect((await patch({ backend: "s3" })).status).toBe(400)
  // Only administrators may read or change any of it.
  expect((await f.api.handler(new Request("http://localhost/api/v1/admin/settings"))).status).toBe(401)
  expect((await f.api.handler(new Request("http://localhost/api/v1/admin/storage/test", { method: "POST", headers: { "Content-Type": "application/json" }, body: "{}" }))).status).toBe(401)
  // The environment applies only while nothing has been saved.
  const env = await server({ STORAGE_BACKEND: "s3", S3_ENDPOINT: fake.endpoint, S3_BUCKET: "second-bucket", S3_PREFIX: "env", S3_ACCESS_KEY_ID: ACCESS, S3_SECRET_ACCESS_KEY: SECRET, S3_FORCE_PATH_STYLE: "true" })
  expect((await (await env.request("/admin/settings")).json()).storage).toEqual({ backend: "s3", source: "environment", endpoint: fake.endpoint, region: "us-east-1", bucket: "second-bucket", prefix: "env/", forcePathStyle: true, accessKeyId: ACCESS, secretAccessKeySet: true })
  expect(await env.storage.current()).toMatchObject({ backend: "s3", s3: { bucket: "second-bucket", prefix: "env/", secretAccessKey: SECRET } })
  expect((await (await env.request("/admin/settings", "PATCH", { storage: { bucket: "cereal-test" } })).json()).storage).toMatchObject({ backend: "s3", source: "stored", bucket: "cereal-test", prefix: "env/", secretAccessKeySet: true })
  // An environment that points somewhere it must not is ignored, not obeyed.
  const unsafe = await server({ STORAGE_BACKEND: "s3", S3_ENDPOINT: "http://169.254.169.254", S3_BUCKET: "cereal-test", S3_ACCESS_KEY_ID: ACCESS, S3_SECRET_ACCESS_KEY: SECRET })
  expect((await unsafe.storage.current()).s3?.endpoint ?? null).toBeNull()
})

test("test connection over HTTP tries a draft without saving it, and uses the stored secret when the draft has none", async () => {
  const f = await server()
  const tryIt = async (draft: Record<string, unknown>) => await (await f.request("/admin/storage/test", "POST", draft)).json()
  expect(await tryIt(f.s3Settings)).toEqual({ ok: true, backend: "s3", step: null, message: "Wrote, read and deleted a test file in cereal-test" })
  expect((await (await f.request("/admin/settings")).json()).storage.source).toBe("default")
  expect(await tryIt({ ...f.s3Settings, secretAccessKey: "definitely-wrong-secret" })).toMatchObject({ ok: false, step: "write", message: "Writing a test file to the bucket failed: the secret access key is wrong (403 SignatureDoesNotMatch)" })
  expect(await tryIt({ ...f.s3Settings, bucket: "missing-bucket" })).toMatchObject({ ok: false, step: "write", message: "Writing a test file to the bucket failed: the bucket does not exist (404 NoSuchBucket)" })
  expect((await f.request("/admin/storage/test", "POST", { ...f.s3Settings, endpoint: "https://169.254.169.254" })).status).toBe(400)
  expect(await tryIt({ backend: "local" })).toMatchObject({ ok: true, backend: "local" })
  await f.request("/admin/settings", "PATCH", { storage: f.s3Settings })
  expect(await tryIt({})).toMatchObject({ ok: true, backend: "s3" })
  expect(await tryIt({ bucket: "second-bucket" })).toMatchObject({ ok: true, message: "Wrote, read and deleted a test file in second-bucket" })
  expect(fake.buckets.get("cereal-test")!.size + fake.buckets.get("second-bucket")!.size).toBe(0)
})

test("audio through a bucket: an uploaded part lands there, is transcribed from there and is deleted with its note; older local audio keeps working", async () => {
  const f = await server()
  const first = await (await f.request("/notes", "POST", { title: "Before the switch" })).json()
  const upload = (noteId: string, seed: number, sequence = 0) => f.request(`/notes/${noteId}/audio/chunks/${crypto.randomUUID()}`, "PUT", { channel: "system", sequence, startMs: 0, audioBase64: wav(seed).toString("base64") })
  // Recorded while storage is local disk, not yet transcribed.
  expect((await upload(first.id, 1)).status).toBe(200)
  expect(await readdir(join(f.audioDir, first.id))).toEqual([`${sha256(wav(1))}.wav`])
  expect((await f.request("/admin/settings", "PATCH", { storage: { ...f.s3Settings, prefix: "cereal" } })).status).toBe(200)
  const second = await (await f.request("/notes", "POST", { title: "After the switch" })).json()
  expect((await upload(second.id, 2)).status).toBe(200)
  const bucket = fake.buckets.get("cereal-test")!
  expect([...bucket.keys()]).toEqual([`cereal/audio/${second.id}/${sha256(wav(2))}.wav`])
  expect(Buffer.from(bucket.get(`cereal/audio/${second.id}/${sha256(wav(2))}.wav`)!.bytes).equals(wav(2))).toBe(true)
  expect(await readdir(f.audioDir)).toEqual([first.id])
  // The worker reads each part from wherever it is.
  await f.processing.runPending()
  expect(f.heard).toEqual([wav(1).length, wav(2).length])
  for (const note of [first, second]) {
    const chunks = await (await f.request(`/notes/${note.id}/audio/chunks`)).json()
    expect(chunks.map((chunk: { state: string }) => chunk.state)).toEqual(["ready"])
  }
  // Uploading the same audio again is the same object, not a second one.
  expect((await upload(second.id, 2, 1)).status).toBe(200)
  expect(bucket.size).toBe(1)
  // Deleting each note removes its audio from where it is.
  expect((await f.request(`/notes/${second.id}`, "DELETE")).status).toBe(200)
  expect(bucket.size).toBe(0)
  expect((await f.request(`/notes/${first.id}`, "DELETE")).status).toBe(200)
  expect(await readdir(f.audioDir)).toEqual([])
})

test("a storage failure during upload is a clean 503 and leaves no record of the part", async () => {
  const f = await server()
  await f.request("/admin/settings", "PATCH", { storage: f.s3Settings })
  const note = await (await f.request("/notes", "POST", { title: "Unlucky" })).json()
  const send = () => f.request(`/notes/${note.id}/audio/chunks/${crypto.randomUUID()}`, "PUT", { channel: "mic", sequence: 0, startMs: 0, audioBase64: wav(3).toString("base64") })
  for (const [status, code] of [[500, "InternalError"], [403, "AccessDenied"], [404, "NoSuchBucket"]] as const) {
    fake.failNext("PUT", status, code)
    const response = await send()
    expect(response.status).toBe(503)
    expect(await response.json()).toEqual({ error: "unavailable", message: "Processing is temporarily unavailable" })
  }
  expect(await (await f.request(`/notes/${note.id}/audio/chunks`)).json()).toEqual([])
  expect(fake.buckets.get("cereal-test")!.size).toBe(0)
  expect(await readdir(f.audioDir)).toEqual([])
  // The store recovers: the same upload now succeeds, once.
  expect((await send()).status).toBe(200)
  expect((await (await f.request(`/notes/${note.id}/audio/chunks`)).json()).length).toBe(1)
  expect(fake.buckets.get("cereal-test")!.size).toBe(1)
  // A part whose object has gone missing fails its transcription; it is not reported as transcribed.
  fake.buckets.get("cereal-test")!.clear()
  await f.processing.runPending()
  expect((await (await f.request(`/notes/${note.id}/audio/chunks`)).json())[0]).toMatchObject({ state: "failed" })
})

test("downloads from a bucket: listed with the recorded digest, streamed through the server with ranges, and disk is still looked in second", async () => {
  const f = await server(), dmg = randomBytes(300_000), zip = randomBytes(1234), older = randomBytes(50)
  const DMG = "Cereal-3.4.5-mac-arm64.dmg", ZIP = "Cereal-3.4.5-mac-arm64.zip"
  await f.request("/admin/settings", "PATCH", { storage: { ...f.s3Settings, prefix: "cereal" } })
  const client = makeS3Client(bucketConfig({ prefix: "cereal/" }))
  // Published with its digest recorded; another version beside it.
  await client.put(`downloads/${DMG}`, dmg, { sha256: sha256(dmg) })
  await client.put("downloads/Cereal-3.4.4-mac-arm64.dmg", older, { sha256: sha256(older) })
  fake.requests.length = 0
  const listed = await (await f.api.handler(new Request("http://localhost/api/v1/downloads"))).json()
  expect(listed).toEqual({ version: "3.4.5", mac: { arch: "arm64", minimumSystem: "14.2", installer: { file: DMG, url: `/api/v1/downloads/files/${DMG}`, size: 300_000, sha256: sha256(dmg) }, update: null }, iosAppStoreUrl: null })
  // The recorded digest is used as it is: the object is described, never fetched.
  expect(fake.requests.every(request => request.method === "HEAD")).toBe(true)
  // An archive uploaded by other means has no recorded digest: it is hashed once from a stream, then remembered by its ETag.
  await makeS3Client(bucketConfig({ prefix: "cereal/" })).put(`downloads/${ZIP}`, zip)
  fake.requests.length = 0
  for (let i = 0; i < 3; i++) expect((await (await f.api.handler(new Request("http://localhost/api/v1/downloads"))).json()).mac.update).toEqual({ file: ZIP, url: `/api/v1/downloads/files/${ZIP}`, size: 1234, sha256: sha256(zip) })
  expect(fake.requests.filter(request => request.method === "GET").length).toBe(1)
  await makeS3Client(bucketConfig({ prefix: "cereal/" })).put(`downloads/${ZIP}`, randomBytes(1234))
  expect((await (await f.api.handler(new Request("http://localhost/api/v1/downloads"))).json()).mac.update.sha256).not.toBe(sha256(zip))
  // Served through this server: same headers as a file on disk, bytes from the bucket.
  const whole = await f.api.handler(new Request(`http://localhost/api/v1/downloads/files/${DMG}`))
  expect(whole.status).toBe(200)
  expect(whole.headers.get("content-type")).toBe("application/x-apple-diskimage")
  expect(whole.headers.get("content-disposition")).toBe(`attachment; filename="${DMG}"`)
  expect(whole.headers.get("etag")).toBe(`"${sha256(dmg)}"`)
  expect(whole.headers.get("cache-control")).toBe("public, max-age=300")
  expect(whole.headers.get("location")).toBeNull()
  expect(sha256(new Uint8Array(await whole.arrayBuffer()))).toBe(sha256(dmg))
  const head = await f.api.handler(new Request(`http://localhost/api/v1/downloads/files/${DMG}`, { method: "HEAD" }))
  expect(head.headers.get("content-length")).toBe("300000")
  const part = await f.api.handler(new Request(`http://localhost/api/v1/downloads/files/${DMG}`, { headers: { Range: "bytes=250000-" } }))
  expect(part.status).toBe(206)
  expect(part.headers.get("content-range")).toBe("bytes 250000-299999/300000")
  expect(Buffer.from(await part.arrayBuffer()).equals(dmg.subarray(250_000))).toBe(true)
  // Only the requested range was asked of the bucket.
  expect(fake.requests.at(-1)).toMatchObject({ method: "GET", range: "bytes=250000-299999" })
  expect((await f.api.handler(new Request(`http://localhost/api/v1/downloads/files/${DMG}`, { headers: { Range: "bytes=300000-" } }))).status).toBe(416)
  expect((await f.api.handler(new Request(`http://localhost/api/v1/downloads/files/${DMG}`, { headers: { "If-None-Match": `"${sha256(dmg)}"` } }))).status).toBe(304)
  // Another version in the bucket, and anything else, is the same 404 as on disk.
  for (const name of ["Cereal-3.4.4-mac-arm64.dmg", "..%2Faudio%2Fx.wav", "nothing.dmg"]) expect((await f.api.handler(new Request(`http://localhost/api/v1/downloads/files/${name}`))).status, name).toBe(404)
  // Over a real connection the proxied bytes are intact, whole and as a range.
  const http = Bun.serve({ port: 0, hostname: "127.0.0.1", fetch: request => f.api.handler(request) })
  cleanup.push(() => { void http.stop(true) })
  expect(sha256(new Uint8Array(await (await fetch(`http://127.0.0.1:${http.port}/api/v1/downloads/files/${DMG}`)).arrayBuffer()))).toBe(sha256(dmg))
  const resumed = await fetch(`http://127.0.0.1:${http.port}/api/v1/downloads/files/${DMG}`, { headers: { Range: "bytes=1000-1999" } })
  expect(resumed.status).toBe(206)
  expect(Buffer.from(await resumed.arrayBuffer()).equals(dmg.subarray(1000, 2000))).toBe(true)
  // Removed from the bucket but present on disk: the build on disk is offered under the S3 setting too.
  await client.delete(`downloads/${DMG}`); await client.delete(`downloads/${ZIP}`)
  expect((await (await f.api.handler(new Request("http://localhost/api/v1/downloads"))).json()).mac).toBeNull()
  const local = randomBytes(4000)
  await writeFile(join(f.downloadsDir, DMG), local)
  expect((await (await f.api.handler(new Request("http://localhost/api/v1/downloads"))).json()).mac.installer).toMatchObject({ size: 4000, sha256: sha256(local) })
  const fromDisk = await f.api.handler(new Request(`http://localhost/api/v1/downloads/files/${DMG}`))
  expect(fromDisk.headers.get("content-length")).toBe("4000")
  expect(Buffer.from(await fromDisk.arrayBuffer()).equals(local)).toBe(true)
  // A bucket that cannot be reached must not take the build on disk down with it.
  await f.request("/admin/settings", "PATCH", { storage: { endpoint: "http://127.0.0.1:9" } })
  expect((await (await f.api.handler(new Request("http://localhost/api/v1/downloads"))).json()).mac.installer.sha256).toBe(sha256(local))
})
