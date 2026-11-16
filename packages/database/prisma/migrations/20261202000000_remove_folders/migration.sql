-- Cereal no longer has folders. Their tables go, with every folder and every note's membership in one; the
-- notes themselves are not touched. The folders permission is taken out of every OAuth registration, grant and
-- token, so none still names it.
DROP TABLE "MeetingFolderNote";
DROP TABLE "MeetingFolder";
UPDATE "OauthClient" SET "scopes"=array_remove("scopes",'folders:write'),"clientCredentialsScopes"=array_remove("clientCredentialsScopes",'folders:write') WHERE 'folders:write'=ANY("scopes") OR 'folders:write'=ANY("clientCredentialsScopes");
UPDATE "OauthResource" SET "allowedScopes"=array_remove("allowedScopes",'folders:write') WHERE 'folders:write'=ANY("allowedScopes");
UPDATE "OauthConsent" SET "scopes"=array_remove("scopes",'folders:write') WHERE 'folders:write'=ANY("scopes");
UPDATE "OauthAccessToken" SET "scopes"=array_remove("scopes",'folders:write') WHERE 'folders:write'=ANY("scopes");
UPDATE "OauthRefreshToken" SET "scopes"=array_remove("scopes",'folders:write') WHERE 'folders:write'=ANY("scopes");
