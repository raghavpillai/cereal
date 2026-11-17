/**
 * How long starting and stopping a recording takes in the desktop app, stage by stage.
 *
 * Runs the API with live transcription in this process (scripts/live/stack.ts: in-memory storage, the
 * fake recogniser unless `--provider elevenlabs`), then launches the debug desktop app with an isolated
 * data directory and a spoken fixture as its microphone (`CEREAL_CAPTURE_FIXTURE`), and has it press
 * Record and Stop by itself (`CEREAL_LATENCY_BENCH`, apps/desktop/src/latency.rs). Both sides write a
 * trace; this lines them up and prints the median and 95th percentile of every stage, cold (the first
 * recording after launch) and warm.
 *
 *   cargo build -p cereal
 *   bun scripts/live/latency-bench.ts --out .local/ship/record-latency/before --cold 20 --warm 20
 *   RUN_LIVE_PROVIDER_TESTS=1 bun --env-file=.env scripts/live/latency-bench.ts --provider elevenlabs ...
 *
 * Options: --provider fake|elevenlabs  --cold <launches of one round>  --warm <rounds in one launch>
 *          --delay <ms added to every HTTP request and upgrade>  --binary <path>  --speak <seconds>
 *          --visible (show the window, never focused; the default keeps it off screen)  --summarise-only
 *          --append --first-launch <n> (add launches to an earlier run in the same directory)
 */
import { spawn, spawnSync } from "node:child_process"
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { join, resolve } from "node:path"

const args = process.argv.slice(2)
const flag = (name: string) => args.includes(`--${name}`)
const value = (name: string) => { const index = args.indexOf(`--${name}`); return index >= 0 ? args[index + 1] : undefined }
const out = resolve(value("out") ?? ".local/ship/record-latency/run")
const provider = value("provider") === "elevenlabs" ? "elevenlabs" : "fake"
if (provider === "elevenlabs" && process.env.RUN_LIVE_PROVIDER_TESTS !== "1") { console.error("The real provider costs money. Set RUN_LIVE_PROVIDER_TESTS=1 to run it."); process.exit(2) }
const cold = Number(value("cold") ?? 20), warm = Number(value("warm") ?? 20), delay = Number(value("delay") ?? 0)
const binary = resolve(value("binary") ?? "target/debug/cereal")
mkdirSync(out, { recursive: true })
const clientTrace = join(out, "client.jsonl"), serverTrace = join(out, "server.jsonl")

if (!flag("summarise-only")) {
  // `--append` adds launches to an earlier run in the same directory.
  if (!flag("append")) for (const file of [clientTrace, serverTrace]) rmSync(file, { force: true })
  // The server's trace is read when its module loads: set it first.
  process.env.LIVE_LATENCY_TRACE = serverTrace
  const { startLiveStack } = await import("./stack")
  const stack = await startLiveStack({ provider, requestDelayMs: delay })
  // Speech made on this Mac (no microphone is recorded): about a minute, from its first sample.
  const scratch = mkdtempSync(join(tmpdir(), "cereal-latency-"))
  const fixture = join(scratch, "speech.wav")
  const text = "Thanks for joining everyone. Let's review the roadmap and decide which features ship first. The beta ran for three weeks and ended last Friday. We had four hundred signups, and most of them recorded at least one meeting. The biggest complaint was battery drain, and the second was transcripts arriving late. Arjun has a fix for the audio engine that cuts idle drain by forty percent. Chloe will write the release notes on Monday. We will meet again on Thursday to confirm the launch date."
  const said = spawnSync("say", ["--file-format=WAVE", "--data-format=LEF32@48000", "-o", fixture, text])
  if (said.status !== 0) throw new Error("say failed")
  const launch = (rounds: number, index: number) => new Promise<void>(done => {
    const data = mkdtempSync(join(tmpdir(), "cereal-latency-data-"))
    const child = spawn(binary, [], { stdio: ["ignore", "ignore", "pipe"], env: {
      ...process.env, CEREAL_DATA_DIR: data, CEREAL_SERVER_URL: stack.base, CEREAL_SERVER_TOKEN: stack.token, CEREAL_ISOLATED_SESSION: "1",
      CEREAL_CAPTURE_FIXTURE: fixture, CEREAL_LATENCY_TRACE: clientTrace, CEREAL_LATENCY_BENCH: String(rounds), CEREAL_ONBOARDING: "0",
      CEREAL_LATENCY_BENCH_SPEAK: value("speak") ?? "4", ...(flag("visible") ? {} : { CEREAL_LATENCY_BENCH_HIDDEN: "1" }),
    } })
    let log = ""
    child.stderr.on("data", chunk => { log += chunk; for (const line of String(chunk).split("\n")) if (line.startsWith("latency bench")) console.log(`[launch ${index}] ${line}`) })
    const timer = setTimeout(() => child.kill("SIGKILL"), (rounds * 90 + 60) * 1000)
    child.on("exit", code => { clearTimeout(timer); writeFileSync(join(out, `launch-${index}.log`), log); if (code) console.log(`[launch ${index}] exited ${code}`); rmSync(data, { recursive: true, force: true }); done() })
  })
  let index = Number(value("first-launch") ?? 0)
  if (warm > 0) await launch(warm + 1, index++)
  for (let i = warm > 0 ? 1 : 0; i < cold; i++) await launch(1, index++)
  rmSync(scratch, { recursive: true, force: true })
  await stack.stop()
}

// ---------------------------------------------------------------- summary
interface Mark { run: number; kind: string; stage: string; ms: number; wall: number }
interface Server { stage: string; noteId?: string; wall: number }
const lines = (file: string) => existsSync(file) ? readFileSync(file, "utf8").split("\n").filter(Boolean).map(line => JSON.parse(line)) : []
const client = lines(clientTrace) as Array<Mark & { check?: string }>, server = lines(serverTrace) as Server[]
// Runs, in file order. A new launch starts its numbering again at 1: that run is the cold one.
interface Run { kind: string; cold: boolean; wall: number; stages: Map<string, number[]> }
const runs: Run[] = []
let current: Run | undefined, lastRun = 0, sinceLaunch = 0
for (const mark of client) {
  if (mark.check) continue
  if (mark.stage === "press") {
    if (mark.run <= lastRun) sinceLaunch = 0
    lastRun = mark.run
    current = { kind: mark.kind, cold: mark.kind === "start" && sinceLaunch === 0, wall: mark.wall, stages: new Map() }
    if (mark.kind === "start") sinceLaunch++
    runs.push(current)
    continue
  }
  if (!current) continue
  const list = current.stages.get(mark.stage) ?? []
  list.push(mark.ms); current.stages.set(mark.stage, list)
}
// A server mark belongs to the newest press before it of a matching kind (start-side or stop-side).
const stopSide = (stage: string) => /end_|recording_stop/.test(stage), stopKind = (kind: string) => kind.startsWith("stop")
for (const mark of server) {
  const run = [...runs].reverse().find(item => item.wall <= mark.wall && mark.wall - item.wall < 15_000 && stopKind(item.kind) === stopSide(mark.stage))
  if (!run) continue
  const list = run.stages.get(mark.stage) ?? []
  if (!list.length) { list.push(Math.round((mark.wall - run.wall) * 10) / 10); run.stages.set(mark.stage, list) }
}
const quantile = (values: number[], q: number) => { const sorted = [...values].sort((a, b) => a - b); return sorted[Math.min(sorted.length - 1, Math.max(0, Math.ceil(q * sorted.length) - 1))]! }
const order = ["ui_state", "ui_painted", "capture_spawned", "capture_native_started", "capture_first_samples", "ui_live", "ui_live_painted", "voice_isolation_on",
  "action_start_sent", "action_resume_sent", "http POST /api/v1/notes", "action_start_answered", "action_resume_answered", "srv_recording_start", "srv_recording_start_done", "srv_recording_resume_done", "live_connect", "stream_spawned", "stream_dial", "target_resolved",
  "srv_upgrade", "ws_open", "srv_hello", "srv_provider_open", "srv_provider_held", "srv_ready", "ready", "first_frame_sent", "srv_first_audio_upstream", "srv_first_interim", "interim_received", "interim_applied", "interim_painted",
  "capture_finished", "capture_finished_seen", "stream_end_cmd", "end_mark_sent", "srv_end_mark", "srv_end_settled", "srv_end_closed", "final_received", "final_applied", "delivered", "action_stop_sent", "srv_recording_stop", "srv_recording_stop_done", "action_stop_answered", "generate_available", "generate_offerable"]
// "Generate notes" is offered once the recording is stopped, its last final is on the note and no request
// is outstanding: derived the same way for every run, whatever the bench waited for.
for (const run of runs) {
  const answered = run.stages.get("action_stop_answered")?.[0], final = run.stages.get("final_applied")?.at(-1)
  if (answered !== undefined) run.stages.set("generate_offerable", [Math.max(answered, final ?? 0)])
}
const groups = new Map<string, Run[]>()
for (const run of runs) { const key = `${run.kind}${run.kind === "start" ? (run.cold ? " (cold)" : " (warm)") : ""}`; groups.set(key, [...(groups.get(key) ?? []), run]) }
const summary: Record<string, Record<string, { n: number; median: number; p95: number }>> = {}
let table = `# Record/stop latency (${provider} recogniser${delay ? `, ${delay} ms added per request` : ""})\n\nms from the press; median / p95 (n). "last" = the last occurrence in the run (finals).\n`
for (const [key, list] of groups) {
  const stages = new Set(list.flatMap(run => [...run.stages.keys()]))
  const named = [...order.filter(stage => stages.has(stage)), ...[...stages].filter(stage => !order.includes(stage)).sort()]
  summary[key] = {}
  table += `\n## ${key} (${list.length} runs)\n\n| stage | median | p95 | n |\n|---|---:|---:|---:|\n`
  for (const stage of named) {
    const first = list.flatMap(run => run.stages.get(stage)?.slice(0, 1) ?? [])
    if (/^http GET \/api\/v1\/(calendar|integrations|models|chats)|ws_refused/.test(stage)) continue
    const repeat = /final_|^http /.test(stage)
    const values = repeat ? list.flatMap(run => run.stages.get(stage)?.slice(-1) ?? []) : first
    const row = { n: values.length, median: quantile(values, 0.5), p95: quantile(values, 0.95) }
    summary[key][repeat ? `${stage} (last)` : stage] = row
    table += `| ${repeat ? `${stage} (last)` : stage} | ${row.median.toFixed(0)} | ${row.p95.toFixed(0)} | ${row.n} |\n`
  }
}
// Correctness: every session's stored text starts with the capture's first word and runs to its end.
const checks = client.filter(mark => mark.check) as unknown as Array<{ check: string; round: number; captures: number[]; segments: Array<{ start: number; end: number | null; text: string }>; gaps: number }>
table += `\n## Stored text against each capture (${checks.length} notes)\n\n`
let problems = 0
for (const check of checks) {
  const total = check.captures.reduce((sum, seconds) => sum + seconds, 0) * 1000
  const first = check.segments[0]?.start ?? null, last = Math.max(0, ...check.segments.map(segment => segment.end ?? segment.start))
  const bad = !check.segments.length || first! > 600 || total - last > 1500 || check.gaps > 0
  if (bad) { problems++; table += `- round ${check.round} note ${check.check}: captures ${check.captures.map(c => c.toFixed(2)).join("+")} s, first word at ${first} ms, last word ends ${last} ms, gaps ${check.gaps}\n` }
}
table += problems ? `\n${problems} of ${checks.length} notes look short.\n` : `All ${checks.length} notes: stored text from the first 600 ms to within 1.5 s of the end, no gaps.\n`
writeFileSync(join(out, "summary.json"), JSON.stringify(summary, null, 2))
writeFileSync(join(out, "table.md"), table)
console.log(table)
