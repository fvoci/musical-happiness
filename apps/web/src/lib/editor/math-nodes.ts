/**
 * The two formula nodes of the editor: one that stands in a line, one that
 * takes a line of its own. Both hold the LaTeX source and nothing else.
 *
 * In the page a formula is the element drawing.ts describes, holding the
 * source; embeds.ts draws the formula into it once the library has arrived.
 * A press on a formula, or Enter while it is selected, opens the box in
 * which the source is changed (math-editing.ts).
 *
 * Typing makes formulas too:
 *
 * - `$x^2$` in a line becomes an inline formula when the closing dollar is
 *   typed;
 * - `$$x^2$$` alone on a line becomes a block formula;
 * - `$$` and a space on an empty line opens the box for a new block formula.
 */
import { InputRule, Node, type Editor, type Range } from '@tiptap/core';
import {
  NodeSelection,
  Plugin,
  PluginKey,
  TextSelection,
  type EditorState,
  type Transaction,
} from '@tiptap/pm/state';

import {
  MATH_SLOTS,
  MATH_SOURCE_ATTRIBUTE,
  mathSlotAttributes,
  type MathNodeName,
} from './drawing.ts';
import { fillMath, watchFormulaFit } from './embeds.ts';
import { editMath } from './math-editing.ts';
import { MAX_LATEX } from './schema.ts';

const latexOf = (node: { attrs: Record<string, unknown> }): string =>
  typeof node.attrs.latex === 'string' ? node.attrs.latex : '';

/** Opens the box for the source of the formula at `position`. */
function editFormulaAt(editor: Editor, name: MathNodeName, position: number): void {
  const node = editor.state.doc.nodeAt(position);
  if (node?.type.name !== name || !editor.isEditable) return;
  const before = latexOf(node);
  /** Whether the formula is still where it was: the document may be replaced from outside meanwhile. */
  const still = (): boolean => {
    const now = editor.isDestroyed ? null : editor.state.doc.nodeAt(position);
    return now?.type.name === name && latexOf(now) === before;
  };
  editMath({
    node: name,
    latex: before,
    page: editor.view.dom.ownerDocument,
    anchor: () => {
      const element = editor.isDestroyed ? null : editor.view.nodeDOM(position);
      return element instanceof HTMLElement ? element.getBoundingClientRect() : undefined;
    },
    apply: (latex) => {
      if (!still()) return;
      const transaction = editor.state.tr;
      if (latex === '') transaction.delete(position, position + node.nodeSize);
      else if (latex !== before) transaction.setNodeMarkup(position, undefined, { latex });
      const after = Math.min(
        transaction.doc.content.size,
        latex === '' ? position : position + node.nodeSize,
      );
      transaction.setSelection(TextSelection.near(transaction.doc.resolve(after)));
      editor.view.dispatch(transaction);
      editor.view.focus();
    },
    cancel: () => {
      if (!editor.isDestroyed) editor.view.focus();
    },
  });
}

/**
 * Puts a block formula in place of `range` (a whole paragraph, in the
 * document the transaction started from) and the cursor behind it. When
 * the formula would end its parent, an empty paragraph follows it, so that
 * there is somewhere to go on writing.
 */
function blockFormulaInto(transaction: Transaction, range: Range, latex: string): void {
  const { schema } = transaction.doc.type;
  const formula = schema.nodes.blockMath?.create({ latex });
  if (!formula) return;
  const steps = transaction.steps.length;
  transaction.replaceRangeWith(range.from, range.to, formula);
  const after = transaction.mapping.slice(steps).map(range.to);
  if (transaction.doc.resolve(after).nodeAfter === null) {
    const paragraph = schema.nodes.paragraph?.create();
    if (paragraph) transaction.insert(after, paragraph);
  }
  transaction.setSelection(TextSelection.near(transaction.doc.resolve(after), 1));
}

/** The empty paragraph around `position`, if that is what is there. */
function emptyParagraphAt(editor: Editor, position: number): Range | undefined {
  if (editor.isDestroyed || position > editor.state.doc.content.size) return undefined;
  const $position = editor.state.doc.resolve(position);
  if ($position.parent.type.name !== 'paragraph' || $position.parent.content.size > 0)
    return undefined;
  return { from: $position.before(), to: $position.after() };
}

/** Opens the box for a new block formula, to take the place of the empty paragraph at `position`. */
function editNewBlockFormula(editor: Editor, position: number): void {
  editMath({
    node: 'blockMath',
    latex: '',
    page: editor.view.dom.ownerDocument,
    anchor: () => (editor.isDestroyed ? undefined : editor.view.coordsAtPos(position)),
    apply: (latex) => {
      const range = emptyParagraphAt(editor, position);
      if (latex !== '' && range) {
        const transaction = editor.state.tr;
        blockFormulaInto(transaction, range, latex);
        editor.view.dispatch(transaction);
      }
      if (!editor.isDestroyed) editor.view.focus();
    },
    cancel: () => {
      if (!editor.isDestroyed) editor.view.focus();
    },
  });
}

/**
 * The paragraph an input rule fired in, when the match is all that is in it
 * and a block formula may take its place (the first block of a list entry
 * has to stay a paragraph).
 */
function replaceableParagraph(state: EditorState, range: Range): Range | undefined {
  const $from = state.doc.resolve(range.from);
  if ($from.parent.type.name !== 'paragraph') return undefined;
  if ($from.parentOffset !== 0 || range.to !== $from.end()) return undefined;
  const holder = $from.node($from.depth - 1).type.name;
  const leads = $from.index($from.depth - 1) === 0;
  if (leads && (holder === 'listItem' || holder === 'taskItem')) return undefined;
  return { from: $from.before(), to: $from.after() };
}

function mathNode(name: MathNodeName) {
  const slot = MATH_SLOTS[name];
  const isBlock = slot.display;
  return Node.create({
    name,
    group: isBlock ? 'block' : 'inline',
    inline: !isBlock,
    atom: true,
    selectable: true,

    addAttributes() {
      return {
        latex: {
          default: null,
          parseHTML: (element: HTMLElement) => element.getAttribute(MATH_SOURCE_ATTRIBUTE),
          // Written by renderHTML below, with the rest of the element.
          renderHTML: () => ({}),
        },
      };
    },

    parseHTML() {
      return [
        {
          tag: `${slot.tag}[data-type="${slot.type}"]`,
          getAttrs: (element: HTMLElement) => {
            const latex = element.getAttribute(MATH_SOURCE_ATTRIBUTE) ?? '';
            // A formula without a source, or with one that is too long, is not one.
            return latex.length > 0 && latex.length <= MAX_LATEX ? null : false;
          },
        },
      ];
    },

    renderHTML({ node }) {
      return [slot.tag, mathSlotAttributes(name, latexOf(node)), latexOf(node)];
    },

    renderText({ node }) {
      return latexOf(node);
    },

    addNodeView() {
      return ({ node, editor, getPos }) => {
        // The page the editor is in. (Its view is not there yet while the first nodes are drawn.)
        const page = document;
        const dom = page.createElement(slot.tag);
        let latex = latexOf(node);
        const show = (): void => {
          for (const [attribute, value] of Object.entries(mathSlotAttributes(name, latex)))
            dom.setAttribute(attribute, value);
          for (const attribute of ['data-state', 'role', 'aria-label'])
            dom.removeAttribute(attribute);
          dom.replaceChildren(page.createTextNode(latex));
          // Without the library the source stays, which is what a failure looks like anyway.
          fillMath(dom).catch(() => undefined);
        };
        show();
        dom.addEventListener('click', () => {
          const position = getPos();
          if (position !== undefined) editFormulaAt(editor, name, position);
        });
        return {
          dom,
          update: (updated) => {
            if (updated.type.name !== name) return false;
            if (latexOf(updated) !== latex) {
              latex = latexOf(updated);
              show();
            }
            return true;
          },
          // What changes inside is the drawing, never the document.
          ignoreMutation: () => true,
        };
      };
    },

    addProseMirrorPlugins() {
      // Once for the editor, with the node that stands in a line: a formula
      // that is wider than its line scrolls inside itself, also after the
      // editor changes its width.
      if (isBlock) return [];
      return [
        new Plugin({
          key: new PluginKey('formulaFit'),
          view: (view) => ({ destroy: watchFormulaFit(view.dom) }),
        }),
      ];
    },

    addKeyboardShortcuts() {
      return {
        Enter: ({ editor }) => {
          const { selection } = editor.state;
          if (!(selection instanceof NodeSelection) || selection.node.type.name !== name)
            return false;
          editFormulaAt(editor, name, selection.from);
          return true;
        },
      };
    },

    addInputRules() {
      const { editor, type } = this;
      if (!isBlock) {
        return [
          // The dollar that opens has no letter, digit, dollar or backslash
          // in front of it and no space behind it; the one that closes has
          // no space in front of it. So "$5 and $10" stays text.
          new InputRule({
            find: /(?<![\\$\p{L}\p{N}])\$([^\s$](?:[^$\n]*[^\s$])?)\$$/u,
            handler: ({ state, range, match }) => {
              const latex = match[1] ?? '';
              if (latex === '' || latex.length > MAX_LATEX) return null;
              state.tr.replaceWith(range.from, range.to, type.create({ latex }));
              return undefined;
            },
          }),
        ];
      }
      return [
        new InputRule({
          find: /^\$\$([^$]+)\$\$$/,
          handler: ({ state, range, match }) => {
            const latex = (match[1] ?? '').trim();
            const paragraph = replaceableParagraph(state, range);
            if (latex === '' || latex.length > MAX_LATEX || !paragraph) return null;
            blockFormulaInto(state.tr, paragraph, latex);
            return undefined;
          },
        }),
        new InputRule({
          find: /^\$\$\s$/,
          handler: ({ state, range }) => {
            const paragraph = replaceableParagraph(state, range);
            if (!paragraph) return null;
            state.tr.delete(range.from, range.to);
            setTimeout(() => {
              if (!editor.isDestroyed) editNewBlockFormula(editor, paragraph.from + 1);
            });
            return undefined;
          },
        }),
      ];
    },
  });
}

export const InlineMath = mathNode('inlineMath');
export const BlockMath = mathNode('blockMath');
