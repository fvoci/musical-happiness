/**
 * ESLint configuration that forbids browser-native modal dialogs (`alert`,
 * `confirm`, `prompt`) under `src/**`.
 *
 * Native dialogs block the main thread, cannot be styled or translated with
 * the rest of the interface, and bypass keyboard and screen-reader patterns.
 * Using `no-restricted-globals` and `no-restricted-properties` catches bare
 * global calls (`confirm(...)`), destructured globals (`const { prompt } = window`),
 * and global-object calls (`window.confirm(...)`, `top.alert(...)`) while
 * leaving local functions, parameters, props, and methods named `confirm`
 * (`ConfirmEmailApp.svelte`, `change.confirm(token)`) untouched.
 */
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

function dialogMessage(call: string): string {
  return `Browser \`${call}()\` is forbidden in app code; render inline feedback, a toast, or a Dialog / AlertDialog component instead.`;
}

const DISABLE_COMMENT = /(?:eslint-disable(?:-next-line|-line)?)\b([\s\S]*?)(?:-->|\*\/|$)/g;
const PROTECTED_RULES = ['no-restricted-globals', 'no-restricted-properties'];

/** Refuses `eslint-disable` comments that silence the browser-dialog rules. */
const noDialogDisable: Rule.RuleModule = {
  meta: {
    type: 'problem',
    docs: { description: 'forbids silencing browser modal dialog lint rules' },
    schema: [],
    messages: {
      protectedRule:
        'Do not silence `{{rule}}` with `eslint-disable`; replace native browser dialogs with app UI.',
    },
  },
  create(context) {
    return {
      Program(node) {
        const text = context.sourceCode.getText();
        for (const match of text.matchAll(DISABLE_COMMENT)) {
          const body = (match[1] ?? '').split('--')[0] ?? '';
          for (const rule of PROTECTED_RULES) {
            if (body.includes(rule)) {
              const loc = context.sourceCode.getLocFromIndex(match.index);
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
