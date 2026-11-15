// One product version, four places. The server reads it from the root package.json and offers the app
// builds named for it (apps/api/src/downloads.ts); the desktop and iPhone apps are built with their own
// copies. This fails when they disagree, so a release cannot ship an app the server will not offer.
//   bun scripts/check-version.ts [repository root]
import { readFileSync } from "node:fs"
import { resolve } from "node:path"
import { MAC_MINIMUM_SYSTEM } from "../apps/api/src/downloads"
import { APP_VERSION_PATTERN } from "../apps/api/src/version"

const root = resolve(process.argv[2] ?? resolve(import.meta.dir, ".."))
const read = (path: string) => readFileSync(resolve(root, path), "utf8")
const only = (path: string, what: string, found: ReadonlyArray<string>) => {
  const distinct = [...new Set(found)]
  if (distinct.length !== 1) throw new Error(`${path}: expected one ${what}, found ${distinct.length ? distinct.join(", ") : "none"}`)
  return distinct[0]!
}
const plist = (source: string, key: string) => [...source.matchAll(new RegExp(`<key>${key}</key>\\s*<string>([^<]*)</string>`, "g"))].map(match => match[1]!)

try {
  const cargo = read("apps/desktop/Cargo.toml"), info = read("apps/desktop/assets/Info.plist"), project = read("apps/ios/project.yml")
  // The [package] table only: dependencies have versions of their own.
  const packageTable = /^\[package\]\s*$([\s\S]*?)(?=^\[|(?![\s\S]))/m.exec(cargo)?.[1] ?? ""
  const places: Array<[where: string, version: string]> = [
    ["package.json (version)", only("package.json", "version", [String((JSON.parse(read("package.json")) as { version?: unknown }).version ?? "")].filter(Boolean))],
    ["apps/desktop/Cargo.toml ([package] version)", only("apps/desktop/Cargo.toml", "[package] version", [...packageTable.matchAll(/^version\s*=\s*"([^"]*)"/gm)].map(match => match[1]!))],
    ["apps/desktop/assets/Info.plist (CFBundleShortVersionString)", only("apps/desktop/assets/Info.plist", "CFBundleShortVersionString", plist(info, "CFBundleShortVersionString"))],
    ["apps/ios/project.yml (MARKETING_VERSION)", only("apps/ios/project.yml", "MARKETING_VERSION", [...project.matchAll(/^\s*MARKETING_VERSION:\s*["']?([^"'\s#]+)["']?/gm)].map(match => match[1]!))],
  ]
  const expected = places[0]![1]
  if (!APP_VERSION_PATTERN.test(expected)) throw new Error(`package.json: "${expected}" is not a version such as 1.2.3`)
  const different = places.filter(([, version]) => version !== expected)
  if (different.length) throw new Error(`The product version must be the same everywhere. package.json says ${expected}, but:\n${different.map(([where, version]) => `  ${where} says ${version}`).join("\n")}`)
  // The downloads answer tells people which macOS they need; the app's own declaration is the truth.
  const minimum = only("apps/desktop/assets/Info.plist", "LSMinimumSystemVersion", plist(info, "LSMinimumSystemVersion"))
  if (minimum !== MAC_MINIMUM_SYSTEM) throw new Error(`apps/desktop/assets/Info.plist needs macOS ${minimum}, but the server tells people ${MAC_MINIMUM_SYSTEM} (MAC_MINIMUM_SYSTEM in apps/api/src/downloads.ts)`)
  console.log(`Product version ${expected} in all ${places.length} places; macOS ${minimum} or later`)
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error))
  process.exit(1)
}
