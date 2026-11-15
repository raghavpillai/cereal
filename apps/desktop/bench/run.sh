#!/bin/bash
# Cereal desktop benchmark: build, generate the stress workspace, run every
# scenario, write a results table.
#
#   apps/desktop/bench/run.sh              release build, full run (about 6 minutes after the build)
#   apps/desktop/bench/run.sh --debug      same scenarios on the debug build (checks the mechanics only)
#   apps/desktop/bench/run.sh --out DIR    where to put logs and results.md (default: a new temp dir)
#   apps/desktop/bench/run.sh --clean      remove target/release afterwards (it is about 1 GB)
#
# Run it on a quiet machine: one-minute load at or under the core count, on
# mains power, nothing else animating. The app takes focus while it runs; leave
# the keyboard and pointer alone, or the hover and idle numbers are yours.
#
# Everything runs in preview mode against throwaway data directories: no
# server, no microphone, never the real workspace.
set -u
cd "$(dirname "$0")/../../.." || exit 1
ROOT=$PWD
PROFILE=release
OUT=""
CLEAN=0
while [ $# -gt 0 ]; do
  case "$1" in
    --debug) PROFILE=debug ;;
    --clean) CLEAN=1 ;;
    --out) OUT=$2; shift ;;
    *) echo "unknown option $1"; exit 2 ;;
  esac
  shift
done
[ -n "$OUT" ] || OUT=$(mktemp -d "${TMPDIR:-/tmp}/cereal-bench.XXXXXX")
mkdir -p "$OUT"
BIN="$ROOT/target/$PROFILE/cereal"
CORES=$(sysctl -n hw.ncpu)
LOAD=$(sysctl -n vm.loadavg | awk '{print $2}')
HZ=$(system_profiler SPDisplaysDataType 2>/dev/null | sed -n 's/.*@ \([0-9.]*\) *Hz.*/\1/p' | head -1)
[ -n "$HZ" ] || HZ=60
{
  echo "date: $(date)"
  echo "build: $PROFILE"
  echo "cores: $CORES"
  echo "load at start: $(sysctl -n vm.loadavg)"
  echo "display: ${HZ} Hz"
  echo "power: $(pmset -g batt 2>/dev/null | head -1)"
} | tee "$OUT/machine.txt"
if awk "BEGIN{exit !($LOAD > $CORES)}"; then
  echo "WARNING: one-minute load $LOAD is above the core count ($CORES); timings will not be trustworthy." | tee -a "$OUT/machine.txt"
fi

echo "== build ($PROFILE)"
if [ "$PROFILE" = release ]; then
  cargo build -p cereal --locked --release 2>&1 | tail -3 || exit 1
else
  cargo build -p cereal --locked 2>&1 | tail -3 || exit 1
fi
[ -x "$BIN" ] || { echo "no binary at $BIN"; exit 1; }

STRESS="$OUT/stress-template"
mkdir -p "$STRESS"
python3 "$ROOT/apps/desktop/bench/stress_workspace.py" "$STRESS" >/dev/null || exit 1

# run <label> <fixture: demo|stress> <tour filter> <timeout s> [app args...]
# Starts one instance on a fresh copy of the fixture and waits for it to quit.
run() {
  local label=$1 fixture=$2 filter=$3 limit=$4; shift 4
  local data; data=$(mktemp -d "$OUT/data.XXXXXX")
  [ "$fixture" = stress ] && cp "$STRESS/workspace.json" "$data/"
  echo "== $label ($fixture)"
  env CEREAL_DATA_DIR="$data" CEREAL_FRAME_STATS=1 CEREAL_TOUR="$filter" \
    "$BIN" --motion-tour "$@" > "$OUT/$label.log" 2>&1 &
  PID=$!
  local waited=0
  while kill -0 $PID 2>/dev/null; do
    sleep 1; waited=$((waited + 1))
    if [ $waited -ge "$limit" ]; then
      echo "timeout after ${limit}s" | tee -a "$OUT/$label.log"; kill $PID; break
    fi
  done
  wait $PID 2>/dev/null
  rm -rf "$data"
  grep -q "^tour done" "$OUT/$label.log" || echo "  (did not finish: see $OUT/$label.log)"
  grep -il "panicked" "$OUT/$label.log" >/dev/null && echo "  PANIC in $OUT/$label.log"
}

# idle <label> <fixture> <tour filter> <step to sample> [app args...]
# Runs one instance that sits in a state for 14 s, and samples its CPU for
# 8 s of that with top (first sample discarded).
idle() {
  local label=$1 fixture=$2 filter=$3 step=$4; shift 4
  local data; data=$(mktemp -d "$OUT/data.XXXXXX")
  [ "$fixture" = stress ] && cp "$STRESS/workspace.json" "$data/"
  echo "== $label ($fixture, idle)"
  env CEREAL_DATA_DIR="$data" CEREAL_FRAME_STATS=1 CEREAL_TOUR="$filter" \
    "$BIN" --motion-tour "$@" > "$OUT/$label.log" 2>&1 &
  PID=$!
  local waited=0
  until grep -q "^tour [0-9]* $step\$" "$OUT/$label.log" 2>/dev/null; do
    sleep 0.5; waited=$((waited + 1))
    kill -0 $PID 2>/dev/null || break
    [ $waited -ge 120 ] && break
  done
  sleep 4   # let the transition into the state finish
  if kill -0 $PID 2>/dev/null; then
    top -l 9 -s 1 -pid $PID -stats cpu 2>/dev/null \
      | awk '/^[0-9.]+ *$/ {n++; if (n > 1) print $1}' > "$OUT/$label.cpu"
  fi
  waited=0
  while kill -0 $PID 2>/dev/null; do
    sleep 1; waited=$((waited + 1))
    [ $waited -ge 60 ] && { kill $PID; break; }
  done
  wait $PID 2>/dev/null
  rm -rf "$data"
}

run tour-demo    demo   ""   240
run tour-stress  stress ""   420
run bench-demo   demo   "b-" 300
run bench-stress stress "b-" 480
# The pill's transcript popup during an hour-long live meeting: the main
# window is minimised, the popup shows the transcript's tail.
run bench-live-pill stress "b-live" 240 --view=pill-expanded
idle idle-home   stress "i-home"   i-home
idle idle-note   stress "i-note"   i-note
idle idle-paused stress "i-paused" i-paused
idle idle-pill   demo   "i-pill"   i-pill --view=pill

echo "load at end: $(sysctl -n vm.loadavg)" | tee -a "$OUT/machine.txt"

python3 - "$OUT" "$HZ" <<'PY'
import re, sys, os, glob
out, hz = sys.argv[1], float(sys.argv[2])
frame = 1000.0 / hz
pattern = re.compile(
    r"stats (?P<step>\S+): (?P<renders>\d+) renders in (?P<seconds>[\d.]+) s \((?P<rate>[^)]*)\).*?"
    r"render mean (?P<mean>[\d.]+) ms, max (?P<max>[\d.]+) ms; (?P<writes>\d+) file writes; "
    r"(?P<copied>\d+) note bytes copied \((?P<per>\d+)/render\); frames (?P<n>\d+) median (?P<fm>[\d.]+) ms, "
    r"p95 (?P<fp>[\d.]+) ms, max (?P<fx>[\d.]+) ms; build median (?P<bm>[\d.]+) ms, p95 (?P<bp>[\d.]+) ms"
    r"(?:; list items built (?P<built>\d+) of (?P<listed>\d+))?"
    r"(?:; frame cpu (?P<cn>\d+) median (?P<cm>[\d.]+) ms, p95 (?P<cp>[\d.]+) ms, max (?P<cx>[\d.]+) ms)?")

# Steps whose renders are paced by the scenario rather than by the display:
# keys 40 ms apart, level changes and the 33 ms level bars, wheel events 50 ms
# apart. On a 60 Hz display those land on a 33 or 50 ms grid.
PACED = [("b-typing", 50.0), ("level-", 50.0), ("b-recording-speech", 33.4), ("b-silence", 50.0), ("b-scroll", 50.0)]

def verdict(step, n, median, worst):
    paced = next((cadence for prefix, cadence in PACED if step.startswith(prefix)), None)
    if n < 3:
        return ""
    if paced is None:
        if median > frame * 1.25:
            return "**over**"
        return "hitch" if worst > frame * 2 + 3 else ""
    if median > paced * 1.15:
        return "**over**"
    return "hitch" if worst > paced + frame * 2 + 3 else "paced"
lines = [f"# Cereal desktop benchmark\n", "```", open(os.path.join(out, "machine.txt")).read().strip(), "```", ""]
lines.append(f"Frame budget at {hz:g} Hz: {frame:.2f} ms. A step is marked **over** when the median gap between its "
             f"consecutive frames exceeds 1.25 budgets ({frame * 1.25:.1f} ms): it cannot keep up. `hitch` means it keeps up "
             f"but one frame took more than two refreshes ({frame * 2:.1f} ms). `paced` steps render when the scenario "
             "feeds them (a key every 40 ms, a wheel event every 50 ms, the 33 ms level bars), so their gaps show that "
             "cadence, not their cost; they are **over** only when they fall behind it.")
lines.append("`build` is the time spent in the window's render function; `frames` are gaps between consecutive renders "
             "of one animation (renders more than 100 ms apart are separate events, not frames). `frame cpu` is the time from "
             "the start of the window's render to the end of its paint pass (build, layout, prepaint and paint) for every "
             "frame of the step: all the work this process does for a frame before the scene goes to the GPU.\n")
lines.append("The `b-live-…` steps are an hour-long meeting (1,500 stored utterances) transcribed live: interim text arrives "
             "five times a second and every four seconds two utterances become final. Their frames are the redraws those "
             "updates cause plus the short fades of arriving lines; `frame cpu` is the number to hold against the budget.\n")
for name, title in [("tour-demo", "Motion tour, demo workspace"), ("tour-stress", "Motion tour, stress workspace"),
                    ("bench-demo", "Scenarios, demo workspace"), ("bench-stress", "Scenarios, stress workspace"),
                    ("bench-live-pill", "Live transcript with the pill's popup open (stress workspace; renders are the popup's)")]:
    path = os.path.join(out, name + ".log")
    if not os.path.exists(path):
        continue
    lines.append(f"## {title}\n")
    lines.append("| step | renders | per s | frames | frame median | p95 | max | build median | p95 | max | frame cpu median | p95 | max | writes | bytes copied/render | list items built | |")
    lines.append("| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |")
    for line in open(path, errors="replace"):
        m = pattern.search(line)
        if not m:
            continue
        g = m.groupdict()
        over = verdict(g["step"], int(g["n"]), float(g["fm"]), float(g["fx"]))
        built = f"{g['built']} of {g['listed']}" if g.get("listed") and g["listed"] != "0" else ""
        cpu = f"{g['cm']} | {g['cp']} | {g['cx']}" if g.get("cn") and g["cn"] != "0" else " | | "
        lines.append(f"| {g['step']} | {g['renders']} | {g['rate'].replace('/s', '')} | {g['n']} | {g['fm']} | {g['fp']} | {g['fx']} | "
                     f"{g['bm']} | {g['bp']} | {g['max']} | {cpu} | {g['writes']} | {g['per']} | {built} | {over} |")
    lines.append("")
checks = [line.strip() for name in ("bench-demo", "bench-stress")
          if os.path.exists(os.path.join(out, name + ".log"))
          for line in open(os.path.join(out, name + ".log"), errors="replace") if line.startswith("live check:")]
if checks:
    lines.append("## Live transcript scroll position\n")
    lines.append("Reported by the scenario itself (demo run, then stress run): a view that follows the transcript stays on its "
                 "newest text, and a reader who scrolled up is not moved while text keeps arriving.\n")
    lines.extend(f"- {check[len('live check: '):]}" for check in checks)
    lines.append("")
lines.append("## Idle\n")
lines.append("Each state is held for 14 s; CPU is sampled once a second for 8 s with `top` (percent of one core).\n")
lines.append("| state | renders while held | CPU mean % | CPU max % |")
lines.append("| --- | ---: | ---: | ---: |")
for name, step, title in [("idle-home", "i-home", "Home open"), ("idle-note", "i-note", "Note open"),
                          ("idle-paused", "i-paused", "Note open, recording paused"),
                          ("idle-pill", "i-pill", "Pill visible, preview recording live")]:
    log = os.path.join(out, name + ".log")
    renders = "?"
    if os.path.exists(log):
        for line in open(log, errors="replace"):
            m = pattern.search(line)
            if m and m.group("step") == step:
                renders = m.group("renders")
    cpu = [float(x) for x in open(os.path.join(out, name + ".cpu")).read().split()] if os.path.exists(os.path.join(out, name + ".cpu")) else []
    mean = f"{sum(cpu) / len(cpu):.1f}" if cpu else "?"
    peak = f"{max(cpu):.1f}" if cpu else "?"
    lines.append(f"| {title} | {renders} | {mean} | {peak} |")
lines.append("")
lines.append("The renders counted for a held state include the transition into it (a note opening, the capsule settling); "
             "a state that is really idle adds none after that. The pill row counts the capsule's own window.")
open(os.path.join(out, "results.md"), "w").write("\n".join(lines) + "\n")
print(os.path.join(out, "results.md"))
PY

if [ "$PROFILE" = release ] && [ $CLEAN -eq 1 ]; then
  echo "== removing target/release"
  rm -rf "$ROOT/target/release"
fi
echo "results: $OUT/results.md"
