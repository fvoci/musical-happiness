/** The share dialog's roles and public link. */
import { describe, expect, it } from 'vitest';

import { roleOfMember } from '$lib/features/shell/share-logic.ts';

import { routePath } from '../../../src/app/router.svelte.ts';

const member = (
  flags: Partial<
    Record<'schemeAdmin' | 'schemeEditor' | 'schemeCommenter' | 'schemeViewer', boolean>
  >,
) => ({
  boardId: 'b',
  userId: 'u',
  minimumRole: '' as const,
  synthetic: false,
  schemeAdmin: false,
  schemeEditor: false,
  schemeCommenter: false,
  schemeViewer: false,
  ...flags,
});

describe('the share dialog', () => {
  it('names the highest role of a membership', () => {
    expect(roleOfMember(member({ schemeAdmin: true, schemeViewer: true }))).toBe('admin');
    expect(roleOfMember(member({ schemeCommenter: true }))).toBe('commenter');
    expect(roleOfMember(member({ schemeViewer: true }))).toBe('viewer');
    expect(roleOfMember(member({}))).toBe('none');
  });

  it('makes the public link the reader’s address of the board, the read token in it', () => {
    expect(routePath({ boardId: 'b1', readToken: 'a+b' }, 'app')).toBe('/app/shared/b1?r=a%2Bb');
  });
});
