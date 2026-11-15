import { afterAll, beforeAll, expect, test } from "bun:test"
import { mkdtemp, mkdir, realpath, rm, symlink, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { makeWebHandler } from "./static"

let directory: string
let handler: ReturnType<typeof makeWebHandler>
beforeAll(async () => {
  // Resolved, because the handler refuses files whose real path leaves its root and macOS temp directories are symlinks.
  directory = await realpath(await mkdtemp(join(tmpdir(), "meeting-web-")))
  await mkdir(join(directory, "public/assets"), { recursive: true })
  await writeFile(join(directory, "public/_shell.html"), "<main>Reader shell</main>")
  await writeFile(join(directory, "public/assets/app.js"), "export const reader=true")
  await writeFile(join(directory, "public/assets/app-large.js"), `export const words=${JSON.stringify("clarity ".repeat(600))}`)
  await writeFile(join(directory, "public/assets/face.ttf"), new Uint8Array(4096).fill(7))
  await writeFile(join(directory, "public/assets/photo.png"), new Uint8Array(4096).fill(9))
  await writeFile(join(directory, "public/mark.svg"), `<svg xmlns="http://www.w3.org/2000/svg">${"<path d=\"M0 0h1v1z\"/>".repeat(100)}</svg>`)
  await writeFile(join(directory, "secret.txt"), "not public")
  await symlink(join(directory, "secret.txt"), join(directory, "public/assets/escape.txt"))
  handler = makeWebHandler(join(directory, "public"))
})
afterAll(async () => { await rm(directory, { recursive: true, force: true }) })
test("a direct shared-note route returns only the SPA shell with privacy headers", async () => {
  const response = await handler(new Request("http://localhost/s/example-token"))
  expect(response.status).toBe(200)
  expect(await response.text()).toContain("Reader shell")
  expect(response.headers.get("Referrer-Policy")).toBe("no-referrer")
  expect(response.headers.get("Cache-Control")).toBe("no-store")
})
test("missing assets and symlinks cannot fall back to HTML or reveal source", async () => {
  for (const path of ["/assets/missing.js", "/assets/escape.txt", "/%2eenv", "/assets/%2e%2e/%2e%2e/secret.txt"]) {
    const response = await handler(new Request(`http://localhost${path}`))
    expect(response.status).toBe(404)
    expect(await response.text()).not.toContain("not public")
  }
})
test("HEAD and method restrictions preserve HTTP semantics", async () => {
  expect(await (await handler(new Request("http://localhost/s/example", { method: "HEAD" }))).text()).toBe("")
  expect((await handler(new Request("http://localhost/s/example", { method: "POST" }))).status).toBe(405)
})
test("fingerprinted assets, fonts included, are cached for good; everything else is never stored", async () => {
  for (const path of ["/assets/app.js", "/assets/app-large.js", "/assets/face.ttf", "/assets/photo.png"]) {
    expect((await handler(new Request(`http://localhost${path}`))).headers.get("Cache-Control")).toBe("public, max-age=31536000, immutable")
  }
  for (const path of ["/mark.svg", "/", "/notes", "/account"]) {
    expect((await handler(new Request(`http://localhost${path}`))).headers.get("Cache-Control")).toBe("no-store")
  }
})
test("compressible files are gzipped only for clients that accept it, and decode to the original", async () => {
  const gzip = { "Accept-Encoding": "br, gzip" }
  for (const path of ["/assets/app-large.js", "/assets/face.ttf", "/mark.svg"]) {
    const plain = await handler(new Request(`http://localhost${path}`))
    const packed = await handler(new Request(`http://localhost${path}`, { headers: gzip }))
    expect(plain.headers.get("Content-Encoding")).toBeNull()
    expect(plain.headers.get("Vary")).toBe("Accept-Encoding")
    expect(packed.headers.get("Content-Encoding")).toBe("gzip")
    expect(packed.headers.get("Vary")).toBe("Accept-Encoding")
    const original = new Uint8Array(await plain.arrayBuffer()), sent = new Uint8Array(await packed.arrayBuffer())
    expect(sent.byteLength).toBeLessThan(original.byteLength)
    expect(Number(packed.headers.get("Content-Length"))).toBe(sent.byteLength)
    expect(Bun.gunzipSync(sent)).toEqual(original)
    // The second request is served from the kept copy and must be identical.
    expect(new Uint8Array(await (await handler(new Request(`http://localhost${path}`, { headers: gzip }))).arrayBuffer())).toEqual(sent)
  }
})
test("small files, images and refused encodings are sent as they are", async () => {
  for (const [path, headers] of [["/assets/app.js", { "Accept-Encoding": "gzip" }], ["/assets/photo.png", { "Accept-Encoding": "gzip" }], ["/assets/app-large.js", { "Accept-Encoding": "gzip;q=0, br" }], ["/assets/app-large.js", { "Accept-Encoding": "identity" }]] as const) {
    expect((await handler(new Request(`http://localhost${path}`, { headers }))).headers.get("Content-Encoding")).toBeNull()
  }
  const head = await handler(new Request("http://localhost/assets/app-large.js", { method: "HEAD", headers: { "Accept-Encoding": "gzip" } }))
  expect(head.headers.get("Content-Encoding")).toBe("gzip")
  expect(await head.text()).toBe("")
})
