/**
 * Maps a provider's clock to the capture's clock.
 *
 * A streaming provider reports times as seconds of audio since its session opened. That clock runs on
 * whatever the server sent upstream: capture audio, but also keep-alive silence, and it does not notice
 * when the client skipped ahead. The timeline keeps one anchor per discontinuity so a provider time can
 * be turned back into a capture sample index, and so the stretch of capture audio behind a span of
 * provider time can be listed exactly (which is what the coverage ledger records).
 */
interface Anchor { upstream: number; capture: number; frozen: boolean }

export class Timeline {
  private anchors: Anchor[] = []
  /** Samples sent upstream so far, of any kind. */
  upstreamSamples = 0
  constructor(readonly sampleRate: number) {}

  /** `samples` of capture audio starting at capture sample `captureSample` are being sent upstream now. */
  audio(captureSample: number, samples: number) {
    const last = this.anchors.at(-1)
    if (!last || last.frozen || last.capture + (this.upstreamSamples - last.upstream) !== captureSample) this.anchors.push({ upstream: this.upstreamSamples, capture: captureSample, frozen: false })
    this.upstreamSamples += samples
  }
  /** Silence that exists only upstream (a keep-alive) while the capture stands at `captureSample`. */
  silence(captureSample: number, samples: number) {
    const last = this.anchors.at(-1)
    if (!last || !last.frozen) this.anchors.push({ upstream: this.upstreamSamples, capture: captureSample, frozen: true })
    this.upstreamSamples += samples
  }
  private anchorAt(upstream: number) {
    let found = this.anchors[0]
    for (const anchor of this.anchors) { if (anchor.upstream <= upstream) found = anchor; else break }
    return found
  }
  /** Capture sample index for a provider time. Times inside injected silence map to where the capture stood. */
  captureSample(upstreamSeconds: number): number {
    const upstream = Math.max(0, Math.min(this.upstreamSamples, Math.round(upstreamSeconds * this.sampleRate)))
    const anchor = this.anchorAt(upstream)
    if (!anchor) return 0
    return anchor.frozen ? anchor.capture : anchor.capture + (upstream - anchor.upstream)
  }
  /** The capture audio sent between two upstream positions, as [from, to) sample ranges in capture order. */
  captureRanges(fromUpstream: number, toUpstream: number): Array<[number, number]> {
    const from = Math.max(0, fromUpstream), to = Math.min(this.upstreamSamples, toUpstream), out: Array<[number, number]> = []
    for (let index = 0; index < this.anchors.length; index++) {
      const anchor = this.anchors[index]!, end = this.anchors[index + 1]?.upstream ?? this.upstreamSamples
      const start = Math.max(anchor.upstream, from), stop = Math.min(end, to)
      if (anchor.frozen || stop <= start) continue
      const range: [number, number] = [anchor.capture + (start - anchor.upstream), anchor.capture + (stop - anchor.upstream)]
      const last = out.at(-1)
      if (last && last[1] === range[0]) last[1] = range[1]; else out.push(range)
    }
    return out
  }
  /** Forget anchors that only describe audio before `upstream`; the mapping for later times is unchanged. */
  prune(upstream: number) {
    while (this.anchors.length > 1 && this.anchors[1]!.upstream <= upstream) this.anchors.shift()
  }
  get anchorCount() { return this.anchors.length }
}
