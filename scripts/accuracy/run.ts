/**
 * The live transcription accuracy and robustness harness. Feeds an invented meeting with known reference
 * text (scripts/accuracy/fixtures.ts) through the real desktop client (apps/desktop/src/live_accuracy.rs,
 * driving `live::Session`) into the real server session code (scripts/accuracy/server.ts, in this process,
 * 127.0.0.1 only), applies a fault scenario, and scores the stored transcript (scripts/accuracy/metrics.ts).
 *
 *   bun scripts/accuracy/fixtures.ts                                   # once: renders .local/ship/accuracy/fixtures
 *   bun scripts/accuracy/run.ts --provider fake --fixture meeting-5m --scenario all
 *   bun --env-file=.env scripts/accuracy/run.ts --provider elevenlabs --fixture meeting-5m --scenario baseline,net-drop
 *   bun --env-file=.env scripts/accuracy/run.ts --provider elevenlabs --client raw --fixture meeting-5m   # the provider alone
 *
 * Providers: `fake` codes every reference word as samples the fake recogniser reads back exactly, so any
 * word lost, repeated or misplaced is the pipeline's doing (expected WER 0); `elevenlabs` is the real
 * provider (ELEVENLABS_API_KEY, about $0.22 per stream-hour; two streams per run). Client `raw` streams
 * the fixture straight to the provider with the server's parameters, no Cereal code in between: the
 * provider's own error rate.
 *
 * Options: --out <dir> (default .local/ship/accuracy/results/<label>) --label <name> --parallel <n>
 *          --speed <capture speed> --language <code>
 */
import { appendFile, mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join, resolve } from "node:path"
import type { FakeWord } from "../../apps/api/src/live/providers/fake"
import { FAKE_WORD_BASE } from "../../apps/api/src/live/providers/fake"
import { RATE, readWav, wavBytes, type ChannelName, type Reference } from "./fixtures"
import { evaluate, type AccuracyReport, type FinalMessage, type HypSegment, type InterimMessage, type RunEvent } from "./metrics"
import { startProxy, type Proxy } from "./proxy"
import { SCENARIOS, type Scenario } from "./scenarios"
import { startHarnessServer, TOKEN, type HarnessServer } from "./server"

const ROOT = resolve(import.meta.dir, "../..")
const args = process.argv.slice(2), value = (name: string) => { const index = args.indexOf(`--${name}`); return index >= 0 ? args[index + 1] : undefined }
const provider = (value("provider") ?? "fake") as "fake" | "elevenlabs", client = (value("client") ?? "desktop") as "desktop" | "raw"
const fixtureNames = (value("fixture") ?? "meeting-5m").split(","), speed = Number(value("speed") ?? 1), parallel = Number(value("parallel") ?? 1)
const label = value("label") ?? `${provider}-${client}`, out = resolve(value("out") ?? join(ROOT, ".local/ship/accuracy/results", label))
const fixturesDir = resolve(value("fixtures") ?? join(ROOT, ".local/ship/accuracy/fixtures"))

// ------------------------------------------------------------------ scenarios

// ------------------------------------------------------------------ fixtures

async function loadReference(name: string): Promise<Reference> { return JSON.parse(await readFile(join(fixturesDir, name, "reference.json"), "utf8")) }
/** The fixture as the fake recogniser hears it: every reference word a run of its own coded sample. */
function fakeAudio(reference: Reference) {
  const vocabulary: FakeWord[] = [], pcm = { mic: new Int16Array(reference.samples), system: new Int16Array(reference.samples) }
  for (const utterance of reference.utterances) {
    if (utterance.kind === "mumble") continue
    for (const word of utterance.words) {
      const from = Math.round(word.startMs * RATE / 1000), to = Math.max(from + 160, Math.round(word.endMs * RATE / 1000) - 16)
      vocabulary.push({ text: word.text, speaker: utterance.speaker, samples: to - from, language: utterance.language })
      const id = vocabulary.length
      if (FAKE_WORD_BASE + id > 16_000) throw new Error("Too many words for the fake recogniser's 16-bit codes")
      pcm[utterance.channel].fill(FAKE_WORD_BASE + id, from, Math.min(to, reference.samples))
    }
  }
  return { vocabulary, pcm }
}

// ------------------------------------------------------------------ the desktop driver

let driverPath: Promise<string> | undefined
/** Builds the desktop test binary once and returns its path. */
function driver() {
  return driverPath ??= (async () => {
    for (let attempt = 0; ; attempt++) {
      const build = Bun.spawn(["cargo", "test", "-p", "cereal", "--no-run", "--message-format=json"], { cwd: ROOT, stdout: "pipe", stderr: "pipe" })
      const text = await new Response(build.stdout).text(), code = await build.exited
      const exe = text.split("\n").flatMap(line => { try { const m = JSON.parse(line); return m.reason === "compiler-artifact" && m.target?.name === "cereal" && m.profile?.test && m.executable ? [m.executable as string] : [] } catch { return [] } }).at(-1)
      if (code === 0 && exe) return exe
      if (attempt >= 5) throw new Error(`cargo test --no-run failed: ${(await new Response(build.stderr).text()).slice(-2000)}`)
      console.warn("desktop build failed (another change in progress?); retrying in 60 s")
      await Bun.sleep(60_000)
    }
  })()
}

interface DriverEvent { t: number; type: string; [key: string]: any }
async function readEvents(path: string): Promise<DriverEvent[]> {
  const text = await readFile(path, "utf8").catch(() => "")
  return text.split("\n").filter(Boolean).flatMap(line => { try { return [JSON.parse(line)] } catch { return [] } })
}

// ------------------------------------------------------------------ one run

interface RunResult {
  fixture: string; provider: string; client: string; scenario: string; description: string; seconds: number
  report: AccuracyReport; gaps: Array<{ channel: string; fromMs: number; toMs: number }>; connections: number; deliveries: string[]
  upstream?: { sessions: number; seconds: number; drops: number }; statuses: Record<string, number>; notes: string[]
}

async function runDesktop(fixtureName: string, scenario: Scenario, work: string): Promise<RunResult> {
  const reference = await loadReference(fixtureName), notes: string[] = []
  const fake = provider === "fake" ? fakeAudio(reference) : undefined
  const audio = { mic: join(fixturesDir, fixtureName, "mic.wav"), system: join(fixturesDir, fixtureName, "system.wav") }
  if (fake) for (const channel of ["mic", "system"] as const) { audio[channel] = join(work, `fake-${channel}.wav`); await writeFile(audio[channel], wavBytes(fake.pcm[channel])) }
  const server: HarnessServer = await startHarnessServer({ provider, ...(fake ? { vocabulary: fake.vocabulary } : {}), legacyKeepAlive: args.includes("--legacy-keepalive") })
  const proxy: Proxy = startProxy(() => server.port)
  const started = performance.now()
  try {
    const note = await server.call("/notes", "POST", { title: `Accuracy ${fixtureName} ${scenario.name} (harness)` })
    await server.call(`/notes/${note.id}/recording`, "POST", { command: "start" })
    const captures = scenario.captures ?? [{}], events: DriverEvent[] = [], runEvents: RunEvent[] = []
    let baseMs = 0
    for (const [index, capture] of captures.entries()) {
      const fromMs = capture.fromMs ?? 0, eventsPath = join(work, `events-${index}.jsonl`), control = join(work, `control-${index}`)
      await writeFile(control, "")
      const config = { url: `ws://127.0.0.1:${proxy.port}/api/v1/notes/${note.id}/live`, token: TOKEN, captureId: `${scenario.name}-${index}-${crypto.randomUUID().slice(0, 8)}`, baseMs,
        ...(value("language") ? { language: value("language") } : {}), events: eventsPath, control,
        mic: capture.mic === false ? null : audio.mic, system: capture.system === false ? null : audio.system,
        writeRate: fake ? 16_000 : 48_000, pcm16: !!fake, speed, fromMs, ...(capture.untilMs ? { untilMs: capture.untilMs } : {}),
        systemDelayMs: capture.systemDelayMs ?? 0, systemSkewPpm: capture.systemSkewPpm ?? 0, prerecordedMs: capture.prerecordedMs ?? 0 }
      const configPath = join(work, `config-${index}.json`)
      await writeFile(configPath, JSON.stringify(config))
      const data = await mkdtemp(join(tmpdir(), "cereal-accuracy-data-"))
      const child = Bun.spawn([await driver(), "live_accuracy::accuracy_drive", "--exact", "--ignored", "--nocapture"], { cwd: join(ROOT, "apps/desktop"), env: { ...process.env, CEREAL_ACCURACY: configPath, CEREAL_DATA_DIR: data }, stdout: "ignore", stderr: "pipe" })
      // Wait for the capture to begin, then fire the scenario's actions at their capture times.
      let begun: number | undefined
      for (const deadline = performance.now() + 60_000; begun === undefined && performance.now() < deadline;) { if ((await readEvents(eventsPath)).some(event => event.type === "started")) begun = performance.now(); else await Bun.sleep(20) }
      if (begun === undefined) throw new Error("the desktop driver did not start")
      let shift = (capture.prerecordedMs ?? 0) / speed, exited = false
      void child.exited.then(() => { exited = true })
      const actions = (scenario.actions ?? []).filter(item => item.atMs >= fromMs && (capture.untilMs === undefined || item.atMs < capture.untilMs)).sort((a, b) => a.atMs - b.atMs)
      const fire = async () => {
        for (const { atMs, action } of actions) {
          const due = begun! + (atMs - fromMs) / speed - shift
          while (performance.now() < due && !exited) await Bun.sleep(10)
          if (exited) return
          const noteMs = baseMs + atMs - fromMs
          runEvents.push({ atMs: noteMs, kind: action.kind })
          if (action.kind === "net-down") { proxy.down(action.ms); runEvents.push({ atMs: noteMs + action.ms, kind: "net-up" }) }
          else if (action.kind === "restart") void server.restart(action.ms)
          else if (action.kind === "provider-drop") server.dropProvider()
          else if (action.kind === "throttle") proxy.throttle(action.bytesPerSecond)
          else if (action.kind === "sever") await appendFile(control, "sever\n")
          else if (action.kind === "pause") { await appendFile(control, "pause\n"); await Bun.sleep(action.ms); await appendFile(control, "resume\n"); shift -= action.ms }
          else if (action.kind === "sleep") {
            proxy.hold(true); process.kill(child.pid, "SIGSTOP")
            await Bun.sleep(action.ms)
            process.kill(child.pid, "SIGCONT"); proxy.hold(false); shift -= action.ms
          }
        }
      }
      const firing = fire()
      // Stop as the app does: after the stream says the capture was delivered (or refused).
      let stopped = false
      for (;;) {
        const seen = await readEvents(eventsPath)
        if (!stopped && seen.some(event => event.type === "delivered")) {
          stopped = true
          const at = performance.now()
          await server.call(`/notes/${note.id}/recording`, "POST", { command: "stop" }).catch(error => notes.push(`stop failed: ${error}`))
          notes.push(`capture ${index}: Stop answered in ${Math.round(performance.now() - at)} ms`)
        }
        if (exited) break
        if (performance.now() - started > 3 * 3600_000) { child.kill(); notes.push("timed out"); break }
        await Bun.sleep(200)
      }
      await firing
      const stderr = await new Response(child.stderr).text(), code = await child.exited
      if (code !== 0) notes.push(`driver exited ${code}: ${stderr.split("\n").filter(line => /panicked|error/.test(line)).slice(0, 3).join(" | ")}`)
      const seen = await readEvents(eventsPath)
      if (!stopped) { notes.push(`capture ${index}: never delivered`); await server.call(`/notes/${note.id}/recording`, "POST", { command: "stop" }).catch(() => undefined) }
      events.push(...seen.map(event => ({ ...event, capture: index })))
      const ended = seen.find(event => event.type === "captureEnded" || (event.type === "command" && event.command === "end"))
      const capturedMs = ended?.captureMs ?? reference.durationMs - fromMs
      if (index + 1 < captures.length) {
        runEvents.push({ atMs: baseMs + capturedMs, kind: "stop-start" })
        await server.call(`/notes/${note.id}/recording`, "POST", { command: "resume" })
        // The app places the next capture after this one's audio.
        baseMs += Math.round(capturedMs)
      } else if (capture.untilMs) runEvents.push({ atMs: baseMs + capturedMs, kind: "stop" })
    }
    const stored = await server.call(`/notes/${note.id}`)
    const segments: HypSegment[] = stored.transcriptSegments
    const untilMs = captures.at(-1)!.untilMs
    const report = evaluate({ reference: { ...reference, utterances: reference.utterances.filter(item => captures.some(capture => (capture.mic === false ? item.channel !== "mic" : true) && (capture.system === false ? item.channel !== "system" : true))) },
      segments, interims: events.filter(event => event.type === "interim") as unknown as InterimMessage[], finals: events.filter(event => event.type === "final") as unknown as FinalMessage[],
      events: runEvents, ...(untilMs ? { untilMs } : {}) })
    const statuses: Record<string, number> = {}
    for (const event of events) if (event.type === "status") statuses[event.phase] = (statuses[event.phase] ?? 0) + 1
    await writeFile(join(work, "segments.json"), JSON.stringify(segments, null, 1))
    await writeFile(join(work, "provider-finals.json"), JSON.stringify(server.providerFinals, null, 1))
    await writeFile(join(work, "server-protocol.json"), JSON.stringify(server.protocol, null, 1))
    await writeFile(join(work, "run-events.json"), JSON.stringify({ runEvents, untilMs: untilMs ?? null }, null, 1))
    return { fixture: fixtureName, provider, client, scenario: scenario.name, description: scenario.description, seconds: Math.round((performance.now() - started) / 1000), report,
      gaps: events.filter(event => event.type === "gap").map(event => ({ channel: event.channel, fromMs: event.fromMs, toMs: event.toMs })),
      connections: events.filter(event => event.type === "revision").length, deliveries: events.filter(event => event.type === "delivered").map(event => event.delivery),
      upstream: { ...server.meter }, statuses, notes }
  } finally { proxy.stop(); await server.stop() }
}

// ------------------------------------------------------------------ the provider alone

/** Streams each channel straight to the provider with the server's parameters and keeps its committed words. */
async function runRaw(fixtureName: string): Promise<RunResult> {
  const reference = await loadReference(fixtureName), started = performance.now()
  if (provider !== "elevenlabs" || !process.env.ELEVENLABS_API_KEY) throw new Error("--client raw needs --provider elevenlabs and ELEVENLABS_API_KEY")
  const segments: HypSegment[] = []
  await Promise.all((["mic", "system"] as const).map(async channel => {
    const pcm = await readWav(join(fixturesDir, fixtureName, `${channel}.wav`))
    const query = new URLSearchParams({ model_id: value("model") ?? "scribe_v3_live", audio_format: `pcm_${RATE}`, include_timestamps: "true", include_language_detection: "true", commit_strategy: "turn_prediction", vad_silence_threshold_secs: "0.4", diarize: "true" })
    const socket = new WebSocket(`wss://api.elevenlabs.io/v1/speech-to-text/realtime?${query}`, { headers: { "xi-api-key": process.env.ELEVENLABS_API_KEY! } } as never)
    let ready = false, closed = false
    const timed: Array<Array<{ text: string; start: number; end: number; speaker?: string }>> = [], diarized: typeof timed = []
    const words = (raw: unknown) => { const list: Array<{ text: string; start: number; end: number; speaker?: string }> = []; for (const w of Array.isArray(raw) ? raw : []) { if (w.type === "spacing") { if (list.length) list.at(-1)!.text += w.text; continue } if (w.type === "word") list.push({ text: w.text, start: w.start, end: w.end, ...(w.speaker_id ? { speaker: w.speaker_id } : {}) }) } return list }
    socket.onmessage = event => {
      const m = JSON.parse(String(event.data))
      if (m.message_type === "session_started") ready = true
      else if (m.message_type === "committed_transcript_with_timestamps") timed.push(words(m.words))
      else if (m.message_type === "diarized_transcript") diarized.push(words(m.words))
    }
    socket.onclose = () => { closed = true }
    while (!ready && !closed) await Bun.sleep(20)
    const t0 = performance.now(), frame = RATE / 10
    for (let at = 0; at < pcm.length && !closed; at += frame) {
      const due = t0 + at / RATE * 1000 / speed
      if (due > performance.now()) await Bun.sleep(due - performance.now())
      const chunk = pcm.subarray(at, Math.min(pcm.length, at + frame))
      socket.send(JSON.stringify({ message_type: "input_audio_chunk", audio_base_64: Buffer.from(chunk.buffer, chunk.byteOffset, chunk.byteLength).toString("base64"), commit: false, sample_rate: RATE }))
    }
    socket.send(JSON.stringify({ message_type: "input_audio_chunk", audio_base_64: "", commit: true, sample_rate: RATE }))
    await Bun.sleep(5000)
    socket.close()
    const commits = timed.map((list, index) => diarized[index] ?? list)
    for (const list of commits) {
      // One segment per speaker turn; a word without a speaker stays with the one before.
      let current: HypSegment | undefined, speaker: string | undefined
      for (const word of list) {
        speaker = word.speaker ?? speaker
        const id = `${channel}:raw:${speaker ?? "?"}`
        if (!current || current.speakerId !== id) { if (current) segments.push(current); current = { channel, speakerId: id, speakerLabel: channel === "mic" ? "Me" : id, text: "", startMs: Math.round(word.start * 1000), endMs: Math.round(word.end * 1000) } }
        current.text += word.text; current.endMs = Math.round(word.end * 1000)
      }
      if (current) segments.push({ ...current, text: current.text.trim() })
    }
  }))
  for (const segment of segments) segment.text = segment.text.trim()
  const report = evaluate({ reference, segments })
  return { fixture: fixtureName, provider, client, scenario: "raw", description: "The provider alone: the fixture streamed straight to it with the server's parameters.", seconds: Math.round((performance.now() - started) / 1000), report, gaps: [], connections: 1, deliveries: [], statuses: {}, notes: [] }
}

// ------------------------------------------------------------------ main

const wanted = value("scenario") ?? "baseline"
const scenarios = client === "raw" ? [{ name: "raw", description: "" } as Scenario] : wanted === "all" ? SCENARIOS : wanted.split(",").map(name => { const found = SCENARIOS.find(item => item.name === name); if (!found) throw new Error(`unknown scenario ${name}; known: ${SCENARIOS.map(item => item.name).join(", ")}`); return found })
await mkdir(out, { recursive: true })
if (client === "desktop") await driver()
const jobs = fixtureNames.flatMap(fixture => scenarios.map(scenario => ({ fixture, scenario })))
const results: RunResult[] = []
async function worker() {
  for (let job = jobs.shift(); job; job = jobs.shift()) {
    const name = `${job.fixture}-${job.scenario.name}`, work = join(out, "runs", name)
    await mkdir(work, { recursive: true })
    console.log(`start ${name}`)
    try {
      const result = client === "raw" ? await runRaw(job.fixture) : await runDesktop(job.fixture, job.scenario, work)
      results.push(result)
      await writeFile(join(out, `${name}.json`), JSON.stringify(result, null, 1))
      const r = result.report
      console.log(`done  ${name}: WER ${(r.wer * 100).toFixed(1)}% (mic ${(r.perChannel.mic.wer * 100).toFixed(1)}%, system ${(r.perChannel.system.wer * 100).toFixed(1)}%), lost runs ${r.lostRuns.length}, duplicated runs ${r.duplicatedRuns.length}, stray ${r.strayScript.segments}, gaps ${result.gaps.length}, ${result.seconds} s${result.notes.length ? `; ${result.notes.join("; ")}` : ""}`)
    } catch (error) { console.error(`FAILED ${name}: ${error instanceof Error ? error.stack : error}`) }
  }
}
await Promise.all(Array.from({ length: Math.max(1, parallel) }, worker))
process.exit(0)
