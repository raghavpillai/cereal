-- Where a browser goes after Google sign-in may now be as long as the signed query an AI client's
-- connection carries through sign-in (16,384 characters, the bound the web app uses).
ALTER TABLE "GoogleSignInState" ALTER COLUMN "returnPath" TYPE VARCHAR(16384);
