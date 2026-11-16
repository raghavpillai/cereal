-- Sharing a note with people of its workspace. NoteAccess: one row per person a note is shared with.
-- NoteSharing: how far the note's links reach ("invited": only those people; "organization": every member of
-- the note's workspace who holds a link) and whether the owner has put the note away. No row means "invited".
CREATE TABLE "NoteSharing" (
    "noteId" UUID NOT NULL,
    "level" VARCHAR(16) NOT NULL DEFAULT 'invited',
    "suspended" BOOLEAN NOT NULL DEFAULT false,
    "updatedAt" TIMESTAMPTZ(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT "NoteSharing_pkey" PRIMARY KEY ("noteId"),
    CONSTRAINT "NoteSharing_level_check" CHECK ("level" IN ('invited', 'organization'))
);
ALTER TABLE "NoteSharing" ADD CONSTRAINT "NoteSharing_noteId_fkey" FOREIGN KEY ("noteId") REFERENCES "Note"("id") ON DELETE CASCADE ON UPDATE CASCADE;

CREATE TABLE "NoteAccess" (
    "noteId" UUID NOT NULL,
    "userId" UUID NOT NULL,
    "addedById" UUID NOT NULL,
    "addedAt" TIMESTAMPTZ(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT "NoteAccess_pkey" PRIMARY KEY ("noteId", "userId")
);
CREATE INDEX "NoteAccess_userId_addedAt_idx" ON "NoteAccess"("userId", "addedAt" DESC);
ALTER TABLE "NoteAccess" ADD CONSTRAINT "NoteAccess_noteId_fkey" FOREIGN KEY ("noteId") REFERENCES "Note"("id") ON DELETE CASCADE ON UPDATE CASCADE;
ALTER TABLE "NoteAccess" ADD CONSTRAINT "NoteAccess_userId_fkey" FOREIGN KEY ("userId") REFERENCES "User"("id") ON DELETE CASCADE ON UPDATE CASCADE;

-- Until now a link was read by anyone signed in to the server. A note that has a link in use keeps the
-- nearest level, every member of its workspace who holds the link, so no link stops working for the people
-- of the workspace it was sent to. Every other note starts at "invited".
INSERT INTO "NoteSharing" ("noteId", "level")
SELECT DISTINCT s."noteId", 'organization' FROM "SharedNote" s
WHERE s."revokedAt" IS NULL AND (s."expiresAt" IS NULL OR s."expiresAt" > CURRENT_TIMESTAMP);
