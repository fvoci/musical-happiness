/**
 * What an open board or wiki of somebody else's comes to for who works in the
 * workspace (`state/join-on-write.ts`, `state/order.ts`): it waits apart, at
 * the sidebar's foot, until the reader's first write joins it and files it
 * among their own; and one made after the sidebar was read is listed there
 * once opened.
 */
import { describe, expect, it } from 'vitest';

import type { Container } from '$lib/logic/wire.ts';
import type { Category } from '$lib/state/category-types.ts';
import { openToJoinOf, WikiStore } from '$lib/state/index.ts';
import { sidebarOf } from '$lib/state/order.ts';

import { setup } from './support.ts';

const category = (id: string, containerIds: string[], hiddenContainerIds: string[] = []) =>
  ({
    id,
    name: id,
    userId: 'me',
    workspaceId: 't',
    type: 'system',
    collapsed: false,
    sortOrder: 0,
    containerIds,
    hiddenContainerIds,
    createdAt: 0,
    updatedAt: 0,
  }) as Category;

const container = (id: string, title: string, mine: Partial<Container> = {}) =>
  ({
    id,
    title,
    createdAt: 0,
    createdBy: 'them',
    visibility: 'open',
    isTemplate: false,
    ...mine,
  }) as Container;

describe('the open ones the reader may join', () => {
  it('are the open ones of others that no category files, by title, kept from the loose list', () => {
    const containers = [
      container('zeta', 'Zeta'),
      container('alpha', 'alpha'),
      container('filed', 'Filed'),
      container('hidden', 'Hidden'),
      container('mine', 'Mine', { createdBy: 'me' }),
      container('closed', 'Closed', { visibility: 'private' }),
      container('template', 'Template', { isTemplate: true }),
    ];
    const categories = [category('general', ['filed'], ['hidden'])];
    expect(openToJoinOf(categories, containers, 'en', 'me').map((one) => one.id)).toEqual([
      'alpha',
      'zeta',
    ]);
    const loose = sidebarOf(categories, containers, 'en', 'me').flatMap((entry) =>
      entry.containers.map((one) => one.id),
    );
    expect(loose).toEqual(['filed', 'closed', 'mine', 'template']);
  });

  it('includes a wiki made after the sidebar was read, once opened, and leaves once written in', async () => {
    const made = setup();
    const reader = made.workspace;
    await reader.load();
    const wiki = await made.other.wiki.createWiki(made.world.workspace.id, {
      title: 'Handbook',
      visibility: 'open',
    });
    const waiting = () =>
      openToJoinOf(reader.categories, reader.containers, reader.locale, reader.me?.id).map(
        (one) => one.title,
      );
    expect(waiting()).toEqual([]);

    const store = new WikiStore(reader, wiki.id);
    await store.load();
    expect(waiting()).toEqual(['Handbook']);

    await store.createPage({ title: 'Mine' });
    await expect.poll(waiting).toEqual([]);
    const filed = reader.sidebar.flatMap((entry) => entry.containers.map((one) => one.id));
    expect(filed).toContain(wiki.id);
    store.close();
  });
});
