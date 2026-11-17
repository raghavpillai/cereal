/**
 * Stage timings of live sessions, for measuring how long starting and stopping a recording takes
 * (`LIVE_LATENCY_TRACE=<file>`). Off, every call returns at once. Each mark is one JSON line with Unix
 * time in milliseconds (the system clock, as the desktop reads it; not the monotonic clock, which drifts
 * from it over a long run), which the desktop's own trace (`apps/desktop/src/latency.rs`) is lined up with.
 * Never any audio or text: the stage, the note and the session only.
 */
import { appendFileSync } from "node:fs"

const file = process.env.LIVE_LATENCY_TRACE
const marked = new Set<string>()

export const tracing = !!file

/** Marks `stage`; with `once`, only the first time for that key (a session, say). */
export function trace(stage: string, noteId?: string, once?: string) {
  if (!file) return
  if (once !== undefined) {
    const key = `${once}:${stage}`
    if (marked.has(key)) return
    marked.add(key)
  }
  try { appendFileSync(file, JSON.stringify({ stage, noteId, wall: Date.now() }) + "\n") } catch { /* measuring must never break a session */ }
}
