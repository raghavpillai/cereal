import { realpath } from "node:fs/promises"
import { extname, resolve, sep } from "node:path"

const securityHeaders = {
  "Referrer-Policy": "no-referrer",
  "X-Content-Type-Options": "nosniff",
  "X-Frame-Options": "DENY",
  "X-Robots-Tag": "noindex, nofollow, noarchive",
}

const compressible = /^(?:text\/|application\/(?:javascript|json|manifest\+json|xml)|image\/svg\+xml|font\/(?:ttf|otf))/
const acceptsGzip = (request: Request) => /(?:^|,)\s*gzip\s*(?:;\s*q=(?!0(?:\.0+)?\s*(?:,|$))[^,]*)?(?:,|$)/i.test(request.headers.get("Accept-Encoding") ?? "")
/** Fingerprinted assets never change, so each is compressed once and kept. Bounded by the size of one build. */
const compressed = new Map<string, Uint8Array<ArrayBuffer>>()

/** Text, scripts, styles, SVG and uncompressed fonts are several times smaller gzipped. */
async function body(request: Request, file: ReturnType<typeof Bun.file>, type: string, remember: string | null): Promise<{ body: BodyInit | null; headers: Record<string, string> }> {
  const head = request.method === "HEAD"
  if (!compressible.test(type) || file.size < 1024) return { body: head ? null : file, headers: {} }
  const headers: Record<string, string> = { Vary: "Accept-Encoding" }
  if (!acceptsGzip(request)) return { body: head ? null : file, headers }
  let bytes = remember ? compressed.get(remember) : undefined
  if (!bytes) {
    bytes = Bun.gzipSync(new Uint8Array(await file.arrayBuffer())) as Uint8Array<ArrayBuffer>
    if (remember && compressed.size < 2048) compressed.set(remember, bytes)
  }
  return { body: head ? null : bytes, headers: { ...headers, "Content-Encoding": "gzip", "Content-Length": String(bytes.byteLength) } }
}

/** Serve the read-only Start SPA without exposing application source or API fallbacks.
 * `head` returns ready-made, already escaped tags for a client route (link previews), added to the shell's `<head>`. */
export function makeWebHandler(directory: string, options: { head?: (pathname: string) => Promise<string | null> } = {}) {
  const root = resolve(directory)
  return async (request: Request): Promise<Response> => {
    if (request.method !== "GET" && request.method !== "HEAD") {
      return new Response("Method not allowed", { status: 405, headers: { ...securityHeaders, Allow: "GET, HEAD" } })
    }
    let pathname: string
    try { pathname = decodeURIComponent(new URL(request.url).pathname) }
    catch { return new Response("Bad request", { status: 400, headers: securityHeaders }) }
    if (pathname.includes("\0") || pathname.includes("\\") || pathname.split("/").some(part => part.startsWith("."))) {
      return new Response("Not found", { status: 404, headers: securityHeaders })
    }
    const candidate = resolve(root, `.${pathname}`)
    if (candidate !== root && !candidate.startsWith(root + sep)) {
      return new Response("Not found", { status: 404, headers: securityHeaders })
    }
    try {
      const actual = await realpath(candidate)
      if (actual.startsWith(root + sep)) {
        const file = Bun.file(actual)
        if (await file.exists() && !(await file.stat()).isDirectory()) {
          // Only fingerprinted files under /assets/ (scripts, styles, fonts, images) are cached, and for good.
          const hashed = pathname.startsWith("/assets/")
          const content = await body(request, file, file.type, hashed ? actual : null)
          const headers = { ...securityHeaders, "Content-Type": file.type, "Cache-Control": hashed ? "public, max-age=31536000, immutable" : "no-store", ...content.headers }
          return new Response(content.body, { headers })
        }
      }
    } catch { /* Missing paths may be client routes, but never missing assets. */ }
    if (pathname.startsWith("/assets/") || extname(pathname)) {
      return new Response("Not found", { status: 404, headers: securityHeaders })
    }
    const shell = Bun.file(resolve(root, "_shell.html"))
    if (!await shell.exists()) return new Response("Web reader is not built", { status: 503, headers: securityHeaders })
    const headers = { ...securityHeaders, "Content-Type": "text/html; charset=utf-8", "Cache-Control": "no-store" }
    const tags = options.head ? await options.head(pathname).catch(() => null) : null
    if (tags) {
      const html = await shell.text(), end = html.indexOf("</head>")
      // A shell without a head is served as it is rather than guessed at.
      if (end >= 0) {
        const page = new TextEncoder().encode(html.slice(0, end) + tags + html.slice(end)), gzip = acceptsGzip(request)
        const bytes = gzip ? Bun.gzipSync(page) as Uint8Array<ArrayBuffer> : page
        return new Response(request.method === "HEAD" ? null : bytes, { headers: { ...headers, Vary: "Accept-Encoding", ...(gzip ? { "Content-Encoding": "gzip" } : {}), "Content-Length": String(bytes.byteLength) } })
      }
    }
    const content = await body(request, shell, "text/html", null)
    return new Response(content.body, { headers: { ...headers, ...content.headers } })
  }
}
