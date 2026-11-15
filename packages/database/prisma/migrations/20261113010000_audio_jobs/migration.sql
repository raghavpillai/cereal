CREATE TABLE "AudioChunk" (
 "id" UUID NOT NULL,
 "noteId" UUID NOT NULL,
 "channel" "AudioChannel" NOT NULL,
 "sequence" INTEGER NOT NULL,
 "startMs" INTEGER NOT NULL,
 "durationMs" INTEGER NOT NULL,
 "hash" CHAR(64) NOT NULL,
 "state" VARCHAR(32) NOT NULL DEFAULT 'queued',
 "error" TEXT,
 "lease" UUID,
 "leaseUntil" TIMESTAMPTZ(3),
 "createdAt" TIMESTAMPTZ(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,
 "updatedAt" TIMESTAMPTZ(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,
 CONSTRAINT "AudioChunk_pkey" PRIMARY KEY ("id"),
 CONSTRAINT "AudioChunk_noteId_fkey" FOREIGN KEY ("noteId") REFERENCES "Note"("id") ON DELETE CASCADE ON UPDATE CASCADE
);
CREATE UNIQUE INDEX "AudioChunk_noteId_channel_sequence_key" ON "AudioChunk"("noteId", "channel", "sequence");
CREATE INDEX "AudioChunk_state_leaseUntil_idx" ON "AudioChunk"("state", "leaseUntil");

ALTER TABLE "Note" ADD COLUMN "generationRun" UUID;
