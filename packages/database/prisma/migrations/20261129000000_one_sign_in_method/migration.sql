-- A server has exactly one way to sign in: email and password, or Google. Where both were on, password stays
-- (Google is turned off; its client stays stored). Google-only servers are left as they are.
UPDATE "InstanceSettings" SET "signInGoogle" = FALSE WHERE "signInPassword" = TRUE AND "signInGoogle" = TRUE;
-- A Google sign-in that proves a Google client before it is trusted: claiming a new server with Google, or an
-- administrator switching the server to Google. Hashes only; the client secret is sealed and cleared when done.
CREATE TABLE "GoogleVerification" (
 "stateHash" CHAR(64) NOT NULL,
 "browserHash" CHAR(64) NOT NULL,
 "purpose" VARCHAR(16) NOT NULL,
 "userId" UUID,
 "clientId" VARCHAR(255) NOT NULL,
 "encryptedSecret" TEXT,
 "encryptedVerifier" TEXT NOT NULL,
 "outcome" VARCHAR(32),
 "expiresAt" TIMESTAMPTZ(3) NOT NULL,
 "createdAt" TIMESTAMPTZ(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,
 CONSTRAINT "GoogleVerification_pkey" PRIMARY KEY ("stateHash")
);
CREATE INDEX "GoogleVerification_browserHash_idx" ON "GoogleVerification"("browserHash");
CREATE INDEX "GoogleVerification_expiresAt_idx" ON "GoogleVerification"("expiresAt");
