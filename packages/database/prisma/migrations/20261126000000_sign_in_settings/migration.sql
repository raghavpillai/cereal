-- Sign-in methods chosen on the admin panel, the Google OAuth client they share with the
-- Google Calendar connection (secret sealed with the settings key), and the two short-lived
-- tables behind Google sign-in and the browser-to-app sign-in handoff.
ALTER TABLE "InstanceSettings"
 ADD COLUMN "signInPassword" BOOLEAN NOT NULL DEFAULT TRUE,
 ADD COLUMN "signInGoogle" BOOLEAN NOT NULL DEFAULT FALSE,
 ADD COLUMN "signInAllowedEmails" JSONB,
 ADD COLUMN "googleClientId" VARCHAR(255),
 ADD COLUMN "googleClientSecret" TEXT;

CREATE TABLE "GoogleSignInState" (
 "stateHash" CHAR(64) PRIMARY KEY,
 "browserHash" CHAR(64) NOT NULL,
 "encryptedVerifier" TEXT NOT NULL,
 "returnPath" VARCHAR(2048) NOT NULL,
 "expiresAt" TIMESTAMPTZ(3) NOT NULL
);
CREATE INDEX "GoogleSignInState_expiresAt_idx" ON "GoogleSignInState"("expiresAt");

CREATE TABLE "AppSignInHandoff" (
 "codeHash" CHAR(64) PRIMARY KEY,
 "userId" UUID NOT NULL REFERENCES "User"("id") ON DELETE CASCADE,
 "challenge" CHAR(43) NOT NULL,
 "expiresAt" TIMESTAMPTZ(3) NOT NULL,
 "createdAt" TIMESTAMPTZ(3) NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX "AppSignInHandoff_userId_idx" ON "AppSignInHandoff"("userId");
CREATE INDEX "AppSignInHandoff_expiresAt_idx" ON "AppSignInHandoff"("expiresAt");
