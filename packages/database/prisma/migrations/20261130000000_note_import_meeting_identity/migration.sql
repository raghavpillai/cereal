-- A note moved here from another account (POST /api/v1/notes/import): when it arrived and the SHA-256 of the
-- request that made it, so the same request sent again is recognised. Null on every note made here.
ALTER TABLE "Note" ADD COLUMN "importedAt" TIMESTAMPTZ(3), ADD COLUMN "importDigest" CHAR(64);
-- A meeting's identity across calendars and servers: the iCalendar UID and which occurrence this is. Not
-- backfilled: both kinds of event are written again at every sync, so the columns fill at the next one.
ALTER TABLE "CalendarEvent" ADD COLUMN "meetingUid" VARCHAR(2048), ADD COLUMN "occurrence" VARCHAR(40), ADD COLUMN "attendeeCount" INTEGER;
ALTER TABLE "GooglePersonalEvent" ADD COLUMN "meetingUid" VARCHAR(2048), ADD COLUMN "occurrence" VARCHAR(40);
