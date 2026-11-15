-- The browser a registration was made from, as schema/postgres/0040_signup_browser.sql
-- says.
ALTER TABLE signups ADD COLUMN browser_hash TEXT;
CREATE UNIQUE INDEX signups_browser_hash_key ON signups (browser_hash);
