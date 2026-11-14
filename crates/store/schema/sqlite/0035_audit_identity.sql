-- Who an identity tied by a record of a move is: the same as
-- schema/postgres/0035_audit_identity.sql, which says what it is.
ALTER TABLE audit_log ADD COLUMN identity TEXT;
