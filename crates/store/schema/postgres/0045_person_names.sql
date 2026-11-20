-- The parts of a person's name, which of them they are shown by, and their title and
-- credentials, each empty for none. display_name is from now on the name they are
-- shown by, made from these whenever they change (fvoci_model::PersonName::shown);
-- the name they wrote to be shown by, which it was until now, is custom_name, and an
-- account that was there is shown by it: nobody's name looks other than it did.
-- name_shown is a code of fvoci_model::NameShown, title one of Title or empty, and
-- credentials the codes of Credential with a space between each, in the person's
-- order.
ALTER TABLE users ADD COLUMN first_name TEXT NOT NULL DEFAULT '';
ALTER TABLE users ADD COLUMN last_name TEXT NOT NULL DEFAULT '';
ALTER TABLE users ADD COLUMN latin_first_name TEXT NOT NULL DEFAULT '';
ALTER TABLE users ADD COLUMN latin_last_name TEXT NOT NULL DEFAULT '';
ALTER TABLE users ADD COLUMN custom_name TEXT NOT NULL DEFAULT '';
ALTER TABLE users ADD COLUMN name_shown TEXT NOT NULL DEFAULT 'custom_name';
ALTER TABLE users ADD COLUMN title TEXT NOT NULL DEFAULT '';
ALTER TABLE users ADD COLUMN credentials TEXT NOT NULL DEFAULT '';
UPDATE users SET custom_name = display_name;
