-- The team space: notes their owners show to a whole workspace. One row is the whole grant: while it exists
-- every member of the workspace may read the note's title and documents; nothing else about the note changes
-- (it stays its owner's, and only they can edit or delete it). The row goes with the note and with the
-- workspace. Nothing existing is shared by this migration: every space starts empty.
CREATE TABLE "TeamSpaceNote" (
    "noteId" UUID NOT NULL,
    "organizationId" UUID NOT NULL,
    "addedById" UUID NOT NULL,
    "addedAt" TIMESTAMPTZ(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT "TeamSpaceNote_pkey" PRIMARY KEY ("noteId")
);
CREATE INDEX "TeamSpaceNote_organizationId_addedAt_idx" ON "TeamSpaceNote"("organizationId", "addedAt" DESC);
ALTER TABLE "TeamSpaceNote" ADD CONSTRAINT "TeamSpaceNote_noteId_fkey" FOREIGN KEY ("noteId") REFERENCES "Note"("id") ON DELETE CASCADE ON UPDATE CASCADE;
ALTER TABLE "TeamSpaceNote" ADD CONSTRAINT "TeamSpaceNote_organizationId_fkey" FOREIGN KEY ("organizationId") REFERENCES "Organization"("id") ON DELETE CASCADE ON UPDATE CASCADE;
