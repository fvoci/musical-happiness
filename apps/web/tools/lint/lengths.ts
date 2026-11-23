/**
 * How long a file and a function of `src` may be.
 *
 * The numbers are the constants below. What is over them today is listed in
 * `tools/lint/length-exceptions.txt` with a ceiling (`<=`): the code may not
 * exceed its ceiling, while shrinking below it (even back within the limit)
 * still passes so parallel work does not collide on the list. A listed file
 * or function that is missing from the code, or a listed ceiling at or below
 * the limit, still fails. Ceilings are lowered and unneeded lines removed
 * when tightening the list with `pnpm run lengths:measure`, or by hand; a
 * line moved with its code to the file or name the code went to is not an
 * added one and needs no evidence.
 *
 * Code that would go over is split first. A ceiling goes up, or a line is
 * added, only where a split would make the code worse, and only with the
 * evidence for that in the body of the commit that brings the code: what the
 * added code is, the seam a split would cut along (`file:line`), and what the
 * split would cost there (one rule in two places, a `switch` over every case
 * cut in two, a type exported for the split alone, a file with no job of its
 * own). That a split is more work is not evidence. The number goes in by hand
 * on that one line, in the same commit, and is the length the check printed,
 * not one line more: do not pad a ceiling above the code.
 *
 * The check cannot tell a raised number from a measured one. A raised or
 * added line shows in the diff of the list, and whoever merges reads it
 * against the evidence and sends back one whose evidence is missing or does
 * not hold. `measure-lengths.ts` rewrites the whole list from the code — to
 * tighten ceilings and drop unneeded lines during cleanup, or once after work
 * that was measured apart is merged; what it raises or adds there (it prints
 * each) is held to the same rule: the body of the merge commit gives the
 * evidence, or the code is split first.
 *
 * A function is measured by ESLint's parsers, from its first line to its
 * last, comments and empty lines included: declarations, function
 * expressions and arrow functions, in `.ts` files and in the script and the
 * markup of `.svelte` files, and with them the snippets of a component and
 * the static blocks of a class. A function inside another is measured on
 * its own as well. The script of a component outside any function is
 * bounded by the limit of the file alone.
 *
 * Lengths are counted in lines, and the lines are Prettier's (`check` runs
 * it before this). Where Prettier is told to keep its hands off
 * (`prettier-ignore`, the paths of `.prettierignore`) a line can hold more
 * than the count says; that is not checked here.
 */
import { readdirSync, readFileSync, writeFileSync } from 'node:fs';
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
/** Code in a form that is not measured, and so not allowed under `src`. */
const UNMEASURED = ['.mts', '.cts', '.mjs', '.cjs', '.tsx', '.jsx'];
/**
 * What a tool writes and nobody edits: the messages compiled by paraglide
 * and the types made from the server's contract. Not measured. The
 * components that the shadcn-svelte CLI wrote (`src/lib/ui`) are measured:
 * they are edited by hand after that, and none is near a limit.
 */
export const GENERATED = ['src/lib/i18n/paraglide/', 'src/lib/api/contract.ts'];
/** The list of what is over a limit, from the web app's root. */
export const EXCEPTIONS = 'tools/lint/length-exceptions.txt';

/** A file, or a function of a file. The file is a path from the web app's root, with `/`. */
export interface Subject {
  kind: 'file' | 'function';
  file: string;
  /**
   * For a function: the name `nameOf` gives it, which is its own after the
   * names of what it is written in (`seedWorld/forEach()`, `Box/method`,
   * `onclick`). Two functions of a file may still have one name; they are
   * told apart by their lengths alone.
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
# (tools/lint/lengths.ts), each with an upper-bound line count (\`<=\`): its ceiling.
#
# Written by \`pnpm run lengths:measure\` (tools/lint/measure-lengths.ts). \`check\`
# enforces each ceiling as an upper bound (\`<=\`): it fails when the code is longer
# than its ceiling, when something not listed is over the limit, when a listed file
# or function is missing from the code, or when a listed ceiling is at or below the
# limit. Code that shrinks below its ceiling still passes; run \`pnpm run lengths:measure\`
# when tightening ceilings and dropping unneeded lines.
#
# By hand a line is lowered, removed, or moved with its code to the file or name the
# code went to; a moved line is not an added one.
#
# Code that would go over is split first. A line is raised or added by hand only in the
# commit that brings the code which needs it, to the length the check printed, and with
# the evidence in that commit's body that a split would make the code worse: what the
# added code is, the seam, and its cost (tools/lint/lengths.ts). Whoever merges reads
# every raised or added line here against that evidence. \`lengths:measure\` is for
# tightening the list or once after work that was measured apart is merged, never to
# make a check pass; what it raises or adds there is held to the same rule, in the body
# of the merge commit.
#
# file <lines> <path>
# function <lines> <path> <name>
`;

/**
 * The list as `rewrite` writes it: what of `measured` is over its limit.
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
 * Compares what was measured with the list, enforcing each ceiling as an
 * upper bound (`<=`). Each line of the answer is one failure: something over
 * the limit that is not listed, something longer than its ceiling, or a
 * listed entry missing from the code. Duplicate-named functions in a file are
 * paired longest-first and checked by count against their listed ceilings.
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
      const { lines } = length;
      if (ceiling === undefined) {
        if (lines > limit) {
          problems.push(
            `${length.file}:${length.at}: ${shown(length)} has ${lines} lines, the limit is ${limit}`,
          );
        }
      } else if (lines > ceiling) {
        problems.push(
          `${length.file}:${length.at}: ${shown(length)} has ${lines} lines, its ceiling is ${ceiling}`,
        );
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

/**
 * The number of lines of a text, as `wc -l` counts a file that ends with a
 * line break. A carriage return alone ends a line too.
 */
export function countLines(text: string): number {
  if (text === '') return 0;
  const lines = text.split(/\r\n|\r|\n/);
  return lines.length - (lines.at(-1) === '' ? 1 : 0);
}

/**
 * The measured files under `root`: paths from `root`, with `/`, sorted. A
 * symbolic link among them is an error, and so is a source in a form that
 * is not measured.
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
      else if (UNMEASURED.some((extension) => entry.name.endsWith(extension))) {
        throw new Error(
          `${child} is code in a form that is not measured: write it as .ts, .svelte or .js`,
        );
      }
    }
  };
  walk(MEASURED);
  return files;
}

/** As much of a node of the syntax tree as naming a function takes. */
interface Named {
  type: string;
  id?: { type: string; name?: string } | null;
  key?: { type: string; name?: unknown };
  callee?: Named;
  property?: { type: string; name?: string };
  name?: unknown;
  parent?: Named | null;
}

const FUNCTIONS = ['FunctionDeclaration', 'FunctionExpression', 'ArrowFunctionExpression'];
/** What is measured like a function: a snippet of a component, a static block of a class. */
const BLOCKS = ['SvelteSnippetBlock', 'StaticBlock'];
const MEMBERS = ['Property', 'MethodDefinition', 'PropertyDefinition'];

function text(value: unknown): string | undefined {
  return typeof value === 'string' ? value : undefined;
}

/** The name a function has by itself, or from where it is the value. */
function ownName(node: Named): string {
  const { id, parent } = node;
  if (node.type === 'StaticBlock') return 'static{}';
  if (id?.name !== undefined && node.type !== 'ArrowFunctionExpression') return id.name;
  if (parent === undefined || parent === null) return '<anonymous>';
  if (parent.type === 'VariableDeclarator' && parent.id?.type === 'Identifier') {
    return parent.id.name ?? '<anonymous>';
  }
  if (MEMBERS.includes(parent.type) && parent.key !== undefined) {
    const name = text(parent.key.name);
    if (parent.key.type === 'Identifier' && name !== undefined) return name;
    if (parent.key.type === 'PrivateIdentifier' && name !== undefined) return `#${name}`;
  }
  // A callback goes by what it is handed to: `forEach()`, `$effect()`.
  if (
    (parent.type === 'CallExpression' || parent.type === 'NewExpression') &&
    parent.callee !== node
  ) {
    const callee = parent.callee;
    const called = callee?.type === 'MemberExpression' ? callee.property?.name : text(callee?.name);
    if (called !== undefined) return `${called}()`;
  }
  // A handler in the markup of a component goes by its attribute: `onclick`.
  if (parent.type === 'SvelteMustacheTag' && parent.parent?.key !== undefined) {
    const attribute = text(parent.parent.key.name);
    if (attribute !== undefined) return attribute;
  }
  return '<anonymous>';
}

/**
 * The name a function goes by in the list: its own, after the names of the
 * functions and the classes it is written in (`seedWorld/forEach()`,
 * `Box/method`). Functions that still share a name are told apart by their
 * lengths alone.
 */
function nameOf(node: Rule.Node): string {
  const self: Named = node;
  const names = [ownName(self)];
  for (let outer = self.parent; outer; outer = outer.parent) {
    if (FUNCTIONS.includes(outer.type) || BLOCKS.includes(outer.type))
      names.unshift(ownName(outer));
    else if (outer.type === 'ClassDeclaration' || outer.type === 'ClassExpression') {
      names.unshift(outer.id?.name ?? ownName(outer));
    }
  }
  return names.join('/');
}

/**
 * Reports every function with its length, as `lines name`: the measuring
 * instrument, not a rule to turn on. `functionsOf` reads its reports.
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
    return Object.fromEntries([...FUNCTIONS, ...BLOCKS].map((type) => [type, report]));
  },
};

const RULE = 'fvoci-lengths/every-function';

/**
 * The ESLint configuration that measures and does nothing else: the parsers
 * for TypeScript and for Svelte, and the one rule above. No comment in a
 * source turns the rule off: not `eslint-disable` in a script
 * (`noInlineConfig`), and not `<!-- eslint-disable -->` in the markup of a
 * component, which is the work of a rule of the Svelte plugin that is off
 * here.
 */
export const measuringConfig: Linter.Config[] = [
  ...svelte.configs.base,
  { files: ['**/*.ts'], ignores: ['**/*.svelte.ts'], languageOptions: { parser: ts.parser } },
  {
    files: ['**/*.svelte', '**/*.svelte.ts', '**/*.svelte.js'],
    languageOptions: { parserOptions: { parser: ts.parser, svelteConfig } },
  },
  {
    linterOptions: { noInlineConfig: true, reportUnusedDisableDirectives: 'off' },
    plugins: { 'fvoci-lengths': { rules: { 'every-function': everyFunction } } },
    rules: { [RULE]: 'error', 'svelte/comment-directive': 'off' },
  },
];

/** What ESLint says of a comment that tried to turn a rule off and was not obeyed. */
const NOT_OBEYED = "has no effect because you have 'noInlineConfig'";

/**
 * The functions among the messages of one file. Anything else ESLint says
 * is an error: a file that does not parse, or one it did not read at all (a
 * file under `node_modules`, a form it has no parser for). The one thing
 * passed over is the notice that a comment was not obeyed.
 */
export function functionsOf(file: string, messages: readonly Linter.LintMessage[]): Length[] {
  const functions: Length[] = [];
  for (const message of messages) {
    if (message.ruleId === RULE) {
      const [lines, name = ''] = message.message.split(' ');
      functions.push({ kind: 'function', file, name, lines: Number(lines), at: message.line });
    } else if (!message.message.includes(NOT_OBEYED)) {
      throw new Error(`${file}:${message.line}: not measured: ${message.message}`);
    }
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
    // The files are named one by one. What ESLint leaves out all the same
    // comes back as a message, which `functionsOf` refuses.
    ignore: false,
  });
  const results = await eslint.lintFiles(files);
  for (const result of results) {
    const file = path.relative(root, result.filePath).split(path.sep).join('/');
    measured.push(...functionsOf(file, result.messages));
  }
  return measured;
}

/** The differences between the code under `root` and the list there, as `judge` gives them. */
export async function check(root: string): Promise<{ measured: Length[]; problems: string[] }> {
  const measured = await measure(root);
  return { measured, problems: judge(measured, readFileSync(path.join(root, EXCEPTIONS), 'utf8')) };
}

/** What the list `after` has above the list `before`: a ceiling that went up, a line that is new. */
export function raised(before: string, after: string): string[] {
  const was = parseExceptions(before).over;
  const now = parseExceptions(after);
  const lines: string[] = [];
  for (const [key, ceilings] of now.over) {
    const subject = now.subjects.get(key);
    if (subject === undefined) continue;
    for (const [index, ceiling] of ceilings.entries()) {
      const old = was.get(key)?.[index];
      if (old === undefined) lines.push(`added: ${shown(subject)}: ${ceiling}`);
      else if (ceiling > old) lines.push(`raised: ${shown(subject)}: ${old} -> ${ceiling}`);
    }
  }
  return lines;
}

/** The list under `root` as it is, or an empty one where there is none that reads. */
function listAt(root: string): string {
  try {
    const list = readFileSync(path.join(root, EXCEPTIONS), 'utf8');
    parseExceptions(list);
    return list;
  } catch {
    return '';
  }
}

/**
 * Writes the list under `root` anew from the code there. Answers what it
 * wrote, and what went up by it: that is said, so that it is seen without
 * reading the diff. A list that is not there or does not read has nothing to
 * compare with.
 */
export async function rewrite(root: string): Promise<{ list: string; raised: string[] }> {
  const list = renderExceptions(await measure(root));
  const before = listAt(root);
  writeFileSync(path.join(root, EXCEPTIONS), list);
  return { list, raised: raised(before, list) };
}
