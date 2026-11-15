import { afterAll, beforeAll, expect, test } from "bun:test"
import { mkdtemp, realpath, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { appleAppSiteAssociation, escapeAttribute, makeLinkPreviews } from "../src/app-links"
import { makeWebHandler } from "../src/static"

const token = "A".repeat(43)
const previews = makeLinkPreviews({ origin: "https://notes.example.test" })
const meta = (html: string, key: string) => new RegExp(`<meta (?:property|name)="${key}" content="([^"]*)"/>`).exec(html)?.[1]

test("a note link previews with the site name, a generic title and an absolute image: never anything from the note", async () => {
  const shared = (await previews("/s/" + token))!
  expect(meta(shared, "og:site_name")).toBe("Cereal")
  expect(meta(shared, "og:type")).toBe("article")
  expect(meta(shared, "og:title")).toBe("Shared note")
  expect(meta(shared, "og:description")).toBe("Sign in to view this note.")
  expect(meta(shared, "og:image")).toBe("https://notes.example.test/link-preview.png")
  expect(meta(shared, "twitter:card")).toBe("summary_large_image")
  expect(shared).not.toContain("apple-itunes-app")
  // The preview is the same for every shared link: a real token, an unknown one and a malformed one.
  // There is nothing to look up, so a crawler without an account cannot tell which links exist.
  for (const path of ["/s/" + "Z".repeat(43), "/s/too-short", "/s/" + token + "extra", "/s/" + token + "/"]) expect(await previews(path)).toBe(shared)
  const privateNote = (await previews("/notes/3f0a1b7e-6f0e-4a55-9f7c-0d2c7f1f6a10"))!
  expect(meta(privateNote, "og:title")).toBe("A note in Cereal")
  expect(meta(privateNote, "og:description")).toBe("Sign in to read this note.")
  for (const path of ["/", "/notes", "/login", "/admin", "/s", "/s/a/b", "/notes/a/b", "/assets/app.js", "/api/v1/shared/" + token]) expect(await previews(path)).toBeNull()
})

test("nothing in the tags comes from the request path, and attribute text is escaped", async () => {
  const html = (await previews(`/s/"><script>alert(1)<script>`))!
  expect(html).toBe((await previews("/s/" + token))!)
  expect(html).not.toMatch(/<(?!meta (?:property|name)="[a-z:_-]+" content="[^"<>]*"\/>)/)
  expect(escapeAttribute(`a&b<c>d"e'f\ng\u2028h`)).toBe("a&amp;b&lt;c&gt;d&quot;e&#39;f g h")
})

test("the Smart App Banner appears only with a numeric App Store id, and its link is escaped", async () => {
  const withApp = makeLinkPreviews({ origin: "https://notes.example.test", iosAppStoreId: " 6471234567 " })
  expect(meta((await withApp("/notes/abc"))!, "apple-itunes-app")).toBe("app-id=6471234567, app-argument=https://notes.example.test/notes/abc")
  expect((await withApp(`/s/x"onload="y`))!).toContain(`app-argument=https://notes.example.test/s/x&quot;onload=&quot;y"/>`)
  const bad = makeLinkPreviews({ origin: "https://notes.example.test", iosAppStoreId: `1" onload="x` })
  expect((await bad("/notes/abc"))!).not.toContain("apple-itunes-app")
})

test("the app site association lists the app and the two link paths, as JSON, without redirect or sign-in", async () => {
  const response = appleAppSiteAssociation(new Request("https://notes.example.test/.well-known/apple-app-site-association"), undefined)
  expect(response.status).toBe(200)
  expect(response.headers.get("Content-Type")).toBe("application/json")
  expect(await response.json()).toEqual({ applinks: { details: [{ appIDs: ["LZWN2FF7YB.dev.cereal.mobile"], components: [{ "/": "/s/*" }, { "/": "/notes/*" }] }] } })
  const custom = appleAppSiteAssociation(new Request("https://notes.example.test/.well-known/apple-app-site-association"), " ABCDE12345.com.example.one , not an id, FGHIJ67890.com.example.two ")
  expect((await custom.json()).applinks.details[0].appIDs).toEqual(["ABCDE12345.com.example.one", "FGHIJ67890.com.example.two"])
  expect((await appleAppSiteAssociation(new Request("https://notes.example.test/.well-known/apple-app-site-association"), "nonsense").json()).applinks.details[0].appIDs).toEqual(["LZWN2FF7YB.dev.cereal.mobile"])
  expect(await appleAppSiteAssociation(new Request("https://notes.example.test/.well-known/apple-app-site-association", { method: "HEAD" }), undefined).text()).toBe("")
  expect(appleAppSiteAssociation(new Request("https://notes.example.test/.well-known/apple-app-site-association", { method: "POST" }), undefined).status).toBe(405)
})

let directory: string
beforeAll(async () => {
  directory = await realpath(await mkdtemp(join(tmpdir(), "meeting-web-previews-")))
  await writeFile(join(directory, "_shell.html"), `<!DOCTYPE html><html><head><title>Cereal</title></head><body><main>Reader shell ${"clarity ".repeat(300)}</main></body></html>`)
})
afterAll(async () => { await rm(directory, { recursive: true, force: true }) })

test("the web shell carries the tags inside its head for link paths only, compressed or not, with the same privacy headers", async () => {
  const handler = makeWebHandler(directory, { head: previews })
  const shared = await handler(new Request("http://localhost/s/" + token))
  const html = await shared.text()
  expect(shared.status).toBe(200)
  expect(html.indexOf(`<meta property="og:title" content="Shared note"/>`)).toBeGreaterThan(html.indexOf("<head>"))
  expect(html.indexOf(`<meta property="og:title"`)).toBeLessThan(html.indexOf("</head>"))
  expect(html).toContain("Reader shell")
  expect(shared.headers.get("Cache-Control")).toBe("no-store"); expect(shared.headers.get("Referrer-Policy")).toBe("no-referrer"); expect(shared.headers.get("X-Content-Type-Options")).toBe("nosniff")
  expect(shared.headers.get("Content-Type")).toBe("text/html; charset=utf-8")
  const zipped = await handler(new Request("http://localhost/s/" + token, { headers: { "Accept-Encoding": "gzip" } }))
  expect(zipped.headers.get("Content-Encoding")).toBe("gzip")
  expect(new TextDecoder().decode(Bun.gunzipSync(new Uint8Array(await zipped.arrayBuffer())))).toBe(html)
  expect(await (await handler(new Request("http://localhost/s/" + token, { method: "HEAD" }))).text()).toBe("")
  expect(await (await handler(new Request("http://localhost/notes/abc"))).text()).toContain(`content="A note in Cereal"`)
  // Every other page is the untouched shell, and a failing preview never breaks the page.
  for (const path of ["/", "/notes", "/login"]) expect(await (await handler(new Request("http://localhost" + path))).text()).not.toContain("og:title")
  const failing = makeWebHandler(directory, { head: async () => { throw new Error("preview failed") } })
  const fallback = await failing(new Request("http://localhost/s/" + token))
  expect(fallback.status).toBe(200); expect(await fallback.text()).toContain("Reader shell")
})
