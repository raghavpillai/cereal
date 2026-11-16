/** The address of an OpenAI-compatible server (Ollama, vLLM, LM Studio, a hosted /v1 API), checked the same way
 * by the server, which is authoritative, and by the web app, which says what is wrong before anything is sent.
 *
 * The address is where the API starts: requests go to `{base}/chat/completions` and `{base}/models`. It is used as
 * given, with or without a trailing `/v1`; nothing is guessed or added. Plain http is allowed, because the usual
 * server is on this machine or the local network. Pure: no DNS, no network. */
export class CompatibleUrlError extends Error {}

/** The tidy form of an address, or a `CompatibleUrlError` that says, in a sentence, what to change. */
export function normalizeCompatibleBaseUrl(raw: string): string {
  const value = raw.trim()
  if (!value) throw new CompatibleUrlError("Enter the server’s address, such as http://localhost:11434/v1.")
  if (value.length > 2048) throw new CompatibleUrlError("That address is too long.")
  let url: URL
  try { url = new URL(value) } catch { throw new CompatibleUrlError("Enter a full address that starts with http:// or https://, such as http://localhost:11434/v1.") }
  if ((url.protocol !== "http:" && url.protocol !== "https:") || !/^https?:\/\//i.test(value) || !url.hostname || /\s/.test(value))
    throw new CompatibleUrlError("Enter a full address that starts with http:// or https://, such as http://localhost:11434/v1.")
  if (url.username || url.password) throw new CompatibleUrlError("Leave the user name and password out of the address. Put a key in the API key field.")
  if (url.search || url.hash || value.includes("?") || value.includes("#")) throw new CompatibleUrlError("Leave out anything after ? or # in the address.")
  return `${url.protocol}//${url.host}${url.pathname.replace(/\/+$/, "")}`
}

/** What is wrong with an address as typed, or null when the server will take it. */
export function compatibleBaseUrlProblem(raw: string): string | null {
  try { normalizeCompatibleBaseUrl(raw); return null } catch (error) { return error instanceof CompatibleUrlError ? error.message : "Enter a valid address." }
}

/** The host to name in a message: "10.0.0.5:11434". */
export function compatibleHost(base: string): string {
  try { return new URL(base).host } catch { return base }
}
