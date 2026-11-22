/**
 * The line under a card's body that says what came of the keys that keep a
 * version (CardBody.svelte): nothing is left running once the card is closed
 * while the version is being kept.
 */
import { render } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { KeepOutcome } from '$lib/features/card/body-versions.svelte.ts';
import CardBody from '$lib/features/card/CardBody.svelte';
import type { BoardStore, CardStore } from '$lib/state/index.ts';

import { doc, paragraph, text } from '../../support/body-docs.ts';

afterEach(() => {
  vi.useRealTimers();
  document.body.replaceChildren();
});

/** A card's body with no room (none is opened: the workspace is not known), kept by `keep`. */
function body(keep: () => Promise<KeepOutcome>): ReturnType<typeof render> {
  const container = {
    workspace: { info: undefined, userName: () => undefined, user: () => undefined },
    canEdit: true,
    orderedMembers: [],
  } as unknown as Pick<BoardStore, 'workspace' | 'canEdit' | 'orderedMembers'>;
  const card = {
    room: undefined,
    body: doc(paragraph(text('The body'))),
    lost: [],
    openBody: () => undefined,
    closeBody: () => undefined,
  } as unknown as CardStore;
  return render(CardBody, { props: { container, card, keep } });
}

const pressSave = (on: Element): void => {
  on.dispatchEvent(
    new KeyboardEvent('keydown', { key: 's', code: 'KeyS', ctrlKey: true, bubbles: true }),
  );
};

describe('what came of the keys', () => {
  it('leaves no timer behind when the card is closed while the version is kept', async () => {
    vi.useFakeTimers();
    let answer: (outcome: KeepOutcome) => void = () => undefined;
    const keep = vi.fn(
      () =>
        new Promise<KeepOutcome>((resolve) => {
          answer = resolve;
        }),
    );
    const view = body(keep);
    const paragraphOf = view.container.querySelector('p');
    if (!paragraphOf) throw new Error('no body drawn');
    pressSave(paragraphOf);
    expect(keep).toHaveBeenCalledTimes(1);
    view.unmount();
    answer('kept');
    await vi.advanceTimersByTimeAsync(0);
    expect(vi.getTimerCount()).toBe(0);
  });
});
