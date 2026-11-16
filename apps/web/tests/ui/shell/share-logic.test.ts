/** The share dialog's roles and public link. */
import { describe, expect, it } from 'vitest';

import { roleOf } from '$lib/state/board-parts.ts';

import { roleOfMember } from '$lib/features/shell/share-logic.ts';

import { routePath } from '../../../src/app/router.svelte.ts';

const member = (
  flags: Partial<
    Record<'schemeAdmin' | 'schemeEditor' | 'schemeCommenter' | 'schemeViewer', boolean>
  >,
) => ({
  containerId: 'b',
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

  it('keeps direct role controls separate from the highest inherited access', () => {
    const inherited = { ...member({ schemeViewer: true }), inheritedRole: 'editor' as const };
    expect(roleOfMember(inherited)).toBe('viewer');
    expect(roleOf(inherited)).toBe('editor');
    expect(roleOf({ ...inherited, schemeAdmin: true })).toBe('admin');
    expect(roleOf({ ...member({ schemeEditor: true }), inheritedRole: 'commenter' })).toBe(
      'editor',
    );
    expect(roleOf({ ...member({}), inheritedRole: 'viewer', synthetic: true })).toBe('viewer');
  });

  it('makes the public link the reader’s address of the board, the read token in it', () => {
    expect(routePath({ boardId: 'b1', readToken: 'a+b' }, 'app')).toBe('/app/shared/b1?r=a%2Bb');
  });
});
