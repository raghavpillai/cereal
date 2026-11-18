/**
 * "Update server" on the admin panel (owner only): GET /api/v1/admin/update, POST …/update/check,
 * POST …/update/start and the live progress stream GET …/update/events.
 *
 * This server never updates itself and never holds the Docker socket. The work is done by
 * `scripts/updater.sh`, which runs on the host. The two meet in one folder, mounted into the container
 * (UPDATER_DIR, /data/updater):
 *
 *   requests/   this server drops a request file here: a timestamp, "check" or "update", and a SHA-256
 *               over the shared secret (UPDATER_TOKEN) and both. That is all it can say: the source of a
 *               new version is configured on the host and cannot be named from here.
 *   state       written by the updater: where it looks, what runs, what it last found.
 *   job         written by the updater: the update under way, or the last one, and how it ended.
 *   log         the lines of that update.
 *   heartbeat   touched by the updater every few seconds while it runs.
 *
 * Everything read from that folder is text to show, never something to act on. The files survive the
 * restart an update causes, so the page reconnects to the new server and reads how the update ended.
 */
import { createHash, randomBytes } from "node:crypto"
import { readFile, rename, stat, writeFile } from "node:fs/promises"
import { join } from "node:path"
import type { PrincipalData } from "@meeting-notes/contracts/api"

export type UpdaterState = "missing" | "stopped" | "ready"
export type UpdateRelation = "same" | "newer" | "older" | "different" | "unknown"
/** How an update ended, as the updater reports it; "running" while it has not. */
export type UpdateJobStatus = "running" | "deployed" | "up-to-date" | "rolled-back" | "rollback-failed" | "failed-no-previous" | "failed" | "interrupted"
export type UpdateStep = "fetch" | "preflight" | "backup" | "build" | "start" | "rollback"
export interface UpdateStatusDTO {
  /** What this server is: its own version, and the commit the updater says is deployed. */
  readonly current: { readonly version: string; readonly commit: string | null }
  readonly updater: {
    /** missing: not installed or not connected to this server. stopped: installed, but not running now. */
    readonly state: UpdaterState
    /** The git remote and ref a newer version is taken from; null until the updater is there. */
    readonly source: string | null
    readonly ref: string | null
  }
  /** What the updater found the last time it looked; null if it has not looked. */
  readonly available: null | { readonly checkedAt: string | null; readonly version: string | null; readonly commit: string | null; readonly relation: UpdateRelation | null; readonly error: string | null }
  /** The update under way, or the last one. */
  readonly job: null | {
    readonly id: string; readonly status: UpdateJobStatus; readonly step: UpdateStep | null; readonly active: boolean
    readonly startedAt: string | null; readonly finishedAt: string | null
    readonly from: { readonly version: string | null; readonly commit: string | null }
    readonly to: { readonly version: string | null; readonly commit: string | null }
    readonly error: string | null
  }
}
export interface UpdatesService {
  readonly status: () => Promise<UpdateStatusDTO>
  /** The log lines of the current or last update, from line `after` on. */
  readonly log: (after: number) => Promise<{ readonly lines: readonly string[]; readonly next: number }>
  /** Asks the updater to look, or to update. `requestedAt` is this server's clock, in seconds. */
  readonly request: (action: "check" | "update") => Promise<{ readonly ok: true; readonly requestedAt: number } | { readonly ok: false; readonly message: string }>
}
export interface UpdatesOptions {
  /** UPDATER_DIR: the folder shared with the updater. Unset: there is no updater. */
  readonly directory?: string | undefined
  /** UPDATER_TOKEN: the secret shared with the updater. */
  readonly token?: string | undefined
  /** This server's own version (`readAppVersion`). */
  readonly version: string
  readonly clock?: () => Date
}

const MAX_LINE = 2000, MAX_LINES = 5000, MAX_FILE = 4 * 1024 * 1024
/** Older than this, the heartbeat means the updater is not running. It touches the file every 2 seconds. */
const HEARTBEAT_STALE_MS = 30_000
const RELATIONS: readonly string[] = ["same", "newer", "older", "different", "unknown"]
const STATUSES: readonly string[] = ["running", "deployed", "up-to-date", "rolled-back", "rollback-failed", "failed-no-previous", "failed", "interrupted"]
const STEPS: readonly string[] = ["fetch", "preflight", "backup", "build", "start", "rollback"]

/** Lines of `key=value`, as the updater writes them. Unknown keys are kept and ignored. */
export function parseKeyValues(text: string): Record<string, string> {
  const values: Record<string, string> = {}
  for (const line of text.slice(0, 64 * 1024).split("\n")) {
    const at = line.indexOf("=")
    if (at > 0 && /^[a-z_]+$/.test(line.slice(0, at))) values[line.slice(0, at)] = line.slice(at + 1).trim().slice(0, 500)
  }
  return values
}
/** The signature of one request: only the two holders of the token can make it. */
export const requestSignature = (token: string, stamp: string, action: string) => createHash("sha256").update(`${token}:${stamp}:${action}`).digest("hex")
const commit = (value: string | undefined) => value && /^[0-9a-f]{7,64}$/.test(value) ? value : null
const text = (value: string | undefined) => value ? value : null
const time = (value: string | undefined) => value && /^\d{9,11}$/.test(value) ? new Date(Number(value) * 1000).toISOString() : null

export function makeUpdates(options: UpdatesOptions): UpdatesService {
  const { directory, token } = options, clock = options.clock ?? (() => new Date())
  const connected = !!directory && !!token && token.length >= 32
  const read = async (name: string) => { try { return (await readFile(join(directory!, name), "utf8")).slice(-MAX_FILE) } catch { return null } }
  let lastStamp = 0
  const status = async (): Promise<UpdateStatusDTO> => {
    const missing: UpdateStatusDTO = { current: { version: options.version, commit: null }, updater: { state: "missing", source: null, ref: null }, available: null, job: null }
    if (!connected) return missing
    const stateText = await read("state")
    if (stateText === null) return missing
    const state = parseKeyValues(stateText), jobText = await read("job"), job = jobText ? parseKeyValues(jobText) : null
    let beat = 0
    try { beat = (await stat(join(directory!, "heartbeat"))).mtimeMs } catch { /* never started */ }
    const jobStatus = job && STATUSES.includes(job.status ?? "") ? job.status as UpdateJobStatus : null
    return {
      current: { version: options.version, commit: commit(state.current_commit) },
      updater: { state: clock().getTime() - beat < HEARTBEAT_STALE_MS ? "ready" : "stopped", source: text(state.remote), ref: text(state.ref) },
      available: state.check_at ? { checkedAt: time(state.check_at), version: text(state.check_version), commit: commit(state.check_commit), relation: RELATIONS.includes(state.check_relation ?? "") ? state.check_relation as UpdateRelation : null, error: text(state.check_error) } : null,
      job: job && job.id && jobStatus ? {
        id: job.id, status: jobStatus, step: STEPS.includes(job.step ?? "") ? job.step as UpdateStep : null, active: jobStatus === "running",
        startedAt: time(job.started), finishedAt: time(job.finished),
        from: { version: text(job.from_version), commit: commit(job.from_commit) }, to: { version: text(job.to_version), commit: commit(job.to_commit) },
        error: text(job.error),
      } : null,
    }
  }
  return {
    status,
    log: async after => {
      const content = connected ? await read("log") : null
      if (!content) return { lines: [], next: 0 }
      const all = content.split("\n"); if (all.at(-1) === "") all.pop()
      const start = Math.max(0, Math.min(Number.isInteger(after) ? after : 0, all.length), all.length - MAX_LINES)
      return { lines: all.slice(start).map(line => line.slice(0, MAX_LINE)), next: all.length }
    },
    request: async action => {
      const now = await status()
      if (now.updater.state === "missing") return { ok: false, message: "The updater is not installed on this server." }
      if (now.updater.state === "stopped") return { ok: false, message: "The updater is installed but not running on the server." }
      if (now.job?.active) return { ok: false, message: "An update is already running." }
      if (action === "update" && !(now.available && !now.available.error && now.available.relation && now.available.relation !== "same" && now.available.relation !== "older"))
        return { ok: false, message: "There is no newer version to update to. Check for updates first." }
      // Strictly rising, so the updater can refuse a copy of an earlier request.
      const stamp = lastStamp = Math.max(clock().getTime(), lastStamp + 1)
      const name = `${stamp}-${randomBytes(6).toString("hex")}`, folder = join(directory!, "requests")
      try {
        await writeFile(join(folder, `${name}.tmp`), `${stamp}\n${action}\n${requestSignature(token!, String(stamp), action)}\n`, { mode: 0o644, flag: "wx" })
        await rename(join(folder, `${name}.tmp`), join(folder, `${name}.req`))
      } catch { return { ok: false, message: "The request could not be handed to the updater. Is its folder mounted into the server?" } }
      return { ok: true, requestedAt: Math.floor(stamp / 1000) }
    },
  }
}

export interface UpdateRouteAccess {
  /** Who is asking, as every other route decides it. */
  readonly resolve: (headers: Headers) => Promise<PrincipalData | null>
  readonly isTrustedOrigin: (origin: string) => boolean
}
const JSON_HEADERS = { "Content-Type": "application/json", "Cache-Control": "no-store", "X-Content-Type-Options": "nosniff", "Referrer-Policy": "no-referrer" }
const json = (status: number, body: unknown, extra: Record<string, string> = {}) => new Response(JSON.stringify(body), { status, headers: { ...JSON_HEADERS, ...extra } })
export const UPDATE_PATH = "/api/v1/admin/update"
/** How often the stream looks at the updater's files. */
const STREAM_TICK_MS = 700

/** The four routes. They sit outside the typed API because one of them is a stream that outlives requests
 * on either side of a restart, and because nothing but the admin panel calls them. */
export function makeUpdateRoutes(service: UpdatesService, access: UpdateRouteAccess, options: { tickMs?: number } = {}) {
  const tickMs = options.tickMs ?? STREAM_TICK_MS
  /** The streams that are open now, so that a server that is stopping can end them instead of waiting for them. */
  const open = new Set<() => void>()
  const handle = async (request: Request): Promise<Response> => {
    const url = new URL(request.url), route = url.pathname.slice(UPDATE_PATH.length)
    const reading = route === "" || route === "/events", acting = route === "/check" || route === "/start"
    if (!reading && !acting) return json(404, { error: "not_found", message: "API route not found" })
    if (request.method !== (reading ? "GET" : "POST")) return json(405, { error: "method_not_allowed", message: "Method not allowed for this API route" }, { Allow: reading ? "GET" : "POST" })
    let principal: PrincipalData | null
    try { principal = await access.resolve(new Headers(request.headers)) } catch { return json(503, { error: "unavailable", message: "Account service is temporarily unavailable" }) }
    if (!principal) return json(401, { error: "unauthorized", message: "Authentication required" })
    if (principal.role !== "owner") return json(403, { error: "forbidden", message: "Owner access required" })
    if (acting && !request.headers.has("authorization")) {
      const origin = request.headers.get("origin")
      if (!origin || !access.isTrustedOrigin(origin)) return json(403, { error: "forbidden", message: "A same-origin request is required" })
    }
    if (route === "") return json(200, await service.status())
    if (acting) {
      const result = await service.request(route === "/check" ? "check" : "update")
      return result.ok ? json(202, { requestedAt: result.requestedAt }) : json(409, { error: "conflict", message: result.message })
    }
    // The stream: `status` whenever it changes, `log` for every new line (its id is the line's number), and
    // `reset` when another update has started and the lines begin again. A page that reconnects says which
    // update it was showing and how many lines it has.
    const shownJob = url.searchParams.get("job") ?? "", shownLines = Number(url.searchParams.get("after") ?? 0)
    const encoder = new TextEncoder()
    let timer: ReturnType<typeof setTimeout> | undefined, closed = false, end = () => {}
    const stream = new ReadableStream<Uint8Array>({
      start(output) {
        let cursor = Number.isInteger(shownLines) && shownLines > 0 ? shownLines : 0, job: string | null = shownJob || null, lastStatus = "", quiet = 0
        const send = (event: string, data: unknown, id?: number) => output.enqueue(encoder.encode(`${id === undefined ? "" : `id: ${id}\n`}event: ${event}\ndata: ${JSON.stringify(data)}\n\n`))
        const tick = async () => {
          if (closed) return
          try {
            const status = await service.status(), current = status.job?.id ?? ""
            // An update's log starts empty: lines the page holds from another update do not belong to it.
            if (job !== null && job !== current && cursor > 0) { cursor = 0; send("reset", { job: current }) }
            job = current
            const serialized = JSON.stringify(status)
            if (serialized !== lastStatus) { lastStatus = serialized; send("status", status); quiet = 0 }
            const log = await service.log(cursor)
            if (log.next < cursor) { cursor = 0; send("reset", { job: current }) }
            else { log.lines.forEach((line, index) => send("log", { line }, log.next - log.lines.length + index + 1)); if (log.lines.length) quiet = 0; cursor = log.next }
            // A comment now and then keeps proxies and the server's idle timeout from closing a quiet stream.
            if (++quiet * tickMs >= 15_000) { quiet = 0; output.enqueue(encoder.encode(": waiting\n\n")) }
          } catch { /* the folder was unreadable this once: look again */ }
          if (!closed) timer = setTimeout(() => void tick(), tickMs)
        }
        end = () => { if (closed) return; closed = true; clearTimeout(timer); open.delete(end); try { output.close() } catch { /* already closed */ } }
        open.add(end)
        request.signal.addEventListener("abort", end, { once: true })
        void tick()
      },
      cancel() { closed = true; clearTimeout(timer); open.delete(end) },
    })
    return new Response(stream, { headers: { "Content-Type": "text/event-stream", "Cache-Control": "no-store", "X-Accel-Buffering": "no", "X-Content-Type-Options": "nosniff", "Referrer-Policy": "no-referrer" } })
  }
  /** Ends every open stream. An update restarts this server while its owner watches one: left open, the stream
   * would hold the old server up until Docker gave up on it. The page opens another on the new server. */
  return Object.assign(handle, { close: () => { for (const end of [...open]) end() } })
}
