import { Effect, Result } from 'effect'
import { FetchHttpClient } from 'effect/http'
import { HttpApiClient } from 'effect/http-api'
import { MeetingsApi } from '@meeting-notes/contracts/api'
import type { ModelCatalogInput, NoteListFilter, OnboardingStepInput, StorageTestInput } from '@meeting-notes/contracts'
import type { CreateCalendarInput, CreateUserInput, UpdateAdminSettingsInput, UpdateCalendarInput, UpdateUserRoleInput, CreateOrganizationInput, UpdateOrganizationInput, InviteOrganizationMemberInput, UpdateOrganizationMemberRoleInput } from '@meeting-notes/contracts'

type Client = HttpApiClient.ForApi<typeof MeetingsApi>

export class AdminClientError extends Error {
  constructor(readonly status: number) {
    super(status === 401 ? 'Your session has ended. Sign in again.'
      : status === 403 ? 'Your account does not have permission to do this.'
      : status === 400 ? 'Check the values and try again.'
      : status === 404 ? 'This item is no longer available. Refresh and try again.'
      : status === 409 ? 'This change conflicts with existing information. Refresh and try again.'
      : 'The server could not finish this request. Please try again.')
    this.name = 'AdminClientError'
  }
}

/** Same-origin cookie transport. JavaScript never reads the HttpOnly session cookie
 * or a bootstrap token. Responses use the canonical Effect HttpApi schemas. */
export function createAdminClient(baseUrl: string, transport: typeof fetch = globalThis.fetch) {
  const origin = new URL(baseUrl).origin
  const authorizedFetch: typeof fetch = Object.assign(async (...[input, init]: Parameters<typeof fetch>) => {
    const request = new Request(input, init)
    if (new URL(request.url).origin !== origin) throw new AdminClientError(0)
    request.headers.delete('Authorization')
    return transport(request, {
      credentials: 'same-origin', cache: 'no-store', redirect: 'error', referrerPolicy: 'no-referrer',
    })
  }, { preconnect: transport.preconnect })
  async function execute<A, E>(operation: (client: Client) => Effect.Effect<A, E>, signal?: AbortSignal): Promise<A> {
    const program = Effect.gen(function* () {
      const client = yield* HttpApiClient.make(MeetingsApi, { baseUrl: origin })
      return yield* operation(client)
    }).pipe(
      Effect.provide(FetchHttpClient.layer),
      Effect.provideService(FetchHttpClient.Fetch, authorizedFetch),
      Effect.provideService(FetchHttpClient.RequestInit, { credentials: 'same-origin', cache: 'no-store', redirect: 'error', referrerPolicy: 'no-referrer' }),
    )
    const result = await Effect.runPromise(Effect.result(program), { signal })
    if (Result.isFailure(result)) {
      if (signal?.aborted) throw new DOMException('Request canceled', 'AbortError')
      const error = result.failure
      const kind = typeof error === 'object' && error !== null && 'error' in error ? error.error : undefined
      throw new AdminClientError(kind === 'unauthorized' ? 401 : kind === 'forbidden' ? 403 : kind === 'conflict' ? 409 : kind === 'invalid_request' ? 400 : kind === 'not_found' ? 404 : 503)
    }
    return result.success
  }
  return {
    listOrganizations: (signal?: AbortSignal) => execute(c => c.owner.listOrganizations(), signal),
    createOrganization: (payload: CreateOrganizationInput, signal?: AbortSignal) => execute(c => c.owner.createOrganization({ payload }), signal),
    getOrganization: (id: string, signal?: AbortSignal) => execute(c => c.owner.getOrganization({ params: { id } }), signal),
    updateOrganization: (id: string, payload: UpdateOrganizationInput, signal?: AbortSignal) => execute(c => c.owner.updateOrganization({ params: { id }, payload }), signal),
    setActiveOrganization: (organizationId: string, signal?: AbortSignal) => execute(c => c.owner.setActiveOrganization({ payload: { organizationId } }), signal),
    inviteOrganizationMember: (id: string, payload: InviteOrganizationMemberInput, signal?: AbortSignal) => execute(c => c.owner.inviteOrganizationMember({ params: { id }, payload }), signal),
    cancelOrganizationInvitation: (id: string, invitationId: string, signal?: AbortSignal) => execute(c => c.owner.cancelOrganizationInvitation({ params: { id, invitationId } }), signal),
    updateOrganizationMemberRole: (id: string, memberId: string, payload: UpdateOrganizationMemberRoleInput, signal?: AbortSignal) => execute(c => c.owner.updateOrganizationMemberRole({ params: { id, memberId }, payload }), signal),
    removeOrganizationMember: (id: string, memberId: string, signal?: AbortSignal) => execute(c => c.owner.removeOrganizationMember({ params: { id, memberId } }), signal),
    listOrganizationInvitations: (signal?: AbortSignal) => execute(c => c.owner.listOrganizationInvitations(), signal),
    acceptOrganizationInvitation: (id: string, signal?: AbortSignal) => execute(c => c.owner.acceptOrganizationInvitation({ params: { id } }), signal),
    rejectOrganizationInvitation: (id: string, signal?: AbortSignal) => execute(c => c.owner.rejectOrganizationInvitation({ params: { id } }), signal),
    getGoogleStatus: (signal?: AbortSignal) => execute(c => c.owner.getGoogleStatus(), signal),
    beginGoogleConnect: (contacts: boolean, signal?: AbortSignal) => execute(c => c.owner.beginGoogleConnect({ payload: { contacts } }), signal),
    listGoogleCalendars: (id: string, signal?: AbortSignal) => execute(c => c.owner.listGoogleCalendars({ params: { id } }), signal),
    updateGoogleCalendar: (id: string, selected: boolean, signal?: AbortSignal) => execute(c => c.owner.updateGoogleCalendar({ params: { id }, payload: { selected } }), signal),
    syncGoogleConnection: (id: string, signal?: AbortSignal) => execute(c => c.owner.syncGoogleConnection({ params: { id } }), signal),
    deleteGoogleConnection: (id: string, signal?: AbortSignal) => execute(c => c.owner.deleteGoogleConnection({ params: { id } }), signal),
    getNoteContext: (id: string, signal?: AbortSignal) => execute(c => c.owner.getNoteContext({ params: { id } }), signal),
    updateNoteContext: (id: string, eventId: string | null, signal?: AbortSignal) => execute(c => c.owner.updateNoteContext({ params: { id }, payload: { eventId } }), signal),
    getSession: (signal?: AbortSignal) => execute(c => c.owner.getSession(), signal),
    listUsers: (signal?: AbortSignal) => execute(c => c.owner.listUsers(), signal),
    createUser: (payload: CreateUserInput, signal?: AbortSignal) => execute(c => c.owner.createUser({ payload }), signal),
    createAdministrator: (payload: CreateUserInput, signal?: AbortSignal) => execute(c => c.owner.createAdministrator({ payload }), signal),
    updateUserRole: (id: string, payload: UpdateUserRoleInput, signal?: AbortSignal) => execute(c => c.owner.updateUserRole({ params: { id }, payload }), signal),
    listNotes: (signal?: AbortSignal) => execute(c => c.owner.listNotes(), signal),
    /** A page of list rows: no bodies, no transcripts. */
    listNoteSummaries: (page: { limit?: number; cursor?: string; filter?: NoteListFilter } = {}, signal?: AbortSignal) => execute(c => c.owner.listNoteSummaries({ query: {
      ...(page.limit === undefined ? {} : { limit: String(page.limit) }), ...(page.cursor === undefined ? {} : { cursor: page.cursor }), ...(page.filter === undefined || page.filter === 'all' ? {} : { filter: page.filter }),
    } }), signal),
    searchNotes: (search: { q: string; limit?: number; cursor?: string; filter?: NoteListFilter }, signal?: AbortSignal) => execute(c => c.owner.searchNotes({ query: {
      q: search.q, ...(search.limit === undefined ? {} : { limit: String(search.limit) }), ...(search.cursor === undefined ? {} : { cursor: search.cursor }), ...(search.filter === undefined || search.filter === 'all' ? {} : { filter: search.filter }),
    } }), signal),
    getNote: (id: string, signal?: AbortSignal) => execute(c => c.owner.getNote({ params: { id } }), signal),
    getSettings: (signal?: AbortSignal) => execute(c => c.owner.getAdminSettings(), signal),
    getModelCatalog: (payload: ModelCatalogInput, signal?: AbortSignal) => execute(c => c.owner.getModelCatalog({payload}), signal),
    updateSettings: (payload: UpdateAdminSettingsInput, signal?: AbortSignal) => execute(c => c.owner.updateAdminSettings({ payload }), signal),
    /** First-run setup: records a step as done or skipped, finishes setup, or (the owner only) shows it again. Each answers with the settings as they now stand. */
    recordOnboardingStep: (payload: OnboardingStepInput, signal?: AbortSignal) => execute(c => c.owner.recordOnboardingStep({ payload }), signal),
    completeOnboarding: (signal?: AbortSignal) => execute(c => c.owner.completeOnboarding(), signal),
    restartOnboarding: (signal?: AbortSignal) => execute(c => c.owner.restartOnboarding(), signal),
    /** Tries the storage settings with a draft laid over them: a small file is written, read back and deleted. Nothing is saved. */
    testStorage: (payload: StorageTestInput, signal?: AbortSignal) => execute(c => c.owner.testStorage({ payload }), signal),
    listCalendars: (signal?: AbortSignal) => execute(c => c.owner.listCalendars(), signal),
    createCalendar: (payload: CreateCalendarInput, signal?: AbortSignal) => execute(c => c.owner.createCalendar({ payload }), signal),
    updateCalendar: (id: string, payload: UpdateCalendarInput, signal?: AbortSignal) => execute(c => c.owner.updateCalendar({ params: { id }, payload }), signal),
    deleteCalendar: (id: string, signal?: AbortSignal) => execute(c => c.owner.deleteCalendar({ params: { id } }), signal),
    syncCalendar: (id: string, signal?: AbortSignal) => execute(c => c.owner.syncCalendar({ params: { id } }), signal),
    listEvents: (from: string, to: string, signal?: AbortSignal) => execute(c => c.owner.listCalendarEvents({ query: { from, to } }), signal),
    prepareNote: (id: string, signal?: AbortSignal) => execute(c => c.owner.createCalendarEventNote({ params: { id } }), signal),
  }
}
export type AdminClient = ReturnType<typeof createAdminClient>
