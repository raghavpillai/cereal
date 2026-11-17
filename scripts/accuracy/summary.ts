/**
 * Re-scores saved harness runs with the current metrics and writes a Markdown table.
 *
 *   bun scripts/accuracy/summary.ts <results dir> [<results dir> ...] [--out summary.md]
 *
 * Each run directory keeps what the scoring needs (segments.json, events-*.jsonl, run-events.json), so a
 * change to the metrics never needs the provider again.
 */
import { readdir, readFile, writeFile } from "node:fs/promises"
import { basename, join, resolve } from "node:path"
import type { Reference } from "./fixtures"
import { evaluate, type AccuracyReport, type RunEvent } from "./metrics"
import { SCENARIOS } from "./scenarios"

const ROOT = resolve(import.meta.dir, "../.."), fixtures = join(ROOT, ".local/ship/accuracy/fixtures")
const args = process.argv.slice(2), outIndex = args.indexOf("--out"), out = outIndex >= 0 ? args[outIndex + 1] : undefined
const dirs = args.filter((arg, index) => !arg.startsWith("--") && index !== outIndex + 1 || outIndex < 0 && !arg.startsWith("--"))
const pct = (value: number | null | undefined) => value === null || value === undefined ? "-" : `${(value * 100).toFixed(1)}%`
const rows: string[] = []
for (const dir of dirs) {
  for (const file of (await readdir(dir)).filter(name => name.endsWith(".json")).sort()) {
    const result = JSON.parse(await readFile(join(dir, file), "utf8"))
    const work = join(dir, "runs", basename(file, ".json"))
    let report: AccuracyReport = result.report
    const segmentsFile = Bun.file(join(work, "segments.json"))
    if (await segmentsFile.exists()) {
      const reference: Reference = JSON.parse(await readFile(join(fixtures, result.fixture, "reference.json"), "utf8"))
      const events = (await Promise.all((await readdir(work)).filter(name => /^events-\d+\.jsonl$/.test(name)).map(async name => (await readFile(join(work, name), "utf8")).split("\n").filter(Boolean).map(line => JSON.parse(line))))).flat()
      let runEvents: RunEvent[] = [], untilMs: number | undefined
      const saved = Bun.file(join(work, "run-events.json"))
      if (await saved.exists()) { const data = await saved.json(); runEvents = data.runEvents; untilMs = data.untilMs ?? undefined }
      else {
        // Runs from before run-events.json was kept: the scenario's own schedule (one capture from 0).
        const scenario = SCENARIOS.find(item => item.name === result.scenario)
        for (const { atMs, action } of scenario?.actions ?? []) { runEvents.push({ atMs, kind: action.kind }); if (action.kind === "net-down") runEvents.push({ atMs: atMs + action.ms, kind: "net-up" }) }
        const captures = scenario?.captures ?? []
        if (captures.length > 1 && captures[0]!.untilMs) runEvents.push({ atMs: captures[0]!.untilMs, kind: "stop-start" })
        if (captures.length === 1 && captures[0]!.untilMs) { untilMs = captures[0]!.untilMs; runEvents.push({ atMs: untilMs, kind: "stop" }) }
      }
      const mic = !(result.scenario === "mic-only")
      report = evaluate({ reference: { ...reference, utterances: reference.utterances.filter(item => mic || item.channel === "mic") }, segments: JSON.parse(await readFile(join(work, "segments.json"), "utf8")),
        interims: events.filter(event => event.type === "interim"), finals: events.filter(event => event.type === "final"), events: runEvents, ...(untilMs ? { untilMs } : {}) })
      if (result.scenario === "mic-only") report = evaluate({ reference: { ...reference, utterances: reference.utterances.filter(item => item.channel === "mic") }, segments: JSON.parse(await readFile(join(work, "segments.json"), "utf8")), interims: events.filter(event => event.type === "interim"), finals: events.filter(event => event.type === "final"), events: runEvents })
      result.report = report
      await writeFile(join(dir, file), JSON.stringify(result, null, 1))
    }
    const r = report, lost = r.lostRuns.reduce((s, x) => s + x.words, 0), dup = r.duplicatedRuns.reduce((s, x) => s + x.words, 0)
    rows.push(`| ${basename(dir)} | ${result.fixture} | ${result.scenario} | ${pct(r.wer)} | ${pct(r.perChannel.mic.wer)} | ${pct(r.perChannel.system.wer)} | ${Object.entries(r.perLanguage).filter(([l]) => l !== "en").map(([l, v]) => `${l} ${pct(v.errorRate)}`).join(", ") || "-"} | ${pct(r.attribution.micAsMe)} / ${pct(r.attribution.system)} | ${lost} (${r.boundary.lostWordsNearEvents} near) | ${dup} (${r.boundary.duplicatedWordsNearEvents} near) | ${r.clippedWords} | ${r.drift.micMedianMs ?? "-"} / ${r.drift.systemMedianMs ?? "-"} / ${r.drift.slopeMsPerHour ?? "-"} | ${r.strayScript.segments} | ${r.hallucinatedInSilence.words} | ${r.flicker.perFinal?.toFixed(1) ?? "-"} / ${r.flicker.rewrites} | ${result.gaps?.length ?? 0} |`)
  }
}
const table = ["| run | fixture | scenario | WER | mic | system | other languages | attribution me / system | lost words | duplicated words | clipped | drift ms: mic / system median, slope per hour | stray script | words in silence | interims per final / rewrites | gaps |",
  "|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|", ...rows].join("\n")
if (out) await writeFile(out, table + "\n")
console.log(table)
