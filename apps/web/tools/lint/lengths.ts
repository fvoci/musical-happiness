/**
 * How long a file and a function of `src` may be.
 *
 * The numbers are the constants below. What is over them today is listed in
 * `tools/lint/length-exceptions.txt` with its length as a ceiling: it may not
 * grow, and when it shrinks the check fails until the ceiling is lowered, or
 * the line removed once the length is within the limit. `check-lengths.ts
 * --measure` writes the list anew from the code as it is, and is the one way
 * a ceiling is to go up.
 *
 * The check holds the code to the list; it cannot tell a number that was
 * raised by hand from one that was measured. By hand a line is lowered,
 * removed, or renamed with its code. A raised or added line shows in the
 * diff of the list, and whoever merges looks for it there.
 *
 * A function is measured by ESLint's parsers, from its first line to its
 * last, comments and empty lines included: declarations, function
 * expressions and arrow functions, in `.ts` files and in the script and the
 * markup of `.svelte` files. A function inside another is measured on its
 * own as well.
 */
import { readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';

import { ESLint, type Linter, type Rule } from 'eslint';
import svelte from 'eslint-plugin-svelte';
import ts from 'typescript-eslint';

import svelteConfig from '../../svelte.config.js';

/** The most lines a file of `src` may have. */
export const MAX_FILE_LINES = 500;
/** The most lines a function may have. */
export const MAX_FUNCTION_LINES = 100;

/** The folder that is measured, from the web app's root. */
export const MEASURED = 'src';
/**
 * The files that are measured: `.ts` covers `.svelte.ts`. There is no `.js`
 * in `src` today; it is measured so that a file does not escape by its name.
 */
const EXTENSIONS = ['.ts', '.svelte', '.js'];
/**
 * What a tool writes and nobody edits: the messages compiled by paraglide
 * and the types made from the server's contract. Not measured.
 */
export const GENERATED = ['src/lib/i18n/paraglide/', 'src/lib/api/contract.ts'];
/** The list of what is over a limit, from the web app's root. */
export const EXCEPTIONS = 'tools/lint/length-exceptions.txt';

/** A file, or a function of a file. The file is a path from the web app's root, with `/`. */
export interface Subject {
  kind: 'file' | 'function';
  file: string;
  /**
   * For a function: its own name, the name of the variable or the member it
   * is the value of, or `<anonymous>`. Two functions of a file may have one
   * name; they are told apart by their lengths alone.
   */
  name?: string;
}

/** A length that was measured. */
export interface Length extends Subject {
  lines: number;
  /** The line the subject starts on, from 1. */
  at: number;
}

/** The lengths over their limit, longest first, by the line a subject has in the list. */
export type Over = Map<string, number[]>;

function limitOf(subject: Subject): number {
  return subject.kind === 'file' ? MAX_FILE_LINES : MAX_FUNCTION_LINES;
}

/** A subject as the list writes it, without its number: `function src/a.ts name`. */
function keyOf(subject: Subject): string {
  return subject.kind === 'file'
    ? `file ${subject.file}`
    : `function ${subject.file} ${subject.name ?? ''}`;
}

/** A subject as a message names it. */
function shown(subject: Subject): string {
  return subject.kind === 'file' ? subject.file : `${subject.file}: function ${subject.name ?? ''}`;
}

function longestFirst(a: number, b: number): number {
  return b - a;
}

/** Reads the list. Empty lines and lines that start with `#` are skipped. */
export function parseExceptions(text: string): { over: Over; subjects: Map<string, Subject> } {
  const over: Over = new Map();
  const subjects = new Map<string, Subject>();
  for (const [index, raw] of text.split('\n').entries()) {
    const line = raw.trim();
    if (line === '' || line.startsWith('#')) continue;
    const [kind, count, file, name, ...rest] = line.split(/\s+/);
    const wellFormed =
      (kind === 'file' && file !== undefined && name === undefined) ||
      (kind === 'function' && name !== undefined && rest.length === 0);
    if (!wellFormed || file === undefined || !/^\d+$/.test(count ?? '')) {
      throw new Error(
        `${EXCEPTIONS}:${index + 1}: expected \`file lines path\` or \`function lines path name\`, found \`${line}\``,
      );
    }
    const subject: Subject = kind === 'file' ? { kind, file } : { kind: 'function', file, name };
    const lines = Number(count);
    if (lines <= limitOf(subject)) {
      throw new Error(
        `${EXCEPTIONS}:${index + 1}: ${lines} lines are within the limit of ${limitOf(subject)}; the line is not needed`,
      );
    }
    const key = keyOf(subject);
    subjects.set(key, subject);
    over.set(key, [...(over.get(key) ?? []), lines].sort(longestFirst));
  }
  return { over, subjects };
}

const EXCEPTIONS_HEAD = `# Files and functions of src that are over the limit of their length
# (tools/lint/lengths.ts), each with the number of lines it has: its ceiling.
#
# Written by \`node tools/lint/check-lengths.ts --measure\`. \`check\` fails when the code
# is longer than its ceiling, when something not listed is over the limit, and when the
# code got shorter, until the number here is lowered (or the line removed, once the
# code is within the limit).
#
# By hand a line is lowered, removed, or renamed with its code; never raised or added.
# The check cannot tell a raised number from a measured one: the diff of this file
# shows it. \`--measure\` is run once after work that was measured apart is merged, not
# to make a check pass.
#
# file <lines> <path>
# function <lines> <path> <name>
`;

/**
 * The list as `--measure` writes it: what of `measured` is over its limit.
 * A path with a space in it is an error: the list could not be read back.
 */
export function renderExceptions(measured: readonly Length[]): string {
  const over = measured.filter((length) => length.lines > limitOf(length));
  const spaced = over.find((length) => /\s/.test(length.file));
  if (spaced !== undefined) {
    throw new Error(`${spaced.file} has a space in its path, which the list cannot hold`);
  }
  const lines = over
    .map((length) => ({ key: keyOf(length), lines: length.lines }))
    .sort((a, b) => (a.key < b.key ? -1 : a.key > b.key ? 1 : b.lines - a.lines))
    .map(({ key, lines }) => {
      const [kind = '', ...rest] = key.split(' ');
      return `${kind} ${lines} ${rest.join(' ')}`;
    });
  return `${EXCEPTIONS_HEAD}\n${lines.map((line) => `${line}\n`).join('')}`;
}

/**
 * Compares what was measured with the list. Each line of the answer is one
 * difference: something over the limit that is not listed, something longer
 * than its ceiling, or a ceiling that is higher than the code needs.
 */
export function judge(measured: readonly Length[], listedText: string): string[] {
  const { over: listed, subjects } = parseExceptions(listedText);
  const found = new Map<string, Length[]>();
  for (const length of measured) {
    const key = keyOf(length);
    if (length.lines > limitOf(length) || listed.has(key)) {
      found.set(key, [...(found.get(key) ?? []), length]);
    }
  }
  const problems: string[] = [];
  for (const [key, lengths] of [...found].sort(([a], [b]) => (a < b ? -1 : 1))) {
    const ceilings = listed.get(key) ?? [];
    // The longest is held to the highest ceiling, and so on down; what is
    // left over on either side is new, or gone.
    for (const [index, length] of lengths.sort((a, b) => b.lines - a.lines).entries()) {
      const limit = limitOf(length);
      const ceiling = ceilings[index];
      const place = `${length.file}:${length.at}`;
      const { lines } = length;
      if (ceiling === undefined) {
        if (lines > limit) {
          problems.push(`${place}: ${shown(length)} has ${lines} lines, the limit is ${limit}`);
        }
      } else if (lines > ceiling) {
        problems.push(`${place}: ${shown(length)} has ${lines} lines, its ceiling is ${ceiling}`);
      } else if (lines < ceiling) {
        const fix =
          lines > limit
            ? `lower its ceiling of ${ceiling}`
            : `within the limit of ${limit}: remove its line (${ceiling})`;
        problems.push(`${EXCEPTIONS}: ${shown(length)} has ${lines} lines now: ${fix}`);
      }
    }
  }
  for (const [key, ceilings] of listed) {
    const subject = subjects.get(key);
    if (subject === undefined) continue;
    for (const ceiling of ceilings.slice(found.get(key)?.length ?? 0)) {
      problems.push(
        `${EXCEPTIONS}: ${shown(subject)} is not in the code: remove its line (${ceiling})`,
      );
    }
  }
  return problems;
}

/** The number of lines of a text, as `wc -l` counts a file that ends with a line break. */
export function countLines(text: string): number {
  if (text === '') return 0;
  return text.split('\n').length - (text.endsWith('\n') ? 1 : 0);
}

/**
 * The measured files under `root`: paths from `root`, with `/`, sorted. A
 * symbolic link among them is an error.
 */
export function measuredFiles(root: string): string[] {
  const files: string[] = [];
  const walk = (relative: string): void => {
    const entries = readdirSync(path.join(root, relative), { withFileTypes: true });
    for (const entry of entries.sort((a, b) => (a.name < b.name ? -1 : 1))) {
      const child = `${relative}/${entry.name}`;
      if (GENERATED.some((made) => child === made || `${child}/` === made)) continue;
      // The files are read where they are; what a link leads to would be
      // measured twice, or never.
      if (entry.isSymbolicLink()) throw new Error(`${child} is a symbolic link`);
      if (entry.isDirectory()) walk(child);
      else if (EXTENSIONS.some((extension) => entry.name.endsWith(extension))) files.push(child);
    }
  };
  walk(MEASURED);
  return files;
}

/** As much of a node of the syntax tree as naming a function takes. */
interface Named {
  type: string;
  id?: { type: string; name?: string } | null;
  key?: { type: string; name?: string };
  parent?: Named;
}

/** The name a function goes by. */
function nameOf(node: Rule.Node): string {
  const { id, parent } = node as Named;
  if (id?.name !== undefined && node.type !== 'ArrowFunctionExpression') return id.name;
  if (parent?.type === 'VariableDeclarator' && parent.id?.type === 'Identifier') {
    return parent.id.name ?? '<anonymous>';
  }
  const member = ['Property', 'MethodDefinition', 'PropertyDefinition'];
  if (parent !== undefined && member.includes(parent.type) && parent.key?.type === 'Identifier') {
    return parent.key.name ?? '<anonymous>';
  }
  return '<anonymous>';
}

/**
 * Reports every function with its length, as `lines name`: the measuring
 * instrument, not a rule to turn on. `measureFunctions` reads its reports.
 */
const everyFunction: Rule.RuleModule = {
  meta: {
    type: 'suggestion',
    docs: { description: 'reports the length of every function' },
    schema: [],
    messages: { length: '{{lines}} {{name}}' },
  },
  create(context) {
    const report = (node: Rule.Node): void => {
      if (node.loc === null || node.loc === undefined) return;
      const lines = node.loc.end.line - node.loc.start.line + 1;
      context.report({
        node,
        messageId: 'length',
        data: { lines: String(lines), name: nameOf(node) },
      });
    };
    return {
      FunctionDeclaration: report,
      FunctionExpression: report,
      ArrowFunctionExpression: report,
    };
  },
};

const RULE = 'fvoci-lengths/every-function';

/**
 * The ESLint configuration that measures and does nothing else: the parsers
 * of the project's own configuration, and the one rule above. No comment in
 * a source turns it off.
 */
export const measuringConfig: Linter.Config[] = [
  ...svelte.configs.base,
  { files: ['**/*.ts'], languageOptions: { parser: ts.parser } },
  {
    files: ['**/*.svelte', '**/*.svelte.ts', '**/*.svelte.js'],
    languageOptions: { parserOptions: { parser: ts.parser, svelteConfig } },
  },
  {
    linterOptions: { noInlineConfig: true, reportUnusedDisableDirectives: 'off' },
    plugins: { 'fvoci-lengths': { rules: { 'every-function': everyFunction } } },
    rules: { [RULE]: 'error' },
  },
];

/**
 * The functions among the messages of one file. A file that does not parse
 * is an error. What else ESLint says is passed over: that a comment in the
 * source tried to turn a rule off and was not obeyed.
 */
export function functionsOf(file: string, messages: readonly Linter.LintMessage[]): Length[] {
  const functions: Length[] = [];
  for (const message of messages) {
    if (message.fatal === true) throw new Error(`${file}:${message.line}: ${message.message}`);
    if (message.ruleId !== RULE) continue;
    const [lines, name = ''] = message.message.split(' ');
    functions.push({ kind: 'function', file, name, lines: Number(lines), at: message.line });
  }
  return functions;
}

/** The length of every measured file under `root` and of every function in them. */
export async function measure(root: string): Promise<Length[]> {
  const files = measuredFiles(root);
  const measured: Length[] = files.map((file) => ({
    kind: 'file',
    file,
    lines: countLines(readFileSync(path.join(root, file), 'utf8')),
    at: 1,
  }));
  const eslint = new ESLint({
    cwd: root,
    overrideConfigFile: true,
    overrideConfig: measuringConfig,
    // The files are named one by one: nothing ESLint would leave out is left out.
    ignore: false,
  });
  const results = await eslint.lintFiles(files);
  for (const result of results) {
    const file = path.relative(root, result.filePath).split(path.sep).join('/');
    measured.push(...functionsOf(file, result.messages));
  }
  return measured;
}
