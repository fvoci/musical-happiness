/**
 * The address and the workspace shown, kept together: the root address goes
 * to the workspace the reader starts in (`Workspace.firstChoice`), another
 * workspace in the address (a link, back or forward) is entered, and the one
 * entered is the one to come back to on this browser. Called while the app's
 * component is made: the effects are that component's.
 */
import type { Workspace } from '$lib/state/index.ts';

import { rememberWorkspace } from './last-workspace.ts';
import type { Router } from './router.svelte.ts';

export function followAddress(routerOf: () => Router, shown: () => Workspace | undefined): void {
  // The root address: to the workspace the reader starts in, in its place.
  $effect(() => {
    const [router, workspace] = [routerOf(), shown()];
    if (workspace?.status !== 'ready' || router.route.unknown || router.route.workspaceId) return;
    if (router.route.invitation !== undefined || router.route.orgId) return;
    if (router.route.meTime) {
      if (!workspace.asked) {
        const chosen = workspace.firstChoice();
        if (chosen) void workspace.enter(chosen);
      }
      return;
    }
    const chosen = workspace.firstChoice();
    if (chosen) router.go({ workspaceId: chosen }, { replace: true });
  });

  // Another workspace in the address: it is entered.
  $effect(() => {
    const workspace = shown();
    const asked = routerOf().route.workspaceId;
    if (workspace?.status === 'ready' && asked && asked !== workspace.asked) {
      void workspace.enter(asked);
    }
  });

  // The workspace entered is the one to come back to on this browser.
  $effect(() => {
    const workspace = shown();
    if (workspace?.entered === 'ready' && workspace.me && workspace.info) {
      rememberWorkspace(workspace.me.id, workspace.info.id);
    }
  });
}
