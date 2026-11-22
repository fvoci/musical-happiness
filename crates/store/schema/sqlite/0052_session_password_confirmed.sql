-- When a session confirmed its account's password, in milliseconds since the Unix
-- epoch; 0 for one that has not. A session made through a provider that has confirmed
-- it opens the account's own space as one made with the password does, and still
-- only what its provider opens of an organization that takes no password. It ends
-- with the session, and one statement sets it (`Store::confirm_session_password`): a
-- session is stored without it.
ALTER TABLE sessions ADD COLUMN password_confirmed_at BIGINT NOT NULL DEFAULT 0;
