-- When an account was given its welcome board, a copy of the built-in one: an account
-- is given one once, as it starts its organization or comes into one as a member.
-- NULL until then. The accounts that are there are past their welcome, whether or
-- not they were given a board: none is made for them when they come into another
-- organization.
ALTER TABLE users ADD COLUMN welcomed_at BIGINT;
UPDATE users SET welcomed_at = create_at;
