/** How a Cereal server is reached, judged from its address alone. Shared by the server (which reports it in
 * `GET /api/v1/auth/methods`) and the web app, so both say the same thing about the same address.
 *
 * - `secure`: HTTPS, or this machine itself (localhost, 127.0.0.0/8, ::1), where nothing crosses a network.
 * - `private`: plain HTTP to an address that only exists on a private network: RFC 1918 (10/8, 172.16/12,
 *   192.168/16), carrier-grade NAT and overlay networks (100.64/10), link-local (169.254/16, fe80::/10),
 *   IPv6 unique local (fc00::/7), names ending .local, .internal, .lan or .home.arpa, and names with no dot.
 * - `public-http`: any other plain HTTP address, and anything that cannot be read as an http(s) address.
 *
 * Pure: no DNS, no network. A name is judged as written, so "10.0.0.1.example.com" is public. */
export type Transport = "secure" | "private" | "public-http"

const PRIVATE_SUFFIXES = [".local", ".internal", ".lan", ".home.arpa"]

/** Four numbers of a dotted IPv4 address, or null. The URL parser has already turned hex, octal and
 * single-number spellings into this form. */
function ipv4(host: string): [number, number, number, number] | null {
  const match = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/.exec(host)
  if (!match) return null
  const parts = match.slice(1).map(Number) as [number, number, number, number]
  return parts.every(part => part <= 255) ? parts : null
}
function classifyIPv4([a, b]: [number, number, number, number]): Transport {
  if (a === 127) return "secure"
  if (a === 10 || (a === 172 && b >= 16 && b <= 31) || (a === 192 && b === 168) || (a === 100 && b >= 64 && b <= 127) || (a === 169 && b === 254)) return "private"
  return "public-http"
}
/** The sixteen-bit groups of an IPv6 address as the URL parser prints it (lower case, "::" for a run of zeros). */
function ipv6(host: string): number[] | null {
  if (!/^[0-9a-f:]+$/.test(host) || (host.match(/::/g) ?? []).length > 1) return null
  const [head = "", tail] = host.split("::")
  const groups = (text: string) => text ? text.split(":").map(group => /^[0-9a-f]{1,4}$/.test(group) ? parseInt(group, 16) : NaN) : []
  const left = groups(head), right = tail === undefined ? [] : groups(tail)
  if ([...left, ...right].some(Number.isNaN)) return null
  if (tail === undefined) return left.length === 8 ? left : null
  return left.length + right.length <= 7 ? [...left, ...new Array<number>(8 - left.length - right.length).fill(0), ...right] : null
}
function classifyIPv6(groups: number[]): Transport {
  const first = groups[0]!
  if (groups.slice(0, 7).every(group => group === 0) && groups[7] === 1) return "secure"
  // An IPv4 address carried in IPv6 (::ffff:a.b.c.d) is judged as that IPv4 address.
  if (groups.slice(0, 5).every(group => group === 0) && groups[5] === 0xffff) return classifyIPv4([groups[6]! >> 8, groups[6]! & 255, groups[7]! >> 8, groups[7]! & 255])
  if ((first & 0xfe00) === 0xfc00 || (first & 0xffc0) === 0xfe80) return "private"
  return "public-http"
}

export function classifyOrigin(address: string): Transport {
  let url: URL
  try { url = new URL(address) } catch { return "public-http" }
  if (url.protocol === "https:") return "secure"
  if (url.protocol !== "http:") return "public-http"
  // A trailing dot is the same name; brackets only mark an IPv6 literal.
  const host = url.hostname.toLowerCase().replace(/\.$/, "")
  if (!host) return "public-http"
  if (host.startsWith("[") && host.endsWith("]")) { const groups = ipv6(host.slice(1, -1)); return groups ? classifyIPv6(groups) : "public-http" }
  const address4 = ipv4(host)
  if (address4) return classifyIPv4(address4)
  // Anything else made only of digits and dots is not a name anyone was given.
  if (/^[\d.]+$/.test(host)) return "public-http"
  if (host === "localhost" || host.endsWith(".localhost")) return "secure"
  if (!host.includes(".") || PRIVATE_SUFFIXES.some(suffix => host.endsWith(suffix))) return "private"
  return "public-http"
}

/** Google only redirects to HTTPS addresses and to this machine, so Google sign-in and the Google Calendar
 * connection need an address that is `secure`. */
export const googleNeedsHttps = (address: string) => classifyOrigin(address) !== "secure"
/** What is said wherever Google cannot be turned on. */
export const GOOGLE_NEEDS_HTTPS = "Google sign-in needs an HTTPS address"
