-- Where the server keeps recorded audio and published app builds, chosen on the admin panel:
-- local disk or an S3-compatible bucket. The bucket's secret access key is sealed with the settings key.
ALTER TABLE "InstanceSettings"
 ADD COLUMN "storageBackend" VARCHAR(16),
 ADD COLUMN "storageS3" JSONB,
 ADD COLUMN "storageS3Secret" TEXT;
