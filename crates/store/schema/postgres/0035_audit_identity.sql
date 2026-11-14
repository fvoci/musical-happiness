-- Who an identity tied by a record of a move is: `identity` of a record of
-- `oidc_identity_bound` or `oidc_identity_carried_over` is the email address the new
-- provider gave for the identity, or its subject where it gave none, as it was when
-- the identity was tied; NULL for the others. The records of the organization itself
-- are read by its administrators alone. The log is never changed, so an address kept
-- here outlives the account and the identity: a way to erase a person must reach it.
ALTER TABLE audit_log ADD COLUMN identity TEXT;
