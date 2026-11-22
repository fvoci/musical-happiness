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
  /** How many times the server was asked: the answer to the last asking alone counts. */
  #asking = 0;
  readonly #api: () => Organizations | undefined;

  constructor(shown: () => Workspace | undefined, api: () => Organizations | undefined) {
    this.#api = api;
    $effect(() => {
      const organizations = api();
      if (shown()?.status !== 'ready' || !organizations || this.#asked) return;
      this.#asked = true;
      this.#ask(organizations, (orgId) => {
        this.orgId = orgId;
      });
    });
  }

  /** The reader started an organization: they administer it, unless another is known already. */
  started(orgId: string | undefined): void {
    this.orgId ??= orgId;
  }

  /**
   * The reader left an organization: the sidebar does not lead to it any
   * more, and the server is asked again whether they administer another. An
   * answer asked for before, which may name the one left, does not count.
   */
  left(orgId: string): void {
    if (this.orgId !== undefined && this.orgId !== orgId) return;
    this.orgId = undefined;
    const organizations = this.#api();
    if (!organizations) return;
    this.#ask(organizations, (next) => {
      this.orgId ??= next;
    });
  }

  /** Asks the server which organization the reader administers; `take` has the answer if it is the last asked for. */
  #ask(organizations: Organizations, take: (orgId: string | undefined) => void): void {
    this.#asking += 1;
    const asking = this.#asking;
    organizations.administered().then(
      (orgId) => {
        if (asking === this.#asking) take(orgId);
      },
      () => {
        // Not told: the way to it is not shown.
      },
    );
  }
}
