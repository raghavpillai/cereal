// Puts a built Mac release where the running server offers it from, then asks the server to confirm.
//
//   bun --env-file=.env scripts/publish-release.ts [files…] [--server https://notes.example.com]
//
// Without files: dist/release/Cereal-<version>-mac-*.dmg and .zip, <version> being the one in package.json
// (scripts/release-macos.sh writes them there). Only the files named for that version are published: they
// are the only ones the server will offer.
//
// Where they go is the server's storage setting, read from its admin API with the owner token
// (ADMIN_API_TOKEN), or from the same environment fallback the server uses (STORAGE_BACKEND, S3_*) when the
// server cannot be asked:
//   - local disk: nothing is copied. The server reads its downloads directory (Compose mounts ./dist/release
//     there), so this only checks that the server now offers exactly these files.
//   - S3-compatible: each file is uploaded to <prefix>downloads/ with its SHA-256 recorded on the object.
//     The server never returns the secret access key, so it is read from S3_SECRET_ACCESS_KEY.
// Nothing here prints a credential.
import { createHash } from "node:crypto"
import { open, readdir, stat } from "node:fs/promises"
import { basename, resolve } from "node:path"
import { macBuildFiles } from "../apps/api/src/downloads"
import { makeS3Client, normalizeS3Endpoint, normalizeS3Prefix, validBucketName, type S3Config } from "../apps/api/src/s3"
import { readAppVersion } from "../apps/api/src/version"

const root = resolve(import.meta.dir, ".."), args = process.argv.slice(2)
const fail = (message: string): never => { console.error(message); process.exit(1) }
const option = (name: string) => { const index = args.indexOf(`--${name}`); if (index < 0) return undefined; const [, value] = args.splice(index, 2); return value ?? fail(`--${name} needs a value`) }
const server = (option("server") ?? process.env.PUBLIC_BASE_URL ?? `http://127.0.0.1:${process.env.PORT ?? "3000"}`).replace(/\/+$/, "")
const version = readAppVersion(), names = macBuildFiles(version), wanted = new Set([names.installer, names.update])

async function sha256Of(path: string): Promise<string> {
  const handle = await open(path, "r"), hash = createHash("sha256"), buffer = Buffer.allocUnsafe(1024 * 1024)
  try { for (let position = 0; ;) { const { bytesRead } = await handle.read(buffer, 0, buffer.length, position); if (!bytesRead) break; hash.update(buffer.subarray(0, bytesRead)); position += bytesRead } }
  finally { await handle.close() }
  return hash.digest("hex")
}
let paths = args.map(path => resolve(path))
if (!paths.length) {
  const directory = resolve(root, "dist/release")
  paths = (await readdir(directory).catch(() => [] as string[])).filter(name => wanted.has(name)).sort().map(name => resolve(directory, name))
  if (!paths.length) fail(`No build of version ${version} in ${directory} (expected ${names.installer}, and ${names.update} for updates). Run scripts/release-macos.sh first.`)
}
const files: Array<{ path: string; name: string; size: number; sha256: string }> = []
for (const path of paths) {
  const name = basename(path)
  if (!wanted.has(name)) fail(`${name} is not a build of version ${version}: the server only offers ${names.installer} and ${names.update}`)
  const info = await stat(path).catch(() => null)
  if (!info?.isFile() || info.size === 0) fail(`${path} is not a file`)
  files.push({ path, name, size: info!.size, sha256: await sha256Of(path) })
}
if (!files.some(file => file.name === names.installer)) console.warn(`Note: ${names.installer} is not among the files; the server offers nothing for the Mac until the installer is published.`)

// Where the server keeps its files: ask it, or fall back to the environment it would read.
interface Storage { backend: "local" | "s3"; source: string; endpoint: string | null; region: string | null; bucket: string | null; prefix: string; forcePathStyle: boolean; accessKeyId: string | null }
const environment = (): Storage => ({ backend: process.env.STORAGE_BACKEND?.trim().toLowerCase() === "s3" ? "s3" : "local", source: "this environment", endpoint: process.env.S3_ENDPOINT?.trim() || null, region: process.env.S3_REGION?.trim() || null,
  bucket: process.env.S3_BUCKET?.trim() || null, prefix: process.env.S3_PREFIX ?? "", forcePathStyle: /^(?:1|true|yes|on)$/i.test(process.env.S3_FORCE_PATH_STYLE?.trim() ?? ""), accessKeyId: process.env.S3_ACCESS_KEY_ID?.trim() || null })
let storage: Storage
const owner = process.env.ADMIN_API_TOKEN
try {
  if (!owner) throw new Error("no owner token")
  const response = await fetch(`${server}/api/v1/admin/settings`, { headers: { authorization: `Bearer ${owner}` }, signal: AbortSignal.timeout(15_000) })
  if (!response.ok) throw new Error(`the server answered ${response.status}`)
  const settings = await response.json() as { storage?: Storage }
  if (!settings.storage) throw new Error("the server does not report its storage")
  storage = { ...settings.storage, source: `${server} (${settings.storage.source})` }
} catch (error) {
  storage = environment()
  console.warn(`Could not read the storage settings from ${server} (${error instanceof Error ? error.message : "no answer"}); using the environment instead.`)
}
console.log(`Version ${version}; storage: ${storage.backend === "s3" ? `S3-compatible bucket ${storage.bucket ?? "?"}${storage.prefix ? `, prefix ${storage.prefix}` : ""}` : "local disk"} (from ${storage.source})`)

if (storage.backend === "s3") {
  const secretAccessKey = process.env.S3_SECRET_ACCESS_KEY?.trim()
  if (!storage.bucket || !validBucketName(storage.bucket) || !storage.accessKeyId) fail("The storage settings have no bucket or access key ID")
  if (!secretAccessKey) fail("Set S3_SECRET_ACCESS_KEY for this command: the server keeps the secret access key sealed and never returns it")
  const config: S3Config = { endpoint: storage.endpoint ? normalizeS3Endpoint(storage.endpoint) : null, region: storage.region || "us-east-1", bucket: storage.bucket!, prefix: normalizeS3Prefix(storage.prefix), forcePathStyle: storage.forcePathStyle, accessKeyId: storage.accessKeyId!, secretAccessKey: secretAccessKey! }
  const client = makeS3Client(config)
  for (const file of files) {
    const key = `downloads/${file.name}`
    try {
      // The file is streamed from disk; its digest travels as the object's `sha256` metadata.
      await client.put(key, Bun.file(file.path), { sha256: file.sha256, contentType: file.name.endsWith(".dmg") ? "application/x-apple-diskimage" : "application/zip" })
      const stored = await client.head(key)
      if (!stored || stored.size !== file.size || stored.sha256 !== file.sha256) fail(`${file.name} was uploaded but the bucket does not report it as sent`)
    } catch (error) { fail(`Uploading ${file.name} failed: ${error instanceof Error ? error.message : "unknown error"}`) }
    console.log(`Uploaded ${file.name} to ${config.bucket}/${config.prefix}${key} (${file.size} bytes, sha256 ${file.sha256})`)
  }
} else {
  for (const file of files) console.log(`${file.name} (${file.size} bytes, sha256 ${file.sha256}): local storage, nothing to upload`)
}

// What matters: the running server now offers exactly these bytes.
const answer = await fetch(`${server}/api/v1/downloads`, { signal: AbortSignal.timeout(120_000) }).then(async response => response.ok ? await response.json() as { version: string; mac: null | { installer: { file: string; size: number; sha256: string }; update: null | { file: string; size: number; sha256: string } } } : null, () => null)
if (!answer) fail(`Published, but ${server}/api/v1/downloads did not answer, so it is not confirmed that the server offers the build`)
if (answer!.version !== version) fail(`The server at ${server} is version ${answer!.version}, not ${version}: it will not offer these files. Deploy version ${version} first.`)
const offered = [answer!.mac?.installer, answer!.mac?.update].filter((file): file is { file: string; size: number; sha256: string } => !!file)
let confirmed = true
for (const file of files) {
  const match = offered.find(candidate => candidate.file === file.name)
  if (match && match.size === file.size && match.sha256 === file.sha256) console.log(`Confirmed: ${server} offers ${file.name} (${match.size} bytes, sha256 ${match.sha256})`)
  else {
    confirmed = false
    console.error(match ? `${server} offers a different ${file.name} (sha256 ${match.sha256}); replace the copy the server reads`
      : file.name === names.update && !answer!.mac ? `${server} does not offer ${file.name}: the update archive is only offered once the installer is published`
      : `${server} does not offer ${file.name}${storage.backend === "local" ? ": copy it into the directory the server reads builds from (DOWNLOADS_HOST_DIR, ./dist/release by default)" : ""}`)
  }
}
if (!confirmed) process.exit(1)
