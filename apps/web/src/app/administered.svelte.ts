/**
 * The organization the signed-in reader administers, which the sidebar leads
 * to: asked of the server once, when the reader is first signed in, and the
 * one they start from the switcher when none was known. Made while the app's
 * component is: the effect is that component's.
 */
import type { Workspace } from '$lib/state/index.ts';
import type { Organizations } from '$lib/state/organization.ts';

export class Administered {
  /** The organization, once known; none where the reader administers none. */
  orgId = $state<string | undefined>(undefined);
  #asked = false;

  constructor(shown: () => Workspace | undefined, api: () => Organizations | undefined) {
    $effect(() => {
      const organizations = api();
      if (shown()?.status !== 'ready' || !organizations || this.#asked) return;
      this.#asked = true;
      organizations.administered().then(
        (orgId) => {
          this.orgId = orgId;
        },
        () => {
          // Not told: the way to it is not shown.
        },
      );
    });
  }

  /** The reader started an organization: they administer it, unless another is known already. */
  started(orgId: string | undefined): void {
    this.orgId ??= orgId;
  }
}
