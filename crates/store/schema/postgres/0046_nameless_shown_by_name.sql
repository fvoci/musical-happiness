-- An account that had no name (and what is left of an erased one) is shown by its
-- parts of a name from now on, as a new account is: 0045 put every account on the name
-- it wrote, which, for one that wrote none, would keep the name it gives later unseen.
-- It looks no other than it did: with every part empty the choice falls to the login
-- name. One that has given a part since keeps its choice, and the name it is shown by.
UPDATE users SET name_shown = 'name'
WHERE name_shown = 'custom_name' AND custom_name = '' AND first_name = ''
    AND last_name = '' AND latin_first_name = '' AND latin_last_name = '';
