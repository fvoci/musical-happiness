-- A link mailed to the address an account had, with the notice that the address was
-- changed, that gives the account that address back (`Store::revert_email_change`):
-- for its owner, should somebody who knew the password have moved it. The link is
-- stored as its digest only. While it stands its address is the account's to have
-- again: no other account and no registration takes it. It ends when it is used, with
-- the links of the account made after it, when the erasing of the account begins, and
-- at `expires_at`, seven days after the change; nothing else ends it: not another
-- change, nor a password set anew.
CREATE TABLE email_reverts (
    token_hash TEXT PRIMARY KEY NOT NULL,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    email TEXT NOT NULL,
    -- Later than every other link of the account, also one made at the same moment.
    create_at BIGINT NOT NULL,
    expires_at BIGINT NOT NULL
);
CREATE INDEX email_reverts_user_id_idx ON email_reverts (user_id, create_at);
CREATE INDEX email_reverts_email_idx ON email_reverts (lower(email));
CREATE INDEX email_reverts_expires_at_idx ON email_reverts (expires_at);
-- Since when the account's password is taken nowhere, in milliseconds since the Unix
-- epoch, 0 while it is taken: not by a login, nor as the proof of anything a session
-- asks. Set where a link gave the account its address back,
-- which says the password was known to somebody else; a new password set through a
-- reset link or by the operator ends it. The hash stays: the account still has a
-- password, so a session made through a provider opens no more than it did.
ALTER TABLE users ADD COLUMN password_locked_at BIGINT NOT NULL DEFAULT 0;
