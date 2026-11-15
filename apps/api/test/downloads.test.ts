import { afterAll, afterEach, beforeAll, expect, test } from "bun:test"
import { createHash, randomBytes } from "node:crypto"
import { mkdir, mkdtemp, readFile, realpath, rm, symlink, utimes, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { makeApiHandler } from "../src/api"
import { makeDownloads, parseRange, MAC_MINIMUM_SYSTEM } from "../src/downloads"
import { makeMemoryRepository } from "../src/repository"
import { readAppVersion, ROOT_PACKAGE_PATH } from "../src/version"

const token = "test-owner-token-with-at-least-32-characters"
const VERSION = "3.4.5", DMG = `Cereal-${VERSION}-mac-arm64.dmg`, ZIP = `Cereal-${VERSION}-mac-arm64.zip`
const sha256 = (bytes: Uint8Array) => createHash("sha256").update(bytes).digest("hex")
let scratch = ""
beforeAll(async () => { scratch = await realpath(await mkdtemp(join(tmpdir(), "cereal-downloads-"))) })
afterAll(async () => { await rm(scratch, { recursive: true, force: true }) })
const disposals: Array<() => Promise<void> | void> = []
afterEach(async () => { for (const dispose of disposals.splice(0)) await dispose() })

let counter = 0
/** A fresh downloads directory and an API handler that serves it, with no signed-in caller. */
async function fixture(options: { version?: string; iosAppStoreId?: string; directory?: string } = {}) {
  const directory = options.directory ?? join(scratch, `published-${counter++}`)
  if (!options.directory) await mkdir(directory)
  const downloads = makeDownloads({ directory, version: options.version ?? VERSION, iosAppStoreId: options.iosAppStoreId })
  const api = makeApiHandler(makeMemoryRepository(), token, { downloads })
  disposals.push(api.dispose)
  const request = (path: string, init: RequestInit = {}) => api.handler(new Request(`http://localhost${path}`, init))
  const manifest = async () => { const response = await request("/api/v1/downloads"); expect(response.status).toBe(200); return await response.json() as any }
  return { directory, request, manifest, api }
}
const file = (name: string) => `/api/v1/downloads/files/${name}`

test("the product version is the root package.json's, and APP_VERSION replaces it only when it is a version", async () => {
  const root = JSON.parse(await readFile(ROOT_PACKAGE_PATH, "utf8")) as { version: string }
  expect(root.version).toMatch(/^\d+\.\d+\.\d+$/)
  expect(readAppVersion({ env: {} })).toBe(root.version)
  expect(readAppVersion({ env: { APP_VERSION: "" } })).toBe(root.version)
  expect(readAppVersion({ env: { APP_VERSION: "9.8.7" } })).toBe("9.8.7")
  expect(readAppVersion({ env: { APP_VERSION: "1.2.3-beta.1" } })).toBe("1.2.3-beta.1")
  for (const bad of ["v1", "1.2", "1.2.3/../..", "../1.2.3", "1.2.3 ", "latest"]) expect(() => readAppVersion({ env: { APP_VERSION: bad } })).toThrow()
  const own = join(scratch, "package.json")
  await writeFile(own, JSON.stringify({ name: "x", version: "2.0.1" }))
  expect(readAppVersion({ env: {}, packagePath: own })).toBe("2.0.1")
  await writeFile(own, JSON.stringify({ name: "x" }))
  expect(() => readAppVersion({ env: {}, packagePath: own })).toThrow()
  await writeFile(own, JSON.stringify({ name: "x", version: "../../etc" }))
  expect(() => readAppVersion({ env: {}, packagePath: own })).toThrow()
  expect(() => readAppVersion({ env: {}, packagePath: join(scratch, "absent.json") })).toThrow()
  expect(() => makeDownloads({ directory: scratch, version: "../1.0.0" })).toThrow()
})

test("the downloads answer is public and says nothing is published when the directory is missing or empty", async () => {
  const missing = await fixture({ directory: join(scratch, "never-created") })
  const response = await missing.request("/api/v1/downloads")
  expect(response.status).toBe(200)
  expect(response.headers.get("cache-control")).toBe("no-store")
  expect(await response.json()).toEqual({ version: VERSION, mac: null, iosAppStoreUrl: null })
  const empty = await fixture()
  expect(await empty.manifest()).toEqual({ version: VERSION, mac: null, iosAppStoreUrl: null })
  // A caller's credentials change nothing, right or wrong.
  expect(await (await empty.request("/api/v1/downloads", { headers: { Authorization: "Bearer wrong" } })).json()).toEqual({ version: VERSION, mac: null, iosAppStoreUrl: null })
  // Without its own configuration the handler offers the server's real version.
  const standard = makeApiHandler(makeMemoryRepository(), token)
  disposals.push(standard.dispose)
  const answer = await (await standard.handler(new Request("http://localhost/api/v1/downloads"))).json() as any
  expect(answer.version).toBe(readAppVersion())
})

test("the installer alone, then with its update archive: names, addresses, sizes and digests come from the files", async () => {
  const { directory, manifest } = await fixture({ iosAppStoreId: " 6471234567 " })
  const dmg = randomBytes(70_001), zip = randomBytes(65_537)
  // The update archive without the installer is not a published build.
  await writeFile(join(directory, ZIP), zip)
  expect((await manifest()).mac).toBeNull()
  await writeFile(join(directory, DMG), dmg)
  await rm(join(directory, ZIP))
  expect(await manifest()).toEqual({
    version: VERSION,
    mac: { arch: "arm64", minimumSystem: MAC_MINIMUM_SYSTEM, installer: { file: DMG, url: file(DMG), size: 70_001, sha256: sha256(dmg) }, update: null },
    iosAppStoreUrl: "https://apps.apple.com/app/id6471234567",
  })
  await writeFile(join(directory, ZIP), zip)
  expect((await manifest()).mac.update).toEqual({ file: ZIP, url: file(ZIP), size: 65_537, sha256: sha256(zip) })
  expect(MAC_MINIMUM_SYSTEM).toBe("14.2")
  for (const id of ["abc", "12 34", "", "https://apps.apple.com/app/id1"]) expect((await makeDownloads({ directory, version: VERSION, iosAppStoreId: id }).manifest()).iosAppStoreUrl).toBeNull()
  // A file that is empty (still being copied in) or a directory with the installer's name is not a build.
  const other = await fixture()
  await writeFile(join(other.directory, DMG), "")
  expect((await other.manifest()).mac).toBeNull()
  await rm(join(other.directory, DMG))
  await mkdir(join(other.directory, DMG))
  expect((await other.manifest()).mac).toBeNull()
  expect((await other.request(file(DMG))).status).toBe(404)
})

test("a published file is served whole with its type, length, name, digest and a short public cache; HEAD has no body", async () => {
  const { directory, request } = await fixture()
  const dmg = randomBytes(40_000), zip = randomBytes(1234)
  await writeFile(join(directory, DMG), dmg); await writeFile(join(directory, ZIP), zip)
  const response = await request(file(DMG))
  expect(response.status).toBe(200)
  expect(response.headers.get("content-type")).toBe("application/x-apple-diskimage")
  expect(response.headers.get("content-length")).toBe("40000")
  expect(response.headers.get("content-disposition")).toBe(`attachment; filename="${DMG}"`)
  expect(response.headers.get("accept-ranges")).toBe("bytes")
  expect(response.headers.get("etag")).toBe(`"${sha256(dmg)}"`)
  expect(response.headers.get("cache-control")).toBe("public, max-age=300")
  expect(response.headers.get("x-content-type-options")).toBe("nosniff")
  expect(Buffer.from(await response.arrayBuffer()).equals(dmg)).toBe(true)
  const archive = await request(file(ZIP))
  expect(archive.headers.get("content-type")).toBe("application/zip")
  expect(archive.headers.get("content-disposition")).toBe(`attachment; filename="${ZIP}"`)
  expect(Buffer.from(await archive.arrayBuffer()).equals(zip)).toBe(true)
  const head = await request(file(DMG), { method: "HEAD" })
  expect(head.status).toBe(200)
  expect(head.headers.get("content-length")).toBe("40000")
  expect(head.headers.get("etag")).toBe(`"${sha256(dmg)}"`)
  expect((await head.arrayBuffer()).byteLength).toBe(0)
  // The copy a client already has is not sent again.
  const unchanged = await request(file(DMG), { headers: { "If-None-Match": `"${sha256(dmg)}"` } })
  expect(unchanged.status).toBe(304)
  expect((await unchanged.arrayBuffer()).byteLength).toBe(0)
  expect((await request(file(DMG), { headers: { "If-None-Match": '"something-else"' } })).status).toBe(200)
  for (const method of ["POST", "PUT", "DELETE", "PATCH"]) {
    const refused = await request(file(DMG), { method })
    expect(refused.status).toBe(405)
    expect(refused.headers.get("allow")).toBe("GET, HEAD")
    expect(await refused.json()).toMatchObject({ error: "method_not_allowed" })
  }
})

test("one byte range is honoured; a range past the end is refused; anything else gets the whole file", async () => {
  const { directory, request } = await fixture()
  const dmg = randomBytes(10_000)
  await writeFile(join(directory, DMG), dmg)
  const ranged = async (range: string, extra: Record<string, string> = {}) => request(file(DMG), { headers: { Range: range, ...extra } })
  for (const [range, start, end] of [["bytes=0-99", 0, 99], ["bytes=100-", 100, 9999], ["bytes=-50", 9950, 9999], ["bytes=9999-9999", 9999, 9999], ["bytes=9000-20000", 9000, 9999], ["bytes=-20000", 0, 9999], ["bytes=0-0", 0, 0]] as const) {
    const response = await ranged(range)
    expect(response.status, range).toBe(206)
    expect(response.headers.get("content-range"), range).toBe(`bytes ${start}-${end}/10000`)
    expect(response.headers.get("content-length"), range).toBe(String(end - start + 1))
    expect(response.headers.get("etag")).toBe(`"${sha256(dmg)}"`)
    expect(Buffer.from(await response.arrayBuffer()).equals(dmg.subarray(start, end + 1)), range).toBe(true)
  }
  for (const range of ["bytes=10000-", "bytes=10000-10001", "bytes=99999-", "bytes=-0"]) {
    const response = await ranged(range)
    expect(response.status, range).toBe(416)
    expect(response.headers.get("content-range"), range).toBe("bytes */10000")
    expect((await response.arrayBuffer()).byteLength).toBe(0)
  }
  // Several ranges, another unit, a backwards or malformed range: the header is ignored.
  for (const range of ["bytes=0-1,5-6", "items=0-5", "bytes=5-2", "bytes=-", "bytes=a-b", "0-5"]) {
    const response = await ranged(range)
    expect(response.status, range).toBe(200)
    expect(response.headers.get("content-range"), range).toBeNull()
    expect((await response.arrayBuffer()).byteLength, range).toBe(10_000)
  }
  // A download is resumed only for the file the client already has part of.
  expect((await ranged("bytes=5000-", { "If-Range": `"${sha256(dmg)}"` })).status).toBe(206)
  const replaced = await ranged("bytes=5000-", { "If-Range": '"an-older-build"' })
  expect(replaced.status).toBe(200)
  expect((await replaced.arrayBuffer()).byteLength).toBe(10_000)
  const head = await request(file(DMG), { method: "HEAD", headers: { Range: "bytes=0-9" } })
  expect(head.status).toBe(206)
  expect(head.headers.get("content-length")).toBe("10")
  expect(parseRange(null, 10)).toBeNull()
  expect(parseRange("bytes=2-3", 10)).toEqual({ start: 2, end: 3 })
  expect(parseRange("bytes=99999999999999999999-", 10)).toBe("unsatisfiable")
})

test("only this version's two names exist: no listing, no other version, no other file, no way out of the directory", async () => {
  const { directory, request } = await fixture()
  await writeFile(join(directory, DMG), randomBytes(100))
  // Real files that must not be reachable: another version's build, a stray file, a file beside the directory.
  await writeFile(join(directory, "Cereal-3.4.4-mac-arm64.dmg"), "older build")
  await writeFile(join(directory, "Cereal-9.0.0-mac-arm64.zip"), "newer build")
  await writeFile(join(directory, "notes.txt"), "private")
  await writeFile(join(directory, ".env"), "SECRET=1")
  await mkdir(join(directory, "nested"))
  await writeFile(join(directory, "nested", DMG), "nested")
  await writeFile(join(scratch, "outside-secret.txt"), "outside")
  const paths = [
    "/api/v1/downloads/files", "/api/v1/downloads/files/", "/api/v1/downloads/files/.",
    file("Cereal-3.4.4-mac-arm64.dmg"), file("Cereal-9.0.0-mac-arm64.zip"), file("notes.txt"), file(".env"),
    file("nested"), file(`nested/${DMG}`), file(`nested%2F${DMG}`), file(`.%2F${DMG}`), file(`%2E%2F${DMG}`),
    file("..%2Foutside-secret.txt"), file("%2e%2e%2foutside-secret.txt"), file("..%5Coutside-secret.txt"), file("../outside-secret.txt"),
    file("..%2F..%2F..%2F..%2Fetc%2Fpasswd"), file("%2Fetc%2Fpasswd"), file(encodeURIComponent(join(scratch, "outside-secret.txt"))),
    file(`${DMG}%00.txt`), file(`${DMG}/`), file(`${DMG}/extra`), file(DMG.toUpperCase()), file(`${DMG}.sha256`), file("%E0%A4%A"), file(`${DMG}%20`),
    file(DMG.replace("3.4.5", "3.4.5%2F..%2F3.4.4")),
  ]
  for (const path of paths) {
    for (const method of ["GET", "HEAD"]) {
      const response = await request(path, { method })
      expect(response.status, `${method} ${path}`).toBe(404)
      expect(response.headers.get("content-type"), path).toContain("application/json")
      expect(response.headers.get("cache-control"), path).toBe("no-store")
      const text = await response.text()
      expect(text).not.toContain("private"); expect(text).not.toContain("outside"); expect(text).not.toContain("build")
      if (method === "GET") expect(JSON.parse(text).error, path).toBe("not_found")
    }
  }
  // ".." is resolved by the URL itself before any route sees it: this is the downloads answer, not a file.
  expect(await (await request("/api/v1/downloads/files/..")).json()).toMatchObject({ version: VERSION })
  // The real name still works, also percent-encoded, and unknown API paths still answer in JSON.
  expect((await request(file(DMG))).status).toBe(200)
  expect((await request(file(DMG.replaceAll("-", "%2D")))).status).toBe(200)
  const unknown = await request("/api/v1/downloadz")
  expect(unknown.status).toBe(404)
  expect(await unknown.json()).toEqual({ error: "not_found", message: "API route not found" })
})

test("a link that leads out of the downloads directory is neither listed nor served; one that stays inside is", async () => {
  const { directory, request, manifest } = await fixture()
  const secret = join(scratch, "secret-installer.dmg"), inside = randomBytes(2048)
  await writeFile(secret, "not for download")
  await symlink(secret, join(directory, DMG))
  expect((await manifest()).mac).toBeNull()
  expect((await request(file(DMG))).status).toBe(404)
  expect((await request(file(DMG), { method: "HEAD" })).status).toBe(404)
  // A link to a directory outside, and a dangling link.
  await rm(join(directory, DMG))
  await symlink(scratch, join(directory, DMG))
  expect((await request(file(DMG))).status).toBe(404)
  await rm(join(directory, DMG))
  await symlink(join(scratch, "does-not-exist"), join(directory, DMG))
  expect((await manifest()).mac).toBeNull()
  // The update archive pointing out while the installer is real: the installer is offered, the archive is not.
  await rm(join(directory, DMG))
  await writeFile(join(directory, DMG), inside)
  await symlink(secret, join(directory, ZIP))
  const listed = await manifest()
  expect(listed.mac.installer.sha256).toBe(sha256(inside))
  expect(listed.mac.update).toBeNull()
  expect((await request(file(ZIP))).status).toBe(404)
  // A link to a file that is itself inside the directory is a way to publish, and is served.
  await rm(join(directory, ZIP))
  await writeFile(join(directory, "build-77.zip"), inside)
  await symlink("build-77.zip", join(directory, ZIP))
  expect((await manifest()).mac.update).toEqual({ file: ZIP, url: file(ZIP), size: 2048, sha256: sha256(inside) })
  expect(Buffer.from(await (await request(file(ZIP))).arrayBuffer()).equals(inside)).toBe(true)
  // The directory itself may be a link (a mounted volume): what matters is where the file really is.
  const linked = join(scratch, "linked-directory")
  await symlink(directory, linked)
  const through = await fixture({ directory: linked })
  expect((await through.manifest()).mac.installer.sha256).toBe(sha256(inside))
  expect((await through.request(file(DMG))).status).toBe(200)
})

test("a digest is remembered for a file's path, size and modification time, and computed again when the file is replaced", async () => {
  const { directory, request, manifest } = await fixture()
  const path = join(directory, DMG), first = Buffer.alloc(5000, 1), second = Buffer.alloc(5000, 2), third = Buffer.alloc(7000, 3)
  const when = new Date("2026-05-01T10:00:00Z")
  await writeFile(path, first); await utimes(path, when, when)
  expect((await manifest()).mac.installer.sha256).toBe(sha256(first))
  // Same size and time, other bytes: the remembered digest is used, which is what shows it was remembered.
  await writeFile(path, second); await utimes(path, when, when)
  expect((await manifest()).mac.installer.sha256).toBe(sha256(first))
  // A new modification time is a new file: the digest, the ETag and the bytes all follow it.
  const later = new Date("2026-05-01T10:00:05Z")
  await utimes(path, later, later)
  expect((await manifest()).mac.installer.sha256).toBe(sha256(second))
  const served = await request(file(DMG))
  expect(served.headers.get("etag")).toBe(`"${sha256(second)}"`)
  expect(Buffer.from(await served.arrayBuffer()).equals(second)).toBe(true)
  // A new size, even at the same time.
  await writeFile(path, third); await utimes(path, later, later)
  expect((await manifest()).mac.installer).toMatchObject({ size: 7000, sha256: sha256(third) })
  expect((await request(file(DMG), { headers: { "If-None-Match": `"${sha256(second)}"` } })).status).toBe(200)
  // Removed: nothing is offered, and the old address is gone.
  await rm(path)
  expect((await manifest()).mac).toBeNull()
  expect((await request(file(DMG))).status).toBe(404)
  // Many readers at once share one computation and agree.
  await writeFile(path, third)
  const answers = await Promise.all(Array.from({ length: 8 }, () => manifest()))
  expect(new Set(answers.map(answer => answer.mac.installer.sha256))).toEqual(new Set([sha256(third)]))
})

test("over a real connection a 10 MB installer arrives intact with its length, and a range resumes it", async () => {
  const { directory, api } = await fixture()
  const dmg = randomBytes(10 * 1024 * 1024 + 123)
  await writeFile(join(directory, DMG), dmg)
  const server = Bun.serve({ port: 0, hostname: "127.0.0.1", fetch: request => api.handler(request) })
  disposals.unshift(() => { void server.stop(true) })
  const base = `http://127.0.0.1:${server.port}`
  const listed = await (await fetch(`${base}/api/v1/downloads`)).json() as any
  expect(listed.mac.installer).toEqual({ file: DMG, url: file(DMG), size: dmg.length, sha256: sha256(dmg) })
  const whole = await fetch(base + listed.mac.installer.url)
  expect(whole.status).toBe(200)
  expect(whole.headers.get("content-length")).toBe(String(dmg.length))
  expect(whole.headers.get("transfer-encoding")).toBeNull()
  expect(whole.headers.get("content-disposition")).toBe(`attachment; filename="${DMG}"`)
  expect(whole.headers.get("cache-control")).toBe("public, max-age=300")
  expect(sha256(new Uint8Array(await whole.arrayBuffer()))).toBe(listed.mac.installer.sha256)
  const head = await fetch(base + file(DMG), { method: "HEAD" })
  expect(head.headers.get("content-length")).toBe(String(dmg.length))
  // A download interrupted at 6 MB picks up from there.
  const rest = await fetch(base + file(DMG), { headers: { Range: `bytes=${6 * 1024 * 1024}-`, "If-Range": whole.headers.get("etag")! } })
  expect(rest.status).toBe(206)
  expect(rest.headers.get("content-range")).toBe(`bytes ${6 * 1024 * 1024}-${dmg.length - 1}/${dmg.length}`)
  expect(rest.headers.get("content-length")).toBe(String(dmg.length - 6 * 1024 * 1024))
  expect(Buffer.from(await rest.arrayBuffer()).equals(dmg.subarray(6 * 1024 * 1024))).toBe(true)
  const past = await fetch(base + file(DMG), { headers: { Range: `bytes=${dmg.length}-` } })
  expect(past.status).toBe(416)
  expect(past.headers.get("content-range")).toBe(`bytes */${dmg.length}`)
})
