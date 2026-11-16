/** What a request to this server's API failed with, in words a person can act on. It lives apart from the
 * typed client so that pages which only report a failure do not load the client to do it. */
export class AdminClientError extends Error {
  /** `serverMessage` is the server's own sentence, for answers that say what to change (an OpenAI-compatible
   * server that could not be reached, a model it did not accept). */
  constructor(readonly status: number, readonly serverMessage?: string) {
    super(status === 401 ? 'Your session has ended. Sign in again.'
      : status === 403 ? 'Your account does not have permission to do this.'
      : status === 400 ? 'Check the values and try again.'
      : status === 404 ? 'This item is no longer available. Refresh and try again.'
      : status === 409 ? 'This change conflicts with existing information. Refresh and try again.'
      : 'The server could not finish this request. Please try again.')
    this.name = 'AdminClientError'
  }
}
