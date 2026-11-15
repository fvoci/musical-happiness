-- The browser a registration was made from: the digest of a secret that only its
-- cookie keeps. The link of the mail confirms the registration there at once, and
-- anywhere else only with the password it was made with; a second registration from
-- the browser replaces the first. NULL for one made before this, which the password
-- confirms.
ALTER TABLE signups ADD COLUMN browser_hash TEXT;
CREATE UNIQUE INDEX signups_browser_hash_key ON signups (browser_hash);
