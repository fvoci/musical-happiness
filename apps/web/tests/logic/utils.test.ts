/**
 * The cases of the original `utils.test.ts`, under the same names, against
 * the modules its functions moved to. The four `htmlFromMarkdown` cases are
 * not here: see the test mapping in PORTING.md.
 *
 * Where the original read `window` (the base URL, the origin, the reader's
 * settings), the functions here take the value as an argument.
 */
import { describe, expect, it, test } from 'vitest';

import { displayDate, displayDateTime, inputDate } from '$lib/logic/format.ts';
import { countCheckboxesInMarkdown, ensureProtocol } from '$lib/logic/text.ts';
import { boardPath, buildUrl } from '$lib/logic/urls.ts';
import {
  SHOW_FULL_NAME,
  SHOW_NICKNAME_FULL_NAME,
  SHOW_USERNAME,
  userDisplayName,
} from '$lib/logic/users.ts';
import { compareVersions } from '$lib/logic/versions.ts';
import { createId } from '$lib/logic/wire.ts';

describe('utils', () => {
  describe('assureProtocol', () => {
    test('should passthrough on valid short protocol', () => {
      expect(ensureProtocol('https://example.com')).toBe('https://example.com');
    });
    test('should passthrough on valid long protocol', () => {
      expect(ensureProtocol('somecustomprotocol://example.com')).toBe(
        'somecustomprotocol://example.com',
      );
    });

    test('should passthrough on valid short protocol', () => {
      expect(ensureProtocol('x://example.com')).toBe('x://example.com');
    });

    test('should add a https for empty protocol', () => {
      expect(ensureProtocol('example.com')).toBe('https://example.com');
    });
  });

  describe('createGuid', () => {
    test('should create 27 char random id for workspace', () => {
      expect(createId('workspace')).toMatch(/^w[ybndrfg8ejkmcpqxot1uwisza345h769]{26}$/);
    });
    test('should create 27 char random id for board', () => {
      expect(createId('board')).toMatch(/^b[ybndrfg8ejkmcpqxot1uwisza345h769]{26}$/);
    });
    test('should create 27 char random id for card', () => {
      expect(createId('card')).toMatch(/^c[ybndrfg8ejkmcpqxot1uwisza345h769]{26}$/);
    });
    test('should create 27 char random id', () => {
      expect(createId('none')).toMatch(/^7[ybndrfg8ejkmcpqxot1uwisza345h769]{26}$/);
    });
  });

  describe('countCheckboxesInMarkdown', () => {
    test('should count checkboxes', () => {
      const text = `
                ## Header
                - [x] one
                - [ ] two
                - [x] three
            `.replace(/\n\s+/gm, '\n');
      const checkboxes = countCheckboxesInMarkdown(text);
      expect(checkboxes.total).toBe(3);
      expect(checkboxes.checked).toBe(2);
    });
  });

  describe('test - buildURL', () => {
    const origin = 'http://localhost';

    test('buildURL, no base', () => {
      expect(buildUrl('test', undefined, origin)).toBe('http://localhost/test');
      expect(buildUrl('/test', undefined, origin)).toBe('http://localhost/test');

      expect(buildUrl('test', undefined)).toBe('/test');
      expect(buildUrl('/test', undefined)).toBe('/test');
    });

    // Changed from the original (Q8): the address starts at the root of the site.
    test('buildURL, base no slash', () => {
      expect(buildUrl('test', 'base', origin)).toBe('http://localhost/base/test');
      expect(buildUrl('/test', 'base', origin)).toBe('http://localhost/base/test');

      expect(buildUrl('test', 'base')).toBe('/base/test');
      expect(buildUrl('/test', 'base')).toBe('/base/test');
    });

    // Changed from the original (Q8), as above.
    test('buildUrl, base with slash', () => {
      expect(buildUrl('test', '/base/', origin)).toBe('http://localhost/base/test');
      expect(buildUrl('/test', '/base/', origin)).toBe('http://localhost/base/test');

      expect(buildUrl('test', '/base/')).toBe('/base/test');
      expect(buildUrl('/test', '/base/')).toBe('/base/test');
    });
  });

  describe('display date', () => {
    const locale = 'en-us';

    it('should show month and day for current year', () => {
      const currentYear = new Date().getFullYear();
      const date = new Date(currentYear, 6, 9);
      expect(displayDate(date, locale)).toBe('July 09');
    });

    it('should show month, day and year for previous year', () => {
      const currentYear = new Date().getFullYear();
      const previousYear = currentYear - 1;
      const date = new Date(previousYear, 6, 9);
      expect(displayDate(date, locale)).toBe(`July 09, ${previousYear}`);
    });
  });

  describe('input date', () => {
    const currentYear = new Date().getFullYear();
    const date = new Date(currentYear, 6, 9);

    it('should show mm/dd/yyyy for current year', () => {
      expect(inputDate(date, 'en-us')).toBe(`07/09/${currentYear}`);
    });

    it('should show dd/mm/yyyy for current year, es local', () => {
      expect(inputDate(date, 'es-es')).toBe(`09/07/${currentYear}`);
    });
  });

  describe('display date and time', () => {
    const locale = 'en-us';

    it('should show month, day and time for current year', () => {
      const currentYear = new Date().getFullYear();
      const date = new Date(currentYear, 6, 9, 15, 20);
      expect(displayDateTime(date, locale)).toBe('July 09 at 3:20 PM');
    });

    it('should show month, day, year and time for previous year', () => {
      const currentYear = new Date().getFullYear();
      const previousYear = currentYear - 1;
      const date = new Date(previousYear, 6, 9, 5, 35);
      expect(displayDateTime(date, locale)).toBe(`July 09, ${previousYear} at 5:35 AM`);
    });
  });

  describe('compare versions', () => {
    it('should return one if b > a', () => {
      expect(compareVersions('0.9.4', '0.10.0')).toBe(1);
    });

    it('should return zero if a = b', () => {
      expect(compareVersions('1.2.3', '1.2.3')).toBe(0);
    });

    it('should return minus one if b < a', () => {
      expect(compareVersions('10.9.4', '10.9.2')).toBe(-1);
    });
  });

  describe('showBoard test', () => {
    it('should switch boards', () => {
      const params = {
        boardId: 'board_id_1',
        viewId: 'view_id_1',
        cardId: 'card_id_1',
        teamId: 'team_id_1',
      };
      const pattern = '/team/:teamId/:boardId?/:viewId?/:cardId?';

      expect(boardPath(pattern, params, 'board_id_2')).toBe('/team/team_id_1/board_id_2');
    });
  });

  describe('getUserDisplayName test', () => {
    const user = {
      id: 'user-id-1',
      username: 'username_1',
      email: 'test@email.com',
      nickname: 'nickname',
      firstname: 'firstname',
      lastname: 'lastname',
    };

    it('should display username, by default', () => {
      const displayName = userDisplayName(user, '');
      expect(displayName).toEqual('username_1');
    });
    it('should display nickname', () => {
      const displayName = userDisplayName(user, SHOW_NICKNAME_FULL_NAME);
      expect(displayName).toEqual('nickname');
    });
    it('should display fullname', () => {
      const displayName = userDisplayName(user, SHOW_FULL_NAME);
      expect(displayName).toEqual('firstname lastname');
    });
    it('should display username', () => {
      const displayName = userDisplayName(user, SHOW_USERNAME);
      expect(displayName).toEqual('username_1');
    });
    it('should display full name, no nickname', () => {
      user.nickname = '';
      const displayName = userDisplayName(user, SHOW_NICKNAME_FULL_NAME);
      expect(displayName).toEqual('firstname lastname');
    });
    it('should display username, no nickname, no full name', () => {
      user.nickname = '';
      user.firstname = '';
      user.lastname = '';
      const displayName = userDisplayName(user, SHOW_NICKNAME_FULL_NAME);
      expect(displayName).toEqual('username_1');
    });
  });
});
