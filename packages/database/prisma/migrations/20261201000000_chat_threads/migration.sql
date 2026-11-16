-- Chat conversations, kept on the server per person so they follow that person to every app
-- (`/api/v1/chats`). A thread is its owner's alone; deleting the person deletes their threads, and deleting
-- a note deletes the threads about it. Nothing existing is changed or backfilled: the apps upload the
-- conversations they kept on the device themselves, once, when they next sign in.
CREATE TABLE "ChatThread" (
    "id" UUID NOT NULL,
    "userId" UUID NOT NULL,
    "noteId" UUID,
    "title" VARCHAR(200),
    "titleSource" VARCHAR(16),
    "turnCount" INTEGER NOT NULL DEFAULT 0,
    "createdAt" TIMESTAMPTZ(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,
    "updatedAt" TIMESTAMPTZ(3) NOT NULL,
    CONSTRAINT "ChatThread_pkey" PRIMARY KEY ("id")
);
CREATE TABLE "ChatMessage" (
    "id" UUID NOT NULL,
    "threadId" UUID NOT NULL,
    "position" INTEGER NOT NULL,
    "role" VARCHAR(16) NOT NULL,
    "content" TEXT NOT NULL DEFAULT '',
    "status" VARCHAR(16) NOT NULL DEFAULT 'complete',
    "error" VARCHAR(2000),
    "model" VARCHAR(200),
    "citations" JSONB NOT NULL DEFAULT '[]',
    "tools" JSONB NOT NULL DEFAULT '[]',
    "elapsedMs" INTEGER,
    "createdAt" TIMESTAMPTZ(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,
    "updatedAt" TIMESTAMPTZ(3) NOT NULL,
    CONSTRAINT "ChatMessage_pkey" PRIMARY KEY ("id")
);
CREATE INDEX "ChatThread_userId_updatedAt_id_idx" ON "ChatThread"("userId", "updatedAt" DESC, "id" DESC);
CREATE INDEX "ChatThread_userId_noteId_updatedAt_idx" ON "ChatThread"("userId", "noteId", "updatedAt" DESC);
CREATE INDEX "ChatThread_noteId_idx" ON "ChatThread"("noteId");
CREATE UNIQUE INDEX "ChatMessage_threadId_position_key" ON "ChatMessage"("threadId", "position");
ALTER TABLE "ChatThread" ADD CONSTRAINT "ChatThread_userId_fkey" FOREIGN KEY ("userId") REFERENCES "User"("id") ON DELETE CASCADE ON UPDATE CASCADE;
ALTER TABLE "ChatThread" ADD CONSTRAINT "ChatThread_noteId_fkey" FOREIGN KEY ("noteId") REFERENCES "Note"("id") ON DELETE CASCADE ON UPDATE CASCADE;
ALTER TABLE "ChatMessage" ADD CONSTRAINT "ChatMessage_threadId_fkey" FOREIGN KEY ("threadId") REFERENCES "ChatThread"("id") ON DELETE CASCADE ON UPDATE CASCADE;
