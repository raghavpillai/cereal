import { afterAll, afterEach, beforeAll, expect, test } from "bun:test"
import { chmod, cp, lstat, mkdir, mkdtemp, readFile, readdir, readlink, realpath, rm, stat, utimes, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join, resolve } from "node:path"
import { makeApiHandler } from "../src/api"
import { bootstrapPrincipal } from "../src/auth"
import { makeMemoryRepository } from "../src/repository"
import { makeUpdateRoutes, makeUpdates, parseKeyValues, requestSignature, UPDATE_PATH, type UpdateStatusDTO, type UpdatesService } from "../src/updates"

const TOKEN = "updater-token-with-at-least-32-characters-0123"
const repoRoot = resolve(import.meta.dir, "../../..")
let scratch = ""
beforeAll(async () => { scratch = await realpath(await mkdtemp(join(tmpdir(), "cereal-updates-"))) })
afterAll(async () => { await rm(scratch, { recursive: true, force: true }) })
const disposals: Array<() => Promise<void> | void> = []
afterEach(async () => { for (const dispose of disposals.splice(0)) await dispose() })

let counter = 0
/** A folder as the updater leaves it, written by hand. */
async function spool(files: Record<string, string> = {}, options: { beat?: boolean } = {}) {
  const directory = join(scratch, `spool-${counter++}`)
  await mkdir(join(directory, "requests"), { recursive: true })
  for (const [name, content] of Object.entries(files)) await writeFile(join(directory, name), content)
  if (options.beat !== false) await writeFile(join(directory, "heartbeat"), "")
  return directory
}
const STATE = "protocol=1\nremote=git@example.test:team/cereal.git\nref=main\ncurrent_commit=1111111111111111111111111111111111111111\ncurrent_version=1.0.0\n"
const FOUND = STATE + "check_at=1763450000\ncheck_commit=2222222222222222222222222222222222222222\ncheck_version=1.1.0\ncheck_relation=newer\ncheck_error=\n"

test("without a folder, a token or the updater's files, the panel is told there is no updater", async () => {
  const absent: UpdateStatusDTO = { current: { version: "1.0.0", commit: null }, updater: { state: "missing", source: null, ref: null }, available: null, job: null }
  expect(await makeUpdates({ version: "1.0.0" }).status()).toEqual(absent)
  expect(await makeUpdates({ version: "1.0.0", directory: await spool({ state: STATE }), token: "short" }).status()).toEqual(absent)
  // The folder Compose mounts before `updater.sh install` has run: empty.
  const empty = makeUpdates({ version: "1.0.0", directory: await spool({}, { beat: false }), token: TOKEN })
  expect(await empty.status()).toEqual(absent)
  expect(await empty.request("check")).toEqual({ ok: false, message: "The updater is not installed on this server." })
  expect(await empty.log(0)).toEqual({ lines: [], next: 0 })
})

test("the updater's files become the status: source, what was found, and the last update", async () => {
  const directory = await spool({
    state: FOUND,
    job: "id=20261118T101500Z-2222222\nstatus=rolled-back\nstep=rollback\nstarted=1763460000\nfinished=1763460300\nfrom_commit=1111111111111111111111111111111111111111\nfrom_version=1.0.0\nto_commit=2222222222222222222222222222222222222222\nto_version=1.1.0\nerror=The new version did not start. The previous version is running again.\nhelper_lines=9\n",
    log: "10:15:00 Looking for main…\n10:15:02 Found version 1.1.0\n",
  })
  const updates = makeUpdates({ version: "1.0.0", directory, token: TOKEN })
  expect(await updates.status()).toEqual({
    current: { version: "1.0.0", commit: "1111111111111111111111111111111111111111" },
    updater: { state: "ready", source: "git@example.test:team/cereal.git", ref: "main" },
    available: { checkedAt: new Date(1763450000_000).toISOString(), version: "1.1.0", commit: "2222222222222222222222222222222222222222", relation: "newer", error: null },
    job: { id: "20261118T101500Z-2222222", status: "rolled-back", step: "rollback", active: false, startedAt: new Date(1763460000_000).toISOString(), finishedAt: new Date(1763460300_000).toISOString(), from: { version: "1.0.0", commit: "1111111111111111111111111111111111111111" }, to: { version: "1.1.0", commit: "2222222222222222222222222222222222222222" }, error: "The new version did not start. The previous version is running again." },
  })
  expect(await updates.log(0)).toEqual({ lines: ["10:15:00 Looking for main…", "10:15:02 Found version 1.1.0"], next: 2 })
  expect(await updates.log(1)).toEqual({ lines: ["10:15:02 Found version 1.1.0"], next: 2 })
  expect(await updates.log(9)).toEqual({ lines: [], next: 2 })
  // What the updater writes is shown, never trusted: a status, step, relation or commit it does not know is dropped.
  await writeFile(join(directory, "state"), STATE + "check_at=1763450000\ncheck_commit=$(reboot)\ncheck_relation=sideways\ncheck_version=1.1.0\n")
  await writeFile(join(directory, "job"), "id=x\nstatus=exploded\n")
  const odd = await updates.status()
  expect(odd.available).toEqual({ checkedAt: new Date(1763450000_000).toISOString(), version: "1.1.0", commit: null, relation: null, error: null })
  expect(odd.job).toBeNull()
  expect(parseKeyValues("a=1\nB=2\n=3\nno equals\nkey_two= spaced \n")).toEqual({ a: "1", key_two: "spaced" })
})

test("a heartbeat that stopped means the updater is not running, and nothing is asked of it", async () => {
  const directory = await spool({ state: FOUND })
  const old = new Date(Date.now() - 5 * 60_000)
  await utimes(join(directory, "heartbeat"), old, old)
  const updates = makeUpdates({ version: "1.0.0", directory, token: TOKEN })
  expect((await updates.status()).updater.state).toBe("stopped")
  expect(await updates.request("update")).toEqual({ ok: false, message: "The updater is installed but not running on the server." })
  expect(await readdir(join(directory, "requests"))).toEqual([])
})

test("a request is one signed file, and only an update that has something to install is asked for", async () => {
  const directory = await spool({ state: FOUND })
  let now = 1763470000_123
  const updates = makeUpdates({ version: "1.0.0", directory, token: TOKEN, clock: () => new Date(now) })
  await utimes(join(directory, "heartbeat"), new Date(now), new Date(now))
  expect(await updates.request("update")).toEqual({ ok: true, requestedAt: 1763470000 })
  expect(await updates.request("check")).toEqual({ ok: true, requestedAt: 1763470000 })
  const files = (await readdir(join(directory, "requests"))).sort()
  expect(files).toHaveLength(2)
  expect(files.every(name => /^\d{13}-[0-9a-f]{12}\.req$/.test(name))).toBe(true)
  const [stamp, action, signature, rest] = (await readFile(join(directory, "requests", files[0]!), "utf8")).split("\n")
  expect([stamp, action, rest]).toEqual(["1763470000123", "update", ""])
  expect(signature).toBe(requestSignature(TOKEN, "1763470000123", "update"))
  // The token itself is never written where the rest of the machine can read it.
  expect(await readFile(join(directory, "requests", files[0]!), "utf8")).not.toContain(TOKEN)
  // The second request in the same millisecond still has a later timestamp.
  expect(files[1]!.startsWith("1763470000124-")).toBe(true)

  for (const [relation, extra] of [["same", ""], ["older", ""], ["newer", "check_error=Could not fetch main\n"]] as const) {
    const other = makeUpdates({ version: "1.0.0", directory: await spool({ state: FOUND.replace("check_relation=newer", `check_relation=${relation}`).replace("check_error=\n", extra) }), token: TOKEN })
    expect(await other.request("update")).toEqual({ ok: false, message: "There is no newer version to update to. Check for updates first." })
    expect((await other.request("check")).ok).toBe(true)
  }
  const busy = makeUpdates({ version: "1.0.0", directory: await spool({ state: FOUND, job: "id=j\nstatus=running\nstep=build\nstarted=1763460000\n" }), token: TOKEN })
  expect((await busy.status()).job).toMatchObject({ active: true, step: "build" })
  expect(await busy.request("update")).toEqual({ ok: false, message: "An update is already running." })
  expect(await busy.request("check")).toEqual({ ok: false, message: "An update is already running." })
  now += 1
})

const owner = { ...bootstrapPrincipal, role: "owner" as const }
function routes(service: UpdatesService) {
  const api = makeApiHandler(makeMemoryRepository(), "api-token-for-update-routes-at-least-32", {
    updates: service,
    principalResolver: async headers => headers.get("authorization") === "Bearer owner" || headers.get("cookie") === "session=owner" ? owner
      : headers.get("authorization") === "Bearer admin" ? { ...owner, role: "admin" as const } : headers.get("authorization") === "Bearer user" ? { ...owner, role: "user" as const } : null,
  })
  disposals.push(api.dispose)
  return (path: string, init: RequestInit = {}) => api.handler(new Request(`http://localhost${path}`, init))
}

test("the update routes are the owner's alone", async () => {
  const directory = await spool({ state: FOUND }), request = routes(makeUpdates({ version: "1.0.0", directory, token: TOKEN }))
  for (const [path, method] of [["", "GET"], ["/events", "GET"], ["/check", "POST"], ["/start", "POST"]] as const) {
    expect((await request(UPDATE_PATH + path, { method })).status).toBe(401)
    for (const role of ["admin", "user"]) {
      const response = await request(UPDATE_PATH + path, { method, headers: { Authorization: `Bearer ${role}` } })
      expect(response.status).toBe(403)
      expect(await response.json()).toEqual({ error: "forbidden", message: "Owner access required" })
    }
  }
  // A browser session must come from this origin to change anything; reading needs no origin.
  expect((await request(UPDATE_PATH + "/start", { method: "POST", headers: { Cookie: "session=owner", Origin: "https://elsewhere.example" } })).status).toBe(403)
  expect((await request(UPDATE_PATH + "/start", { method: "POST", headers: { Cookie: "session=owner" } })).status).toBe(403)
  expect(await readdir(join(directory, "requests"))).toEqual([])
  expect((await request(UPDATE_PATH, { headers: { Cookie: "session=owner" } })).status).toBe(200)
  expect((await request(UPDATE_PATH + "/start", { headers: { Authorization: "Bearer owner" } })).status).toBe(405)
  expect((await request(UPDATE_PATH + "/elsewhere", { headers: { Authorization: "Bearer owner" } })).status).toBe(404)

  const status = await request(UPDATE_PATH, { headers: { Authorization: "Bearer owner" } })
  expect(status.headers.get("cache-control")).toBe("no-store")
  expect(await status.json()).toMatchObject({ current: { version: "1.0.0" }, updater: { state: "ready", ref: "main" }, available: { relation: "newer", version: "1.1.0" } })
  const started = await request(UPDATE_PATH + "/start", { method: "POST", headers: { Authorization: "Bearer owner" } })
  expect(started.status).toBe(202)
  expect(typeof (await started.json() as { requestedAt: number }).requestedAt).toBe("number")
  expect(await readdir(join(directory, "requests"))).toHaveLength(1)
})

test("a server with no updater answers the owner plainly and refuses to start anything", async () => {
  const request = routes(makeUpdates({ version: "1.0.0" }))
  expect(await (await request(UPDATE_PATH, { headers: { Authorization: "Bearer owner" } })).json()).toEqual({ current: { version: "1.0.0", commit: null }, updater: { state: "missing", source: null, ref: null }, available: null, job: null })
  const refused = await request(UPDATE_PATH + "/start", { method: "POST", headers: { Authorization: "Bearer owner" } })
  expect(refused.status).toBe(409)
  expect(await refused.json()).toEqual({ error: "conflict", message: "The updater is not installed on this server." })
})

/** Reads server-sent events until `enough` says so. */
async function events(response: Response, enough: (seen: Array<{ event: string; data: any; id?: string }>) => boolean, during?: (seen: Array<{ event: string; data: any }>) => Promise<void>) {
  const reader = response.body!.getReader(), decoder = new TextDecoder(), seen: Array<{ event: string; data: any; id?: string }> = []
  let buffer = ""
  const limit = Date.now() + 8000
  while (!enough(seen) && Date.now() < limit) {
    const part = await reader.read()
    if (part.done) break
    buffer += decoder.decode(part.value, { stream: true })
    let end: number
    while ((end = buffer.indexOf("\n\n")) >= 0) {
      const block = buffer.slice(0, end); buffer = buffer.slice(end + 2)
      const event = /^event: (.*)$/m.exec(block)?.[1], data = /^data: (.*)$/m.exec(block)?.[1], id = /^id: (.*)$/m.exec(block)?.[1]
      if (event && data) seen.push({ event, data: JSON.parse(data), ...(id ? { id } : {}) })
    }
    if (during) await during(seen)
  }
  await reader.cancel()
  return seen
}

test("the progress stream sends the status, every log line once, and starts again when another update does", async () => {
  const job = (id: string, status: string) => `id=${id}\nstatus=${status}\nstep=build\nstarted=1763460000\n`
  const directory = await spool({ state: FOUND, job: job("one", "running"), log: "first\nsecond\n" })
  const handler = makeUpdateRoutes(makeUpdates({ version: "1.0.0", directory, token: TOKEN }), { resolve: async () => owner, isTrustedOrigin: () => true }, { tickMs: 20 })
  const open = (query = "") => { const abort = new AbortController(); disposals.push(() => abort.abort()); return handler(new Request(`http://localhost${UPDATE_PATH}/events${query}`, { signal: abort.signal })) }

  const response = await open()
  expect(response.headers.get("content-type")).toBe("text/event-stream")
  expect(response.headers.get("x-accel-buffering")).toBe("no")
  let appended = false, restarted = false
  const seen = await events(response, all => all.some(e => e.event === "log" && e.data.line === "fresh"), async all => {
    if (!appended && all.some(e => e.data.line === "second")) { appended = true; await writeFile(join(directory, "log"), "first\nsecond\nthird\n") }
    // Another update begins: its log starts empty, then fills.
    if (appended && !restarted && all.some(e => e.data.line === "third")) { restarted = true; await writeFile(join(directory, "log"), ""); await writeFile(join(directory, "job"), job("two", "running")); await Bun.sleep(80); await writeFile(join(directory, "log"), "fresh\n") }
  })
  expect(seen[0]).toMatchObject({ event: "status", data: { job: { id: "one", active: true } } })
  expect(seen.filter(e => e.event === "log").map(e => [e.id, e.data.line])).toEqual([["1", "first"], ["2", "second"], ["3", "third"], ["1", "fresh"]])
  const reset = seen.findIndex(e => e.event === "reset")
  expect(reset).toBeGreaterThan(seen.findIndex(e => e.data.line === "third"))
  expect(reset).toBeLessThan(seen.findIndex(e => e.data.line === "fresh"))

  // A server that is stopping ends the streams itself: the page is not what keeps it alive.
  const held = (await open()).body!.getReader()
  await held.read()
  handler.close()
  const ended = await Promise.race([(async () => { for (;;) { if ((await held.read()).done) return "ended" } })(), Bun.sleep(1000).then(() => "still open")])
  expect(ended).toBe("ended")

  // A page that reconnects after the restart says what it has: the same update continues where it was…
  await writeFile(join(directory, "job"), job("two", "deployed")); await writeFile(join(directory, "log"), "fresh\nmore\ndone\n")
  const resumed = await events(await open("?job=two&after=1"), all => all.some(e => e.data.line === "done"))
  expect(resumed.filter(e => e.event !== "status").map(e => [e.event, e.data.line])).toEqual([["log", "more"], ["log", "done"]])
  expect(resumed.find(e => e.event === "status")?.data.job).toMatchObject({ id: "two", status: "deployed", active: false })
  // …and one that was showing another update gets this one from its first line.
  const other = await events(await open("?job=one&after=2"), all => all.some(e => e.data.line === "done"))
  expect(other.filter(e => e.event !== "status").map(e => e.event === "reset" ? "reset" : e.data.line)).toEqual(["reset", "fresh", "more", "done"])
})

// ── The real updater (scripts/updater.sh and scripts/release-helper.sh), with a fake `docker` ─────────────
// A local git repository stands for the private one, a folder for the deployment. Nothing here talks to a
// Docker daemon or to another machine: `docker` on the PATH of every process below is test/fake-docker.sh.
const run = async (command: string[], options: { cwd?: string; env?: Record<string, string> } = {}) => {
  const child = Bun.spawn(command, { cwd: options.cwd, env: options.env ?? process.env as Record<string, string>, stdout: "pipe", stderr: "pipe" })
  const [out, err, code] = await Promise.all([new Response(child.stdout).text(), new Response(child.stderr).text(), child.exited])
  return { out, err, code }
}
const must = async (command: string[], options: { cwd?: string; env?: Record<string, string> } = {}) => {
  const result = await run(command, options)
  if (result.code !== 0) throw new Error(`${command.join(" ")} failed (${result.code}): ${result.err}${result.out}`)
  return result.out.trim()
}
async function until<T>(read: () => Promise<T>, done: (value: T) => boolean, what: string, ms = 30_000): Promise<T> {
  const limit = Date.now() + ms
  let value = await read()
  while (!done(value)) {
    if (Date.now() > limit) throw new Error(`Timed out waiting for ${what}: ${JSON.stringify(value)}`)
    await Bun.sleep(100)
    value = await read()
  }
  return value
}

test("the updater installs a newer commit, goes back when it does not start, and ignores requests it did not get from the server", async () => {
  const root = join(scratch, "host"), home = join(root, "home"), origin = join(root, "origin"), work = join(root, "work"), deployment = join(home, "cereal")
  const fake = join(root, "fake-docker"), bin = join(root, "bin")
  await mkdir(join(fake, "images"), { recursive: true }); await mkdir(bin, { recursive: true }); await mkdir(home, { recursive: true })
  await cp(join(repoRoot, "apps/api/test/fake-docker.sh"), join(bin, "docker")); await chmod(join(bin, "docker"), 0o755)
  await writeFile(join(fake, "running"), "local\n"); await writeFile(join(fake, "health"), "healthy\n"); await writeFile(join(fake, "images", "local"), "")
  const env = {
    PATH: `${bin}:/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:/usr/local/bin`, HOME: home, FAKE_DOCKER: fake, DOCKER_HOST: "unix:///nonexistent/docker.sock",
    CEREAL_UPDATER_POLL: "0.1", CEREAL_HEALTH_TRIES: "3", CEREAL_HEALTH_SLEEP: "0.1", LC_ALL: "C",
    GIT_CONFIG_GLOBAL: "/dev/null", GIT_CONFIG_SYSTEM: "/dev/null", GIT_AUTHOR_NAME: "t", GIT_AUTHOR_EMAIL: "t@example.test", GIT_COMMITTER_NAME: "t", GIT_COMMITTER_EMAIL: "t@example.test",
  }
  expect(await must(["which", "docker"], { env })).toBe(join(bin, "docker"))

  // The "private repository": version 1.0.0, with the scripts as they are in this checkout.
  await must(["git", "init", "--quiet", "--bare", "--initial-branch=main", origin], { env })
  await must(["git", "clone", "--quiet", origin, work], { env })
  await mkdir(join(work, "scripts"))
  for (const name of ["updater.sh", "release-helper.sh"]) await cp(join(repoRoot, "scripts", name), join(work, "scripts", name))
  await writeFile(join(work, "compose.yaml"), "name: meeting-notes\nservices: {}\n")
  const commit = async (version: string, files: Record<string, string | null> = {}) => {
    await writeFile(join(work, "package.json"), JSON.stringify({ name: "cereal", version }, null, 2) + "\n")
    for (const [name, content] of Object.entries(files)) content === null ? await rm(join(work, name), { force: true }) : await writeFile(join(work, name), content)
    await must(["git", "add", "-A"], { cwd: work, env }); await must(["git", "commit", "--quiet", "-m", `version ${version}`], { cwd: work, env })
    await must(["git", "push", "--quiet", "origin", "HEAD:main"], { cwd: work, env })
    return must(["git", "rev-parse", "HEAD"], { cwd: work, env })
  }
  const first = await commit("1.0.0")

  // The deployment as scripts/setup.sh leaves it: a plain folder with a private .env, running `first`.
  await mkdir(deployment)
  await must(["sh", "-c", `git --git-dir="${origin}" archive ${first} | tar -x -C "${deployment}"`], { env })
  await writeFile(join(deployment, ".env"), "POSTGRES_PASSWORD=secret-database-password\nADMIN_API_TOKEN=secret-setup-code\nPUBLIC_BASE_URL=https://notes.example.test\n", { mode: 0o600 })
  await writeFile(join(deployment, "DEPLOYED_COMMIT"), first + "\n")
  const updater = join(repoRoot, "scripts/updater.sh"), spoolDir = join(home, "cereal-releases/.updater/spool")

  // Before it is installed there is nothing to run.
  expect((await run(["bash", updater, "check", "--dir", deployment], { env })).err).toContain("not installed")
  const installed = await run(["bash", updater, "install", origin, "main", "--dir", deployment], { env })
  expect(installed.code).toBe(0)
  expect(installed.out).toContain("main in " + origin + " is version 1.0.0")
  expect(installed.out).toContain("docker compose up -d")
  const dotenv = await readFile(join(deployment, ".env"), "utf8")
  expect(dotenv).toContain("POSTGRES_PASSWORD=secret-database-password\nADMIN_API_TOKEN=secret-setup-code\nPUBLIC_BASE_URL=https://notes.example.test\n")
  expect(dotenv).toContain(`UPDATER_DIR=${spoolDir}\n`)
  const token = /^UPDATER_TOKEN=([0-9a-f]{64})$/m.exec(dotenv)?.[1] ?? ""
  expect(token).toHaveLength(64)
  expect((await stat(join(deployment, ".env"))).mode & 0o777).toBe(0o600)
  expect((await stat(join(home, "cereal-releases/.updater/config"))).mode & 0o777).toBe(0o600)
  expect((await stat(join(home, "cereal-releases/.updater"))).mode & 0o777).toBe(0o700)
  expect((await stat(join(spoolDir, "requests"))).mode & 0o7777).toBe(0o1733)
  // What the server's container reads, as another user, is readable by it; the secret beside it is not.
  for (const name of ["state", "log"]) expect((await stat(join(spoolDir, name))).mode & 0o777).toBe(0o644)
  // Installing again keeps the token the running server already holds.
  expect((await run(["bash", updater, "install", origin, "main", "--dir", deployment], { env })).code).toBe(0)
  expect(await readFile(join(deployment, ".env"), "utf8")).toBe(dotenv)
  expect(await readFile(join(home, "cereal-releases/.updater/cereal-updater.service"), "utf8")).toContain(`ExecStart=/bin/bash ${deployment}/scripts/updater.sh run --dir ${deployment}`)
  // Installing started nothing and called Docker for nothing.
  expect(await readFile(join(fake, "calls"), "utf8").catch(() => "")).toBe("")

  const updates = makeUpdates({ version: "1.0.0", directory: spoolDir, token })
  expect((await updates.status()).updater.state).toBe("stopped")
  const service = Bun.spawn(["bash", updater, "run", "--dir", deployment], { env, stdout: "ignore", stderr: "ignore" })
  disposals.push(async () => { service.kill(); await service.exited; await run(["pkill", "-f", `updater.sh run --dir ${deployment}`]) })
  let status = await until(() => updates.status(), s => s.updater.state === "ready" && s.available?.relation === "same", "the updater to start and look")
  expect(status).toMatchObject({ current: { commit: first }, updater: { source: origin, ref: "main" }, available: { version: "1.0.0", commit: first, error: null }, job: null })
  expect(await updates.request("update")).toEqual({ ok: false, message: "There is no newer version to update to. Check for updates first." })

  // A newer commit is pushed. "Check for updates" finds it.
  const second = await commit("1.1.0")
  expect((await updates.request("check")).ok).toBe(true)
  status = await until(() => updates.status(), s => s.available?.commit === second, "the check to find 1.1.0")
  expect(status.available).toMatchObject({ version: "1.1.0", relation: "newer", error: null })
  expect(await readdir(join(spoolDir, "requests"))).toEqual([])

  // Files that are not the server's requests: a wrong signature, a right signature over another action, and
  // (below) a copy of a request that was already used. None of them starts anything.
  const stamp = String(Date.now() + 5)
  await writeFile(join(spoolDir, "requests", "forged.req"), `${stamp}\nupdate\n${"0".repeat(64)}\n`)
  await writeFile(join(spoolDir, "requests", "swapped.req"), `${stamp}\nupdate\n${requestSignature(token, stamp, "check")}\n`)
  await writeFile(join(spoolDir, "requests", "shell.req"), `${stamp}\nupdate; touch ${join(root, "pwned")}\n${requestSignature(token, stamp, "update")}\n`)
  await until(() => readdir(join(spoolDir, "requests")), names => names.length === 0, "the forged requests to be thrown away")
  await Bun.sleep(300)
  expect((await updates.status()).job).toBeNull()
  expect(await Bun.file(join(root, "pwned")).exists()).toBe(false)

  // "Update server": 1.0.0 → 1.1.0.
  const before = Date.now()
  expect((await updates.request("update")).ok).toBe(true)
  status = await until(() => updates.status(), s => !!s.job && !s.job.active, "the update to finish", 60_000)
  expect(status.job).toMatchObject({ status: "deployed", error: null, from: { version: "1.0.0", commit: first }, to: { version: "1.1.0", commit: second } })
  expect(status.job!.id).toMatch(new RegExp(`^\\d{8}T\\d{6}Z-${second.slice(0, 7)}$`))
  for (const name of ["state", "job", "log"]) expect((await stat(join(spoolDir, name))).mode & 0o777).toBe(0o644)
  const releaseTwo = await readlink(deployment)
  expect(releaseTwo).toBe(`cereal-releases/${status.job!.id}`)
  expect((await readFile(join(deployment, "DEPLOYED_COMMIT"), "utf8")).trim()).toBe(second)
  expect(JSON.parse(await readFile(join(deployment, "package.json"), "utf8")).version).toBe("1.1.0")
  // Same settings: the private .env went with it, untouched, and so did the updater's two lines.
  expect(await readFile(join(deployment, ".env"), "utf8")).toBe(dotenv)
  expect((await readFile(join(fake, "running"), "utf8")).trim()).toBe(second.slice(0, 12))
  const backups = await readdir(join(home, "cereal-backups"))
  expect(backups).toHaveLength(1)
  expect(await readFile(join(home, "cereal-backups", backups[0]!), "utf8")).toContain("FAKE DATABASE DUMP while running local")
  const firstLog = (await updates.log(0)).lines.join("\n")
  for (const line of ["Looking for main", "Found version 1.1.0", "Checking the server", "Backing up the database", "database backup:", "moved the deployment into", "Building the image meeting-notes:" + second.slice(0, 12), "Starting the new version", "The new version is healthy.", "Updated to 1.1.0"])
    expect(firstLog).toContain(line)
  // In the order of scripts/deploy-ssh.sh: dump, build, and only then the switch.
  const calls = (await readFile(join(fake, "calls"), "utf8")).split("\n").map(line => line.split(" | ")[1] ?? "")
  const at = (text: string) => calls.findIndex(call => call.startsWith(text))
  expect(at("compose exec -T db pg_dump")).toBeGreaterThan(-1)
  expect(at("compose build")).toBeGreaterThan(at("compose exec -T db pg_dump"))
  expect(at("compose up -d --remove-orphans")).toBeGreaterThan(at("compose build"))
  // Afterwards it looks again, and carries on as the updater of the release that now runs.
  status = await until(() => updates.status(), s => s.available?.relation === "same" && s.current.commit === second && s.updater.state === "ready", "the updater to settle after the update")
  expect((await run(["pgrep", "-f", `${deployment}/scripts/updater.sh run --dir ${deployment}`])).out.trim()).not.toBe("")

  // A request with a genuine signature but a timestamp no later than the last one used (a copy of an earlier
  // request, dropped again by someone else on the machine): thrown away.
  const earlier = String(before - 1000)
  await writeFile(join(spoolDir, "requests", "replayed.req"), `${earlier}\nupdate\n${requestSignature(token, earlier, "update")}\n`)
  await until(() => readdir(join(spoolDir, "requests")), names => names.length === 0, "the replayed request to be thrown away")
  await Bun.sleep(300)
  expect((await updates.status()).job!.id).toBe(status.job!.id)

  // 1.2.0 starts but never becomes healthy: the previous release is put back.
  const third = await commit("1.2.0", { UNHEALTHY: "this version starts and never answers\n" })
  expect((await updates.request("check")).ok).toBe(true)
  await until(() => updates.status(), s => s.available?.commit === third, "the check to find 1.2.0")
  expect((await updates.request("update")).ok).toBe(true)
  status = await until(() => updates.status(), s => s.job?.to.commit === third && !s.job.active, "the failed update to finish", 60_000)
  expect(status.job).toMatchObject({ status: "rolled-back", step: "rollback", error: "The new version did not start. The previous version is running again.", from: { version: "1.1.0" }, to: { version: "1.2.0" } })
  expect(await readlink(deployment)).toBe(releaseTwo)
  expect((await readFile(join(fake, "running"), "utf8")).trim()).toBe(second.slice(0, 12))
  expect((await readFile(join(fake, "health"), "utf8")).trim()).toBe("healthy")
  const secondLog = (await updates.log(0)).lines.join("\n")
  for (const line of ["The new version did not become healthy.", "Putting the previous release back", "The previous release is running again.", "The database backup taken before this update:"]) expect(secondLog).toContain(line)
  expect(secondLog).not.toContain("Updated to 1.1.0")
  // The version on offer is still there to try again once it is fixed.
  status = await until(() => updates.status(), s => s.available?.relation === "newer" && s.available.commit === third, "the updater to look again")
  expect(status.current.commit).toBe(second)

  // 1.2.1 does not build: the running server is never touched.
  const fourth = await commit("1.2.1", { UNHEALTHY: null, BUILD_FAILS: "x\n" })
  expect((await updates.request("check")).ok).toBe(true)
  await until(() => updates.status(), s => s.available?.commit === fourth, "the check to find 1.2.1")
  const upsBefore = (await readFile(join(fake, "calls"), "utf8")).split("compose up").length
  expect((await updates.request("update")).ok).toBe(true)
  status = await until(() => updates.status(), s => s.job?.to.commit === fourth && !s.job.active, "the build failure", 60_000)
  expect(status.job).toMatchObject({ status: "failed", step: "build", error: "The image did not build. Nothing changed." })
  expect((await updates.log(0)).lines.join("\n")).toContain("this release does not build")
  expect(await readlink(deployment)).toBe(releaseTwo)
  expect((await readFile(join(fake, "calls"), "utf8")).split("compose up").length).toBe(upsBefore)
  expect((await readdir(join(home, "cereal-releases"))).filter(name => name.includes(fourth.slice(0, 7)))).toEqual([])

  // 1.3.0 is fine again.
  const fifth = await commit("1.3.0", { BUILD_FAILS: null })
  expect((await updates.request("check")).ok).toBe(true)
  await until(() => updates.status(), s => s.available?.commit === fifth, "the check to find 1.3.0")
  expect((await updates.request("update")).ok).toBe(true)
  status = await until(() => updates.status(), s => s.job?.to.commit === fifth && !s.job.active, "the last update", 60_000)
  expect(status.job).toMatchObject({ status: "deployed", from: { version: "1.1.0" }, to: { version: "1.3.0" } })
  expect((await lstat(deployment)).isSymbolicLink()).toBe(true)
  expect((await readFile(join(deployment, "DEPLOYED_COMMIT"), "utf8")).trim()).toBe(fifth)
  expect(await readFile(join(deployment, ".env"), "utf8")).toBe(dotenv)
  // Every call went to the fake, and each was one it knows.
  expect(await readFile(join(fake, "calls"), "utf8")).not.toContain("unexpected")
}, 180_000)
