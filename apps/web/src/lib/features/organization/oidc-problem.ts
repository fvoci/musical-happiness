/** Human words for the provider settings' RFC 9457 refusals. */
import { m } from '$lib/i18n/paraglide/messages.js';
import { isBackendError } from '$lib/state/backend.ts';

export function oidcProblem(error: unknown): string {
  if (!isBackendError(error)) return m.org_sso_failed();
  switch (error.reason) {
    case 'oidc_issuer_unverified':
      return m.org_sso_issuer_unverified();
    case 'secret_key_required':
      return m.org_sso_key_required();
    case 'sso_unproven':
      return m.org_sso_unproven();
    case 'sso_enforced':
      return m.org_sso_enforced();
    case 'oidc_unavailable':
      return m.account_oidc_failed();
  }
  if (error.status === 400) return m.org_sso_invalid();
  if (error.status === 409) return m.org_sso_conflict();
  if (error.status === 404) return m.org_missing();
  return m.org_sso_failed();
}
