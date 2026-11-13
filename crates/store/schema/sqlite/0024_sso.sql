-- An organization that takes no password, and an account that links a provider to
-- itself: the same columns as schema/postgres/0024_sso.sql, which says what they are.

ALTER TABLE oidc_providers ADD COLUMN sso_required BOOLEAN NOT NULL DEFAULT false;

ALTER TABLE oidc_login_flows ADD COLUMN link_session_id TEXT;
