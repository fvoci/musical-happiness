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
