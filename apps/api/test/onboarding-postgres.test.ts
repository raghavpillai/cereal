import { afterAll, beforeAll, describe, expect, test } from "bun:test"
import { randomBytes } from "node:crypto"
import { readFile } from "node:fs/promises"
import { resolve } from "node:path"
import type { AdminSettingsDTO } from "@meeting-notes/contracts"
import { makeAuthenticationService, BOOTSTRAP_OWNER_ID } from "../src/auth"
import { makePrismaRepository } from "../src/prisma-repository"
import { makePrismaCalendarStore } from "../src/calendar-store"
import { makeAdministrationService } from "../src/administration"
import { secretBoxFromKey } from "../src/admin-secrets"
import { makeApiHandler } from "../src/api"

const enabled = process.env.RUN_AUTH_PG_TESTS === "1" && !!process.env.DATABASE_URL

/** Runs only in a dedicated throwaway database: it returns the bootstrap owner to "never claimed", claims it and
 * unclaims it again, so it must not share a process with tests that are signed in as that owner. */
describe.skipIf(!enabled)("first-run setup on PostgreSQL: claiming, recorded steps, finishing and the migration rule", () => {
  const resources = enabled ? makePrismaRepository(process.env.DATABASE_URL!) : null, db = resources?.database
  const setupCode = "isolated-onboarding-setup-code-at-least-32-characters", baseURL = "http://localhost:3000", settingsId = "onboarding-test"
  const authentication = db ? makeAuthenticationService(db, setupCode, { secret: randomBytes(32).toString("base64url"), baseURL }) : null
  const administration = db ? makeAdministrationService(makePrismaCalendarStore(db, settingsId), secretBoxFromKey(randomBytes(32)), { environment: {} }) : null
  const api = resources ? makeApiHandler(resources.repository, setupCode, { authentication: authentication!, administration: administration! }) : null
  const password = "test-only-password-" + randomBytes(12).toString("hex")
  const request = (path: string, method = "GET", body?: unknown, credential: string | null = setupCode) => api!.handler(new Request(baseURL + path, {
    method, headers: { ...(credential ? { Authorization: "Bearer " + credential } : {}), ...(body === undefined ? {} : { "Content-Type": "application/json" }) },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  }))
  const settings = async (credential: string | null = setupCode) => await (await request("/api/v1/admin/settings", "GET", undefined, credential)).json() as AdminSettingsDTO
  const methods = async () => await (await request("/api/v1/auth/methods", "GET", undefined, null)).json() as { setupRequired?: boolean }
  async function signIn(email: string) {
    const response = await request("/api/auth/sign-in/email", "POST", { email, password, rememberMe: true }, null)
    expect(response.status).toBe(200)
    return response.headers.get("set-auth-token")!
  }

  /** A server nobody has claimed: the owner row exists (the migration creates it) and has no password. */
  async function unclaim() {
    await db!.session.deleteMany({ where: { userId: BOOTSTRAP_OWNER_ID } })
    await db!.account.deleteMany({ where: { userId: BOOTSTRAP_OWNER_ID } })
    await db!.user.deleteMany({ where: { email: { in: ["claimed-owner@example.test", "setup-admin@example.test"] }, id: { not: BOOTSTRAP_OWNER_ID } } })
    await db!.user.update({ where: { id: BOOTSTRAP_OWNER_ID }, data: { name: "Owner", email: "owner@localhost.invalid" } })
    await db!.instanceSettings.deleteMany({ where: { id: settingsId } })
  }
  beforeAll(unclaim)
  // Left as it was found, so a file that claims the owner itself can run after this one.
  afterAll(async () => { await unclaim().catch(() => {}); await api?.dispose(); await resources?.close() })

  test("an unclaimed server says so publicly, and cannot be finished", async () => {
    expect((await methods()).setupRequired).toBe(true)
    // The setup code works before anyone has an account; the owner step is still open.
    const fresh = await settings()
    expect(fresh.onboarding).toEqual({ completedAt: null, steps: { owner: false, name: false, language: false, speech: false, models: false, signIn: false, storage: false } })
    const early = await request("/api/v1/admin/onboarding/complete", "POST")
    expect(early.status).toBe(409)
    expect((await early.json()).message).toBe("Finish the required setup steps first: owner, language, speech")
    expect((await request("/api/v1/admin/onboarding", "POST", { step: "owner", state: "done" })).status).toBe(409)
  })

  test("claiming with the setup code creates the owner; the server is then no longer waiting", async () => {
    expect((await request("/api/v1/admin/owner/setup", "POST", { name: "Claimed Owner", email: "claimed-owner@example.test", password }, "not-the-setup-code-not-the-setup-code")).status).toBe(401)
    expect((await methods()).setupRequired).toBe(true)
    const claimed = await request("/api/v1/admin/owner/setup", "POST", { name: "Claimed Owner", email: "claimed-owner@example.test", password })
    expect(claimed.status).toBe(200)
    expect((await claimed.json()).id).toBe(BOOTSTRAP_OWNER_ID)
    expect((await methods()).setupRequired).toBe(false)
    expect((await request("/api/v1/admin/owner/setup", "POST", { name: "Again", email: "again@example.test", password })).status).toBe(409)
    // The new credentials sign in, and that session sees the owner step as done.
    const session = await signIn("claimed-owner@example.test")
    expect((await settings(session)).onboarding!.steps.owner).toBe(true)
  })

  test("recorded steps and the finished time are stored; finishing and restarting keep their rules", async () => {
    const owner = await signIn("claimed-owner@example.test")
    const created = await request("/api/v1/admin/users", "POST", { name: "Setup Admin", email: "setup-admin@example.test", password })
    expect(created.status).toBe(200)
    expect((await request(`/api/v1/admin/users/${(await created.json()).id}/role`, "PATCH", { role: "admin" })).status).toBe(200)
    const admin = await signIn("setup-admin@example.test")

    expect((await request("/api/v1/admin/onboarding", "POST", { step: "signIn", state: "done" }, admin)).status).toBe(200)
    expect((await request("/api/v1/admin/onboarding", "POST", { step: "storage", state: "skipped" }, owner)).status).toBe(200)
    expect((await request("/api/v1/admin/settings", "PATCH", { instanceName: "Harbour Studio" }, owner)).status).toBe(200)
    const row = await db!.instanceSettings.findUniqueOrThrow({ where: { id: settingsId } })
    expect(row.onboardingSteps).toEqual({ signIn: "done", storage: "skipped" })
    expect(row.onboardingCompletedAt).toBeNull()
    expect((await settings(owner)).onboarding!.steps).toEqual({ owner: true, name: true, language: false, speech: false, models: false, signIn: true, storage: true })

    // Still missing the two provider keys.
    const refused = await request("/api/v1/admin/onboarding/complete", "POST", undefined, owner)
    expect(refused.status).toBe(409)
    expect((await refused.json()).message).toBe("Finish the required setup steps first: language, speech")
    expect((await request("/api/v1/admin/settings", "PATCH", { openAiApiKey: "sk-onboarding-postgres-openai-0001", elevenLabsApiKey: "sk_onboarding_postgres_eleven_0002" }, owner)).status).toBe(200)

    const before = Date.now()
    const done = await request("/api/v1/admin/onboarding/complete", "POST", undefined, admin)
    expect(done.status).toBe(200)
    const completedAt = (await done.json() as AdminSettingsDTO).onboarding!.completedAt!
    expect(Math.abs(Date.parse(completedAt) - before)).toBeLessThan(60_000)
    // What was stored reads back as the same instant.
    expect((await db!.instanceSettings.findUniqueOrThrow({ where: { id: settingsId } })).onboardingCompletedAt!.toISOString()).toBe(completedAt)
    expect((await settings(owner)).onboarding!.completedAt).toBe(completedAt)

    const forbidden = await request("/api/v1/admin/onboarding/restart", "POST", undefined, admin)
    expect(forbidden.status).toBe(403)
    expect((await settings(owner)).onboarding!.completedAt).toBe(completedAt)
    const restarted = await request("/api/v1/admin/onboarding/restart", "POST", undefined, owner)
    expect(restarted.status).toBe(200)
    expect((await restarted.json() as AdminSettingsDTO).onboarding).toEqual({ completedAt: null, steps: { owner: true, name: true, language: true, speech: true, models: false, signIn: false, storage: false } })
    const cleared = await db!.instanceSettings.findUniqueOrThrow({ where: { id: settingsId } })
    expect(cleared.onboardingCompletedAt).toBeNull()
    expect(cleared.onboardingSteps).toEqual({})
    expect(cleared.openAiSecret).not.toBeNull()
  })

  test("the migration marks a server that already had a provider key as set up, and leaves a fresh one alone", async () => {
    const sql = await readFile(resolve(import.meta.dir, "../../../packages/database/prisma/migrations/20261128000000_onboarding/migration.sql"), "utf8")
    const rule = /UPDATE "InstanceSettings"[\s\S]*?;/.exec(sql)?.[0]
    expect(rule).toBeDefined()
    const seen = new Map<string, Date | null>()
    class Undo extends Error {}
    // Tried on rows as they were before the columns existed, then rolled back: nothing here outlives the test.
    await db!.$transaction(async tx => {
      await tx.instanceSettings.createMany({ data: [
        { id: "rule-fresh" }, { id: "rule-named", instanceName: "Named, no keys" },
        { id: "rule-router", openRouterSecret: "v1.sealed.router.key" }, { id: "rule-openai", openAiSecret: "v1.sealed.openai.key" },
        { id: "rule-anthropic", anthropicSecret: "v1.sealed.anthropic.key" }, { id: "rule-eleven", elevenLabsSecret: "v1.sealed.eleven.key" },
      ] })
      await tx.$executeRawUnsafe(rule!)
      for (const row of await tx.instanceSettings.findMany({ where: { id: { startsWith: "rule-" } } })) seen.set(row.id, row.onboardingCompletedAt)
      throw new Undo()
    }).catch(error => { if (!(error instanceof Undo)) throw error })
    expect(seen.size).toBe(6)
    expect(seen.get("rule-fresh")).toBeNull()
    expect(seen.get("rule-named")).toBeNull()
    for (const id of ["rule-router", "rule-openai", "rule-anthropic", "rule-eleven"]) expect(seen.get(id)).toBeInstanceOf(Date)
    expect(await db!.instanceSettings.count({ where: { id: { startsWith: "rule-" } } })).toBe(0)
  })
})
