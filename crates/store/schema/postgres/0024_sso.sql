-- An organization that takes no password, and an account that links a provider to
-- itself.
--
-- `sso_required`: what is the organization's opens only for a session made through
-- this provider (`sessions.login_provider_id` names it). The sessions stay: each
-- request is asked. While it is on, the provider is not switched off, removed, or
-- replaced by another issuer or client: the row that enforces is the row people
-- sign in through.
--
-- `link_session_id`: a sign-in under way that was started from a session, to tie the
-- identity the provider vouches for to that session's account in place of signing in.
-- NULL for a sign-in. No key, as `sessions.login_provider_id` has none: whether the
-- session is still there, and still good, is asked when the sign-in comes back.

ALTER TABLE oidc_providers ADD COLUMN sso_required BOOLEAN NOT NULL DEFAULT false;

ALTER TABLE oidc_login_flows ADD COLUMN link_session_id UUID;
