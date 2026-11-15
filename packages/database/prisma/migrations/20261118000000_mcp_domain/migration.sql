-- Private, owner-scoped MCP organization context. Membership never grants notes.
ALTER TABLE "Note" ADD COLUMN "organizationId" UUID REFERENCES "Organization"("id") ON DELETE SET NULL;
CREATE INDEX "Note_ownerId_organizationId_date_idx" ON "Note"("ownerId","organizationId","date");
CREATE TABLE "UserWorkspaceState" (
 "userId" UUID PRIMARY KEY REFERENCES "User"("id") ON DELETE CASCADE,
 "activeOrganizationId" UUID REFERENCES "Organization"("id") ON DELETE SET NULL,
 "updatedAt" TIMESTAMPTZ(3) NOT NULL DEFAULT CURRENT_TIMESTAMP
);
INSERT INTO "UserWorkspaceState"("userId","activeOrganizationId")
 SELECT "User"."id",(SELECT "organizationId" FROM "Member" WHERE "Member"."userId"="User"."id"
 ORDER BY ("organizationId"='00000000-0000-4000-8000-000000000002'::uuid) DESC,"createdAt","id" LIMIT 1) FROM "User";
UPDATE "Note" SET "organizationId"="UserWorkspaceState"."activeOrganizationId" FROM "UserWorkspaceState" WHERE "Note"."ownerId"="UserWorkspaceState"."userId";
CREATE TABLE "MeetingFolder" (
 "id" UUID PRIMARY KEY,"ownerId" UUID NOT NULL REFERENCES "User"("id") ON DELETE CASCADE,
 "organizationId" UUID NOT NULL REFERENCES "Organization"("id") ON DELETE CASCADE,
 "name" VARCHAR(200) NOT NULL,"description" VARCHAR(2000) NOT NULL DEFAULT '',
 "createdAt" TIMESTAMPTZ(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,"updatedAt" TIMESTAMPTZ(3) NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX "MeetingFolder_ownerId_organizationId_createdAt_idx" ON "MeetingFolder"("ownerId","organizationId","createdAt");
CREATE TABLE "MeetingFolderNote" (
 "folderId" UUID NOT NULL REFERENCES "MeetingFolder"("id") ON DELETE CASCADE,
 "noteId" UUID NOT NULL REFERENCES "Note"("id") ON DELETE CASCADE,
 "createdAt" TIMESTAMPTZ(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,PRIMARY KEY("folderId","noteId")
);
CREATE INDEX "MeetingFolderNote_noteId_idx" ON "MeetingFolderNote"("noteId");
CREATE TABLE "NoteAttachment" (
 "id" UUID PRIMARY KEY,"noteId" UUID NOT NULL REFERENCES "Note"("id") ON DELETE CASCADE,
 "name" VARCHAR(255) NOT NULL,"mimeType" VARCHAR(40) NOT NULL,
 "data" BYTEA NOT NULL CHECK(octet_length("data") BETWEEN 1 AND 5242880),
 "createdAt" TIMESTAMPTZ(3) NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX "NoteAttachment_noteId_createdAt_idx" ON "NoteAttachment"("noteId","createdAt");
CREATE TABLE "AgentBrief" (
 "userId" UUID NOT NULL REFERENCES "User"("id") ON DELETE CASCADE,
 "organizationId" UUID NOT NULL REFERENCES "Organization"("id") ON DELETE CASCADE,
 "body" TEXT NOT NULL DEFAULT '' CHECK(length("body")<=20000),
 "updatedAt" TIMESTAMPTZ(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,PRIMARY KEY("userId","organizationId")
);

-- Better Auth plugin schema: text OAuth identifiers; account/session foreign keys remain UUID.
CREATE TABLE "OauthClient" (
 "id" TEXT PRIMARY KEY,
 "clientId" TEXT NOT NULL UNIQUE,
 "clientSecret" TEXT,
 "clientDiscoveryId" TEXT,
 "disabled" BOOLEAN DEFAULT false,
 "skipConsent" BOOLEAN,
 "enableEndSession" BOOLEAN,
 "subjectType" TEXT,
 "scopes" TEXT[] NOT NULL DEFAULT '{}'::TEXT[],
 "clientCredentialsScopes" TEXT[] NOT NULL DEFAULT '{}'::TEXT[],
 "userId" UUID REFERENCES "User"("id") ON DELETE CASCADE,
 "createdAt" TIMESTAMPTZ(3),
 "updatedAt" TIMESTAMPTZ(3),
 "name" TEXT,
 "uri" TEXT,
 "icon" TEXT,
 "contacts" TEXT[] NOT NULL DEFAULT '{}'::TEXT[],
 "tos" TEXT,
 "policy" TEXT,
 "softwareId" TEXT,
 "softwareVersion" TEXT,
 "softwareStatement" TEXT,
 "redirectUris" TEXT[] NOT NULL DEFAULT '{}'::TEXT[],
 "postLogoutRedirectUris" TEXT[] NOT NULL DEFAULT '{}'::TEXT[],
 "backchannelLogoutUri" TEXT,
 "backchannelLogoutSessionRequired" BOOLEAN,
 "tokenEndpointAuthMethod" TEXT,
 "applicationType" TEXT,
 "jwks" TEXT,
 "jwksUri" TEXT,
 "grantTypes" TEXT[] NOT NULL DEFAULT '{}'::TEXT[],
 "responseTypes" TEXT[] NOT NULL DEFAULT '{}'::TEXT[],
 "requirePKCE" BOOLEAN,
 "dpopBoundAccessTokens" BOOLEAN DEFAULT false,
 "referenceId" TEXT,
 "metadata" JSONB
);
CREATE INDEX "OauthClient_userId_idx" ON "OauthClient"("userId");
CREATE TABLE "OauthResource" (
 "id" TEXT PRIMARY KEY,
 "identifier" TEXT NOT NULL UNIQUE,
 "name" TEXT NOT NULL,
 "accessTokenTtl" INTEGER,
 "refreshTokenTtl" INTEGER,
 "signingAlgorithm" TEXT,
 "signingKeyId" TEXT,
 "allowedScopes" TEXT[] NOT NULL DEFAULT '{}'::TEXT[],
 "customClaims" JSONB,
 "dpopBoundAccessTokensRequired" BOOLEAN DEFAULT false,
 "disabled" BOOLEAN DEFAULT false,
 "createdAt" TIMESTAMPTZ(3),
 "updatedAt" TIMESTAMPTZ(3),
 "policyVersion" INTEGER DEFAULT 1,
 "metadata" JSONB
);
CREATE TABLE "OauthClientResource" (
 "id" TEXT PRIMARY KEY,
 "clientId" TEXT NOT NULL REFERENCES "OauthClient"("clientId") ON DELETE CASCADE,
 "resourceId" TEXT NOT NULL REFERENCES "OauthResource"("identifier") ON DELETE CASCADE,
 "metadata" JSONB,
 "createdAt" TIMESTAMPTZ(3),
 UNIQUE("clientId","resourceId")
);
CREATE INDEX "OauthClientResource_clientId_idx" ON "OauthClientResource"("clientId");
CREATE INDEX "OauthClientResource_resourceId_idx" ON "OauthClientResource"("resourceId");
CREATE TABLE "OauthRefreshToken" (
 "id" TEXT PRIMARY KEY,
 "token" TEXT NOT NULL UNIQUE,
 "clientId" TEXT NOT NULL REFERENCES "OauthClient"("clientId") ON DELETE CASCADE,
 "sessionId" UUID REFERENCES "Session"("id") ON DELETE SET NULL,
 "userId" UUID NOT NULL REFERENCES "User"("id") ON DELETE CASCADE,
 "referenceId" TEXT,
 "authorizationCodeId" TEXT,
 "resources" TEXT[] NOT NULL DEFAULT '{}'::TEXT[],
 "requestedUserInfoClaims" TEXT[] NOT NULL DEFAULT '{}'::TEXT[],
 "expiresAt" TIMESTAMPTZ(3) NOT NULL,
 "createdAt" TIMESTAMPTZ(3) NOT NULL,
 "revoked" TIMESTAMPTZ(3),
 "rotatedAt" TIMESTAMPTZ(3),
 "rotationReplayResponse" TEXT,
 "rotationReplayExpiresAt" TIMESTAMPTZ(3),
 "authTime" TIMESTAMPTZ(3),
 "confirmation" JSONB,
 "scopes" TEXT[] NOT NULL DEFAULT '{}'::TEXT[]
);
CREATE INDEX "OauthRefreshToken_clientId_idx" ON "OauthRefreshToken"("clientId");
CREATE INDEX "OauthRefreshToken_sessionId_idx" ON "OauthRefreshToken"("sessionId");
CREATE INDEX "OauthRefreshToken_userId_idx" ON "OauthRefreshToken"("userId");
CREATE INDEX "OauthRefreshToken_authorizationCodeId_idx" ON "OauthRefreshToken"("authorizationCodeId");
CREATE TABLE "OauthAccessToken" (
 "id" TEXT PRIMARY KEY,
 "token" TEXT NOT NULL UNIQUE,
 "clientId" TEXT NOT NULL REFERENCES "OauthClient"("clientId") ON DELETE CASCADE,
 "sessionId" UUID REFERENCES "Session"("id") ON DELETE SET NULL,
 "userId" UUID REFERENCES "User"("id") ON DELETE CASCADE,
 "referenceId" TEXT,
 "authorizationCodeId" TEXT,
 "resources" TEXT[] NOT NULL DEFAULT '{}'::TEXT[],
 "requestedUserInfoClaims" TEXT[] NOT NULL DEFAULT '{}'::TEXT[],
 "refreshId" TEXT REFERENCES "OauthRefreshToken"("id") ON DELETE CASCADE,
 "expiresAt" TIMESTAMPTZ(3) NOT NULL,
 "createdAt" TIMESTAMPTZ(3) NOT NULL,
 "revoked" TIMESTAMPTZ(3),
 "confirmation" JSONB,
 "scopes" TEXT[] NOT NULL DEFAULT '{}'::TEXT[]
);
CREATE INDEX "OauthAccessToken_clientId_idx" ON "OauthAccessToken"("clientId");
CREATE INDEX "OauthAccessToken_sessionId_idx" ON "OauthAccessToken"("sessionId");
CREATE INDEX "OauthAccessToken_userId_idx" ON "OauthAccessToken"("userId");
CREATE INDEX "OauthAccessToken_authorizationCodeId_idx" ON "OauthAccessToken"("authorizationCodeId");
CREATE INDEX "OauthAccessToken_refreshId_idx" ON "OauthAccessToken"("refreshId");
CREATE TABLE "OauthConsent" (
 "id" TEXT PRIMARY KEY,
 "clientId" TEXT NOT NULL REFERENCES "OauthClient"("clientId") ON DELETE CASCADE,
 "userId" UUID REFERENCES "User"("id") ON DELETE CASCADE,
 "referenceId" TEXT,
 "resources" TEXT[] NOT NULL DEFAULT '{}'::TEXT[],
 "requestedUserInfoClaims" TEXT[] NOT NULL DEFAULT '{}'::TEXT[],
 "scopes" TEXT[] NOT NULL DEFAULT '{}'::TEXT[],
 "createdAt" TIMESTAMPTZ(3) NOT NULL,
 "updatedAt" TIMESTAMPTZ(3) NOT NULL
);
CREATE INDEX "OauthConsent_clientId_idx" ON "OauthConsent"("clientId");
CREATE INDEX "OauthConsent_userId_idx" ON "OauthConsent"("userId");
CREATE TABLE "OauthClientAssertion" (
 "id" TEXT PRIMARY KEY,
 "expiresAt" TIMESTAMPTZ(3) NOT NULL
);
CREATE TABLE "Jwks" (
 "id" TEXT PRIMARY KEY,
 "publicKey" TEXT NOT NULL,
 "privateKey" TEXT NOT NULL,
 "createdAt" TIMESTAMPTZ(3) NOT NULL,
 "expiresAt" TIMESTAMPTZ(3),
 "alg" TEXT,
 "crv" TEXT
);
