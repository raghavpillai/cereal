CREATE TABLE "User" (
 "id" UUID NOT NULL, "name" TEXT NOT NULL, "email" TEXT NOT NULL, "emailVerified" BOOLEAN NOT NULL DEFAULT FALSE,
 "image" TEXT, "role" VARCHAR(16) NOT NULL DEFAULT 'user' CHECK ("role" IN ('owner','admin','user')),
 "createdAt" TIMESTAMPTZ(3) NOT NULL DEFAULT CURRENT_TIMESTAMP, "updatedAt" TIMESTAMPTZ(3) NOT NULL,
 CONSTRAINT "User_pkey" PRIMARY KEY ("id")
);
CREATE UNIQUE INDEX "User_email_key" ON "User"("email");
CREATE UNIQUE INDEX "User_single_owner" ON "User"("role") WHERE "role"='owner';
INSERT INTO "User" ("id","name","email","emailVerified","role","updatedAt") VALUES ('00000000-0000-4000-8000-000000000001','Owner','owner@localhost.invalid',FALSE,'owner',CURRENT_TIMESTAMP);
CREATE TABLE "Session" (
 "id" UUID NOT NULL, "token" TEXT NOT NULL, "expiresAt" TIMESTAMPTZ(3) NOT NULL,
 "createdAt" TIMESTAMPTZ(3) NOT NULL DEFAULT CURRENT_TIMESTAMP, "updatedAt" TIMESTAMPTZ(3) NOT NULL,
 "ipAddress" TEXT, "userAgent" TEXT, "userId" UUID NOT NULL,
 CONSTRAINT "Session_pkey" PRIMARY KEY ("id"), CONSTRAINT "Session_userId_fkey" FOREIGN KEY ("userId") REFERENCES "User"("id") ON DELETE CASCADE ON UPDATE CASCADE
);
CREATE UNIQUE INDEX "Session_token_key" ON "Session"("token"); CREATE INDEX "Session_userId_idx" ON "Session"("userId");
CREATE TABLE "Account" (
 "id" UUID NOT NULL, "accountId" TEXT NOT NULL, "providerId" TEXT NOT NULL, "userId" UUID NOT NULL,
 "accessToken" TEXT, "refreshToken" TEXT, "idToken" TEXT, "accessTokenExpiresAt" TIMESTAMPTZ(3), "refreshTokenExpiresAt" TIMESTAMPTZ(3), "scope" TEXT, "password" TEXT,
 "createdAt" TIMESTAMPTZ(3) NOT NULL DEFAULT CURRENT_TIMESTAMP, "updatedAt" TIMESTAMPTZ(3) NOT NULL,
 CONSTRAINT "Account_pkey" PRIMARY KEY ("id"), CONSTRAINT "Account_userId_fkey" FOREIGN KEY ("userId") REFERENCES "User"("id") ON DELETE CASCADE ON UPDATE CASCADE
);
CREATE INDEX "Account_userId_idx" ON "Account"("userId"); CREATE UNIQUE INDEX "Account_providerId_accountId_key" ON "Account"("providerId","accountId");
CREATE TABLE "Verification" (
 "id" UUID NOT NULL, "identifier" TEXT NOT NULL, "value" TEXT NOT NULL, "expiresAt" TIMESTAMPTZ(3) NOT NULL,
 "createdAt" TIMESTAMPTZ(3) NOT NULL DEFAULT CURRENT_TIMESTAMP, "updatedAt" TIMESTAMPTZ(3) NOT NULL, CONSTRAINT "Verification_pkey" PRIMARY KEY ("id")
);
CREATE INDEX "Verification_identifier_idx" ON "Verification"("identifier");
ALTER TABLE "Note" ADD COLUMN "ownerId" UUID NOT NULL DEFAULT '00000000-0000-4000-8000-000000000001';
ALTER TABLE "Note" ADD CONSTRAINT "Note_ownerId_fkey" FOREIGN KEY ("ownerId") REFERENCES "User"("id") ON DELETE CASCADE ON UPDATE CASCADE;
CREATE INDEX "Note_ownerId_updatedAt_idx" ON "Note"("ownerId","updatedAt");
ALTER TABLE "CalendarMeeting" DROP CONSTRAINT "CalendarMeeting_pkey";
ALTER TABLE "CalendarMeeting" ADD COLUMN "userId" UUID NOT NULL DEFAULT '00000000-0000-4000-8000-000000000001';
ALTER TABLE "CalendarMeeting" ADD CONSTRAINT "CalendarMeeting_pkey" PRIMARY KEY ("userId","meetingKey");
ALTER TABLE "CalendarMeeting" ADD CONSTRAINT "CalendarMeeting_userId_fkey" FOREIGN KEY ("userId") REFERENCES "User"("id") ON DELETE CASCADE ON UPDATE CASCADE;
