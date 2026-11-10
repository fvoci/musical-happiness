/**
 * The read-only view of a body must stay light and must never build markup
 * from text. Both are properties of its source, so the source is read here:
 * everything the view imports, followed to the end, has to be free of the
 * editor and of the Markdown reader, and no file on the way may use an API
 * that turns a string into elements.
 */
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';

import { describe, expect, it } from 'vitest';

const ROOT = path.resolve(import.meta.dirname, '../..');
const LIB = path.join(ROOT, 'src/lib');

const ENTRIES = ['src/lib/editor/render.ts', 'src/lib/editor/BodyView.svelte'];

/** Packages the view must not pull in, however indirectly. */
const HEAVY = [/^@tiptap\//, /^prosemirror-/, /^marked$/, /^marked\//];

/** Ways of making elements out of a string. The view has none of them. */
const MARKUP_FROM_TEXT = [
  'innerHTML',
  'outerHTML',
  'insertAdjacentHTML',
  '{@html',
  'document.write',
  'DOMParser',
  'parseFromString',
  'createContextualFragment',
  'setHTMLUnsafe',
  'setHTML(',
  'srcdoc',
  'new Function',
  'eval(',
];

/** Every module specifier a source file imports from, statically or not. */
function specifiersOf(source: string): string[] {
  const found = new Set<string>();
  const patterns = [
    /\bimport\s+(?:type\s+)?[^'";]*?\sfrom\s*['"]([^'"]+)['"]/g,
    /\bexport\s+(?:type\s+)?[^'";]*?\sfrom\s*['"]([^'"]+)['"]/g,
    /\bimport\s*['"]([^'"]+)['"]/g,
    /\bimport\s*\(\s*['"]([^'"]+)['"]\s*\)/g,
  ];
  for (const pattern of patterns) {
    for (const match of source.matchAll(pattern)) if (match[1]) found.add(match[1]);
  }
  return [...found];
}

function resolve(from: string, specifier: string): string | undefined {
  if (specifier.startsWith('$lib/')) return path.join(LIB, specifier.slice('$lib/'.length));
  if (specifier.startsWith('.')) return path.resolve(path.dirname(from), specifier);
  return undefined;
}

/** The files reachable from an entry, and the packages they import. */
function graphOf(entry: string): { files: string[]; packages: string[] } {
  const files = new Set<string>();
  const packages = new Set<string>();
  const queue = [path.join(ROOT, entry)];
  for (let file = queue.pop(); file !== undefined; file = queue.pop()) {
    if (files.has(file)) continue;
    expect([file, existsSync(file)]).toEqual([file, true]);
    files.add(file);
    if (!/\.(ts|js|svelte)$/.test(file)) continue;
    for (const specifier of specifiersOf(readFileSync(file, 'utf8'))) {
      const target = resolve(file, specifier);
      if (target) queue.push(target);
      else packages.add(specifier);
    }
  }
  return {
    files: [...files].map((file) => path.relative(ROOT, file)).sort(),
    packages: [...packages].sort(),
  };
}

describe('what the read-only view is made of', () => {
  it('finds imports however they are written', () => {
    expect(
      specifiersOf(
        [
          "import a from 'one';",
          "import { b, type C } from '$lib/two.ts';",
          "import type { D } from './three.ts';",
          "import './four.css';",
          "export { e } from '../five.ts';",
          "export * from 'six';",
          "const f = await import('seven');",
          'import {\n  g,\n  h,\n} from "eight";',
        ].join('\n'),
      ).sort(),
    ).toEqual(
      [
        '$lib/two.ts',
        '../five.ts',
        './four.css',
        './three.ts',
        'eight',
        'one',
        'seven',
        'six',
      ].sort(),
    );
  });

  it('draws with nothing but the list, the validator and the address of files', () => {
    expect(graphOf('src/lib/editor/render.ts')).toEqual({
      files: [
        'src/lib/editor/drawing.ts',
        'src/lib/editor/files.ts',
        'src/lib/editor/render.ts',
        'src/lib/editor/schema.ts',
      ],
      packages: [],
    });
  });

  it('adds only the framework and the style sheet for the component', () => {
    expect(graphOf('src/lib/editor/BodyView.svelte')).toEqual({
      files: [
        'src/lib/editor/BodyView.svelte',
        'src/lib/editor/drawing.ts',
        'src/lib/editor/editor.css',
        'src/lib/editor/files.ts',
        'src/lib/editor/render.ts',
        'src/lib/editor/schema.ts',
      ],
      packages: ['svelte', 'svelte/elements'],
    });
  });

  it.each(ENTRIES)('%s reaches neither the editor nor the Markdown reader', (entry) => {
    const { packages } = graphOf(entry);
    for (const name of packages) {
      expect([name, HEAVY.some((pattern) => pattern.test(name))]).toEqual([name, false]);
    }
  });

  it('would notice the editor if it were imported', () => {
    // The same walk from the editor's own module does find what is forbidden.
    const { packages } = graphOf('src/lib/editor/extensions.ts');
    expect(packages.some((name) => HEAVY.some((pattern) => pattern.test(name)))).toBe(true);
    const markdown = graphOf('src/lib/editor/markdown.ts');
    expect(markdown.packages).toContain('marked');
  });

  it.each(ENTRIES)('%s and what it imports never make elements out of a string', (entry) => {
    for (const file of graphOf(entry).files) {
      const source = readFileSync(path.join(ROOT, file), 'utf8');
      for (const api of MARKUP_FROM_TEXT) {
        expect([file, api, source.includes(api)]).toEqual([file, api, false]);
      }
    }
  });
});
