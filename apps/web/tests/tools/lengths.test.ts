/**
 * The limits on the length of files and functions (tools/lint/lengths.ts):
 * what is measured, and how the measure is held against the list of
 * exceptions, in both directions.
 */
import { mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { Linter } from 'eslint';
import { afterAll, describe, expect, it } from 'vitest';

import {
  EXCEPTIONS,
  MAX_FILE_LINES,
  MAX_FUNCTION_LINES,
  countLines,
  functionsOf,
  judge,
  measure,
  measuredFiles,
  measuringConfig,
  parseExceptions,
  renderExceptions,
  type Length,
} from '../../tools/lint/lengths.ts';

const ROOT = path.resolve(import.meta.dirname, '../..');

function file(name: string, lines: number): Length {
  return { kind: 'file', file: name, lines, at: 1 };
}

function fn(name: string, lines: number, at = 1, inFile = 'src/a.ts'): Length {
  return { kind: 'function', file: inFile, name, lines, at };
}

function messages(measured: Length[], list: string): string[] {
  return judge(measured, list);
}

/** The functions ESLint finds in `code`, as `name lines`. */
function functions(filename: string, code: string): string[] {
  const linter = new Linter({ cwd: ROOT });
  const found = linter.verify(code, measuringConfig, { filename: path.join(ROOT, filename) });
  return functionsOf(filename, found).map((length) => `${length.name ?? ''} ${length.lines}`);
}

/** A function declaration of `lines` lines. */
function long(name: string, lines: number): string {
  return [`function ${name}() {`, ...Array<string>(lines - 2).fill('  // …'), '}'].join('\n');
}

describe('the list of exceptions', () => {
  it('lets what is within its limit pass without a line', () => {
    const measured = [file('src/a.ts', MAX_FILE_LINES), fn('short', MAX_FUNCTION_LINES)];
    expect(messages(measured, '')).toEqual([]);
  });

  it('refuses a long file and a long function that are not listed', () => {
    const measured = [file('src/a.ts', MAX_FILE_LINES + 1), fn('grown', 101, 7)];
    expect(messages(measured, '# nothing\n')).toEqual([
      'src/a.ts:1: src/a.ts has 501 lines, the limit is 500',
      'src/a.ts:7: src/a.ts: function grown has 101 lines, the limit is 100',
    ]);
  });

  it('holds what is listed to its ceiling, up and down', () => {
    const list = 'file 600 src/a.ts\nfunction 150 src/a.ts draw\n';
    expect(messages([file('src/a.ts', 600), fn('draw', 150)], list)).toEqual([]);
    expect(messages([file('src/a.ts', 601), fn('draw', 151, 9)], list)).toEqual([
      'src/a.ts:1: src/a.ts has 601 lines, its ceiling is 600',
      'src/a.ts:9: src/a.ts: function draw has 151 lines, its ceiling is 150',
    ]);
    expect(messages([file('src/a.ts', 599), fn('draw', 149)], list)).toEqual([
      `${EXCEPTIONS}: src/a.ts has 599 lines now: lower its ceiling of 600`,
      `${EXCEPTIONS}: src/a.ts: function draw has 149 lines now: lower its ceiling of 150`,
    ]);
    expect(messages([file('src/a.ts', 500), fn('draw', 100)], list)).toEqual([
      `${EXCEPTIONS}: src/a.ts has 500 lines now: within the limit of 500: remove its line (600)`,
      `${EXCEPTIONS}: src/a.ts: function draw has 100 lines now: within the limit of 100: remove its line (150)`,
    ]);
  });

  it('refuses a line whose file or function is gone', () => {
    const list = 'file 600 src/gone.ts\nfunction 150 src/a.ts renamed\n';
    expect(messages([file('src/a.ts', 10), fn('other', 10)], list)).toEqual([
      `${EXCEPTIONS}: src/gone.ts is not in the code: remove its line (600)`,
      `${EXCEPTIONS}: src/a.ts: function renamed is not in the code: remove its line (150)`,
    ]);
  });

  it('tells two functions of one name apart by their lengths', () => {
    const list = 'function 150 src/a.ts <anonymous>\nfunction 120 src/a.ts <anonymous>\n';
    const two = (first: number, second: number): Length[] => [
      fn('<anonymous>', first, 1),
      fn('<anonymous>', second, 200),
      fn('<anonymous>', 3, 400),
    ];
    expect(messages(two(120, 150), list)).toEqual([]);
    expect(messages(two(121, 150), list)).toEqual([
      'src/a.ts:1: src/a.ts: function <anonymous> has 121 lines, its ceiling is 120',
    ]);
    expect(messages(two(150, 150), list)).toEqual([
      'src/a.ts:200: src/a.ts: function <anonymous> has 150 lines, its ceiling is 120',
    ]);
    expect(messages([...two(120, 150), fn('<anonymous>', 101, 600)], list)).toEqual([
      'src/a.ts:600: src/a.ts: function <anonymous> has 101 lines, the limit is 100',
    ]);
  });

  it('is written from what was measured, and what it writes passes', () => {
    const measured = [
      file('src/b.ts', 700),
      file('src/a.ts', 20),
      fn('seed', 223, 1, 'src/b.ts'),
      fn('<anonymous>', 139, 30, 'src/b.ts'),
      fn('<anonymous>', 180, 300, 'src/b.ts'),
      fn('small', 5, 2, 'src/b.ts'),
    ];
    const list = renderExceptions(measured);
    expect(list.split('\n').filter((line) => line !== '' && !line.startsWith('#'))).toEqual([
      'file 700 src/b.ts',
      'function 180 src/b.ts <anonymous>',
      'function 139 src/b.ts <anonymous>',
      'function 223 src/b.ts seed',
    ]);
    expect(messages(measured, list)).toEqual([]);
    expect(() => renderExceptions([file('src/my file.ts', 501)])).toThrow(
      'has a space in its path',
    );
  });

  it('refuses a line it cannot read, and one that is not needed', () => {
    for (const line of [
      'file src/a.ts',
      'file many src/a.ts',
      'file 600 src/a.ts extra',
      'function 150 src/a.ts',
      'function 150 src/a.ts a b',
      'module 600 src/a.ts',
    ]) {
      expect(() => parseExceptions(line), line).toThrow('expected `file lines path`');
    }
    expect(() => parseExceptions('file 500 src/a.ts')).toThrow('within the limit of 500');
    expect(() => parseExceptions('function 100 src/a.ts f')).toThrow('within the limit of 100');
  });
});

describe('what is measured', () => {
  it('counts the lines of a text as its line breaks do', () => {
    expect(countLines('')).toBe(0);
    expect(countLines('a\n')).toBe(1);
    expect(countLines('a\nb')).toBe(2);
    expect(countLines('a\n\nb\n')).toBe(3);
  });

  it('finds a planted long function, also behind a comment that turns rules off', () => {
    const code = `/* eslint-disable */\n${long('planted', 120)}\n`;
    expect(functions('src/lib/logic/planted.ts', code)).toEqual(['planted 120']);
    expect(
      messages(
        functionsOf('src/a.ts', [
          ...new Linter({ cwd: ROOT }).verify(code, measuringConfig, {
            filename: path.join(ROOT, 'src/a.ts'),
          }),
        ]),
        '',
      ),
    ).toEqual(['src/a.ts:2: src/a.ts: function planted has 120 lines, the limit is 100']);
  });

  it('names a function by itself, its variable or its member', () => {
    const code = [
      'export function declared() {}',
      'const arrow = () => {',
      '  return [1].map((value) => value);',
      '};',
      'const named = function inner() {};',
      'class Box {',
      '  method() {}',
      '  field = () => {};',
      '}',
      'const table = { entry() {}, other: () => 1 };',
      'export default async function () {',
      '  await Promise.resolve();',
      '}',
    ].join('\n');
    expect(functions('src/lib/logic/names.ts', code)).toEqual([
      'declared 1',
      'arrow 3',
      '<anonymous> 1',
      'inner 1',
      'method 1',
      'field 1',
      'entry 1',
      'other 1',
      '<anonymous> 3',
    ]);
  });

  it('measures the script and the markup of a component', () => {
    const code = [
      '<script lang="ts">',
      '  let count = $state(0);',
      '  function add(): void {',
      '    count += 1;',
      '  }',
      '</script>',
      '',
      '<button',
      '  onclick={() => {',
      '    add();',
      '  }}>{count}</button',
      '>',
    ].join('\n');
    expect(functions('src/lib/components/Planted.svelte', code)).toEqual([
      'add 3',
      '<anonymous> 3',
    ]);
  });

  it('refuses a file that does not parse', () => {
    expect(() => functions('src/lib/logic/broken.ts', 'function (')).toThrow(
      'src/lib/logic/broken.ts:1',
    );
  });
});

describe('the files that are measured', () => {
  const root = mkdtempSync(path.join(tmpdir(), 'fvoci-lengths-'));
  afterAll(() => {
    rmSync(root, { recursive: true, force: true });
  });
  const write = (name: string, text: string): void => {
    mkdirSync(path.dirname(path.join(root, name)), { recursive: true });
    writeFileSync(path.join(root, name), text);
  };

  it('are the sources under src, without what a tool writes', async () => {
    write('src/main.ts', 'export const a = 1;\n');
    write('src/lib/state/board.svelte.ts', `${long('planted', 101)}\n`);
    write('src/lib/features/Card.svelte', '<p>card</p>\n');
    write('src/styles/app.css', 'p {}\n');
    write('src/lib/plain.js', `${long('plain', 101)}\n`);
    write('src/lib/long.ts', 'export {};\n'.repeat(MAX_FILE_LINES + 1));
    write('src/lib/api/contract.ts', 'export {};\n'.repeat(MAX_FILE_LINES + 1));
    write('src/lib/i18n/paraglide/messages.ts', 'export {};\n'.repeat(MAX_FILE_LINES + 1));
    write('tools/outside.ts', 'export {};\n'.repeat(MAX_FILE_LINES + 1));

    expect(measuredFiles(root)).toEqual([
      'src/lib/features/Card.svelte',
      'src/lib/long.ts',
      'src/lib/plain.js',
      'src/lib/state/board.svelte.ts',
      'src/main.ts',
    ]);
    expect(messages(await measure(root), '')).toEqual([
      'src/lib/long.ts:1: src/lib/long.ts has 501 lines, the limit is 500',
      'src/lib/plain.js:1: src/lib/plain.js: function plain has 101 lines, the limit is 100',
      'src/lib/state/board.svelte.ts:1: src/lib/state/board.svelte.ts: function planted has 101 lines, the limit is 100',
    ]);
  });

  it('do not include a symbolic link', () => {
    symlinkSync(path.join(root, 'src/main.ts'), path.join(root, 'src/linked.ts'));
    expect(() => measuredFiles(root)).toThrow('src/linked.ts is a symbolic link');
    rmSync(path.join(root, 'src/linked.ts'));
  });
});
