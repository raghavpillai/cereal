-- Live (streaming) transcription: docs/design/live-transcription.md sections 4.4, 4.5 and 4.10.
ALTER TABLE "TranscriptSegment" ADD COLUMN "origin" VARCHAR(16) NOT NULL DEFAULT 'batch';
ALTER TABLE "AudioChunk" ADD COLUMN "resolution" VARCHAR(16);
ALTER TABLE "InstanceSettings" ADD COLUMN "liveTranscriptionModel" JSONB;

CREATE TABLE "LiveCoverage" (
 "id" UUID NOT NULL,
 "noteId" UUID NOT NULL,
 "channel" "AudioChannel" NOT NULL,
 "fromMs" INTEGER NOT NULL,
 "toMs" INTEGER NOT NULL,
 "source" VARCHAR(16) NOT NULL DEFAULT 'live',
 "sessionId" UUID,
 "chunkId" UUID,
 "updatedAt" TIMESTAMPTZ(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,
 CONSTRAINT "LiveCoverage_pkey" PRIMARY KEY ("id"),
 CONSTRAINT "LiveCoverage_noteId_fkey" FOREIGN KEY ("noteId") REFERENCES "Note"("id") ON DELETE CASCADE ON UPDATE CASCADE
);
CREATE INDEX "LiveCoverage_noteId_channel_fromMs_idx" ON "LiveCoverage"("noteId", "channel", "fromMs");

CREATE TABLE "NoteSpeaker" (
 "id" UUID NOT NULL,
 "noteId" UUID NOT NULL,
 "channel" "AudioChannel" NOT NULL,
 "number" INTEGER NOT NULL,
 "displayName" VARCHAR(200),
 "mergedIntoId" UUID,
 "createdAt" TIMESTAMPTZ(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,
 CONSTRAINT "NoteSpeaker_pkey" PRIMARY KEY ("id"),
 CONSTRAINT "NoteSpeaker_noteId_fkey" FOREIGN KEY ("noteId") REFERENCES "Note"("id") ON DELETE CASCADE ON UPDATE CASCADE
);
CREATE UNIQUE INDEX "NoteSpeaker_noteId_channel_number_key" ON "NoteSpeaker"("noteId", "channel", "number");
