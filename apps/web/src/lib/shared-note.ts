import { createSharedNotesClient, SharedNoteClientError } from '@meeting-notes/contracts/client'

export class ReaderLoadError extends Error {
  constructor() { super('Shared note could not be loaded') }
}

export async function getSharedNote(token: string, signal?: AbortSignal) {
  try {
    const note = await createSharedNotesClient(window.location.origin).getSharedNote(token, signal)
    return { kind: 'ready' as const, note }
  } catch (error) {
    if (signal?.aborted) throw error
    if (error instanceof SharedNoteClientError && error.status === 404) return { kind: 'unavailable' as const }
    throw new ReaderLoadError()
  }
}
