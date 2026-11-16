/**
 * Streams a synthetic meeting (scripts/live/make-fixture.ts) to a running Cereal server as a client would,
 * and reports what came back. Unlike scripts/replay-live.ts this uses a real, deployed server with its real
 * database, recogniser and language model, so it costs a little and creates one note, which it deletes.
 *
 *   bun --env-file=.env scripts/live/check-deployed.ts --fixture <directory> [--base http://127.0.0.1:3000]
 *       [--seconds 80] [--ask-at 45] [--keep]
 *
 * Authenticates with ADMIN_API_TOKEN from the environment (the owner's bootstrap token).
 */
import { readFile } from "node:fs/promises"
import { join } from "node:path"
import { encodeLiveAudioFrame, type LiveServerMessage } from "@meeting-notes/contracts/live"
import type { FixtureManifest } from "./make-fixture"
import { wavFile } from "./make-fixture"

const args = process.argv.slice(2)
const value = (name: string) => { const index = args.indexOf(`--${name}`); return index >= 0 ? args[index + 1] : undefined }
const directory = value("fixture"), base = value("base") ?? "http://127.0.0.1:3000", token = process.env.ADMIN_API_TOKEN
if (!directory || !token) { console.error("usage: bun --env-file=.env scripts/live/check-deployed.ts --fixture <directory>"); process.exit(2) }
const limitSeconds = Number(value("seconds") ?? 80), askAt = Number(value("ask-at") ?? 45)

const manifest: FixtureManifest = JSON.parse(await readFile(join(directory, "manifest.json"), "utf8"))
const rate = manifest.sampleRate, channels = Object.keys(manifest.channels) as Array<"mic" | "system">
const pcm = Object.fromEntries(await Promise.all(channels.map(async channel => [channel, new Uint8Array(await readFile(join(directory, manifest.channels[channel]!.file))).subarray(44)] as const))) as Record<"mic" | "system", Uint8Array>
const totalSamples = Math.min(limitSeconds * rate, Math.max(...channels.map(channel => manifest.channels[channel]!.samples)))
const lines = manifest.lines.filter(line => line.lastSample < totalSamples)

const headers = { Authorization: `Bearer ${token}`, "Content-Type": "application/json" }
const api = async (path: string, method = "GET", body?: unknown) => {
  const response = await fetch(`${base}/api/v1${path}`, { method, headers, ...(body === undefined ? {} : { body: JSON.stringify(body) }) })
  if (!response.ok) throw new Error(`${method} ${path} failed with ${response.status}: ${(await response.text()).slice(0, 300)}`)
  return response
}
const note = await (await api("/notes", "POST", { title: "Live transcription deployment check (safe to delete)" })).json() as { id: string }
let failed = false
try {
  await api(`/notes/${note.id}/recording`, "POST", { command: "start" })
  const started = performance.now(), now = () => performance.now() - started
  const received: Array<{ at: number; message: LiveServerMessage }> = []
  let ready: Extract<LiveServerMessage, { type: "ready" }> | undefined, closeCode: number | undefined
  const socket = new WebSocket(`${base.replace("http", "ws")}/api/v1/notes/${note.id}/live`, { headers: { Authorization: `Bearer ${token}` } } as never)
  socket.onmessage = event => { const message = JSON.parse(String(event.data)) as LiveServerMessage; received.push({ at: now(), message }); if (message.type === "ready") ready = message; if (message.type === "error" || message.type === "gap") console.log(`  server ${message.type}: ${JSON.stringify(message)}`) }
  socket.onclose = event => { closeCode = event.code }
  socket.onopen = () => socket.send(JSON.stringify({ type: "hello", protocol: 1, captureId: "deploy-check", language: "en", streams: channels.map(channel => ({ channel, sampleRate: rate, encoding: "pcm_s16le", baseMs: 0, resumeFromSample: 0 })) }))
  for (const deadline = performance.now() + 15_000; !ready;) { if (closeCode !== undefined || performance.now() > deadline) throw new Error(`The live socket was refused (close code ${closeCode ?? "none, timed out"})`); await Bun.sleep(20) }
  console.log(`ready: ${ready!.provider} ${ready!.model}; streaming ${(totalSamples / rate).toFixed(0)} s on ${channels.join(" + ")}`)

  const frameSamples = Math.round(rate / 10), endAt = new Map<number, number>()
  let cursor = 0, chat: Promise<void> | undefined

  const ask = async () => {
    const lastLine = [...lines].reverse().find(line => endAt.has(lines.indexOf(line)))
    const asked = now()
    const response = await api(`/notes/${note.id}/chat/stream`, "POST", { message: "What was the very last thing said just now? Quote it.", history: [], includeSources: true })
    const reader = response.body!.getReader(), decoder = new TextDecoder()
    let body = "", firstAt: number | undefined
    for (;;) { const { done, value: bytes } = await reader.read(); if (done) break; firstAt ??= now(); body += decoder.decode(bytes, { stream: true }) }
    const text = [...body.matchAll(/^data: (.*)$/gm)].map(match => { try { const event = JSON.parse(match[1]!); return typeof event.delta === "string" ? event.delta : typeof event.text === "string" ? event.text : "" } catch { return "" } }).join("")
    console.log(`\nchat asked at ${(asked / 1000).toFixed(1)} s; first bytes after ${Math.round((firstAt ?? now()) - asked)} ms, finished after ${Math.round(now() - asked)} ms`)
    console.log(`  last line spoken before asking: "${lastLine?.text ?? "(none)"}"`)
    console.log(`  answer: ${text.trim().slice(0, 700) || body.slice(0, 700)}\n`)
  }
  const origin = performance.now()
  while (cursor < totalSamples) {
    const captured = Math.min(totalSamples, Math.floor((performance.now() - origin) * rate / 1000))
    if (!chat && captured >= askAt * rate) chat = ask().catch(error => { failed = true; console.log(`  chat failed: ${error}`) })
    const end = Math.min(captured, cursor + frameSamples)
    if (end - cursor < frameSamples && end < totalSamples) { await Bun.sleep(2); continue }
    for (const channel of channels) socket.send(encodeLiveAudioFrame({ channel, firstSample: cursor, pcm: pcm[channel].subarray(cursor * 2, end * 2) }) as Uint8Array<ArrayBuffer>)
    lines.forEach((line, index) => { if (!endAt.has(index) && line.lastSample < end) endAt.set(index, now()) })
    cursor = end
  }
  await Bun.sleep(3000)
  await chat
  socket.send(JSON.stringify({ type: "mark", kind: "end", atSample: Object.fromEntries(channels.map(channel => [channel, totalSamples])) }))
  for (const deadline = performance.now() + 8000; closeCode === undefined && performance.now() < deadline;) await Bun.sleep(20)
  const stopAt = performance.now()
  await api(`/notes/${note.id}/recording`, "POST", { command: "stop" })
  console.log(`socket closed with ${closeCode}; stop answered in ${Math.round(performance.now() - stopAt)} ms`)

  const delays = (type: "interim" | "final") => lines.flatMap((line, index) => {
    const pattern = new RegExp(line.probe, "i"), lineStartMs = line.firstSample * 1000 / rate
    const hit = received.find(({ message }) => message.type === type && (type === "interim" ? (message as { channel: string }).channel : (message as { segment: { channel: string } }).segment.channel) === line.channel
      && pattern.test(message.type === "interim" ? message.text : (message as { segment: { text: string } }).segment.text)
      && (message.type === "interim" ? message.endMs : (message as { segment: { endMs: number | null; startMs: number } }).segment.endMs ?? 0) >= lineStartMs)
    return hit && endAt.has(index) ? [Math.max(0, hit.at - endAt.get(index)!)] : []
  })
  const stats = (values: number[]) => { const sorted = [...values].sort((a, b) => a - b), at = (p: number) => sorted.length ? Math.round(sorted[Math.min(sorted.length - 1, Math.ceil(p * sorted.length) - 1)]!) : null; return `n=${sorted.length} of ${lines.length}, median ${at(0.5)} ms, p95 ${at(0.95)} ms, max ${at(1)} ms` }
  console.log(`speech end to interim: ${stats(delays("interim"))}`)
  console.log(`speech end to final:   ${stats(delays("final"))}`)
  console.log(`messages: ${JSON.stringify(Object.fromEntries([...new Set(received.map(item => item.message.type))].map(type => [type, received.filter(item => item.message.type === type).length])))}`)

  // Give the worker time to resolve the durable parts against live coverage, then read the stored note.
  await Bun.sleep(8000)
  const stored = await (await api(`/notes/${note.id}`)).json() as { recordingState: string; transcriptRevision: number; transcriptSegments: Array<{ channel: string; speakerLabel: string | null; text: string; startMs: number }> }
  const finals = received.filter(item => item.message.type === "final").length
  console.log(`stored note: ${stored.transcriptSegments.length} segments (the socket delivered ${finals} finals), revision ${stored.transcriptRevision}, recording ${stored.recordingState}`)
  for (const segment of [...stored.transcriptSegments].sort((a, b) => a.startMs - b.startMs)) console.log(`  ${segment.channel.padEnd(6)} ${(segment.speakerLabel ?? "?").padEnd(10)} ${(segment.startMs / 1000).toFixed(1).padStart(5)}s  ${segment.text}`)
  console.log(`expected script:`)
  for (const line of lines) console.log(`  ${line.channel.padEnd(6)} ${line.voice.padEnd(10)} ${(line.firstSample / rate).toFixed(1).padStart(5)}s  ${line.text}`)
  console.log(`NOTE_ID=${note.id}`)
} catch (error) {
  failed = true
  console.error(`FAILED: ${error}`)
} finally {
  if (!args.includes("--keep")) { await api(`/notes/${note.id}`, "DELETE").then(() => console.log("check note deleted"), error => console.error(`could not delete the check note ${note.id}: ${error}`)) }
}
process.exit(failed ? 1 : 0)
