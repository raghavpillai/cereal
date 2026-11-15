-- Configuration that used to be read from the environment and is now set on the admin panel:
-- live transcription limits and the iPhone app's identifiers. `importedEnvironment` remembers which
-- environment variables were taken into the settings once at start-up, so they are never read again.
ALTER TABLE "InstanceSettings"
 ADD COLUMN "liveMaxSessions" INTEGER,
 ADD COLUMN "liveHoldSeconds" INTEGER,
 ADD COLUMN "iosAppIds" JSONB,
 ADD COLUMN "iosAppStoreId" VARCHAR(20),
 ADD COLUMN "importedEnvironment" JSONB;
-- The product's first default name. An instance that still carries it never chose a name.
UPDATE "InstanceSettings" SET "instanceName" = 'Cereal' WHERE "instanceName" = 'Meeting Notes';
