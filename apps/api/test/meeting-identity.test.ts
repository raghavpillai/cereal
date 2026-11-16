import { afterEach, describe, expect, test } from "bun:test"
import { Schema } from "effect"
import { CalendarEventDTO, CalendarEventsDTO, HealthDTO, NoteDTO } from "@meeting-notes/contracts"
import { makeApiHandler } from "../src/api"
import { makeMemoryRepository } from "../src/repository"
import { makeMemoryCalendarStore } from "../src/calendar-store"
import { makeAdministrationService } from "../src/administration"
import { makeMemoryGoogleStore, type GoogleCalendar } from "../src/google-store"
import { makeGoogleService, normalizeGoogleEvent } from "../src/google"
import { otherInvitees, parseCalendarFeed } from "../src/calendar-feed"
import { secretBoxFromKey } from "../src/admin-secrets"

/**
 * The same meetings, described the two ways a server meets them: as an iCalendar feed (the shape of a
 * Google Calendar "secret address" export: UTC or TZID times, RECURRENCE-ID overrides, ORGANIZER and
 * ATTENDEE lines with CUTYPE) and as Google Calendar API events (`singleEvents=true`: `iCalUID`,
 * `recurringEventId`, `originalStartTime`). These are fixtures in the recorded shapes, written by hand.
 * They have NOT been compared with a real Google calendar read both ways; that comparison is the gate in
 * the design (section 3.2) and needs real Google credentials.
 */
const now = new Date("2026-11-15T12:00:00Z")
const zone = `BEGIN:VTIMEZONE\r\nTZID:America/New_York\r\nBEGIN:STANDARD\r\nDTSTART:19701101T020000\r\nTZOFFSETFROM:-0400\r\nTZOFFSETTO:-0500\r\nRRULE:FREQ=YEARLY;BYMONTH=11;BYDAY=1SU\r\nEND:STANDARD\r\nBEGIN:DAYLIGHT\r\nDTSTART:19700308T020000\r\nTZOFFSETFROM:-0500\r\nTZOFFSETTO:-0400\r\nRRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=2SU\r\nEND:DAYLIGHT\r\nEND:VTIMEZONE\r\n`
const feed = (...events: string[]) => `BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Google Inc//Google Calendar 70.9054//EN\r\n${zone}${events.map(lines => `BEGIN:VEVENT\r\nDTSTAMP:20261115T000000Z\r\n${lines.trim().split("\n").map(line => line.trim()).join("\r\n")}\r\nEND:VEVENT\r\n`).join("")}END:VCALENDAR\r\n`
const people = `ORGANIZER;CN=Raghav:mailto:me@example.test
ATTENDEE;CUTYPE=INDIVIDUAL;ROLE=REQ-PARTICIPANT;PARTSTAT=ACCEPTED;CN=Raghav;X-NUM-GUESTS=0:mailto:me@example.test
ATTENDEE;CUTYPE=INDIVIDUAL;ROLE=REQ-PARTICIPANT;PARTSTAT=NEEDS-ACTION;CN=Ada;X-NUM-GUESTS=0:mailto:ada@example.test
ATTENDEE;CUTYPE=INDIVIDUAL;ROLE=REQ-PARTICIPANT;PARTSTAT=ACCEPTED;X-NUM-GUESTS=0:mailto:Grace@Example.test
ATTENDEE;CUTYPE=RESOURCE;ROLE=REQ-PARTICIPANT;PARTSTAT=ACCEPTED;CN=Room 4;X-NUM-GUESTS=0:mailto:room4@resource.calendar.google.com`
const ics = {
  single: `DTSTART:20261118T150000Z\nDTEND:20261118T160000Z\nUID:single-7hq2@google.com\nSUMMARY:Design review\n${people}`,
  weekly: `DTSTART;TZID=America/New_York:20261102T100000\nDTEND;TZID=America/New_York:20261102T103000\nRRULE:FREQ=WEEKLY;BYDAY=MO\nUID:weekly-91ab@google.com\nSUMMARY:Weekly sync\n${people}`,
  // The occurrence of 23 November, moved to the next afternoon. Its RECURRENCE-ID is still the original start.
  moved: `DTSTART;TZID=America/New_York:20261124T140000\nDTEND;TZID=America/New_York:20261124T143000\nRECURRENCE-ID;TZID=America/New_York:20261123T100000\nUID:weekly-91ab@google.com\nSUMMARY:Weekly sync (moved)\n${people}`,
  allDaySingle: `DTSTART;VALUE=DATE:20261120\nDTEND;VALUE=DATE:20261121\nUID:offsite-55cd@google.com\nSUMMARY:Offsite`,
  allDayWeekly: `DTSTART;VALUE=DATE:20261106\nDTEND;VALUE=DATE:20261107\nRRULE:FREQ=WEEKLY;BYDAY=FR\nUID:fridays-02ef@google.com\nSUMMARY:No-meeting Friday`,
}
const calendar: GoogleCalendar = { id: "30000000-0000-4000-8000-000000000001", connectionId: "30000000-0000-4000-8000-000000000002", remoteId: "primary", name: "Raghav", color: "#87e5e8", timeZone: "America/New_York", primary: true, selected: true, accessRole: "owner" }
const attendees = [
  { email: "me@example.test", displayName: "Raghav", self: true, organizer: true, responseStatus: "accepted" },
  { email: "ada@example.test", displayName: "Ada", responseStatus: "needsAction" },
  { email: "grace@example.test", responseStatus: "accepted" },
  { email: "room4@resource.calendar.google.com", displayName: "Room 4", resource: true, responseStatus: "accepted" },
]
const timed = (start: string, end: string) => ({ start: { dateTime: start, timeZone: "America/New_York" }, end: { dateTime: end, timeZone: "America/New_York" } })
const google = {
  single: { id: "7hq2", iCalUID: "single-7hq2@google.com", summary: "Design review", ...timed("2026-11-18T10:00:00-05:00", "2026-11-18T11:00:00-05:00"), attendees },
  weekly: { id: "91ab_20261116T150000Z", iCalUID: "weekly-91ab@google.com", recurringEventId: "91ab", originalStartTime: { dateTime: "2026-11-16T10:00:00-05:00", timeZone: "America/New_York" }, summary: "Weekly sync", ...timed("2026-11-16T10:00:00-05:00", "2026-11-16T10:30:00-05:00"), attendees },
  moved: { id: "91ab_20261123T150000Z", iCalUID: "weekly-91ab@google.com", recurringEventId: "91ab", originalStartTime: { dateTime: "2026-11-23T10:00:00-05:00", timeZone: "America/New_York" }, summary: "Weekly sync (moved)", ...timed("2026-11-24T14:00:00-05:00", "2026-11-24T14:30:00-05:00"), attendees },
  allDaySingle: { id: "55cd", iCalUID: "offsite-55cd@google.com", summary: "Offsite", start: { date: "2026-11-20" }, end: { date: "2026-11-21" } },
  allDayWeekly: { id: "02ef_20261120", iCalUID: "fridays-02ef@google.com", recurringEventId: "02ef", originalStartTime: { date: "2026-11-20" }, summary: "No-meeting Friday", start: { date: "2026-11-20" }, end: { date: "2026-11-21" } },
}
const pair = (event: { meetingUid?: string | null; occurrence?: string | null } | null | undefined) => [event?.meetingUid, event?.occurrence]
const fromFeed = async (...events: string[]) => parseCalendarFeed(crypto.randomUUID(), feed(...events), now)

describe("meeting identity: an ICS feed and a Google payload describing the same meeting", () => {
  test("a single event gives equal (meetingUid, occurrence)", async () => {
    const [fed] = await fromFeed(ics.single), api = normalizeGoogleEvent(google.single, calendar)
    expect(pair(fed)).toEqual(["single-7hq2@google.com", "single"])
    expect(pair(api)).toEqual(pair(fed))
  })
  test("one occurrence of a weekly series gives equal (meetingUid, occurrence)", async () => {
    const fed = (await fromFeed(ics.weekly)).find(event => event.startAt === "2026-11-16T15:00:00.000Z"), api = normalizeGoogleEvent(google.weekly, calendar)
    expect(pair(fed)).toEqual(["weekly-91ab@google.com", "instant:2026-11-16T15:00:00.000Z"])
    expect(pair(api)).toEqual(pair(fed))
    // Each occurrence is its own meeting; the series is the UID alone.
    const all = await fromFeed(ics.weekly)
    expect(new Set(all.map(event => event.occurrence)).size).toBe(all.length)
    expect(new Set(all.map(event => event.meetingUid))).toEqual(new Set(["weekly-91ab@google.com"]))
  })
  test("a moved occurrence keeps the original start as its occurrence", async () => {
    const fed = (await fromFeed(ics.weekly, ics.moved)).find(event => event.title === "Weekly sync (moved)"), api = normalizeGoogleEvent(google.moved, calendar)
    expect(fed!.startAt).toBe("2026-11-24T19:00:00.000Z")
    expect(pair(fed)).toEqual(["weekly-91ab@google.com", "instant:2026-11-23T15:00:00.000Z"])
    expect(pair(api)).toEqual(pair(fed))
    expect(api!.startAt).toBe(fed!.startAt)
  })
  test("an all-day occurrence gives equal (meetingUid, occurrence), and an all-day single event is single", async () => {
    const events = await fromFeed(ics.allDayWeekly, ics.allDaySingle)
    const friday = events.find(event => event.title === "No-meeting Friday" && event.startDate === "2026-11-20"), offsite = events.find(event => event.title === "Offsite")
    expect(pair(friday)).toEqual(["fridays-02ef@google.com", "date:2026-11-20"])
    expect(pair(normalizeGoogleEvent(google.allDayWeekly, calendar))).toEqual(pair(friday))
    expect(pair(offsite)).toEqual(["offsite-55cd@google.com", "single"])
    expect(pair(normalizeGoogleEvent(google.allDaySingle, calendar))).toEqual(pair(offsite))
  })
  test("an event without a UID gives null", async () => {
    const api = normalizeGoogleEvent({ ...google.single, iCalUID: undefined }, calendar)!
    expect(api.meetingUid).toBeNull()
    // The server's own fallback key (`remote:<calendar>:<id>`) still binds notes, and is never sent as an identity.
    expect(api.meetingKey).toMatch(/^[a-f0-9]{64}$/)
    expect(JSON.stringify({ ...api, meetingKey: undefined })).not.toContain("remote:")
    expect(normalizeGoogleEvent({ ...google.single, iCalUID: "   " }, calendar)!.meetingUid).toBeNull()
    // A feed event without a UID cannot be kept apart from any other, and is not stored at all (as before).
    expect(await fromFeed(ics.single.replace(/UID:.*\n/, ""))).toEqual([])
  })
  test("an over-long UID is refused by the schema at 2,049 characters, and neither source sends one", async () => {
    const event = { id: crypto.randomUUID(), calendarId: crypto.randomUUID(), calendarName: "Work", color: "#87e5e8", title: "x", startAt: "2026-11-18T15:00:00.000Z", endAt: "2026-11-18T16:00:00.000Z", allDay: false, startDate: "2026-11-18", endDate: "2026-11-18", timeZone: null, location: null, meetingUrl: null, noteId: null, occurrence: "single" }
    const decode = Schema.decodeUnknownSync(CalendarEventDTO)
    expect(decode({ ...event, meetingUid: "u".repeat(2048) }).meetingUid).toHaveLength(2048)
    expect(() => decode({ ...event, meetingUid: "u".repeat(2049) })).toThrow()
    expect(() => decode({ ...event, meetingUid: "" })).toThrow()
    for (const occurrence of ["Single", "date:2026-11-8", "instant:2026-11-18T15:00:00Z", "instant:2026-11-18T10:00:00.000-05:00", "2026-11-18", " single"]) expect(() => decode({ ...event, occurrence }), occurrence).toThrow()
    // A UID too long to send is left out (a shortened one would name another meeting); the event itself still lists.
    const long = "u".repeat(2049)
    expect(normalizeGoogleEvent({ ...google.single, iCalUID: long }, calendar)!.meetingUid).toBeNull()
    expect(normalizeGoogleEvent({ ...google.single, iCalUID: "u".repeat(2048) }, calendar)!.meetingUid).toHaveLength(2048)
    const [fed] = await fromFeed(ics.single.replace("single-7hq2@google.com", long))
    expect(fed!.meetingUid).toBeNull()
    expect(fed!.occurrence).toBe("single")
  })
  test("an occurrence Google describes with a date that is not one is sent as no occurrence, and the event still lists", () => {
    const odd = normalizeGoogleEvent({ ...google.allDayWeekly, originalStartTime: { date: "2026-1-5" } }, calendar)!
    expect(odd.occurrence).toBeNull()
    expect(odd.meetingUid).toBe("fridays-02ef@google.com")
    expect(Schema.decodeUnknownSync(CalendarEventDTO)({ ...odd, calendarName: "Raghav", color: "#87e5e8", noteId: null }).occurrence).toBeNull()
    // A recurring instance whose original start Google does not give falls back to its own start.
    expect(normalizeGoogleEvent({ ...google.weekly, originalStartTime: undefined }, calendar)!.occurrence).toBe("instant:2026-11-16T15:00:00.000Z")
    expect(normalizeGoogleEvent({ ...google.weekly, originalStartTime: { dateTime: "not a time" } }, calendar)!.occurrence).toBe("instant:2026-11-16T15:00:00.000Z")
  })
  test("known difference: a feed with only an edited occurrence and no series gives single where Google gives instant", async () => {
    // Without its series the override is, to an iCalendar reader, one event that does not repeat. The apps
    // join the two by title and time (design 3.3); this test records the difference so it is not a surprise.
    const [fed] = await fromFeed(ics.moved), api = normalizeGoogleEvent(google.moved, calendar)!
    expect(pair(fed)).toEqual(["weekly-91ab@google.com", "single"])
    expect(pair(api)).toEqual(["weekly-91ab@google.com", "instant:2026-11-23T15:00:00.000Z"])
    expect([fed!.title, fed!.startAt, fed!.endAt]).toEqual([api.title, api.startAt, api.endAt])
  })
  test("the identity does not depend on the time zone the server runs in", async () => {
    const script = `import {parseCalendarFeed} from ${JSON.stringify(new URL("../src/calendar-feed.ts", import.meta.url).pathname)};const events=await parseCalendarFeed("30000000-0000-4000-8000-000000000009",process.env.FEED,new Date(${JSON.stringify(now.toISOString())}));console.log(JSON.stringify(events.map(e=>[e.meetingUid,e.occurrence,e.startAt,e.endAt,e.startDate,e.endDate]).sort()))`
    const source = feed(ics.single, ics.weekly, ics.moved, ics.allDaySingle, ics.allDayWeekly)
    const outputs = await Promise.all(["UTC", "America/Los_Angeles", "Asia/Tokyo", "Pacific/Kiritimati"].map(async TZ => {
      const child = Bun.spawn(["bun", "--no-env-file", "-e", script], { env: { ...process.env, TZ, FEED: source }, stdout: "pipe", stderr: "pipe" })
      const [out, code] = await Promise.all([new Response(child.stdout).text(), child.exited])
      expect(code, TZ).toBe(0)
      return out.trim()
    }))
    expect(new Set(outputs).size).toBe(1)
    // An all-day date reaches the parser as midnight in the server's own zone; the identity reads the day back in that zone.
    expect(outputs[0]).toContain('["fridays-02ef@google.com","date:2026-11-20","2026-11-20T00:00:00.000Z","2026-11-21T00:00:00.000Z","2026-11-20","2026-11-21"]')
    expect(outputs[0]).toContain('["weekly-91ab@google.com","instant:2026-11-23T15:00:00.000Z","2026-11-24T19:00:00.000Z","2026-11-24T19:30:00.000Z","2026-11-24","2026-11-24"]')
  }, 60_000)
})

describe("attendeeCount: other invitees, rooms excluded", () => {
  test("a feed counts the people on ATTENDEE lines other than the organizer", async () => {
    const [counted] = await fromFeed(ics.single), [alone] = await fromFeed(ics.allDaySingle)
    // Raghav (organizer), Ada, Grace and a room: two other people, whoever of the three is looking.
    expect(counted!.attendeeCount).toBe(2)
    expect(alone!.attendeeCount).toBe(0)
  })
  test("feeds that list the organizer among the attendees and feeds that do not give the same count", () => {
    const person = (email: string, CUTYPE = "INDIVIDUAL") => ({ params: { CUTYPE }, val: `mailto:${email}` })
    const organizer = { params: { CN: "Host" }, val: "MAILTO:host@example.test" }
    expect(otherInvitees([person("host@example.test"), person("a@example.test"), person("b@example.test")], organizer)).toBe(2)
    expect(otherInvitees([person("a@example.test"), person("b@example.test")], organizer)).toBe(2)
    expect(otherInvitees(person("a@example.test"), organizer)).toBe(1)
    expect(otherInvitees([person("a@example.test"), person("A@Example.test"), "mailto:a@example.test"], undefined)).toBe(1)
    expect(otherInvitees([person("room@example.test", "ROOM"), person("projector@example.test", "resource")], organizer)).toBe(0)
    expect(otherInvitees(undefined, organizer)).toBe(0)
  })
})

describe("calendar events over HTTP", () => {
  const close: Array<() => Promise<void>> = []
  afterEach(async () => { for (const dispose of close.splice(0)) await dispose() })
  const token = "meeting-identity-test-token-at-least-32-characters"
  test("shared (ICS) and personal (Google) events carry the identity and the count; an older client schema still decodes them", async () => {
    const repository = makeMemoryRepository(), shared = makeMemoryCalendarStore(repository), personal = makeMemoryGoogleStore(repository, shared)
    const administration = makeAdministrationService(shared, undefined, { clock: () => now })
    const box = secretBoxFromKey(Buffer.alloc(32, 7))
    const owner = "00000000-0000-4000-8000-000000000001"
    const googleService = makeGoogleService(personal, box, { clientId: "client", clientSecret: "secret", baseURL: "https://notes.example.test", clock: () => now })
    const api = makeApiHandler(repository, token, { administration, google: googleService, clock: () => now })
    close.push(api.dispose)
    const subscription = await shared.createCalendar({ name: "Team", provider: "ics", color: "#87e5e8", enabled: true, encryptedURL: "sealed", sourceHost: "calendar.example.test" })
    const fed = await parseCalendarFeed(subscription.id, feed(ics.single, ics.weekly, ics.moved, ics.allDayWeekly), now)
    // One event as a server stored it before the fields existed: nothing to send until the next refresh.
    const { meetingUid: _uid, occurrence: _occurrence, attendeeCount: _count, ...old } = (await parseCalendarFeed(subscription.id, feed(ics.allDaySingle), now))[0]!
    await shared.replaceEvents(subscription.id, "sealed", [...fed, old], now.toISOString())
    const connection = await personal.saveConnection({ userId: owner, subject: "subject", email: "me@example.test", displayName: null, contactsEnabled: false, status: "connected", encryptedGrant: box.seal("{}") })
    await personal.saveCalendars(owner, connection.id, [{ remoteId: "primary", name: "Raghav", color: "#87e5e8", timeZone: "America/New_York", primary: true, accessRole: "owner" }])
    const mine = (await personal.listCalendars(owner, connection.id))[0]!
    await personal.replaceEvents(owner, mine.id, [normalizeGoogleEvent(google.single, mine)!, normalizeGoogleEvent(google.moved, mine)!, normalizeGoogleEvent({ ...google.allDaySingle, iCalUID: undefined }, mine)!])
    const response = await api.handler(new Request("http://localhost/api/v1/calendar/events?from=2026-11-01T00:00:00Z&to=2026-12-01T00:00:00Z", { headers: { Authorization: `Bearer ${token}` } }))
    expect(response.status).toBe(200)
    const text = await response.text(), events = Schema.decodeUnknownSync(CalendarEventsDTO)(JSON.parse(text))
    expect(text).not.toContain("meetingKey"); expect(text).not.toContain("sourceKey"); expect(text).not.toContain("remote:")
    const by = (calendarName: string, title: string, startDate?: string) => events.find(event => event.calendarName === calendarName && event.title === title && (!startDate || event.startDate === startDate))!
    // The same meeting through the two sources: equal pairs, so the apps' SHA-256(meetingUid + "\n" + occurrence) is equal.
    for (const title of ["Design review", "Weekly sync (moved)"]) expect(pair(by("Team", title))).toEqual(pair(by("Raghav", title)))
    expect(pair(by("Team", "Design review"))).toEqual(["single-7hq2@google.com", "single"])
    expect(pair(by("Team", "Weekly sync (moved)"))).toEqual(["weekly-91ab@google.com", "instant:2026-11-23T15:00:00.000Z"])
    expect(pair(by("Team", "No-meeting Friday", "2026-11-20"))).toEqual(["fridays-02ef@google.com", "date:2026-11-20"])
    // Others invited: Ada and Grace. The Google source knows who the person is; the feed leaves the organizer out.
    expect(by("Team", "Design review").attendeeCount).toBe(2)
    expect(by("Raghav", "Design review").attendeeCount).toBe(2)
    expect(by("Raghav", "Offsite")).toMatchObject({ meetingUid: null, occurrence: "single", attendeeCount: 0 })
    expect(by("Team", "Offsite")).toMatchObject({ meetingUid: null, occurrence: null, attendeeCount: null })
    // A client built before the fields existed reads the same answer.
    const { meetingUid: _a, occurrence: _b, attendeeCount: _c, ...olderFields } = CalendarEventDTO.fields
    const older = Schema.decodeUnknownSync(Schema.Array(Schema.Struct(olderFields)))(JSON.parse(text))
    expect(older).toHaveLength(events.length)
  })
  test("health lists the three features, and an older client schema still decodes the answer", async () => {
    const api = makeApiHandler(makeMemoryRepository(), token)
    close.push(api.dispose)
    const response = await api.handler(new Request("http://localhost/api/v1/health"))
    const body = await response.json()
    expect(body).toEqual({ status: "ok", version: "v1", features: ["note-import", "conditional-delete", "meeting-identity"] })
    expect(Schema.decodeUnknownSync(HealthDTO)(body).features).toContain("note-import")
    const older = Schema.Struct({ status: Schema.Literal("ok"), version: Schema.Literal("v1") })
    expect(Schema.decodeUnknownSync(older)(body)).toEqual({ status: "ok", version: "v1" })
    // And a newer client reads an older server: no features at all.
    expect(Schema.decodeUnknownSync(HealthDTO)({ status: "ok", version: "v1" }).features).toBeUndefined()
    expect(Schema.decodeUnknownSync(NoteDTO)({ id: crypto.randomUUID(), title: "", body: "", enhanced: "", date: "2026-11-15", duration: "", createdAt: now.toISOString(), updatedAt: now.toISOString(), transcriptSegments: [], recordingState: "idle", generationState: "idle", transcriptRevision: 0, generatedRevision: null }).importedAt).toBeUndefined()
  })
})

const postgres = process.env.RUN_PG_TESTS === "1" && !!process.env.DATABASE_URL && /^\/mn_test_[a-f0-9]+$/.test(new URL(process.env.DATABASE_URL).pathname)
describe.skipIf(!postgres)("meeting identity on PostgreSQL", () => {
  test("both stores keep the identity through a refresh and send it; rows stored before the columns existed send null", async () => {
    const { makePrismaRepository } = await import("../src/prisma-repository"), { makePrismaCalendarStore } = await import("../src/calendar-store"), { makePrismaGoogleStore } = await import("../src/google-store")
    const resources = makePrismaRepository(process.env.DATABASE_URL!), db = resources.database, user = crypto.randomUUID(), host = `identity-${user}.example.test`
    try {
      await db.user.create({ data: { id: user, name: "Identity test", email: `identity-${user}@example.test`, role: "user" } })
      const shared = makePrismaCalendarStore(db), personal = makePrismaGoogleStore(db)
      const subscription = await shared.createCalendar({ name: "Team", provider: "ics", color: "#87e5e8", enabled: true, encryptedURL: "sealed-" + user, sourceHost: host })
      const fed = await parseCalendarFeed(subscription.id, feed(ics.single, ics.weekly, ics.moved, ics.allDayWeekly), now)
      const { meetingUid: _uid, occurrence: _occurrence, attendeeCount: _count, ...old } = (await parseCalendarFeed(subscription.id, feed(ics.allDaySingle), now))[0]!
      expect(await shared.replaceEvents(subscription.id, subscription.encryptedURL, [...fed, old], now.toISOString())).toBe(true)
      const listed = (await shared.listEvents("2026-11-01T00:00:00Z", "2026-12-01T00:00:00Z", user)).filter(event => event.calendarId === subscription.id)
      const stored = (title: string) => listed.find(event => event.title === title)!
      expect([stored("Design review").meetingUid, stored("Design review").occurrence, stored("Design review").attendeeCount]).toEqual(["single-7hq2@google.com", "single", 2])
      expect(pair(stored("Weekly sync (moved)"))).toEqual(["weekly-91ab@google.com", "instant:2026-11-23T15:00:00.000Z"])
      expect(pair(listed.find(event => event.title === "No-meeting Friday" && event.startDate === "2026-11-20"))).toEqual(["fridays-02ef@google.com", "date:2026-11-20"])
      expect([stored("Offsite").meetingUid, stored("Offsite").occurrence, stored("Offsite").attendeeCount]).toEqual([null, null, null])
      // The next refresh writes every event again, and the columns fill.
      expect(await shared.replaceEvents(subscription.id, subscription.encryptedURL, await parseCalendarFeed(subscription.id, feed(ics.allDaySingle), now), now.toISOString())).toBe(true)
      const refreshed = (await shared.listEvents("2026-11-01T00:00:00Z", "2026-12-01T00:00:00Z", user)).find(event => event.calendarId === subscription.id)!
      expect([refreshed.meetingUid, refreshed.occurrence, refreshed.attendeeCount]).toEqual(["offsite-55cd@google.com", "single", 0])

      const connection = await personal.saveConnection({ userId: user, subject: "subject-" + user, email: "me@example.test", displayName: null, contactsEnabled: false, status: "connected", encryptedGrant: "sealed" })
      await personal.saveCalendars(user, connection.id, [{ remoteId: "primary", name: "Raghav", color: "#87e5e8", timeZone: "America/New_York", primary: true, accessRole: "owner" }])
      const mine = (await personal.listCalendars(user, connection.id))[0]!
      expect(await personal.replaceEvents(user, mine.id, [normalizeGoogleEvent(google.single, mine)!, normalizeGoogleEvent(google.moved, mine)!, normalizeGoogleEvent(google.allDayWeekly, mine)!, normalizeGoogleEvent({ ...google.allDaySingle, iCalUID: undefined }, mine)!])).toBe(true)
      const events = Schema.decodeUnknownSync(CalendarEventsDTO)(await personal.listEvents(user, "2026-11-01T00:00:00Z", "2026-12-01T00:00:00Z"))
      const own = (title: string) => events.find(event => event.title === title)!
      expect([own("Design review").meetingUid, own("Design review").occurrence, own("Design review").attendeeCount]).toEqual(["single-7hq2@google.com", "single", 2])
      expect(pair(own("Weekly sync (moved)"))).toEqual(pair(stored("Weekly sync (moved)")))
      expect(pair(own("No-meeting Friday"))).toEqual(["fridays-02ef@google.com", "date:2026-11-20"])
      expect([own("Offsite").meetingUid, own("Offsite").occurrence, own("Offsite").attendeeCount]).toEqual([null, "single", 0])
      expect(JSON.stringify(events)).not.toContain("meetingKey")
      // A row from before the migration's columns were filled.
      await db.googlePersonalEvent.updateMany({ where: { calendarId: mine.id }, data: { meetingUid: null, occurrence: null } })
      for (const event of await personal.listEvents(user, "2026-11-01T00:00:00Z", "2026-12-01T00:00:00Z")) expect([event.meetingUid, event.occurrence]).toEqual([null, null])
    } finally {
      await db.calendarSubscription.deleteMany({ where: { sourceHost: host } })
      await db.user.deleteMany({ where: { id: user } })
      await resources.close()
    }
  })
})
