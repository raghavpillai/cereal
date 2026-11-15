import type { SharedNoteDTO } from '@meeting-notes/contracts'
import { createSharedNotesClient, SharedNoteClientError } from '@meeting-notes/contracts/client'

/** Reading a shared snapshot. A link is read by an account on its server: the request
 * carries the session cookie, and a server that does not know the reader says so. */
export type SharedNoteResult = { kind: 'ready'; note: SharedNoteDTO } | { kind: 'unavailable' } | { kind: 'signed-out' }

export class ReaderLoadError extends Error {
  constructor() { super('Shared note could not be loaded') }
}

const TOKEN = /^[A-Za-z0-9_-]{43}$/

export async function getSharedNote(token: string, signal?: AbortSignal, transport: typeof fetch = globalThis.fetch, origin: string = window.location.origin): Promise<SharedNoteResult> {
  // A token that cannot be one is never sent anywhere.
  if (!TOKEN.test(token)) return { kind: 'unavailable' }
  try {
    return { kind: 'ready', note: await createSharedNotesClient(new URL(origin).origin, transport).getSharedNote(token, signal) }
  } catch (error) {
    if (signal?.aborted) throw error
    // Not signed in: the page goes to sign-in and comes back. Revoked, expired or unknown: unavailable.
    if (error instanceof SharedNoteClientError && error.status === 401) return { kind: 'signed-out' }
    if (error instanceof SharedNoteClientError && error.status === 404) return { kind: 'unavailable' }
    throw new ReaderLoadError()
  }
}
