/**
 * ESLint configuration and out-of-band source check forbidding browser-native
 * modal dialogs (`alert`, `confirm`, `prompt`) and any inline `eslint-disable`
 * comment that could silence them under `src/**`.
 *
 * Because ESLint's own inline comment processor lets bare `/* eslint-disable * /`
 * or `// eslint-disable-line fvoci-dialogs/no-dialog-disable, no-restricted-globals`
 * suppress an ESLint rule from inside the linted file, `findDialogDisableViolations`
 * and `checkDialogs` also scan raw source files outside ESLint (`check-dialogs.ts`).
 */
import { lstatSync, readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';

import type { Linter, Rule } from 'eslint';

export const BROWSER_DIALOGS = ['alert', 'confirm', 'prompt'] as const;
export const GLOBAL_DIALOG_RECEIVERS = [
  'window',
  'globalThis',
  'self',
  'frames',
  'parent',
  'top',
] as const;

export const DIALOG_CHECKED_DIR = 'src';
export const DIALOG_EXTENSIONS = ['.ts', '.svelte', '.js'] as const;
export const DIALOG_GENERATED = [
  'src/lib/i18n/paraglide/',
  'src/styles/fonts/',
  'src/lib/api/contract.ts',
] as const;

function dialogMessage(call: string): string {
  return `Browser \`${call}()\` is forbidden in app code; render inline feedback, a toast, or a Dialog / AlertDialog component instead.`;
}

const DISABLE_COMMENT = /(?:eslint-disable(?:-next-line|-line)?)\b([^\r\n]*?)(?:-->|\*\/|$)/gm;
export const PROTECTED_DIALOG_RULES = [
  'fvoci-dialogs/no-dialog-disable',
  'no-restricted-globals',
  'no-restricted-properties',
] as const;

function lineNumberAt(source: string, index: number): number {
  let line = 1;
  for (let i = 0; i < index; i += 1) {
    if (source.charCodeAt(i) === 10) line += 1;
  }
  return line;
}

/**
 * Pure text scan catching bare `eslint-disable` directives (which silence all
 * rules) and any `eslint-disable` directive naming a protected dialog rule.
 */
export function findDialogDisableViolations(file: string, source: string): string[] {
  const problems: string[] = [];
  for (const match of source.matchAll(DISABLE_COMMENT)) {
    const body = ((match[1] ?? '').split('--')[0] ?? '').trim();
    const line = lineNumberAt(source, match.index);
    if (body === '') {
      problems.push(
        `${file}:${line}: bare \`eslint-disable\` without rule names is forbidden; name the exact non-protected rule being disabled`,
      );
      continue;
    }
    const tokens = body
      .split(/[,\s]+/)
      .map((token) => token.trim())
      .filter((token) => token !== '');
    for (const rule of PROTECTED_DIALOG_RULES) {
      if (tokens.includes(rule)) {
        problems.push(
          `${file}:${line}: do not silence \`${rule}\` with \`eslint-disable\`; replace native browser dialogs with app UI`,
        );
      }
    }
  }
  return problems;
}

export function dialogSourceFiles(root: string): string[] {
  const files: string[] = [];
  const walk = (relative: string): void => {
    const entries = readdirSync(path.join(root, relative), { withFileTypes: true });
    for (const entry of entries.sort((a, b) => (a.name < b.name ? -1 : 1))) {
      const child = `${relative}/${entry.name}`;
      if (DIALOG_GENERATED.some((made) => child === made || `${child}/` === made)) continue;
      if (entry.isSymbolicLink() || lstatSync(path.join(root, child)).isSymbolicLink()) {
        throw new Error(`${child} is a symbolic link`);
      }
      if (entry.isDirectory()) walk(child);
      else if (DIALOG_EXTENSIONS.some((ext) => entry.name.endsWith(ext))) files.push(child);
    }
  };
  walk(DIALOG_CHECKED_DIR);
  return files;
}

export function checkDialogs(root: string): { files: string[]; problems: string[] } {
  const files = dialogSourceFiles(root);
  const problems: string[] = [];
  for (const file of files) {
    const source = readFileSync(path.join(root, file), 'utf8');
    problems.push(...findDialogDisableViolations(file, source));
  }
  return { files, problems };
}

/** Refuses `eslint-disable` comments that silence the browser-dialog rules. */
const noDialogDisable: Rule.RuleModule = {
  meta: {
    type: 'problem',
    docs: { description: 'forbids silencing browser modal dialog lint rules' },
    schema: [],
    messages: {
      bareDisable:
        'Bare `eslint-disable` without rule names is forbidden; name the exact non-protected rule being disabled.',
      protectedRule:
        'Do not silence `{{rule}}` with `eslint-disable`; replace native browser dialogs with app UI.',
    },
  },
  create(context) {
    return {
      Program(node) {
        const text = context.sourceCode.getText();
        for (const match of text.matchAll(DISABLE_COMMENT)) {
          const body = ((match[1] ?? '').split('--')[0] ?? '').trim();
          const loc = context.sourceCode.getLocFromIndex(match.index);
          if (body === '') {
            context.report({ node, loc, messageId: 'bareDisable' });
            continue;
          }
          const tokens = body
            .split(/[,\s]+/)
            .map((token) => token.trim())
            .filter((token) => token !== '');
          for (const rule of PROTECTED_DIALOG_RULES) {
            if (tokens.includes(rule)) {
              context.report({
                node,
                loc,
                messageId: 'protectedRule',
                data: { rule },
              });
            }
          }
        }
      },
    };
  },
};

export const noBrowserDialogs: Linter.Config = {
  files: ['src/**'],
  plugins: {
    'fvoci-dialogs': {
      rules: {
        'no-dialog-disable': noDialogDisable,
      },
    },
  },
  rules: {
    'fvoci-dialogs/no-dialog-disable': 'error',
    'no-restricted-globals': [
      'error',
      ...BROWSER_DIALOGS.map((name) => ({
        name,
        message: dialogMessage(name),
      })),
    ],
    'no-restricted-properties': [
      'error',
      ...GLOBAL_DIALOG_RECEIVERS.flatMap((object) =>
        BROWSER_DIALOGS.map((property) => ({
          object,
          property,
          message: dialogMessage(`${object}.${property}`),
        })),
      ),
    ],
  },
};
