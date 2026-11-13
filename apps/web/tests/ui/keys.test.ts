/** Native modifiers, visible hints and editor closing share one platform choice. */
import { afterEach, describe, expect, it, vi } from 'vitest';

import { closesPlace } from '$lib/editor/place-state.ts';
import { isApplePlatform, primaryModifier, shortcutKeys, shortcutLabel } from '$lib/keys.ts';

afterEach(() => vi.restoreAllMocks());

for (const [platform, apple] of [
  ['MacIntel', true],
  ['iPhone', true],
  ['iPad', true],
  ['iPod', true],
  ['Win32', false],
  ['Linux x86_64', false],
  ['Linux armv8l', false],
  ['', false],
] as const) {
  describe(`keys on ${platform || 'an unknown platform'}`, () => {
    it('accepts exactly the native modifier and labels the same keys', () => {
      vi.spyOn(navigator, 'platform', 'get').mockReturnValue(platform);
      expect(isApplePlatform()).toBe(apple);
      expect(primaryModifier({ ctrlKey: !apple, metaKey: apple })).toBe(true);
      expect(primaryModifier({ ctrlKey: apple, metaKey: !apple })).toBe(false);
      expect(primaryModifier({ ctrlKey: true, metaKey: true })).toBe(false);
      expect(primaryModifier({ ctrlKey: false, metaKey: false })).toBe(false);
      expect(shortcutLabel('Z')).toBe(apple ? '⌘Z' : 'Ctrl+Z');
      expect(shortcutLabel('Z', { shift: true })).toBe(apple ? '⇧⌘Z' : 'Ctrl+Shift+Z');
      expect(shortcutLabel('Enter', { alt: true })).toBe(apple ? '⌥⌘Enter' : 'Ctrl+Alt+Enter');
      expect(shortcutKeys('Z', { shift: true })).toBe(apple ? 'Meta+Shift+Z' : 'Control+Shift+Z');
    });

    it('closes an editor place with its native modifier, never a composing Enter or Escape', () => {
      vi.spyOn(navigator, 'platform', 'get').mockReturnValue(platform);
      const modifier = { ctrlKey: !apple, metaKey: apple };
      expect(closesPlace(new KeyboardEvent('keydown', { key: 'Enter', ...modifier }))).toBe(true);
      expect(
        closesPlace(
          new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: apple, metaKey: !apple }),
        ),
      ).toBe(false);
      for (const key of ['Enter', 'Escape']) {
        expect(
          closesPlace(new KeyboardEvent('keydown', { key, ...modifier, isComposing: true })),
        ).toBe(false);
        expect(closesPlace(new KeyboardEvent('keydown', { key, ...modifier, keyCode: 229 }))).toBe(
          false,
        );
      }
      expect(closesPlace(new KeyboardEvent('keydown', { key: 'Escape' }))).toBe(true);
    });
  });
}
