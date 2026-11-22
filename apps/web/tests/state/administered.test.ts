/**
 * The organization the reader administers, which the sidebar leads to: once
 * they leave it, it is not led to, and the server is asked whether they
 * administer another.
 */
import { flushSync } from 'svelte';
import { describe, expect, it, vi } from 'vitest';

import { administeredBy } from './administered-host.svelte.ts';

describe('the organization the reader administers', () => {
  it('is asked again once the reader leaves it, and another is led to', async () => {
    const asked = vi
      .fn<() => Promise<string | undefined>>()
      .mockResolvedValueOnce('a')
      .mockResolvedValueOnce('b');
    const { administered, destroy } = administeredBy(asked);
    flushSync();
    await vi.waitFor(() => {
      expect(administered.orgId).toBe('a');
    });
    administered.left('a');
    expect(administered.orgId).toBeUndefined();
    await vi.waitFor(() => {
      expect(administered.orgId).toBe('b');
    });
    expect(asked).toHaveBeenCalledTimes(2);
    destroy();
  });

  it('stays as it is when the reader leaves another', async () => {
    const asked = vi.fn<() => Promise<string | undefined>>().mockResolvedValue('a');
    const { administered, destroy } = administeredBy(asked);
    flushSync();
    await vi.waitFor(() => {
      expect(administered.orgId).toBe('a');
    });
    administered.left('c');
    expect(administered.orgId).toBe('a');
    expect(asked).toHaveBeenCalledOnce();
    destroy();
  });
});
