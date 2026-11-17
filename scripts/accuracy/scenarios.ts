/** The fault scenarios of the accuracy harness (run.ts). Times are fixture milliseconds of meeting-5m. */
export type Action =
  | { kind: "net-down"; ms: number }
  | { kind: "restart"; ms: number }
  | { kind: "provider-drop" }
  | { kind: "throttle"; bytesPerSecond: number | null }
  | { kind: "sleep"; ms: number }
  | { kind: "sever" }
  | { kind: "pause"; ms: number }
export interface Capture { fromMs?: number; untilMs?: number; mic?: boolean; system?: boolean; systemDelayMs?: number; systemSkewPpm?: number; prerecordedMs?: number }
export interface Scenario { name: string; description: string; actions?: Array<{ atMs: number; action: Action }>; captures?: Capture[]; fixtures?: string[] }
/** Times are fixture milliseconds of meeting-5m (an action lands mid-sentence where it says so). */
export const SCENARIOS: Scenario[] = [
  { name: "baseline", description: "No faults." },
  { name: "net-drop", description: "Network lost mid-sentence for 8 s at 65 s and for 30 s at 160 s; the backlog is replayed.", actions: [{ atMs: 65_000, action: { kind: "net-down", ms: 8_000 } }, { atMs: 160_000, action: { kind: "net-down", ms: 30_000 } }] },
  { name: "sever", description: "TCP socket cut under the client mid-sentence at 105 s (reconnects at once).", actions: [{ atMs: 105_000, action: { kind: "sever" } }] },
  { name: "server-restart", description: "Server process dies mid-sentence at 104 s and is back 5 s later.", actions: [{ atMs: 104_000, action: { kind: "restart", ms: 5_000 } }] },
  { name: "provider-drop", description: "Provider sessions dropped mid-sentence at 75 s and 165 s.", actions: [{ atMs: 75_000, action: { kind: "provider-drop" } }, { atMs: 165_000, action: { kind: "provider-drop" } }] },
  { name: "stop-mid-word", description: "Stop while a word is being said (capture ends at 104.5 s).", captures: [{ untilMs: 104_500 }] },
  { name: "rapid-stop-start", description: "Stop mid-sentence at 105 s and start again at once (a second capture on the same note).", captures: [{ untilMs: 105_000 }, { fromMs: 105_000 }] },
  { name: "clock-skew", description: "System audio starts 700 ms after the microphone and its clock runs 300 ppm fast.", captures: [{ systemDelayMs: 700, systemSkewPpm: 300 }] },
  { name: "slow-uplink", description: "Uplink throttled to 30 kB/s (half of what two channels need) from 30 s to 150 s.", actions: [{ atMs: 30_000, action: { kind: "throttle", bytesPerSecond: 30_000 } }, { atMs: 150_000, action: { kind: "throttle", bytesPerSecond: null } }] },
  { name: "offline-backlog", description: "Offline from 20 s to 110 s; the 90 s backlog is replayed at up to 3x real time.", actions: [{ atMs: 20_000, action: { kind: "net-down", ms: 90_000 } }] },
  { name: "offline-start", description: "The stream starts with 60 s already captured (offline at Start).", captures: [{ prerecordedMs: 60_000 }] },
  { name: "sleep", description: "Laptop asleep for 45 s at 140 s: the client process and its capture clock stop, the network holds.", actions: [{ atMs: 140_000, action: { kind: "sleep", ms: 45_000 } }] },
  { name: "pause", description: "Recording paused for 20 s at 90 s, then resumed.", actions: [{ atMs: 90_000, action: { kind: "pause", ms: 20_000 } }] },
  { name: "mic-only", description: "Only the microphone channel is captured.", captures: [{ system: false }] },
]

