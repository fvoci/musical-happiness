/** The raw-colour gate (`tools/lint/colors.ts`): what it refuses, and where colours live. */
import path from 'node:path';

import { describe, expect, it } from 'vitest';

import { COLOR_HOMES, checkColors, findRawColors } from '../../tools/lint/colors.ts';

const ROOT = path.resolve(import.meta.dirname, '../..');

describe('raw colours', () => {
  it('are refused as values, functions and palette classes', () => {
    const source = [
      "const shade = '#1a2b3c';",
      '<path fill="#fff" />',
      '.x { color: #abcdef80; background: oklch(0.5 0.1 200); }',
      '<div class="bg-red-500 text-white bg-[#000]"></div>',
      "const veil = 'rgba(0, 0, 0, 0.4)';",
    ].join('\n');
    expect(findRawColors('src/a.svelte', source)).toHaveLength(8);
  });

  it('are not private fields, card numbers or tokens', () => {
    const source = [
      'readonly #dead: () => void;',
      'this.#dead();',
      '// `#12` finds the card numbered 12',
      '<div class="bg-primary text-muted-foreground border-destructive/40"></div>',
      '{#each list as item (item.id)}',
    ].join('\n');
    expect(findRawColors('src/a.svelte', source)).toEqual([]);
  });

  it('live only in the homes the gate names, each with its reason', () => {
    for (const home of COLOR_HOMES) expect(home.reason).not.toBe('');
    expect(findRawColors('src/styles/app.css', ':root { --x: oklch(1 0 0); }')).toEqual([]);
    expect(checkColors(ROOT).problems).toEqual([]);
  });
});
