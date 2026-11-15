CREATE TYPE "AudioChannel" AS ENUM ('mic', 'system');
CREATE TABLE "Note" (
  "id" UUID NOT NULL,
  "title" TEXT NOT NULL DEFAULT '',
  "body" TEXT NOT NULL DEFAULT '',
  "enhanced" TEXT NOT NULL DEFAULT '',
  "date" DATE NOT NULL,
  "duration" VARCHAR(32) NOT NULL DEFAULT '',
  "recordingState" VARCHAR(32) NOT NULL DEFAULT 'idle',
  "generationState" VARCHAR(32) NOT NULL DEFAULT 'idle',
  "transcriptRevision" INTEGER NOT NULL DEFAULT 0,
  "generatedRevision" INTEGER,
  "createdAt" TIMESTAMPTZ(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,
  "updatedAt" TIMESTAMPTZ(3) NOT NULL,
  CONSTRAINT "Note_pkey" PRIMARY KEY ("id"),
  CONSTRAINT "Note_recordingState" CHECK ("recordingState" IN ('idle', 'recording', 'stopped')),
  CONSTRAINT "Note_generationState" CHECK ("generationState" IN ('idle', 'analyzing_transcript', 'writing_notes', 'ready', 'failed', 'unsupported')),
  CONSTRAINT "Note_revision_order" CHECK ("transcriptRevision" >= 0 AND ("generatedRevision" IS NULL OR ("generatedRevision" >= 0 AND "generatedRevision" <= "transcriptRevision"))),
  CONSTRAINT "Note_title_length" CHECK (char_length("title") <= 500),
  CONSTRAINT "Note_body_length" CHECK (char_length("body") <= 1000000),
  CONSTRAINT "Note_enhanced_length" CHECK (char_length("enhanced") <= 1000000)
);
CREATE TABLE "TranscriptSegment" (
  "id" UUID NOT NULL,
  "noteId" UUID NOT NULL,
  "channel" "AudioChannel" NOT NULL,
  "speakerId" VARCHAR(100) NOT NULL,
  "speakerLabel" VARCHAR(200),
  "text" TEXT NOT NULL,
  "isFinal" BOOLEAN NOT NULL DEFAULT false,
  "startMs" INTEGER NOT NULL,
  "endMs" INTEGER,
  CONSTRAINT "TranscriptSegment_pkey" PRIMARY KEY ("id"),
  CONSTRAINT "TranscriptSegment_start_nonnegative" CHECK ("startMs" >= 0),
  CONSTRAINT "TranscriptSegment_end_order" CHECK ("endMs" IS NULL OR "endMs" >= "startMs"),
  CONSTRAINT "TranscriptSegment_text_length" CHECK (char_length("text") <= 20000),
  CONSTRAINT "TranscriptSegment_noteId_fkey" FOREIGN KEY ("noteId") REFERENCES "Note"("id") ON DELETE CASCADE ON UPDATE CASCADE
);
CREATE TABLE "SharedNote" (
  "id" UUID NOT NULL,
  "noteId" UUID NOT NULL,
  "tokenHash" CHAR(64) NOT NULL,
  "snapshot" JSONB NOT NULL,
  "createdAt" TIMESTAMPTZ(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,
  "expiresAt" TIMESTAMPTZ(3),
  "revokedAt" TIMESTAMPTZ(3),
  CONSTRAINT "SharedNote_pkey" PRIMARY KEY ("id"),
  CONSTRAINT "SharedNote_noteId_fkey" FOREIGN KEY ("noteId") REFERENCES "Note"("id") ON DELETE CASCADE ON UPDATE CASCADE
);
CREATE INDEX "Note_updatedAt_idx" ON "Note"("updatedAt" DESC);
CREATE INDEX "TranscriptSegment_noteId_startMs_idx" ON "TranscriptSegment"("noteId", "startMs");
CREATE UNIQUE INDEX "SharedNote_tokenHash_key" ON "SharedNote"("tokenHash");
CREATE INDEX "SharedNote_noteId_createdAt_idx" ON "SharedNote"("noteId", "createdAt" DESC);
