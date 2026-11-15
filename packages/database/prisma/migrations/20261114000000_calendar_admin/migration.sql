CREATE TABLE "InstanceSettings" (
 "id" TEXT NOT NULL DEFAULT 'instance', "instanceName" VARCHAR(100) NOT NULL DEFAULT 'Meeting Notes',
 "languageModel" VARCHAR(150), "openRouterSecret" TEXT, "elevenLabsSecret" TEXT,
 "calendarRefreshMinutes" INTEGER NOT NULL DEFAULT 15, "calendarLookAheadDays" INTEGER NOT NULL DEFAULT 30,
 "updatedAt" TIMESTAMPTZ(3) NOT NULL, CONSTRAINT "InstanceSettings_pkey" PRIMARY KEY ("id")
);
CREATE TABLE "CalendarSubscription" (
 "id" UUID NOT NULL, "name" VARCHAR(200) NOT NULL, "provider" VARCHAR(32) NOT NULL DEFAULT 'ics',
 "color" CHAR(7) NOT NULL DEFAULT '#87e5e8', "enabled" BOOLEAN NOT NULL DEFAULT TRUE,
 "encryptedURL" TEXT NOT NULL, "sourceHost" VARCHAR(253) NOT NULL, "lastSyncedAt" TIMESTAMPTZ(3), "lastSyncError" TEXT,
 "createdAt" TIMESTAMPTZ(3) NOT NULL DEFAULT CURRENT_TIMESTAMP, "updatedAt" TIMESTAMPTZ(3) NOT NULL,
 CONSTRAINT "CalendarSubscription_pkey" PRIMARY KEY ("id")
);
CREATE TABLE "CalendarEvent" (
 "id" UUID NOT NULL, "calendarId" UUID NOT NULL, "sourceKey" CHAR(64) NOT NULL, "meetingKey" CHAR(64) NOT NULL,
 "title" VARCHAR(500) NOT NULL, "startAt" TIMESTAMPTZ(3) NOT NULL, "endAt" TIMESTAMPTZ(3) NOT NULL,
 "allDay" BOOLEAN NOT NULL DEFAULT FALSE, "startDate" CHAR(10) NOT NULL, "endDate" CHAR(10) NOT NULL,
 "timeZone" VARCHAR(200), "location" VARCHAR(1000), "meetingUrl" VARCHAR(4096),
 CONSTRAINT "CalendarEvent_pkey" PRIMARY KEY ("id"),
 CONSTRAINT "CalendarEvent_calendarId_fkey" FOREIGN KEY ("calendarId") REFERENCES "CalendarSubscription"("id") ON DELETE CASCADE ON UPDATE CASCADE
);
CREATE UNIQUE INDEX "CalendarEvent_calendarId_sourceKey_key" ON "CalendarEvent"("calendarId","sourceKey");
CREATE INDEX "CalendarEvent_startAt_endAt_idx" ON "CalendarEvent"("startAt","endAt");
CREATE INDEX "CalendarEvent_meetingKey_idx" ON "CalendarEvent"("meetingKey");
CREATE TABLE "CalendarMeeting" (
 "meetingKey" CHAR(64) NOT NULL, "noteId" UUID NOT NULL,
 CONSTRAINT "CalendarMeeting_pkey" PRIMARY KEY ("meetingKey"),
 CONSTRAINT "CalendarMeeting_noteId_fkey" FOREIGN KEY ("noteId") REFERENCES "Note"("id") ON DELETE CASCADE ON UPDATE CASCADE
);
CREATE UNIQUE INDEX "CalendarMeeting_noteId_key" ON "CalendarMeeting"("noteId");
