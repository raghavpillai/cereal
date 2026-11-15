import { Effect, Result } from "effect"
import { FetchHttpClient } from "effect/http"
import { HttpApiClient } from "effect/http-api"
import { MeetingsApi } from "./api"
import type { SharedNoteDTO } from "./index"

export class SharedNoteClientError extends Error {
  constructor(readonly status: number, message: string) { super(message); this.name = "SharedNoteClientError" }
}

/** Typed anonymous reader. No owner credential or cookie can enter this client. */
export function createSharedNotesClient(baseUrl: string, fetchImplementation: typeof fetch = globalThis.fetch) {
  return {
    async getSharedNote(token: string, signal?: AbortSignal): Promise<SharedNoteDTO> {
      const program = Effect.gen(function*() {
        const client = yield* HttpApiClient.make(MeetingsApi, { baseUrl })
        return yield* client.public.getSharedNote({ params: { token } })
      }).pipe(
        Effect.provide(FetchHttpClient.layer),
        Effect.provideService(FetchHttpClient.Fetch, fetchImplementation),
        Effect.provideService(FetchHttpClient.RequestInit, { credentials: "omit", cache: "no-store", referrerPolicy: "no-referrer", redirect: "error" }),
      )
      const result = await Effect.runPromise(Effect.result(program), { signal })
      if (Result.isFailure(result)) {
        const error = result.failure
        if (typeof error === "object" && error !== null && "error" in error) {
          if (error.error === "not_found") throw new SharedNoteClientError(404, "Shared note not found")
          if (error.error === "unavailable") throw new SharedNoteClientError(503, "The note service is temporarily unavailable")
        }
        throw new SharedNoteClientError(0, "Unable to load the shared note")
      }
      return result.success
    },
  }
}
