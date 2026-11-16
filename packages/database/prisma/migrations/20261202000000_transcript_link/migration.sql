-- The "Chat with meeting transcript" line last added to a note's generated notes, exactly as written. Null on
-- every existing note: none has one yet, so its next generation adds it.
ALTER TABLE "Note" ADD COLUMN "transcriptLink" TEXT;
