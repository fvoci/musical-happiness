/** The share dialog's roles and public link. */
import { describe, expect, it } from 'vitest';

import { publicLinkOf, roleOfMember } from '$lib/features/shell/share-logic.ts';

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

  it('puts the read token on the board’s address', () => {
    expect(publicLinkOf('https://boards.example/app/board/b1', 'a+b')).toBe(
      'https://boards.example/app/board/b1?readToken=a%2Bb',
    );
  });
});
