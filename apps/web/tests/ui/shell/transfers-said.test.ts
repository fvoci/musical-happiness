/**
 * What the stack of transfers (Transfers.svelte) tells a screen reader: a
 * line of its log for each start, end and failure, so that two at once are
 * both read and the same words again are read again; what was said before
 * the workspace shown was is not said again when it comes back.
 */
import { cleanup, render, screen } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, describe, expect, it } from 'vitest';

import Transfers from '$lib/features/shell/Transfers.svelte';
import type { Workspace } from '$lib/state/index.ts';
import { Transfers as TransferList } from '$lib/state/transfers.svelte.ts';

afterEach(() => {
  cleanup();
});

const never = (): Promise<string> => new Promise(() => undefined);
const file = (name: string): File => new File(['x'], name);

/** One stack seen from two workspaces, as the page shows one and then the other. */
function twoWorkspaces() {
  const shell = {
    report: () => undefined,
    ends: () => false,
    info: { id: 'one' } as { id: string } | undefined,
  };
  const transfers = new TransferList(shell);
  const shown = (id: string) => ({ transfers, info: { id } }) as unknown as Workspace;
  return { shell, transfers, one: shown('one'), two: shown('two') };
}

const lines = (): string[] =>
  [...screen.getByTestId('transfers-said').querySelectorAll('p')].map((p) => p.textContent);

describe('what the stack says', () => {
  it('reads each of two started at once, and the same words twice', async () => {
    const { transfers, one } = twoWorkspaces();
    render(Transfers, { workspace: one });
    expect(screen.getByTestId('transfers-said').getAttribute('role')).toBe('log');
    transfers.upload({ file: file('same.txt'), send: never });
    transfers.upload({ file: file('same.txt'), send: never });
    await tick();
    expect(lines()).toEqual(['Uploading same.txt', 'Uploading same.txt']);
  });

  it('says nothing again of what was said before the workspace shown came back', async () => {
    const { shell, transfers, one, two } = twoWorkspaces();
    const view = render(Transfers, { workspace: one });
    transfers.upload({ file: file('here.txt'), send: never });
    await tick();
    expect(lines()).toEqual(['Uploading here.txt']);
    await view.rerender({ workspace: two });
    expect(lines()).toEqual([]);
    shell.info = { id: 'two' };
    transfers.upload({ file: file('there.txt'), send: never });
    await tick();
    expect(lines()).toEqual(['Uploading there.txt']);
    await view.rerender({ workspace: one });
    expect(lines()).toEqual([]);
    shell.info = { id: 'one' };
    transfers.upload({ file: file('again.txt'), send: never });
    await tick();
    expect(lines()).toEqual(['Uploading again.txt']);
  });

  it('names the key that reaches it once', async () => {
    const { transfers, one } = twoWorkspaces();
    render(Transfers, { workspace: one });
    transfers.upload({ file: file('a.txt'), send: never });
    await tick();
    const stack = screen.getByRole('region', { name: 'Transfers (F8)' });
    expect(stack.hasAttribute('aria-keyshortcuts')).toBe(false);
  });
});
