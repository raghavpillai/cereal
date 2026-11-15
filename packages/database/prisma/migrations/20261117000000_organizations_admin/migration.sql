-- Better Auth Admin + Organization. No note or share ownership is modified.
ALTER TABLE "User" ADD COLUMN "banned" BOOLEAN NOT NULL DEFAULT false,
 ADD COLUMN "banReason" TEXT, ADD COLUMN "banExpires" TIMESTAMPTZ(3);
CREATE TABLE "Organization" (
 "id" UUID PRIMARY KEY, "name" VARCHAR(100) NOT NULL,
 "slug" VARCHAR(100) NOT NULL UNIQUE, "logo" TEXT, "metadata" TEXT,
 "createdAt" TIMESTAMPTZ(3) NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE "Member" (
 "id" UUID PRIMARY KEY, "organizationId" UUID NOT NULL REFERENCES "Organization"("id") ON DELETE CASCADE,
 "userId" UUID NOT NULL REFERENCES "User"("id") ON DELETE CASCADE,
 "role" VARCHAR(16) NOT NULL DEFAULT 'member' CHECK ("role" IN ('owner','admin','member')),
 "createdAt" TIMESTAMPTZ(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,
 UNIQUE("organizationId","userId")
);
CREATE INDEX "Member_userId_idx" ON "Member"("userId");
CREATE TABLE "Invitation" (
 "id" UUID PRIMARY KEY, "organizationId" UUID NOT NULL REFERENCES "Organization"("id") ON DELETE CASCADE,
 "email" VARCHAR(254) NOT NULL, "role" VARCHAR(16) NOT NULL DEFAULT 'member' CHECK ("role" IN ('admin','member')),
 "status" VARCHAR(16) NOT NULL DEFAULT 'pending' CHECK ("status" IN ('pending','accepted','rejected','canceled')),
 "inviterId" UUID NOT NULL REFERENCES "User"("id") ON DELETE CASCADE,
 "createdAt" TIMESTAMPTZ(3) NOT NULL DEFAULT CURRENT_TIMESTAMP, "expiresAt" TIMESTAMPTZ(3) NOT NULL
);
CREATE INDEX "Invitation_organizationId_status_idx" ON "Invitation"("organizationId","status");
CREATE INDEX "Invitation_email_status_idx" ON "Invitation"("email","status");
ALTER TABLE "Session" ADD COLUMN "impersonatedBy" UUID,
 ADD COLUMN "activeOrganizationId" UUID REFERENCES "Organization"("id") ON DELETE SET NULL;
-- Existing accounts retain private notes; a membership is navigation context only.
INSERT INTO "Organization"("id","name","slug") VALUES
 ('00000000-0000-4000-8000-000000000002','Default workspace','default') ON CONFLICT DO NOTHING;
INSERT INTO "Member"("id","organizationId","userId","role")
 SELECT gen_random_uuid(),'00000000-0000-4000-8000-000000000002',"id",
 CASE WHEN "id"='00000000-0000-4000-8000-000000000001'::uuid THEN 'owner' ELSE 'member' END
 FROM "User" ON CONFLICT("organizationId","userId") DO NOTHING;
