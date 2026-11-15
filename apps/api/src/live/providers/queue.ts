/** A single-consumer async queue: adapters push provider events, the session iterates them. */
export class EventQueue<T> implements AsyncIterable<T> {
  private items: T[] = []
  private waiting: ((result: IteratorResult<T>) => void) | undefined
  private closed = false
  push(item: T) {
    if (this.closed) return
    if (this.waiting) { const resolve = this.waiting; this.waiting = undefined; resolve({ value: item, done: false }) }
    else this.items.push(item)
  }
  close() {
    this.closed = true
    if (this.waiting) { const resolve = this.waiting; this.waiting = undefined; resolve({ value: undefined, done: true }) }
  }
  get isClosed() { return this.closed }
  [Symbol.asyncIterator](): AsyncIterator<T> {
    return {
      next: () => {
        if (this.items.length) return Promise.resolve({ value: this.items.shift()!, done: false })
        if (this.closed) return Promise.resolve({ value: undefined, done: true })
        return new Promise(resolve => { this.waiting = resolve })
      },
      return: () => { this.close(); return Promise.resolve({ value: undefined, done: true }) },
    }
  }
}

/** Why a provider session could not be opened or kept. The message never contains provider text. */
export class LiveProviderError extends Error {
  constructor(readonly reason: "unavailable" | "over_limit" | "not_configured", readonly retryable: boolean, readonly detail?: string) { super(`live transcription ${reason}`) }
}
