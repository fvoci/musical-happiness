-- The same as schema/postgres/0046_nameless_shown_by_name.sql, which says why.
UPDATE users SET name_shown = 'name'
WHERE name_shown = 'custom_name' AND custom_name = '';
