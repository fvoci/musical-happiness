/**
 * Which toggles of a body are open, in one reader's editor: no part of the
 * document (details.ts). Opening or closing one is a transaction of the
 * editor that changes nothing in the document, so nothing of it is written
 * to the shared document, sent to others or kept for undo.
 *
 * The open toggles are node decorations of the editor's own (this file's
 * plugin), which the node view of a toggle reads. A toggle opens
 *
 * - when its chevron is pressed (and closes again so),
 * - when it is made here (`openAt`: from a line, or by Enter in its summary),
 * - when the selection comes into what it hides (with the arrow keys, by a
 *   search, by undo): no caret is ever where it cannot be seen.
 *
 * Closing a toggle that has the caret in what it hides puts the caret at
 * the end of its summary.
 *
 * A change that comes from the shared document replaces the blocks it
 * changed (shared-place.ts), so mapping the decorations through it would
 * close every toggle around what another person wrote. Its decorations are
 * mapped through what really changed instead: the stretch between where the
 * two documents start and stop being the same.
 */
import type { NodeViewRenderer } from '@tiptap/core';
import type { Node as DocumentNode, ResolvedPos } from '@tiptap/pm/model';
import { Plugin, PluginKey, TextSelection, type EditorState } from '@tiptap/pm/state';
import { Mapping, StepMap } from '@tiptap/pm/transform';
import { Decoration, DecorationSet } from '@tiptap/pm/view';
import type { Transaction } from '@tiptap/pm/state';

import { m } from '$lib/i18n/paraglide/messages.js';

import { chevronIcon } from './block-icons.ts';
import { fromShared } from './shared-place.ts';

const DETAILS = 'details';
const DETAILS_CONTENT = 'detailsContent';

export const detailsOpenKey = new PluginKey<DecorationSet>('detailsOpen');

/** A toggle to open or close: where it is after the first `steps` steps of its transaction. */
interface Opening {
  at: number;
  open: boolean;
  steps: number;
}

/** What marks a toggle open among the decorations. */
const OPEN_SPEC = { detailsOpen: true };

/**
 * Opens (or closes) the toggle at `at` with a transaction: `at` is a
 * position in the transaction's document as it is now, which its later
 * steps move along.
 */
export function openAt(tr: Transaction, at: number, open = true): Transaction {
  const openings = (tr.getMeta(detailsOpenKey) as Opening[] | undefined) ?? [];
  return tr.setMeta(detailsOpenKey, [...openings, { at, open, steps: tr.steps.length }]);
}

/** A mapping of what really changed from one document to the other (see above). */
function changeOf(before: DocumentNode, after: DocumentNode): Mapping {
  const start = before.content.findDiffStart(after.content);
  const end = before.content.findDiffEnd(after.content);
  if (start === null || end === null) return new Mapping();
  // Where the same thing was written again around a change, the two ends overlap.
  const overlap = Math.max(0, start - Math.min(end.a, end.b));
  return new Mapping([new StepMap([start, end.a + overlap - start, end.b + overlap - start])]);
}

/** The decoration that marks the toggle at `at` open, if it is. */
function openOne(set: DecorationSet, at: number, size: number): Decoration[] {
  return set
    .find(at, at + size, (spec: { detailsOpen?: boolean }) => spec.detailsOpen === true)
    .filter((one) => one.from === at && one.to === at + size);
}

/** The set with the toggle at `at` open or closed; the same set where nothing is a toggle there. */
function withOpen(set: DecorationSet, doc: DocumentNode, at: number, open: boolean): DecorationSet {
  const node = doc.nodeAt(at);
  if (node?.type.name !== DETAILS) return set;
  const found = openOne(set, at, node.nodeSize);
  if (!open) return found.length > 0 ? set.remove(found) : set;
  if (found.length > 0) return set;
  return set.add(doc, [Decoration.node(at, at + node.nodeSize, {}, OPEN_SPEC)]);
}

/** The set with every toggle open whose hidden part holds the place. */
function openAround(set: DecorationSet, doc: DocumentNode, $pos: ResolvedPos): DecorationSet {
  let next = set;
  for (let depth = $pos.depth; depth > 1; depth -= 1) {
    if ($pos.node(depth).type.name === DETAILS_CONTENT)
      next = withOpen(next, doc, $pos.before(depth - 1), true);
  }
  return next;
}

function apply(tr: Transaction, set: DecorationSet, state: EditorState): DecorationSet {
  let next = set;
  if (tr.docChanged)
    next = next.map(fromShared(tr) ? changeOf(tr.before, tr.doc) : tr.mapping, tr.doc);
  const openings = (tr.getMeta(detailsOpenKey) as Opening[] | undefined) ?? [];
  for (const { at, open, steps } of openings)
    next = withOpen(next, tr.doc, tr.mapping.slice(steps).map(at), open);
  if (tr.selectionSet || tr.docChanged) {
    const { $anchor, $head } = state.selection;
    next = openAround(openAround(next, tr.doc, $anchor), tr.doc, $head);
  }
  return next;
}

/** The plugin that keeps which toggles are open. */
export function detailsOpenPlugin(): Plugin<DecorationSet> {
  return new Plugin<DecorationSet>({
    key: detailsOpenKey,
    state: {
      init: () => DecorationSet.empty,
      apply: (tr, set, _before, state) => apply(tr, set, state),
    },
    props: {
      decorations: (state) => detailsOpenKey.getState(state),
    },
  });
}

/** Whether the toggle at `at` is open in a state. */
export function isOpenAt(state: EditorState, at: number): boolean {
  const node = state.doc.nodeAt(at);
  const set = detailsOpenKey.getState(state);
  return node !== null && set !== undefined && openOne(set, at, node.nodeSize).length > 0;
}

/** The transaction that opens or closes the toggle at `at`, the caret out of what it hides. */
function turned(state: EditorState, at: number, open: boolean): Transaction {
  const tr = openAt(state.tr, at, open);
  const toggle = state.doc.nodeAt(at);
  if (open || !toggle) return tr;
  const summaryEnd = at + toggle.child(0).nodeSize;
  const { from, to } = state.selection;
  const hidden = (pos: number) => pos > summaryEnd && pos < at + toggle.nodeSize;
  if (hidden(from) || hidden(to)) tr.setSelection(TextSelection.create(tr.doc, summaryEnd));
  return tr;
}

const isOpen = (decorations: readonly Decoration[]): boolean =>
  decorations.some((one) => (one.spec as { detailsOpen?: boolean }).detailsOpen === true);

/**
 * The node view of a toggle: its chevron, a button, then its two parts. The
 * button may be pressed in a body that can only be read, too: opening a
 * toggle is reading it.
 */
export const detailsView: NodeViewRenderer = ({ HTMLAttributes, decorations, view, getPos }) => {
  const page = view.dom.ownerDocument;
  const dom = page.createElement('div');
  for (const [name, value] of Object.entries(HTMLAttributes))
    if (typeof value === 'string') dom.setAttribute(name, value);
  dom.classList.add('doc-details');
  dom.setAttribute('data-details', '');
  const button = page.createElement('button');
  button.type = 'button';
  button.className = 'doc-details-button';
  button.contentEditable = 'false';
  button.append(chevronIcon(page));
  const contentDOM = page.createElement('div');
  contentDOM.className = 'doc-details-parts';
  dom.append(button, contentDOM);

  let open = false;
  const show = (now: boolean): void => {
    open = now;
    dom.toggleAttribute('data-open', now);
    button.setAttribute('aria-expanded', String(now));
    button.setAttribute('aria-label', now ? m.editor_details_close() : m.editor_details_open());
  };
  show(isOpen(decorations));
  // The press is the button's: the caret stays where it was, the focus too.
  button.addEventListener('mousedown', (event) => {
    event.preventDefault();
  });
  button.addEventListener('click', (event) => {
    event.preventDefault();
    const at = getPos();
    if (at !== undefined) view.dispatch(turned(view.state, at, !open));
  });

  return {
    dom,
    contentDOM,
    update: (node, now) => {
      if (node.type.name !== DETAILS) return false;
      show(isOpen(now));
      return true;
    },
    stopEvent: (event) => event.target instanceof Node && button.contains(event.target),
    ignoreMutation: (mutation) =>
      mutation.type !== 'selection' &&
      (!contentDOM.contains(mutation.target) ||
        (mutation.type === 'attributes' && mutation.target === contentDOM)),
  };
};
