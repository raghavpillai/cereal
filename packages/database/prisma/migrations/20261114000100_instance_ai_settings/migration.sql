ALTER TABLE "InstanceSettings"
  ADD COLUMN "instanceLogo" TEXT,
  ADD COLUMN "chatModels" JSONB,
  ADD COLUMN "generationModel" JSONB,
  ADD COLUMN "transcriptionModel" JSONB,
  ADD COLUMN "openAiSecret" TEXT,
  ADD COLUMN "anthropicSecret" TEXT;

ALTER TABLE "InstanceSettings" ALTER COLUMN "instanceName" SET DEFAULT 'Cereal';
