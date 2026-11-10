/**
 * The layering rule of src/lib, run through ESLint itself on small sources:
 * a feature importing another feature fails, and so does shared code
 * importing a feature.
 */
import path from 'node:path';

import { Linter } from 'eslint';
import { describe, expect, it } from 'vitest';

import layers from '../../tools/lint/layers.ts';

const ROOT = path.resolve(import.meta.dirname, '../..');

function messages(file: string, code: string): string[] {
  const linter = new Linter({ cwd: ROOT });
  return linter
    .verify(
      code,
      [
        {
          files: ['**/*.ts'],
          plugins: { fvoci: layers },
          rules: { 'fvoci/feature-boundaries': 'error' },
        },
      ],
      { filename: path.join(ROOT, file) },
    )
    .map((message) => message.message);
}

describe('feature boundaries', () => {
  const inBoard = 'src/lib/features/board/Board.ts';

  it('lets a feature import from itself and from the shared layers', () => {
    const code = [
      "import { a } from './parts/a.ts';",
      "import { b } from '$lib/features/board/b.ts';",
      "import { c } from '$lib/logic/filter.ts';",
      "import { d } from '../../ui/button/index.js';",
      "import { e } from 'svelte';",
    ].join('\n');
    expect(messages(inBoard, code)).toEqual([]);
  });

  it('refuses an import from another feature, by alias or by relative path', () => {
    expect(messages(inBoard, "import { x } from '$lib/features/card/Card.ts';")).toEqual([
      expect.stringContaining('The feature "board" imports from the feature "card"'),
    ]);
    expect(messages(inBoard, "import { x } from '../card/Card.ts';")).toHaveLength(1);
    expect(messages(inBoard, "export * from '../card/Card.ts';")).toHaveLength(1);
    expect(messages(inBoard, "const x = await import('$lib/features/card/Card.ts');")).toHaveLength(
      1,
    );
  });

  it('counts the first folder under features as the feature', () => {
    const inKanban = 'src/lib/features/views/kanban/Kanban.ts';
    expect(messages(inKanban, "import { x } from '../table/Table.ts';")).toEqual([]);
    expect(messages(inKanban, "import { x } from '../../board/Board.ts';")).toHaveLength(1);
  });

  it('refuses shared code that imports a feature', () => {
    expect(
      messages('src/lib/logic/sort.ts', "import { x } from '$lib/features/board/b.ts';"),
    ).toEqual([expect.stringContaining('lib/logic imports from the feature "board"')]);
    expect(
      messages('src/lib/state/boards.ts', "import { x } from '../features/board/b.ts';"),
    ).toHaveLength(1);
  });

  it('leaves the app entry free to import any feature', () => {
    expect(messages('src/app/App.ts', "import { x } from '$lib/features/board/b.ts';")).toEqual([]);
  });
});

function deviceTime(code: string): string[] {
  const linter = new Linter({ cwd: ROOT });
  return linter
    .verify(
      code,
      [
        {
          files: ['**/*.ts'],
          plugins: { fvoci: layers },
          rules: { 'fvoci/no-device-time': 'error' },
        },
      ],
      { filename: path.join(ROOT, 'src/lib/logic/example.ts') },
    )
    .map((message) => message.message);
}

describe('no device time in the logic', () => {
  it('refuses the methods of Date that answer in the time zone of the device', () => {
    for (const method of [
      'getHours',
      'getDate',
      'getDay',
      'getMonth',
      'getFullYear',
      'getTimezoneOffset',
    ]) {
      expect(deviceTime(`const x = date.${method}();`)).toEqual([
        expect.stringContaining(`\`${method}\` works in the time zone of the device`),
      ]);
    }
    expect(deviceTime('date.setHours(0, 0, 0, 0);')).toHaveLength(1);
    expect(deviceTime('date.setDate(date.getUTCDate() + 1);')).toHaveLength(1);
    expect(deviceTime("const text = date.toLocaleDateString('en');")).toHaveLength(1);
    expect(deviceTime("const text = date.toLocaleTimeString('en');")).toHaveLength(1);
  });

  it('refuses a Date built from parts or from text', () => {
    expect(deviceTime('const d = new Date(2026, 2, 5);')).toEqual([
      expect.stringContaining('reads its arguments as device time'),
    ]);
    expect(deviceTime('const d = new Date(2026, 2, 5, 12, 0);')).toHaveLength(1);
    expect(deviceTime("const d = new Date('December 7, 2023');")).toHaveLength(1);
    expect(deviceTime('const d = new Date(`${year}-03-05T00:00`);')).toHaveLength(1);
  });

  it('refuses formatting without a time zone, and asking the device for its zone', () => {
    expect(deviceTime("new Intl.DateTimeFormat('en').format(moment);")).toEqual([
      expect.stringContaining('formats in the time zone of the device'),
    ]);
    expect(
      deviceTime("new Intl.DateTimeFormat('en', { month: 'long' }).format(moment);"),
    ).toHaveLength(1);
    expect(deviceTime('Intl.DateTimeFormat().format(moment);')).toHaveLength(1);
    expect(deviceTime('const zone = Intl.DateTimeFormat().resolvedOptions().timeZone;')).toEqual(
      expect.arrayContaining([expect.stringContaining('read in lib/regional.ts only')]),
    );
  });

  it('lets through what does not depend on the device', () => {
    const code = [
      'const a = new Date(instant).getUTCHours();',
      'const b = new Date(instant).getTime();',
      'const c = Date.UTC(2026, 2, 5);',
      'const d = Date.now();',
      'const e = new Date();',
      "const f = new Intl.DateTimeFormat('en', { timeZone, month: 'long' }).format(instant);",
      "const g = new Intl.DateTimeFormat('en', { ...options, timeZone: 'UTC' }).format(instant);",
      "const h = new Intl.DateTimeFormat('en', { hour: 'numeric', timeZone: 'UTC' }).resolvedOptions().hourCycle;",
      'const i = new Date(instant).toISOString();',
    ].join('\n');
    expect(deviceTime(code)).toEqual([]);
  });
});

describe('the time zone of the device', () => {
  it('is read in src/lib/regional.ts and nowhere else in the app', async () => {
    const { readdirSync, readFileSync } = await import('node:fs');
    const asksTheDevice = /resolvedOptions\(\)\s*\.timeZone|getLocalTimeZone|getTimezoneOffset/;
    const found: string[] = [];
    const walk = (folder: string): void => {
      for (const entry of readdirSync(path.join(ROOT, folder), { withFileTypes: true })) {
        const file = `${folder}/${entry.name}`;
        if (entry.isDirectory()) {
          if (file !== 'src/lib/i18n/paraglide') walk(file);
        } else if (/\.(ts|svelte)$/.test(entry.name)) {
          if (asksTheDevice.test(readFileSync(path.join(ROOT, file), 'utf8'))) found.push(file);
        }
      }
    };
    walk('src');
    expect(found).toEqual(['src/lib/regional.ts']);
  });
});
