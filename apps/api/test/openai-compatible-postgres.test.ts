/** The OpenAI-compatible provider's settings in PostgreSQL: the address in its own column, tidy; the key sealed
 * and never returned; and live transcription read as on where an older server stored "off". */
import { afterAll, describe, expect, test } from "bun:test"
import { Effect } from "effect"
import { makePrismaCalendarStore } from "../src/calendar-store"
import { makePrismaRepository } from "../src/prisma-repository"
import { makeAdministrationService } from "../src/administration"

const url = process.env.DATABASE_URL
const enabled = process.env.RUN_PG_TESTS === "1" && Boolean(url)
const settingsId = "openai-compatible-test"

describe.skipIf(!enabled)("OpenAI-compatible settings on PostgreSQL", () => {
  const resources = enabled ? makePrismaRepository(url!) : null
  afterAll(async () => {
    await resources?.database.instanceSettings.deleteMany({ where: { id: settingsId } }).catch(() => undefined)
    await resources?.close()
  })
  test("the address and key round-trip; the key is sealed and never returned", async () => {
    const key = "sk-compatible-postgres-key-0123WXYZ"
    const transport = Object.assign(async (input: unknown) => String(input).endsWith("/models") ? Response.json({ data: [{ id: "llama3.2:3b" }] }) : Response.json({ choices: [{ message: { content: "OK" } }] }), { preconnect: () => {} }) as typeof fetch
    const store = makePrismaCalendarStore(resources!.database, settingsId), admin = makeAdministrationService(store, undefined, { fetch: transport, environment: {} })
    const saved = await Effect.runPromise(admin.updateSettings({ openAiCompatibleBaseUrl: "http://10.0.0.5:11434/v1/", openAiCompatibleApiKey: key, chatModels: [{ provider: "openai_compatible", model: "llama3.2:3b" }] }))
    expect(saved.openAiCompatible).toEqual({ baseUrl: "http://10.0.0.5:11434/v1", apiKeySet: true, apiKeyHint: "sk-…WXYZ" })
    expect(JSON.stringify(saved)).not.toContain(key)
    const row = await resources!.database.instanceSettings.findUniqueOrThrow({ where: { id: settingsId } })
    expect(row.openAiCompatibleBaseUrl).toBe("http://10.0.0.5:11434/v1")
    expect(row.openAiCompatibleSecret).toStartWith("v1.")
    expect(row.openAiCompatibleSecret).not.toContain(key)
    expect(await admin.configuration()).toMatchObject({ languageProvider: "openai_compatible", languageModel: "llama3.2:3b", openAiCompatibleBaseUrl: "http://10.0.0.5:11434/v1", openAiCompatibleKey: key })
    // An unrelated save keeps them; null clears each.
    await Effect.runPromise(admin.updateSettings({ instanceName: "Compatible" }))
    expect((await store.getSettings())!.openAiCompatibleBaseUrl).toBe("http://10.0.0.5:11434/v1")
    // The address stays while chat uses the server; with chat moved elsewhere in the same change it can go.
    await expect(Effect.runPromise(admin.updateSettings({ openAiCompatibleBaseUrl: null }))).rejects.toMatchObject({ error: "invalid_request", message: expect.stringContaining("still used for chat") })
    expect((await resources!.database.instanceSettings.findUniqueOrThrow({ where: { id: settingsId } })).openAiCompatibleBaseUrl).toBe("http://10.0.0.5:11434/v1")
    const cleared = await Effect.runPromise(admin.updateSettings({ openAiCompatibleApiKey: null, openAiCompatibleBaseUrl: null, chatModels: [{ provider: "openrouter", model: "openai/gpt-6-sol" }] }))
    expect(cleared.openAiCompatible).toEqual({ baseUrl: null, apiKeySet: false, apiKeyHint: null })
    // With no live model stored, the default live model is in use.
    expect((await Effect.runPromise(admin.getSettings())).liveTranscriptionModel).toEqual({ provider: "elevenlabs", model: "scribe_v3_live" })
  })
})
