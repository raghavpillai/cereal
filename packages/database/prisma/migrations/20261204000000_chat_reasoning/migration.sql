-- The model's reasoning behind a saved chat answer, as the provider gave it: a list of stretches, each with its
-- text and how many of the answer's tool steps had started before it began. Existing answers have none.
ALTER TABLE "ChatMessage" ADD COLUMN "reasoning" JSONB NOT NULL DEFAULT '[]';
