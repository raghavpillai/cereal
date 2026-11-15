-- Personal Google data is deliberately separate from instance-shared ICS feeds.
CREATE TABLE "GoogleOAuthState" (
 "stateHash" CHAR(64) PRIMARY KEY, "userId" UUID NOT NULL REFERENCES "User"("id") ON DELETE CASCADE,
 "sessionHash" CHAR(64) NOT NULL, "encryptedVerifier" TEXT NOT NULL, "contacts" BOOLEAN NOT NULL DEFAULT FALSE,
 "expiresAt" TIMESTAMPTZ(3) NOT NULL
);
CREATE INDEX "GoogleOAuthState_expiresAt_idx" ON "GoogleOAuthState"("expiresAt");
CREATE TABLE "GoogleConnection" (
 "id" UUID PRIMARY KEY, "userId" UUID NOT NULL REFERENCES "User"("id") ON DELETE CASCADE,
 "subject" VARCHAR(255) NOT NULL, "email" VARCHAR(254) NOT NULL, "displayName" VARCHAR(500),
 "encryptedGrant" TEXT NOT NULL, "contactsEnabled" BOOLEAN NOT NULL DEFAULT FALSE,
 "status" VARCHAR(32) NOT NULL DEFAULT 'connected' CHECK ("status" IN ('connected','reconnect_required')),
 "lastSyncedAt" TIMESTAMPTZ(3), "lastSyncError" TEXT,
 "createdAt" TIMESTAMPTZ(3) NOT NULL DEFAULT CURRENT_TIMESTAMP, "updatedAt" TIMESTAMPTZ(3) NOT NULL
);
CREATE UNIQUE INDEX "GoogleConnection_userId_subject_key" ON "GoogleConnection"("userId","subject");
CREATE INDEX "GoogleConnection_userId_idx" ON "GoogleConnection"("userId");
CREATE TABLE "GoogleCalendar" (
 "id" UUID PRIMARY KEY, "connectionId" UUID NOT NULL REFERENCES "GoogleConnection"("id") ON DELETE CASCADE,
 "remoteId" VARCHAR(1024) NOT NULL, "name" VARCHAR(200) NOT NULL, "color" CHAR(7) NOT NULL,
 "timeZone" VARCHAR(200) NOT NULL, "primary" BOOLEAN NOT NULL DEFAULT FALSE, "selected" BOOLEAN NOT NULL DEFAULT FALSE,
 "accessRole" VARCHAR(64) NOT NULL
);
CREATE UNIQUE INDEX "GoogleCalendar_connectionId_remoteId_key" ON "GoogleCalendar"("connectionId","remoteId");
CREATE TABLE "GooglePersonalEvent" (
 "id" UUID PRIMARY KEY, "calendarId" UUID NOT NULL REFERENCES "GoogleCalendar"("id") ON DELETE CASCADE,
 "remoteId" VARCHAR(1024) NOT NULL, "meetingKey" CHAR(64) NOT NULL, "title" VARCHAR(500) NOT NULL,
 "startAt" TIMESTAMPTZ(3) NOT NULL, "endAt" TIMESTAMPTZ(3) NOT NULL, "allDay" BOOLEAN NOT NULL,
 "startDate" CHAR(10) NOT NULL, "endDate" CHAR(10) NOT NULL, "timeZone" VARCHAR(200),
 "location" VARCHAR(1000), "meetingUrl" VARCHAR(4096), "context" JSONB NOT NULL
);
CREATE UNIQUE INDEX "GooglePersonalEvent_calendarId_remoteId_key" ON "GooglePersonalEvent"("calendarId","remoteId");
CREATE INDEX "GooglePersonalEvent_startAt_endAt_idx" ON "GooglePersonalEvent"("startAt","endAt");
-- The binding intentionally survives an event cache refresh or disconnect.
CREATE TABLE "GoogleMeeting" (
 "userId" UUID NOT NULL REFERENCES "User"("id") ON DELETE CASCADE, "meetingKey" CHAR(64) NOT NULL,
 "noteId" UUID NOT NULL UNIQUE REFERENCES "Note"("id") ON DELETE CASCADE, PRIMARY KEY ("userId","meetingKey")
);
CREATE TABLE "NoteMeetingContext" (
 "noteId" UUID PRIMARY KEY REFERENCES "Note"("id") ON DELETE CASCADE,
 "userId" UUID NOT NULL REFERENCES "User"("id") ON DELETE CASCADE, "meeting" JSONB NOT NULL,
 "updatedAt" TIMESTAMPTZ(3) NOT NULL
);
CREATE INDEX "NoteMeetingContext_userId_idx" ON "NoteMeetingContext"("userId");
