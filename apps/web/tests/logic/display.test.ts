/** The display settings: read leniently, settled per device. */
import { describe, expect, it } from 'vitest';

import {
  DEFAULT_DISPLAY_SETTINGS,
  readDisplaySettings,
  resolveDisplay,
} from '../../src/lib/logic/display.ts';

describe('reading the display settings', () => {
  it('gives the defaults for nothing, and for what is not an object', () => {
    for (const stored of [undefined, null, 'dark', 7, []]) {
      expect(readDisplaySettings(stored)).toEqual(DEFAULT_DISPLAY_SETTINGS);
    }
    expect(DEFAULT_DISPLAY_SETTINGS).toEqual({
      brightness: 'auto',
      cardOpen: 'center',
      density: 'auto',
      vibration: true,
    });
  });

  it('keeps the values it knows and replaces the ones it does not', () => {
    expect(
      readDisplaySettings({
        brightness: 'dark',
        cardOpen: 'side',
        density: 'enormous',
        vibration: 'yes',
        somethingNew: 1,
      }),
    ).toEqual({ brightness: 'dark', cardOpen: 'side', density: 'auto', vibration: true });
    expect(readDisplaySettings({ vibration: false }).vibration).toBe(false);
  });
});

describe('settling the display settings for a device', () => {
  const light = { prefersDark: false, coarsePointer: false };
  const darkTouch = { prefersDark: true, coarsePointer: true };

  it('follows the device where the setting is auto', () => {
    expect(resolveDisplay(DEFAULT_DISPLAY_SETTINGS, light)).toEqual({
      brightness: 'light',
      cardOpen: 'center',
      density: 'compact',
      vibration: true,
    });
    expect(resolveDisplay(DEFAULT_DISPLAY_SETTINGS, darkTouch)).toMatchObject({
      brightness: 'dark',
      density: 'comfortable',
    });
  });

  it('lets a fixed choice win over the device', () => {
    const fixed = { ...DEFAULT_DISPLAY_SETTINGS, brightness: 'light', density: 'compact' } as const;
    expect(resolveDisplay(fixed, darkTouch)).toMatchObject({
      brightness: 'light',
      density: 'compact',
    });
  });

  it('opens a card in the middle until the other ways are built', () => {
    for (const cardOpen of ['center', 'side', 'full'] as const) {
      expect(resolveDisplay({ ...DEFAULT_DISPLAY_SETTINGS, cardOpen }, light).cardOpen).toBe(
        'center',
      );
    }
  });
});
