/** The iPhone app that opens `/s/…` and `/notes/…` links of this server. */
const DEFAULT_IOS_APP_ID = "LZWN2FF7YB.dev.cereal.mobile"
/** The preview image shipped with the web app (apps/web/public), 1200 by 630. */
export const LINK_PREVIEW_IMAGE = "/link-preview.png"

/** GET /.well-known/apple-app-site-association: which app may open which paths. `appIds` is IOS_APP_IDS. */
export function appleAppSiteAssociation(request: Request, appIds: string | undefined): Response {
  const headers = { "Cache-Control": "public, max-age=3600", "X-Content-Type-Options": "nosniff" }
  if (request.method !== "GET" && request.method !== "HEAD") return new Response(null, { status: 405, headers: { ...headers, Allow: "GET, HEAD" } })
  const listed = (appIds ?? "").split(",").map(id => id.trim()).filter(id => /^[A-Z0-9]{10}\.[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+$/.test(id))
  const body = JSON.stringify({ applinks: { details: [{ appIDs: listed.length ? listed : [DEFAULT_IOS_APP_ID], components: [{ "/": "/s/*" }, { "/": "/notes/*" }] }] } })
  return new Response(request.method === "HEAD" ? null : body, { headers: { ...headers, "Content-Type": "application/json" } })
}

/** Text placed inside an HTML attribute: markup characters are escaped and control characters go. */
export function escapeAttribute(value: string): string {
  return value.replace(/[\u0000-\u001f\u007f\u2028\u2029]/g, " ").replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;").replace(/'/g, "&#39;")
}

const tag = (attribute: "property" | "name", key: string, content: string) => `<meta ${attribute}="${key}" content="${escapeAttribute(content)}"/>`

/** The `<head>` tags a messaging app reads to draw a preview of a note link. Reading a note, shared or not,
 * needs an account on this server, and the apps that fetch previews are not signed in: so every preview is
 * generic, and no title or text of any note is ever put in the tags. Returns null for every other path. */
export function makeLinkPreviews(options: {
  /** The public origin; previews need absolute URLs and must not trust the Host header. */
  origin: string
  /** The app's numeric App Store ID (set on the admin panel): adds the Smart App Banner when the app is on the App Store.
   * A function is asked for every page, so a change applies at once. */
  iosAppStoreId?: string | (() => Promise<string | null>)
}) {
  const valid = (id: string | null | undefined) => /^\d{1,20}$/.test(id?.trim() ?? "") ? id!.trim() : null
  return async (pathname: string): Promise<string | null> => {
    const shared = /^\/s\/[^/]+\/?$/.test(pathname), note = /^\/notes\/[^/]+(?:\/chat)?\/?$/.test(pathname)
    if (!shared && !note) return null
    const appStoreId = valid(typeof options.iosAppStoreId === "function" ? await options.iosAppStoreId().catch(() => null) : options.iosAppStoreId)
    return [
      tag("property", "og:site_name", "Cereal"), tag("property", "og:type", "article"),
      tag("property", "og:title", shared ? "Shared note" : "A note in Cereal"), tag("property", "og:description", shared ? "Sign in to view this note." : "Sign in to read this note."),
      tag("property", "og:image", options.origin + LINK_PREVIEW_IMAGE), tag("property", "og:image:width", "1200"), tag("property", "og:image:height", "630"),
      tag("name", "twitter:card", "summary_large_image"),
      ...(appStoreId ? [tag("name", "apple-itunes-app", `app-id=${appStoreId}, app-argument=${options.origin}${pathname}`)] : []),
    ].join("")
  }
}
