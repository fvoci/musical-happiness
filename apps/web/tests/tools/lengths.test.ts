/**
 * The limits on the length of files and functions (tools/lint/lengths.ts):
 * what is measured, and how the measure is held against the list of
 * exceptions, in both directions.
 */
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { Linter } from 'eslint';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import {
  EXCEPTIONS,
  MAX_FILE_LINES,
  MAX_FUNCTION_LINES,
  check,
  countLines,
  functionsOf,
  judge,
  measure,
  measuredFiles,
  measuringConfig,
  parseExceptions,
  raised,
  renderExceptions,
  rewrite,
  type Length,
} from '../../tools/lint/lengths.ts';

const ROOT = path.resolve(import.meta.dirname, '../..');
/**
 * Time for a test that starts ESLint or Node several times: far more than it
 * takes, so that a busy machine does not fail it.
 */
const SLOW = 120_000;

function file(name: string, lines: number): Length {
  return { kind: 'file', file: name, lines, at: 1 };
}

function fn(name: string, lines: number, at = 1, inFile = 'src/a.ts'): Length {
  return { kind: 'function', file: inFile, name, lines, at };
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
    expect(judge(measured, '')).toEqual([]);
  });

  it('refuses a long file and a long function that are not listed', () => {
    const measured = [file('src/a.ts', MAX_FILE_LINES + 1), fn('grown', 101, 7)];
    expect(judge(measured, '# nothing\n')).toEqual([
      'src/a.ts:1: src/a.ts has 501 lines, the limit is 500',
      'src/a.ts:7: src/a.ts: function grown has 101 lines, the limit is 100',
    ]);
  });

  it('holds what is listed to its ceiling, up and down', () => {
    const list = 'file 600 src/a.ts\nfunction 150 src/a.ts draw\n';
    expect(judge([file('src/a.ts', 600), fn('draw', 150)], list)).toEqual([]);
    expect(judge([file('src/a.ts', 601), fn('draw', 151, 9)], list)).toEqual([
      'src/a.ts:1: src/a.ts has 601 lines, its ceiling is 600',
      'src/a.ts:9: src/a.ts: function draw has 151 lines, its ceiling is 150',
    ]);
    expect(judge([file('src/a.ts', 599), fn('draw', 149, 9)], list)).toEqual([
      `${EXCEPTIONS}: src/a.ts has 599 lines now: lower its ceiling of 600`,
      `${EXCEPTIONS}: src/a.ts: function draw (line 9) has 149 lines now: lower its ceiling of 150`,
    ]);
    expect(judge([file('src/a.ts', 500), fn('draw', 100, 9)], list)).toEqual([
      `${EXCEPTIONS}: src/a.ts has 500 lines now, within the limit of 500: remove its line (600)`,
      `${EXCEPTIONS}: src/a.ts: function draw (line 9) has 100 lines now, within the limit of 100: remove its line (150)`,
    ]);
  });

  it('refuses a line whose file or function is gone', () => {
    const list = 'file 600 src/gone.ts\nfunction 150 src/a.ts renamed\n';
    expect(judge([file('src/a.ts', 10), fn('other', 10)], list)).toEqual([
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
    expect(judge(two(120, 150), list)).toEqual([]);
    expect(judge(two(121, 150), list)).toEqual([
      'src/a.ts:1: src/a.ts: function <anonymous> has 121 lines, its ceiling is 120',
    ]);
    expect(judge(two(150, 150), list)).toEqual([
      'src/a.ts:200: src/a.ts: function <anonymous> has 150 lines, its ceiling is 120',
    ]);
    expect(judge([...two(120, 150), fn('<anonymous>', 101, 600)], list)).toEqual([
      'src/a.ts:600: src/a.ts: function <anonymous> has 101 lines, the limit is 100',
    ]);
  });

  it('is written from what was measured, and what it writes passes', () => {
    const measured = [
      file('src/b.ts', 700),
      file('src/a.ts', 20),
      fn('seed', 223, 1, 'src/b.ts'),
      fn('seed/forEach()', 139, 30, 'src/b.ts'),
      fn('seed/forEach()', 180, 300, 'src/b.ts'),
      fn('small', 5, 2, 'src/b.ts'),
    ];
    const list = renderExceptions(measured);
    expect(list.split('\n').filter((line) => line !== '' && !line.startsWith('#'))).toEqual([
      'file 700 src/b.ts',
      'function 223 src/b.ts seed',
      'function 180 src/b.ts seed/forEach()',
      'function 139 src/b.ts seed/forEach()',
    ]);
    expect(judge(measured, list)).toEqual([]);
    expect(() => renderExceptions([file('src/my file.ts', 501)])).toThrow(
      'has a space in its path',
    );
  });

  it('says what a rewriting raised', () => {
    const before = 'file 600 src/a.ts\nfunction 150 src/a.ts draw\n';
    const after = 'file 700 src/a.ts\nfunction 140 src/a.ts draw\nfunction 101 src/a.ts add\n';
    expect(raised(before, after)).toEqual([
      'raised: src/a.ts: 600 -> 700',
      'added: src/a.ts: function add: 101',
    ]);
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
  it('counts the lines of a text by its line breaks, of any kind', () => {
    expect(countLines('')).toBe(0);
    expect(countLines('a\n')).toBe(1);
    expect(countLines('a\nb')).toBe(2);
    expect(countLines('a\n\nb\n')).toBe(3);
    expect(countLines('a\r\nb\r\n')).toBe(2);
    expect(countLines('a\rb\rc\r')).toBe(3);
  });

  it('finds a planted long function, also behind a comment that turns rules off', () => {
    const planted = long('planted', 120);
    for (const comment of [
      '/* eslint-disable */',
      '/* eslint fvoci-lengths/every-function: off */',
      '// eslint-disable-next-line fvoci-lengths/every-function',
    ]) {
      const code = `${comment}\n${planted}\n`;
      expect(functions('src/lib/logic/planted.ts', code), comment).toEqual(['planted 120']);
    }
    const linter = new Linter({ cwd: ROOT });
    const found = linter.verify(`/* eslint-disable */\n${planted}\n`, measuringConfig, {
      filename: path.join(ROOT, 'src/a.ts'),
    });
    expect(judge(functionsOf('src/a.ts', found), '')).toEqual([
      'src/a.ts:2: src/a.ts: function planted has 120 lines, the limit is 100',
    ]);
  });

  it('finds a long handler in the markup behind a comment of the markup', () => {
    const handler = ['onclick={() => {', ...Array<string>(118).fill('  // …'), '}}'].join('\n');
    for (const comment of [
      '<!-- eslint-disable -->',
      '<!-- eslint-disable fvoci-lengths/every-function -->',
      '<!-- eslint-disable-next-line fvoci-lengths/every-function -->',
    ]) {
      const code = `<p>text</p>\n${comment}\n<button\n${handler}>go</button\n>\n`;
      expect(functions('src/lib/components/Planted.svelte', code), comment).toEqual([
        'onclick 120',
      ]);
    }
  });

  it('names a function by itself, its variable, its member or what it is handed to', () => {
    const code = [
      'export function declared() {}',
      'const arrow = () => {',
      '  return [1].map((value) => value);',
      '};',
      'const named = function inner() {};',
      'class Box {',
      '  static {',
      '    setup(() => 1);',
      '  }',
      '  method() {}',
      '  field = () => {};',
      '  #hidden() {}',
      '}',
      'const table = { entry() {}, other: () => 1, [computed]: () => 2 };',
      'export default async function () {',
      '  await new Promise((resolve) => resolve(1));',
      '}',
    ].join('\n');
    expect(functions('src/lib/logic/names.ts', code)).toEqual([
      'declared 1',
      'arrow 3',
      'arrow/map() 1',
      'inner 1',
      'Box/static{} 3',
      'Box/static{}/setup() 1',
      'Box/method 1',
      'Box/field 1',
      'Box/#hidden 1',
      'entry 1',
      'other 1',
      'computed 1',
      '<anonymous> 3',
      '<anonymous>/Promise() 1',
    ]);
  });

  it('measures the script, the markup and the snippets of a component', () => {
    const code = [
      '<script lang="ts">',
      '  let count = $state(0);',
      '  function add(): void {',
      '    count += 1;',
      '  }',
      '  $effect(() => {',
      '    add();',
      '  });',
      '</script>',
      '',
      '{#snippet row(label: string)}',
      '  <li>{label}</li>',
      '{/snippet}',
      '<button',
      '  onclick={() => {',
      '    add();',
      '  }}>{count}</button',
      '>',
    ].join('\n');
    expect(functions('src/lib/components/Planted.svelte', code)).toEqual([
      'add 3',
      '$effect() 3',
      'row 3',
      'onclick 3',
    ]);
  });

  it('refuses a file that ESLint does not read to its end', () => {
    expect(() => functions('src/lib/logic/broken.ts', 'function (')).toThrow(
      'src/lib/logic/broken.ts:1',
    );
    const ignored = { ruleId: null, severity: 1 as const, line: 0, column: 0 };
    expect(() =>
      functionsOf('src/node_modules/a.ts', [
        { ...ignored, message: 'File ignored by default because it is located under …' },
      ]),
    ).toThrow('src/node_modules/a.ts:0: not measured: File ignored');
    expect(
      functionsOf('src/a.ts', [
        {
          ...ignored,
          message: "'/* eslint-disable */' has no effect because you have 'noInlineConfig' setting",
        },
      ]),
    ).toEqual([]);
  });
});

describe('the files that are measured', () => {
  let root = '';
  beforeEach(() => {
    root = mkdtempSync(path.join(tmpdir(), 'fvoci-lengths-'));
  });
  afterEach(() => {
    rmSync(root, { recursive: true, force: true });
  });
  const write = (name: string, text: string): void => {
    mkdirSync(path.dirname(path.join(root, name)), { recursive: true });
    writeFileSync(path.join(root, name), text);
  };
  const tooLong = 'export {};\n'.repeat(MAX_FILE_LINES + 1);

  it(
    'are the sources under src, without what a tool writes',
    async () => {
      write('src/main.ts', 'export const a = 1;\n');
      write('src/lib/state/board.svelte.ts', `${long('planted', 101)}\n`);
      write('src/lib/features/Card.svelte', '<p>card</p>\n');
      write('src/styles/app.css', 'p {}\n');
      write('src/lib/plain.js', `${long('plain', 101)}\n`);
      write('src/lib/long.ts', tooLong);
      write('src/lib/api/contract.ts', tooLong);
      write('src/lib/i18n/paraglide/messages.ts', tooLong);
      write('tools/outside.ts', tooLong);

      expect(measuredFiles(root)).toEqual([
        'src/lib/features/Card.svelte',
        'src/lib/long.ts',
        'src/lib/plain.js',
        'src/lib/state/board.svelte.ts',
        'src/main.ts',
      ]);
      const planted = [
        'src/lib/long.ts:1: src/lib/long.ts has 501 lines, the limit is 500',
        'src/lib/plain.js:1: src/lib/plain.js: function plain has 101 lines, the limit is 100',
        'src/lib/state/board.svelte.ts:1: src/lib/state/board.svelte.ts: function planted has 101 lines, the limit is 100',
      ];
      expect(judge(await measure(root), '')).toEqual(planted);

      // The check and the rewriting, against a list on disk.
      write(EXCEPTIONS, '');
      expect((await check(root)).problems).toEqual(planted);
      const { list, raised: wentUp } = await rewrite(root);
      expect(readFileSync(path.join(root, EXCEPTIONS), 'utf8')).toBe(list);
      expect(wentUp).toEqual([
        'added: src/lib/long.ts: 501',
        'added: src/lib/plain.js: function plain: 101',
        'added: src/lib/state/board.svelte.ts: function planted: 101',
      ]);
      expect(list.split('\n').filter((line) => /^(file|function) /.test(line))).toEqual([
        'file 501 src/lib/long.ts',
        'function 101 src/lib/plain.js plain',
        'function 101 src/lib/state/board.svelte.ts planted',
      ]);
      expect((await check(root)).problems).toEqual([]);
      write('src/lib/plain.js', `${long('plain', 102)}\n`);
      expect((await check(root)).problems).toEqual([
        'src/lib/plain.js:1: src/lib/plain.js: function plain has 102 lines, its ceiling is 101',
      ]);
    },
    SLOW,
  );

  it('do not include a symbolic link, or code in a form that is not measured', () => {
    write('src/main.ts', 'export const a = 1;\n');
    symlinkSync(path.join(root, 'src/main.ts'), path.join(root, 'src/linked.ts'));
    expect(() => measuredFiles(root)).toThrow('src/linked.ts is a symbolic link');
    rmSync(path.join(root, 'src/linked.ts'));
    for (const name of ['a.mts', 'a.cts', 'a.mjs', 'a.cjs', 'a.tsx', 'a.jsx']) {
      write(`src/lib/${name}`, 'export {};\n');
      expect(() => measuredFiles(root), name).toThrow(
        `src/lib/${name} is code in a form that is not measured`,
      );
      rmSync(path.join(root, `src/lib/${name}`));
    }
  });
});

describe('the check as a command', () => {
  it(
    'takes no argument, so that no argument of `check` reaches a mode that writes',
    () => {
      const before = readFileSync(path.join(ROOT, EXCEPTIONS), 'utf8');
      const result = spawnSync(process.execPath, ['tools/lint/check-lengths.ts', '--measure'], {
        cwd: ROOT,
        encoding: 'utf8',
      });
      expect(result.status).toBe(2);
      expect(result.stderr).toContain('no arguments');
      expect(readFileSync(path.join(ROOT, EXCEPTIONS), 'utf8')).toBe(before);
    },
    SLOW,
  );
});
