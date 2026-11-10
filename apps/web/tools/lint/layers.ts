/**
 * ESLint rules for the layering of src/lib.
 *
 * - A feature (`src/lib/features/<name>/`) does not import from another
 *   feature. What two features share moves down into `logic`, `state`, `api`
 *   or `ui`.
 * - Those shared layers do not import from a feature.
 *
 * The rule looks at the path an import resolves to, so `$lib/features/x` and
 * `../x` are treated alike.
 */
import path from 'node:path';

import type { ESLint, Rule } from 'eslint';

const LIB = `${path.sep}src${path.sep}lib${path.sep}`;

interface Place {
  layer: string;
  /** The first folder under `features`; undefined in any other layer. */
  feature: string | undefined;
}

/** Where under src/lib a file lies, if it does. */
function place(file: string): Place | undefined {
  const at = file.lastIndexOf(LIB);
  if (at === -1) return undefined;
  const [layer = '', second] = file.slice(at + LIB.length).split(path.sep);
  return { layer, feature: layer === 'features' ? second : undefined };
}

/** The file path an import specifier points at, for `$lib/...` and relative specifiers. */
function resolve(specifier: string, importer: string): string | undefined {
  if (specifier.startsWith('$lib/')) {
    const at = importer.lastIndexOf(LIB);
    if (at === -1) return undefined;
    return path.join(importer.slice(0, at + LIB.length), specifier.slice('$lib/'.length));
  }
  if (specifier.startsWith('.')) return path.resolve(path.dirname(importer), specifier);
  return undefined;
}

const featureBoundaries: Rule.RuleModule = {
  meta: {
    type: 'problem',
    docs: {
      description: 'features do not import each other; shared layers do not import features',
    },
    schema: [],
    messages: {
      crossFeature:
        'The feature "{{from}}" imports from the feature "{{to}}". Move what they share into lib/logic, lib/state, lib/api or lib/ui.',
      sharedImportsFeature:
        'lib/{{layer}} imports from the feature "{{feature}}". Shared code must not depend on a feature.',
    },
  },
  create(context) {
    const from = place(context.filename);
    if (!from) return {};

    function check(node: Rule.Node, source: unknown): void {
      const specifier =
        typeof source === 'object' && source !== null && 'value' in source
          ? source.value
          : undefined;
      if (typeof specifier !== 'string') return;
      const target = resolve(specifier, context.filename);
      const to = target === undefined ? undefined : place(target);
      if (to?.feature === undefined) return;

      if (from?.feature === undefined) {
        context.report({
          node,
          messageId: 'sharedImportsFeature',
          data: { layer: from?.layer ?? '', feature: to.feature },
        });
      } else if (to.feature !== from.feature) {
        context.report({
          node,
          messageId: 'crossFeature',
          data: { from: from.feature, to: to.feature },
        });
      }
    }

    return {
      ImportDeclaration: (node) => {
        check(node, node.source);
      },
      ExportNamedDeclaration: (node) => {
        check(node, node.source);
      },
      ExportAllDeclaration: (node) => {
        check(node, node.source);
      },
      ImportExpression: (node) => {
        check(node, node.source);
      },
    };
  },
};

const plugin: ESLint.Plugin = { rules: { 'feature-boundaries': featureBoundaries } };

export default plugin;
