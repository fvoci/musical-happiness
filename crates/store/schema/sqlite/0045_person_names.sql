-- The parts of a name, the choice, the title and the credentials: the same as
-- schema/postgres/0045_person_names.sql, which says what each is.
ALTER TABLE users ADD COLUMN first_name TEXT NOT NULL DEFAULT '';
ALTER TABLE users ADD COLUMN last_name TEXT NOT NULL DEFAULT '';
ALTER TABLE users ADD COLUMN latin_first_name TEXT NOT NULL DEFAULT '';
ALTER TABLE users ADD COLUMN latin_last_name TEXT NOT NULL DEFAULT '';
ALTER TABLE users ADD COLUMN custom_name TEXT NOT NULL DEFAULT '';
ALTER TABLE users ADD COLUMN name_shown TEXT NOT NULL DEFAULT 'custom_name';
ALTER TABLE users ADD COLUMN title TEXT NOT NULL DEFAULT '';
ALTER TABLE users ADD COLUMN credentials TEXT NOT NULL DEFAULT '';
UPDATE users SET custom_name = display_name;
