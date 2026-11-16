-- Speech is transcribed live only. Audio parts are no longer uploaded or transcribed in batch, and only a streaming
-- speech model can be selected.

-- Parts uploaded by older versions: nothing reads them any more. Their audio files are removed with their notes.
-- Parts that were never transcribed have no transcript and no way to get one: say how many before they go.
DO $$
DECLARE untranscribed integer; notes integer;
BEGIN
  SELECT count(*), count(DISTINCT "noteId") INTO untranscribed, notes FROM "AudioChunk" WHERE "state" <> 'ready';
  RAISE NOTICE 'live_only: dropping % audio parts that were never transcribed (in % notes); % parts in all', untranscribed, notes, (SELECT count(*) FROM "AudioChunk");
END $$;
DROP TABLE "AudioChunk";

-- The coverage ledger no longer distinguishes batch claims: every range in it is audio a transcript exists for.
ALTER TABLE "LiveCoverage" DROP COLUMN "source", DROP COLUMN "chunkId";

-- The batch speech model setting is gone. A live model that is not a streaming model (or "off", saved by an older
-- version) is cleared, which reads as the default live model.
ALTER TABLE "InstanceSettings" DROP COLUMN "transcriptionModel";
UPDATE "InstanceSettings" SET "liveTranscriptionModel" = NULL
 WHERE "liveTranscriptionModel" IS NOT NULL
   AND NOT COALESCE(jsonb_typeof("liveTranscriptionModel"::jsonb) = 'object'
     AND "liveTranscriptionModel"::jsonb->>'provider' = 'elevenlabs'
     AND "liveTranscriptionModel"::jsonb->>'model' IN ('scribe_v3_live', 'scribe_v2_realtime'), false);

-- Stretches captured but never transcribed, shown in the transcript as missing.
CREATE TABLE "LiveGap" (
 "id" UUID NOT NULL,
 "noteId" UUID NOT NULL,
 "channel" "AudioChannel" NOT NULL,
 "fromMs" INTEGER NOT NULL,
 "toMs" INTEGER NOT NULL,
 "reason" VARCHAR(32) NOT NULL,
 "createdAt" TIMESTAMPTZ(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,
 CONSTRAINT "LiveGap_pkey" PRIMARY KEY ("id"),
 CONSTRAINT "LiveGap_noteId_fkey" FOREIGN KEY ("noteId") REFERENCES "Note"("id") ON DELETE CASCADE ON UPDATE CASCADE
);
CREATE INDEX "LiveGap_noteId_fromMs_idx" ON "LiveGap"("noteId", "fromMs");
