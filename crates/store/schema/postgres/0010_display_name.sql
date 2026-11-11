-- The name a user is shown by, which the user chooses; empty for none, and then the
-- screen shows the login name. Not unique.
ALTER TABLE users ADD COLUMN display_name TEXT NOT NULL DEFAULT '';
