-- First-run setup on the admin panel. `onboardingCompletedAt` is set when the setup flow is finished;
-- `onboardingSteps` records the steps that were confirmed or skipped there, such as
-- {"signIn":"done","storage":"skipped"}. The other steps are read from what is actually stored.
ALTER TABLE "InstanceSettings"
 ADD COLUMN "onboardingCompletedAt" TIMESTAMPTZ(3),
 ADD COLUMN "onboardingSteps" JSONB;
-- A server that already has a provider key stored was set up before this flow existed: it is finished.
-- A fresh one has no row here, or a row without keys, and starts with the setup flow.
UPDATE "InstanceSettings" SET "onboardingCompletedAt" = CURRENT_TIMESTAMP
 WHERE "onboardingCompletedAt" IS NULL
   AND ("openRouterSecret" IS NOT NULL OR "openAiSecret" IS NOT NULL OR "anthropicSecret" IS NOT NULL OR "elevenLabsSecret" IS NOT NULL);
