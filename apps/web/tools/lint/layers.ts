/**
 * ESLint rules of this project: the layering of src/lib, and keeping the
 * time zone of the device out of the logic (`no-device-time`, below).
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

/** Methods of `Date` that answer in, or are set in, the time zone of the device. */
const DEVICE_TIME_METHODS = new Set([
  'getDate',
  'getDay',
  'getFullYear',
  'getHours',
  'getMilliseconds',
  'getMinutes',
  'getMonth',
  'getSeconds',
  'getTimezoneOffset',
  'getYear',
  'setDate',
  'setFullYear',
  'setHours',
  'setMilliseconds',
  'setMinutes',
  'setMonth',
  'setSeconds',
  'toDateString',
  'toLocaleDateString',
  'toLocaleTimeString',
  'toTimeString',
]);

/** `Intl.DateTimeFormat`, as an expression. */
function isDateTimeFormat(node: Rule.Node | { type: string }): boolean {
  const member = node as {
    type: string;
    object?: { type: string; name?: string };
    property?: { type: string; name?: string };
  };
  return (
    member.type === 'MemberExpression' &&
    member.object?.type === 'Identifier' &&
    member.object.name === 'Intl' &&
    member.property?.type === 'Identifier' &&
    member.property.name === 'DateTimeFormat'
  );
}

/**
 * The logic must give the same result wherever it runs, so it may not ask
 * the device what time it is "here". The reader's time zone is an argument.
 *
 * Refused:
 * - the methods of `Date` that work in device time (`getHours`, `setDate`, ...);
 *   the `getUTC...` ones are fine
 * - `new Date(year, month, ...)`, which reads its numbers as device time, and
 *   `new Date('some text')`, which may
 * - `Intl.DateTimeFormat` without a `timeZone` option, which formats in device time
 * - reading the device's zone from `resolvedOptions().timeZone`
 */
const noDeviceTime: Rule.RuleModule = {
  meta: {
    type: 'problem',
    docs: { description: 'the logic does not use the time zone of the device' },
    schema: [],
    messages: {
      method:
        "`{{name}}` works in the time zone of the device. Take the reader's time zone as an argument (see lib/logic/dates.ts).",
      construct:
        'This `new Date(...)` reads its arguments as device time. Use `Date.UTC(...)` or the functions of lib/logic/dates.ts.',
      format:
        "`Intl.DateTimeFormat` without a `timeZone` option formats in the time zone of the device. Pass the reader's time zone.",
      zone: 'The time zone of the device is read in lib/regional.ts only. Take it as an argument.',
    },
  },
  create(context) {
    function checkFormat(
      node: Rule.Node,
      args: readonly { type: string; properties?: unknown[] }[],
    ): void {
      const options = args[1];
      const properties = (options?.type === 'ObjectExpression' ? options.properties : undefined) as
        { type: string; key?: { type: string; name?: string; value?: unknown } }[] | undefined;
      const named = properties?.some(
        (property) =>
          property.type === 'SpreadElement' ||
          (property.type === 'Property' &&
            (property.key?.name === 'timeZone' || property.key?.value === 'timeZone')),
      );
      // Options that are not written out here (a variable) are given the benefit of the doubt.
      if (options === undefined || (options.type === 'ObjectExpression' && !named)) {
        context.report({ node, messageId: 'format' });
      }
    }

    return {
      CallExpression(node) {
        const callee = node.callee;
        if (isDateTimeFormat(callee)) checkFormat(node, node.arguments);
        if (callee.type !== 'MemberExpression' || callee.property.type !== 'Identifier') return;
        if (DEVICE_TIME_METHODS.has(callee.property.name)) {
          context.report({ node, messageId: 'method', data: { name: callee.property.name } });
        }
      },
      NewExpression(node) {
        if (isDateTimeFormat(node.callee)) checkFormat(node, node.arguments);
        if (node.callee.type !== 'Identifier' || node.callee.name !== 'Date') return;
        const [first] = node.arguments;
        const text = first?.type === 'Literal' && typeof first.value === 'string';
        const template = first?.type === 'TemplateLiteral';
        if (node.arguments.length >= 2 || text || template) {
          context.report({ node, messageId: 'construct' });
        }
      },
      MemberExpression(node) {
        if (
          node.property.type === 'Identifier' &&
          node.property.name === 'timeZone' &&
          node.object.type === 'CallExpression' &&
          node.object.callee.type === 'MemberExpression' &&
          node.object.callee.property.type === 'Identifier' &&
          node.object.callee.property.name === 'resolvedOptions'
        ) {
          context.report({ node, messageId: 'zone' });
        }
      },
    };
  },
};

const plugin: ESLint.Plugin = {
  rules: { 'feature-boundaries': featureBoundaries, 'no-device-time': noDeviceTime },
};

export default plugin;
