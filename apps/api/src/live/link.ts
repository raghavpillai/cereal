/**
 * A provider session together with the voices it has numbered.
 *
 * A streaming provider's speaker ids mean something only inside one provider session, so the sure way
 * to keep a note's speaker labels across a Stop and a Resume is to keep that session. The hub holds
 * the links of a live session that ended in an orderly way for `holdMs`, and the note's next live
 * session takes them over (`LiveSession.open`). The loop that reads the provider's events therefore
 * belongs to the link and hands each event to whichever live session owns it at the time.
 */
import type { LiveTranscriberEvent, LiveTranscriberSession } from "@meeting-notes/contracts/live"
import type { AudioChannelName } from "./repository"

/** What a held provider session was opened for. Only an identical request may take it over. */
export interface LinkKey { readonly provider: string; readonly model: string; readonly channel: AudioChannelName; readonly sampleRate: number; readonly language?: string }
type MergeEvent = Extract<LiveTranscriberEvent, { type: "speakers_merged" }>
/** `null` says the provider session is over. */
export type LinkOwner = (event: LiveTranscriberEvent | null) => unknown

export class ProviderLink {
  /** Provider speaker to note speaker number. */
  speakers: Record<string, number> = {}
  /** Samples sent since the provider session opened, of any kind: where the provider's clock stands. */
  sent = 0
  /** The provider session ended or reported an error: it cannot be taken over. */
  lost = false
  /** Settles when the provider session has delivered its last event. */
  readonly done: Promise<void>
  private owner: LinkOwner | undefined
  /** Speaker merges the provider announced while nobody owned the link. */
  private merges: MergeEvent[] = []

  constructor(readonly session: LiveTranscriberSession, readonly abort: AbortController, readonly key: LinkKey) { this.done = this.read() }
  private async read() {
    try {
      for await (const event of this.session.events) {
        if (event.type === "error") this.lost = true
        // One event at a time, so finals are stored in the order the provider produced them.
        if (this.owner) await this.owner(event)
        else if (event.type === "speakers_merged") this.merges.push(event)
      }
    } catch { /* treated as a lost provider session */ }
    this.lost = true
    try { await this.owner?.(null) } catch { /* the owner is already gone */ }
  }
  /** Events go to `owner` from now on. Returns the merges announced while the link was held. */
  own(owner: LinkOwner): MergeEvent[] { this.owner = owner; return this.merges.splice(0) }
  /** The owning session is over; text the provider still produces for it is dropped. */
  release() { this.owner = undefined }
  fits(key: LinkKey) {
    const mine = this.key
    return !this.lost && mine.provider === key.provider && mine.model === key.model && mine.channel === key.channel && mine.sampleRate === key.sampleRate && mine.language === key.language
  }
  /** Keeps the provider from closing an idle session. Silence a keep-alive adds moves its clock. */
  keepAlive() { if (!this.lost) this.sent += Math.round(this.session.keepAlive() * this.key.sampleRate) }
  async close() { this.lost = true; this.owner = undefined; this.abort.abort(); await this.session.close().catch(() => undefined) }
}
