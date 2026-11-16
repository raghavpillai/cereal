-- The OpenAI-compatible language provider: where its API starts, and its optional key (sealed like the others).
ALTER TABLE "InstanceSettings"
 ADD COLUMN "openAiCompatibleBaseUrl" VARCHAR(2048),
 ADD COLUMN "openAiCompatibleSecret" TEXT;
