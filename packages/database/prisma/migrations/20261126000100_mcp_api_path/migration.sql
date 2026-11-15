-- The MCP endpoint moved from <origin>/mcp to <origin>/api/mcp; the old address still answers. Each address is
-- an OAuth resource. Clients registered while only <origin>/mcp existed are linked to that resource alone, and
-- the provider issues a token only for a resource its client is linked to. So that such a client can also be
-- pointed at the new address, the new resource is created beside the old one here (same policy) and every
-- existing link is copied to it. On a database with no MCP clients this does nothing.
INSERT INTO "OauthResource" ("id","identifier","name","accessTokenTtl","refreshTokenTtl","signingAlgorithm","signingKeyId","allowedScopes","customClaims","dpopBoundAccessTokensRequired","disabled","createdAt","updatedAt","policyVersion","metadata")
SELECT gen_random_uuid()::text, regexp_replace(old."identifier", '/mcp$', '/api/mcp'), old."name", old."accessTokenTtl", old."refreshTokenTtl", old."signingAlgorithm", old."signingKeyId", old."allowedScopes", old."customClaims", old."dpopBoundAccessTokensRequired", old."disabled", CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, old."policyVersion", old."metadata"
FROM "OauthResource" old
WHERE old."identifier" ~ '^https?://[^/]+/mcp$'
ON CONFLICT ("identifier") DO NOTHING;

INSERT INTO "OauthClientResource" ("id","clientId","resourceId","metadata","createdAt")
SELECT gen_random_uuid()::text, link."clientId", regexp_replace(link."resourceId", '/mcp$', '/api/mcp'), link."metadata", CURRENT_TIMESTAMP
FROM "OauthClientResource" link
WHERE link."resourceId" ~ '^https?://[^/]+/mcp$'
ON CONFLICT ("clientId","resourceId") DO NOTHING;
